//! Generic learn-based controller mapper and ALSA MIDI / NI HID input layer.
//!
//! Provides:
//! - [`MidiParserState`] and [`parse_midi_bytes`]: Pure streaming MIDI parser with running status,
//!   realtime byte preservation, sysex skipping, 14-bit CC pair combination, and relative encoder detection.
//! - [`diff_hid_reports`]: Pure report diffing for unpublished Native Instruments HID devices (X1 MK3, Z1 MK2, etc.).
//! - [`GenericControllerMapping`]: Plain-text controller mapping persistence (`~/.config/retraktor/controllers/<device>.txt`).
//! - [`DDJ_FLX4_DEFAULT_MAPPING`]: Official Pioneer DJ DDJ-FLX4 factory mapping preset.
//! - [`GenericControllerManager`]: Hot-plug background reader for ALSA raw MIDI and read-only generic NI hidraw nodes.

use eframe::egui::{self, RichText, Sense, Stroke, Ui, Vec2};
use retraktor_core::SurfaceEvent;
use retraktor_hid::{lookup_ni_device, NI_USB_VID, TRAKTOR_MX2_PID};
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{spawn, JoinHandle};
use std::time::Duration;

use crate::controller_map::{action_label, BUTTON_ACTIONS, CONTINUOUS_ACTIONS};
use crate::theme::TraktorTheme;

// ============================================================================
// 1. Pure MIDI Byte-Stream Parser
// ============================================================================

/// Stateful MIDI byte-stream parser.
#[derive(Debug, Clone, Default)]
pub struct MidiParserState {
    running_status: Option<u8>,
    in_sysex: bool,
    data_buf: [u8; 2],
    data_len: usize,
    expected_len: usize,
    /// Pending 14-bit CC MSB bytes: `[channel (0..15)][msb_cc (0..31)]`.
    pending_msb: [[Option<u8>; 32]; 16],
    /// Tracks recent values per CC to classify relative encoders (centered on 64 or 0/127).
    cc_history: HashMap<(u8, u8), Vec<u8>>,
    /// Dynamically learned or detected relative encoder CCs.
    cc_is_relative: HashMap<(u8, u8), bool>,
}

impl MidiParserState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicitly mark a CC as a relative encoder.
    pub fn set_relative(&mut self, ch: u8, cc: u8, relative: bool) {
        self.cc_is_relative.insert((ch, cc), relative);
    }
}

/// Pure parser function: feeds arbitrary byte chunks from a MIDI stream and returns decoded [`SurfaceEvent`]s.
///
/// Complies with MIDI 1.0 specification:
/// - Preserves running status across data bytes.
/// - System Realtime bytes (`0xF8..=0xFF`) are ignored and do not interrupt running status or data bytes.
/// - System Exclusive (`0xF0..0xF7`) streams are safely skipped.
/// - Note On with velocity 0 is decoded as Note Off (`pressed: false`).
/// - 14-bit CC pairs (MSB `0..=31` followed by LSB `32..=63`) are combined.
/// - Relative encoders sending 63/65 (centered on 64) or 1/127 (centered on 0) emit [`SurfaceEvent::Encoder`].
pub fn parse_midi_bytes(
    state: &mut MidiParserState,
    device: &str,
    bytes: &[u8],
) -> Vec<SurfaceEvent> {
    let mut events = Vec::new();

    for &b in bytes {
        // 1. System Realtime bytes (0xF8..=0xFF): ignore immediately; do not alter running status
        if b >= 0xF8 {
            continue;
        }

        // 2. System Exclusive handling
        if b == 0xF0 {
            state.in_sysex = true;
            state.running_status = None;
            state.data_len = 0;
            continue;
        }
        if b == 0xF7 {
            state.in_sysex = false;
            continue;
        }
        if state.in_sysex {
            if b >= 0x80 {
                // Any status byte aborts sysex
                state.in_sysex = false;
            } else {
                // Payload inside sysex is skipped
                continue;
            }
        }

        // 3. Status Bytes (0x80..=0xF6)
        if b >= 0x80 {
            if (0xF1..=0xF6).contains(&b) {
                // System common message (clears running status)
                state.running_status = None;
                state.data_len = 0;
                continue;
            }

            // Channel Voice Message
            state.running_status = Some(b);
            state.data_len = 0;
            state.expected_len = match b & 0xF0 {
                0xC0 | 0xD0 => 1, // Program Change, Channel Pressure
                _ => 2,           // Note On/Off, Poly Pressure, CC, Pitch Bend
            };
            continue;
        }

        // 4. Data Bytes (0x00..=0x7F)
        let Some(status) = state.running_status else {
            // Stray data byte with no running status
            continue;
        };

        if state.data_len < 2 {
            state.data_buf[state.data_len] = b;
            state.data_len += 1;
        }

        if state.data_len == state.expected_len {
            let ch = (status & 0x0F) + 1; // 1-based (1..=16)
            let msg_type = status & 0xF0;

            match msg_type {
                0x80 => {
                    // Note Off
                    let note = state.data_buf[0];
                    events.push(SurfaceEvent::Button {
                        control: format!("midi.{device}.ch{ch}.note{note}"),
                        pressed: false,
                    });
                }
                0x90 => {
                    // Note On
                    let note = state.data_buf[0];
                    let vel = state.data_buf[1];
                    let pressed = vel > 0;
                    events.push(SurfaceEvent::Button {
                        control: format!("midi.{device}.ch{ch}.note{note}"),
                        pressed,
                    });
                }
                0xB0 => {
                    // Control Change
                    let cc = state.data_buf[0];
                    let val = state.data_buf[1];

                    // Track relative encoder pattern (centered around 64 or 0/127)
                    let hist = state.cc_history.entry((ch, cc)).or_default();
                    hist.push(val);
                    if hist.len() > 6 {
                        hist.remove(0);
                    }

                    let is_rel = state.cc_is_relative.get(&(ch, cc)).copied().unwrap_or_else(|| {
                        if hist.len() >= 2 {
                            let all_64_centered = hist.iter().all(|&v| (60..=68).contains(&v) && v != 64);
                            let all_0_centered = hist.iter().all(|&v| v <= 5 || v >= 123);
                            all_64_centered || all_0_centered
                        } else {
                            false
                        }
                    });

                    if is_rel {
                        state.cc_is_relative.insert((ch, cc), true);
                        let delta = if (58..=70).contains(&val) {
                            // 64-centered: 65 is +1, 63 is -1
                            (val as i32) - 64
                        } else if val < 64 {
                            val as i32
                        } else {
                            (val as i32) - 128
                        };
                        events.push(SurfaceEvent::Encoder {
                            control: format!("midi.{device}.ch{ch}.cc{cc}"),
                            delta: delta as i64,
                        });
                    } else if cc < 32 {
                        // 14-bit CC MSB
                        state.pending_msb[(ch - 1) as usize][cc as usize] = Some(val);
                        // Emit 7-bit normalized fallback
                        events.push(SurfaceEvent::Fader {
                            control: format!("midi.{device}.ch{ch}.cc{cc}"),
                            value: val as f64 / 127.0,
                        });
                    } else if (32..64).contains(&cc) {
                        // 14-bit CC LSB
                        let msb_cc = cc - 32;
                        if let Some(msb) = state.pending_msb[(ch - 1) as usize][msb_cc as usize] {
                            let val14 = ((msb as u16) << 7) | (val as u16);
                            let norm = (val14 as f64) / 16383.0;
                            events.push(SurfaceEvent::Fader {
                                control: format!("midi.{device}.ch{ch}.cc{msb_cc}"),
                                value: norm,
                            });
                        } else {
                            events.push(SurfaceEvent::Fader {
                                control: format!("midi.{device}.ch{ch}.cc{cc}"),
                                value: val as f64 / 127.0,
                            });
                        }
                    } else {
                        // Standard CC (>= 64)
                        events.push(SurfaceEvent::Fader {
                            control: format!("midi.{device}.ch{ch}.cc{cc}"),
                            value: val as f64 / 127.0,
                        });
                    }
                }
                0xE0 => {
                    // Pitch Bend (14-bit: LSB then MSB)
                    let lsb = state.data_buf[0];
                    let msb = state.data_buf[1];
                    let val14 = ((msb as u16) << 7) | (lsb as u16);
                    let norm = (val14 as f64) / 16383.0;
                    events.push(SurfaceEvent::Fader {
                        control: format!("midi.{device}.ch{ch}.bend"),
                        value: norm,
                    });
                }
                _ => {}
            }

            // Reset data buffer for the next message under running status
            state.data_len = 0;
        }
    }

    events
}

// ============================================================================
// 2. Pure Generic NI HID Report Diff
// ============================================================================

/// Tracks observed min/max values for smooth analog channels.
#[derive(Debug, Clone, Default)]
pub struct HidAnalogTracker {
    pub ranges: HashMap<String, (u16, u16)>,
}

/// Diffs consecutive input reports of the same report id for unpublished Native Instruments HID devices.
///
/// Output report layouts for devices like the Traktor X1 MK3 or Z1 MK2 are unpublished.
/// This pure diff engine derives events without sending any commands to the hardware:
/// - Single bit flip -> Button `hid.<pid>.r<report>.b<byte>.<bit>`
/// - Smooth byte or 16-bit LE value changes -> Fader `hid.<pid>.r<report>.a<byte>` normalized by seen min/max
pub fn diff_hid_reports(
    pid: u16,
    prev: &[u8],
    curr: &[u8],
    tracker: &mut HidAnalogTracker,
) -> Vec<SurfaceEvent> {
    let mut events = Vec::new();
    if prev.is_empty() || curr.is_empty() || prev[0] != curr[0] {
        return events;
    }

    let report_id = curr[0];
    let prev_payload = &prev[1..];
    let curr_payload = &curr[1..];
    let len = prev_payload.len().min(curr_payload.len());

    // 1. Bit diff for Buttons: single or sparse bit flips
    for b_idx in 0..len {
        let p = prev_payload[b_idx];
        let c = curr_payload[b_idx];
        let diff = p ^ c;
        if diff != 0 {
            for bit in 0..8 {
                if (diff & (1 << bit)) != 0 {
                    let pressed = (c & (1 << bit)) != 0;
                    events.push(SurfaceEvent::Button {
                        control: format!("hid.{pid:04x}.r{report_id}.b{b_idx}.{bit}"),
                        pressed,
                    });
                }
            }
        }
    }

    // 2. Smooth value changes for 16-bit LE pairs (even offsets)
    let mut b = 0;
    while b + 1 < len {
        let p_u16 = u16::from_le_bytes([prev_payload[b], prev_payload[b + 1]]);
        let c_u16 = u16::from_le_bytes([curr_payload[b], curr_payload[b + 1]]);
        if p_u16 != c_u16 {
            let delta = (c_u16 as i32 - p_u16 as i32).abs();
            // Smooth movement (analog knob/fader/jog ADC delta)
            if delta < 2048 {
                let ctl = format!("hid.{pid:04x}.r{report_id}.a{b}");
                let range = tracker.ranges.entry(ctl.clone()).or_insert((c_u16, c_u16));
                if c_u16 < range.0 {
                    range.0 = c_u16;
                }
                if c_u16 > range.1 {
                    range.1 = c_u16;
                }
                let norm = if range.1 > range.0 {
                    (c_u16 - range.0) as f64 / (range.1 - range.0) as f64
                } else {
                    (c_u16 as f64) / 4095.0
                };
                events.push(SurfaceEvent::Fader {
                    control: ctl,
                    value: norm.clamp(0.0, 1.0),
                });
            }
        }
        b += 2;
    }

    events
}

// ============================================================================
// 3. Generic Controller Mapping File Format
// ============================================================================

/// Single learned or configured control mapping.
#[derive(Debug, Clone, PartialEq)]
pub struct MappingEntry {
    pub control_id: String,
    pub friendly_name: String,
    pub action: String,
}

/// Complete mapping for a generic MIDI or HID controller.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GenericControllerMapping {
    pub device_name: String,
    pub entries: Vec<MappingEntry>,
}

impl GenericControllerMapping {
    pub fn new(device_name: impl Into<String>) -> Self {
        Self {
            device_name: device_name.into(),
            entries: Vec::new(),
        }
    }

    /// Resolves the storage path: `~/.config/retraktor/controllers/<device-name>.txt`.
    pub fn file_path(device_slug: &str) -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        PathBuf::from(home)
            .join(".config/retraktor/controllers")
            .join(format!("{device_slug}.txt"))
    }

    /// Parses a mapping file tolerantly, ignoring blank lines and comments.
    pub fn parse(device_name: &str, text: &str) -> Self {
        let mut entries = Vec::new();
        for line in text.lines() {
            let l = line.trim();
            if l.is_empty() || l.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = l.split('\t').collect();
            match parts.as_slice() {
                [cid, name, act, ..] => {
                    entries.push(MappingEntry {
                        control_id: cid.trim().to_string(),
                        friendly_name: name.trim().to_string(),
                        action: act.trim().to_string(),
                    });
                }
                [cid, act] => {
                    let default_name = format_friendly_control_id(cid.trim());
                    entries.push(MappingEntry {
                        control_id: cid.trim().to_string(),
                        friendly_name: default_name,
                        action: act.trim().to_string(),
                    });
                }
                _ => {}
            }
        }
        Self {
            device_name: device_name.to_string(),
            entries,
        }
    }

    /// Serializes to the standard plain-text line format.
    pub fn to_text(&self) -> String {
        let mut out = format!("# ReTraktor Controller Mapping: {}\n", self.device_name);
        out.push_str("# Format: <control_id>\\t<friendly_name>\\t<action>\n\n");
        for e in &self.entries {
            out.push_str(&format!("{}\t{}\t{}\n", e.control_id, e.friendly_name, e.action));
        }
        out
    }

    /// Saves the mapping to disk.
    pub fn save(&self, device_slug: &str) -> std::io::Result<()> {
        let path = Self::file_path(device_slug);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, self.to_text())
    }

    /// Loads the mapping from disk if present.
    pub fn load(device_slug: &str) -> Option<Self> {
        let path = Self::file_path(device_slug);
        let content = std::fs::read_to_string(path).ok()?;
        Some(Self::parse(device_slug, &content))
    }

    /// Inserts or updates a mapping entry.
    pub fn set_action(&mut self, control_id: &str, action: &str) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.control_id == control_id) {
            e.action = action.to_string();
        } else {
            self.entries.push(MappingEntry {
                control_id: control_id.to_string(),
                friendly_name: format_friendly_control_id(control_id),
                action: action.to_string(),
            });
        }
    }

    /// Removes a mapping entry by control id.
    pub fn remove(&mut self, control_id: &str) {
        self.entries.retain(|e| e.control_id != control_id);
    }
}

/// Converts a canonical control id to a friendly editable name.
pub fn format_friendly_control_id(id: &str) -> String {
    if let Some(rest) = id.strip_prefix("midi.") {
        let parts: Vec<&str> = rest.split('.').collect();
        if parts.len() >= 3 {
            let ch = parts[1].trim_start_matches("ch");
            let ctl = parts[2];
            return if let Some(n) = ctl.strip_prefix("note") {
                format!("CH{ch} Note {n}")
            } else if let Some(c) = ctl.strip_prefix("cc") {
                format!("CH{ch} CC {c}")
            } else if ctl == "bend" {
                format!("CH{ch} Pitch Bend")
            } else {
                format!("CH{ch} {ctl}")
            };
        }
    } else if let Some(rest) = id.strip_prefix("hid.") {
        return format!("HID {rest}");
    }
    id.to_string()
}

// ============================================================================
// 4. Pioneer DDJ-FLX4 Factory Mapping Preset
// ============================================================================

/// Official Pioneer DJ / AlphaTheta DDJ-FLX4 MIDI mapping table.
///
/// Published numbers taken directly from AlphaTheta "DDJ-FLX4 List of MIDI messages".
/// Verified against hardware specifications:
/// - Deck 1: Channel 1 (`0x90` / `0xB0`)
/// - Deck 2: Channel 2 (`0x91` / `0xB1`)
/// - Mixer & Global: Channel 7 (`0x96` / `0xB6`)
/// - Deck 1 Hot Cues: Channel 8 (`0x97`)
/// - Deck 2 Hot Cues: Channel 10 (`0x99`)
///
/// Intentionally omitted (untransmittable or unsupported hardware-only controls):
/// - Master Headphone Cue: DDJ-FLX4 has no physical Master Cue button (internal DAC only).
/// - Headphone Level / Mix: Analog pots routed directly to the audio codec.
/// - Mic Level: Hardware pot, no MIDI message sent.
/// - Smart CFX / Smart Fader: Hardware macro buttons omitted.
pub const DDJ_FLX4_DEFAULT_MAPPING: &[(&str, &str, &str)] = &[
    // --- DECK 1 (Channel 1) ---
    ("midi.ddj_flx4.ch1.note11", "Deck A Play / Pause", "play:1"),
    ("midi.ddj_flx4.ch1.note12", "Deck A Cue", "cue:1"),
    ("midi.ddj_flx4.ch1.note88", "Deck A Sync", "sync:1"),
    ("midi.ddj_flx4.ch1.note54", "Deck A Jog Platter Touch", "jog_touch:1"),
    ("midi.ddj_flx4.ch1.cc34", "Deck A Jog Platter Turn (Vinyl)", "jog:1"),
    ("midi.ddj_flx4.ch1.cc33", "Deck A Jog Rim Turn (Nudge)", "jog:1"),
    ("midi.ddj_flx4.ch1.cc0", "Deck A Tempo Fader", "pitch:1"),
    ("midi.ddj_flx4.ch1.cc19", "Deck A Channel Fader", "volume:1"),
    ("midi.ddj_flx4.ch1.cc4", "Deck A Trim (Gain)", "gain:1"),
    ("midi.ddj_flx4.ch1.cc7", "Deck A EQ High", "eq_hi:1"),
    ("midi.ddj_flx4.ch1.cc11", "Deck A EQ Mid", "eq_mid:1"),
    ("midi.ddj_flx4.ch1.cc15", "Deck A EQ Low", "eq_low:1"),
    ("midi.ddj_flx4.ch1.note84", "Deck A Headphone Cue", "headphone:1"),
    ("midi.ddj_flx4.ch1.note16", "Deck A Loop In / 4-Beat", "loop:1"),
    ("midi.ddj_flx4.ch1.note17", "Deck A Loop Out", "loop:1"),
    ("midi.ddj_flx4.ch1.note77", "Deck A 4-Beat / Reloop Exit", "loop:1"),
    ("midi.ddj_flx4.ch1.note81", "Deck A Halve Loop", "loop_half:1"),
    ("midi.ddj_flx4.ch1.note83", "Deck A Double Loop", "loop_double:1"),

    // --- DECK 2 (Channel 2) ---
    ("midi.ddj_flx4.ch2.note11", "Deck B Play / Pause", "play:2"),
    ("midi.ddj_flx4.ch2.note12", "Deck B Cue", "cue:2"),
    ("midi.ddj_flx4.ch2.note88", "Deck B Sync", "sync:2"),
    ("midi.ddj_flx4.ch2.note54", "Deck B Jog Platter Touch", "jog_touch:2"),
    ("midi.ddj_flx4.ch2.cc34", "Deck B Jog Platter Turn (Vinyl)", "jog:2"),
    ("midi.ddj_flx4.ch2.cc33", "Deck B Jog Rim Turn (Nudge)", "jog:2"),
    ("midi.ddj_flx4.ch2.cc0", "Deck B Tempo Fader", "pitch:2"),
    ("midi.ddj_flx4.ch2.cc19", "Deck B Channel Fader", "volume:2"),
    ("midi.ddj_flx4.ch2.cc4", "Deck B Trim (Gain)", "gain:2"),
    ("midi.ddj_flx4.ch2.cc7", "Deck B EQ High", "eq_hi:2"),
    ("midi.ddj_flx4.ch2.cc11", "Deck B EQ Mid", "eq_mid:2"),
    ("midi.ddj_flx4.ch2.cc15", "Deck B EQ Low", "eq_low:2"),
    ("midi.ddj_flx4.ch2.note84", "Deck B Headphone Cue", "headphone:2"),
    ("midi.ddj_flx4.ch2.note16", "Deck B Loop In / 4-Beat", "loop:2"),
    ("midi.ddj_flx4.ch2.note17", "Deck B Loop Out", "loop:2"),
    ("midi.ddj_flx4.ch2.note77", "Deck B 4-Beat / Reloop Exit", "loop:2"),
    ("midi.ddj_flx4.ch2.note81", "Deck B Halve Loop", "loop_half:2"),
    ("midi.ddj_flx4.ch2.note83", "Deck B Double Loop", "loop_double:2"),

    // --- MIXER & BROWSE (Channel 7) ---
    ("midi.ddj_flx4.ch7.cc31", "Crossfader", "xfader"),
    ("midi.ddj_flx4.ch7.cc23", "Deck A Filter / CFX", "filter:1"),
    ("midi.ddj_flx4.ch7.cc24", "Deck B Filter / CFX", "filter:2"),
    ("midi.ddj_flx4.ch7.cc64", "Browse Encoder", "browse"),
    ("midi.ddj_flx4.ch7.note70", "Load Track Deck A", "load:1"),
    ("midi.ddj_flx4.ch7.note71", "Load Track Deck B", "load:2"),

    // --- HOT CUES (Deck 1: Channel 8, Notes 0..7) ---
    ("midi.ddj_flx4.ch8.note0", "Deck A Hotcue 1", "hotcue:1:1"),
    ("midi.ddj_flx4.ch8.note1", "Deck A Hotcue 2", "hotcue:1:2"),
    ("midi.ddj_flx4.ch8.note2", "Deck A Hotcue 3", "hotcue:1:3"),
    ("midi.ddj_flx4.ch8.note3", "Deck A Hotcue 4", "hotcue:1:4"),
    ("midi.ddj_flx4.ch8.note4", "Deck A Hotcue 5", "hotcue:1:5"),
    ("midi.ddj_flx4.ch8.note5", "Deck A Hotcue 6", "hotcue:1:6"),
    ("midi.ddj_flx4.ch8.note6", "Deck A Hotcue 7", "hotcue:1:7"),
    ("midi.ddj_flx4.ch8.note7", "Deck A Hotcue 8", "hotcue:1:8"),

    // --- HOT CUES (Deck 2: Channel 10, Notes 0..7) ---
    ("midi.ddj_flx4.ch10.note0", "Deck B Hotcue 1", "hotcue:2:1"),
    ("midi.ddj_flx4.ch10.note1", "Deck B Hotcue 2", "hotcue:2:2"),
    ("midi.ddj_flx4.ch10.note2", "Deck B Hotcue 3", "hotcue:2:3"),
    ("midi.ddj_flx4.ch10.note3", "Deck B Hotcue 4", "hotcue:2:4"),
    ("midi.ddj_flx4.ch10.note4", "Deck B Hotcue 5", "hotcue:2:5"),
    ("midi.ddj_flx4.ch10.note5", "Deck B Hotcue 6", "hotcue:2:6"),
    ("midi.ddj_flx4.ch10.note6", "Deck B Hotcue 7", "hotcue:2:7"),
    ("midi.ddj_flx4.ch10.note7", "Deck B Hotcue 8", "hotcue:2:8"),
];

/// Returns the factory default mapping for the DDJ-FLX4.
pub fn flx4_default_mapping() -> GenericControllerMapping {
    let mut m = GenericControllerMapping::new("Pioneer DDJ-FLX4");
    for &(cid, name, act) in DDJ_FLX4_DEFAULT_MAPPING {
        m.entries.push(MappingEntry {
            control_id: cid.to_string(),
            friendly_name: name.to_string(),
            action: act.to_string(),
        });
    }
    m
}

// ============================================================================
// 5. Hardware Scanner & Background Reader
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceProtocol {
    Midi,
    GenericNiHid,
}

#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    pub name: String,
    pub slug: String,
    pub dev_node: String,
    pub protocol: DeviceProtocol,
    pub connected: bool,
}

/// Discovers connected ALSA raw MIDI devices (`/dev/snd/midiC*D*`).
pub fn scan_linux_alsa_midi_devices() -> Vec<DiscoveredDevice> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/dev/snd") else {
        return found;
    };

    for entry in entries.flatten() {
        let fname = entry.file_name().to_string_lossy().to_string();
        if !fname.starts_with("midiC") {
            continue;
        }
        let dev_node = format!("/dev/snd/{fname}");

        // Attempt to extract card index, e.g. midiC1D0 -> card 1
        let mut card_name = fname.clone();
        if let Some(rest) = fname.strip_prefix("midiC") {
            if let Some((card_idx, _)) = rest.split_once('D') {
                let id_path = format!("/proc/asound/card{card_idx}/id");
                if let Ok(id_str) = std::fs::read_to_string(&id_path) {
                    let trimmed = id_str.trim();
                    if !trimmed.is_empty() {
                        card_name = trimmed.to_string();
                    }
                }
            }
        }

        let slug = slugify(&card_name);
        found.push(DiscoveredDevice {
            name: card_name,
            slug,
            dev_node,
            protocol: DeviceProtocol::Midi,
            connected: true,
        });
    }

    found.sort_by(|a, b| a.dev_node.cmp(&b.dev_node));
    found
}

/// Discovers connected Native Instruments HID devices that are NOT the MX2.
pub fn scan_linux_generic_ni_hid_devices() -> Vec<DiscoveredDevice> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/class/hidraw") else {
        return found;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let uevent_path = entry.path().join("device/uevent");
        let Ok(uevent) = std::fs::read_to_string(&uevent_path) else {
            continue;
        };

        let mut dev_vid = 0u16;
        let mut dev_pid = 0u16;
        let mut hid_name = String::new();

        for line in uevent.lines() {
            if let Some(rest) = line.strip_prefix("HID_ID=") {
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() == 3 {
                    dev_vid = u16::from_str_radix(parts[1].trim_start_matches('0'), 16).unwrap_or(0);
                    dev_pid = u16::from_str_radix(parts[2].trim_start_matches('0'), 16).unwrap_or(0);
                }
            } else if let Some(hname) = line.strip_prefix("HID_NAME=") {
                hid_name = hname.to_string();
            }
        }

        if dev_vid == NI_USB_VID && dev_pid != TRAKTOR_MX2_PID {
            let model = lookup_ni_device(dev_vid, dev_pid)
                .map(|d| d.model.to_string())
                .unwrap_or_else(|| if !hid_name.is_empty() { hid_name } else { format!("NI Device {dev_pid:04x}") });
            let slug = slugify(&model);
            found.push(DiscoveredDevice {
                name: model,
                slug,
                dev_node: format!("/dev/{name}"),
                protocol: DeviceProtocol::GenericNiHid,
                connected: true,
            });
        }
    }

    found.sort_by(|a, b| a.dev_node.cmp(&b.dev_node));
    found
}

fn slugify(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

/// Hot-plug background manager that polls ALSA raw MIDI and generic NI HID devices.
pub struct GenericControllerManager {
    event_rx: Receiver<SurfaceEvent>,
    stop_signal: Arc<AtomicBool>,
    _worker_thread: Option<JoinHandle<()>>,
    pub discovered: Arc<Mutex<Vec<DiscoveredDevice>>>,
    pub active_mapping: GenericControllerMapping,
    pub active_device_slug: Option<String>,
    pub learn_mode: bool,
    pub selected_control_id: Option<String>,
}

impl Default for GenericControllerManager {
    fn default() -> Self {
        Self::new()
    }
}

impl GenericControllerManager {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        let stop_signal = Arc::new(AtomicBool::new(false));
        let discovered = Arc::new(Mutex::new(Vec::new()));

        let stop_clone = Arc::clone(&stop_signal);
        let disc_clone = Arc::clone(&discovered);

        let worker = spawn(move || {
            run_hardware_worker(tx, stop_clone, disc_clone);
        });

        Self {
            event_rx: rx,
            stop_signal,
            _worker_thread: Some(worker),
            discovered,
            active_mapping: GenericControllerMapping::default(),
            active_device_slug: None,
            learn_mode: false,
            selected_control_id: None,
        }
    }

    /// Selects an active device and loads its mapping (or factory preset if FLX4).
    pub fn select_device(&mut self, slug: &str, name: &str) {
        self.active_device_slug = Some(slug.to_string());
        if let Some(loaded) = GenericControllerMapping::load(slug) {
            self.active_mapping = loaded;
        } else if slug.contains("flx4") || slug.contains("ddj") {
            self.active_mapping = flx4_default_mapping();
            let _ = self.active_mapping.save(slug);
        } else {
            self.active_mapping = GenericControllerMapping::new(name);
        }
    }

    /// Drains queued surface events from the background thread.
    pub fn poll_events(&mut self) -> Vec<SurfaceEvent> {
        let mut events = Vec::new();
        while let Ok(ev) = self.event_rx.try_recv() {
            // If in learn mode, catch the touched control and select/add it
            if self.learn_mode {
                let touched_id = match &ev {
                    SurfaceEvent::Button { control, pressed: true } => Some(control.clone()),
                    SurfaceEvent::Fader { control, .. } => Some(control.clone()),
                    SurfaceEvent::Encoder { control, .. } => Some(control.clone()),
                    _ => None,
                };
                if let Some(cid) = touched_id {
                    self.selected_control_id = Some(cid.clone());
                    if !self.active_mapping.entries.iter().any(|e| e.control_id == cid) {
                        self.active_mapping.entries.push(MappingEntry {
                            control_id: cid.clone(),
                            friendly_name: format_friendly_control_id(&cid),
                            action: "none".to_string(),
                        });
                    }
                    self.learn_mode = false;
                }
            }
            events.push(ev);
        }
        events
    }
}

impl Drop for GenericControllerManager {
    fn drop(&mut self) {
        self.stop_signal.store(true, Ordering::Relaxed);
    }
}

/// Worker loop running on a dedicated thread, rescanning every 2 seconds and reading non-blocking.
fn run_hardware_worker(
    tx: Sender<SurfaceEvent>,
    stop: Arc<AtomicBool>,
    discovered_out: Arc<Mutex<Vec<DiscoveredDevice>>>,
) {
    let mut midi_parsers: HashMap<String, (MidiParserState, std::fs::File)> = HashMap::new();
    let mut hid_decoders: HashMap<String, (u16, Vec<u8>, HidAnalogTracker, std::fs::File)> = HashMap::new();
    let mut last_scan = std::time::Instant::now();

    while !stop.load(Ordering::Relaxed) {
        // 1. Rescan device nodes every 2 seconds
        if last_scan.elapsed() >= Duration::from_secs(2) || (midi_parsers.is_empty() && hid_decoders.is_empty()) {
            last_scan = std::time::Instant::now();

            let midi_devs = scan_linux_alsa_midi_devices();
            let hid_devs = scan_linux_generic_ni_hid_devices();

            let mut all_devs = midi_devs.clone();
            all_devs.extend(hid_devs.clone());

            if let Ok(mut lock) = discovered_out.lock() {
                *lock = all_devs;
            }

            // Open new MIDI devices non-blocking
            for dev in midi_devs {
                if !midi_parsers.contains_key(&dev.dev_node) {
                    let mut opts = OpenOptions::new();
                    opts.read(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        opts.custom_flags(0x0800); // O_NONBLOCK
                    }
                    if let Ok(file) = opts.open(&dev.dev_node) {
                        midi_parsers.insert(dev.dev_node, (MidiParserState::new(), file));
                    }
                }
            }

            // Open new Generic NI HID devices read-only non-blocking
            for dev in hid_devs {
                if !hid_decoders.contains_key(&dev.dev_node) {
                    let mut opts = OpenOptions::new();
                    opts.read(true); // STRICTLY READ-ONLY: do not send output reports
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        opts.custom_flags(0x0800); // O_NONBLOCK
                    }
                    if let Ok(file) = opts.open(&dev.dev_node) {
                        // Extract PID if known, default 0x2200 (X1 MK3)
                        let pid = lookup_ni_device_pid(&dev.name).unwrap_or(0x2200);
                        hid_decoders.insert(
                            dev.dev_node,
                            (pid, Vec::new(), HidAnalogTracker::default(), file),
                        );
                    }
                }
            }
        }

        // 2. Poll open MIDI devices
        let mut midi_dead = Vec::new();
        for (node, (state, file)) in midi_parsers.iter_mut() {
            let mut buf = [0u8; 256];
            match file.read(&mut buf) {
                Ok(0) => {
                    // Device disconnected
                    midi_dead.push(node.clone());
                }
                Ok(n) => {
                    let slug = node
                        .strip_prefix("/dev/snd/")
                        .unwrap_or("midi")
                        .to_lowercase();
                    let evs = parse_midi_bytes(state, &slug, &buf[..n]);
                    for ev in evs {
                        let _ = tx.send(ev);
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::BrokenPipe || e.kind() == std::io::ErrorKind::NotConnected => {
                    midi_dead.push(node.clone());
                }
                Err(_) => {}
            }
        }
        for node in midi_dead {
            midi_parsers.remove(&node);
        }

        // 3. Poll open generic NI HID devices
        let mut hid_dead = Vec::new();
        for (node, (pid, prev, tracker, file)) in hid_decoders.iter_mut() {
            let mut buf = [0u8; 256];
            match file.read(&mut buf) {
                Ok(0) => {
                    hid_dead.push(node.clone());
                }
                Ok(n) => {
                    let report = &buf[..n];
                    if !prev.is_empty() {
                        let evs = diff_hid_reports(*pid, prev, report, tracker);
                        for ev in evs {
                            let _ = tx.send(ev);
                        }
                    }
                    *prev = report.to_vec();
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(ref e) if e.kind() == std::io::ErrorKind::BrokenPipe || e.kind() == std::io::ErrorKind::NotConnected => {
                    hid_dead.push(node.clone());
                }
                Err(_) => {}
            }
        }
        for node in hid_dead {
            hid_decoders.remove(&node);
        }

        // Sleep 5ms to avoid spinning
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn lookup_ni_device_pid(name: &str) -> Option<u16> {
    for dev in retraktor_hid::NI_DEVICES.iter() {
        if name.contains(dev.model) {
            return Some(dev.pid);
        }
    }
    None
}

// ============================================================================
// 6. Generic Mapper UI for Preferences
// ============================================================================

/// Renders the "Other controllers" Preferences page.
pub fn show_other_controllers_page(
    ui: &mut Ui,
    mgr: &mut GenericControllerManager,
    last_touched_ctl: Option<&str>,
) {
    ui.add_space(4.0);

    // 1. Experimental Notice Banner
    egui::Frame::none()
        .fill(TraktorTheme::BG_PANEL())
        .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT()))
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(12.0, 8.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("ℹ Experimental: learn mode, no lights or screens")
                        .size(12.5)
                        .color(TraktorTheme::TEXT_PRIMARY())
                        .strong(),
                );
            });
            ui.label(
                RichText::new("Generic MIDI controllers and non-MX2 Native Instruments HID hardware (X1 MK3, Z1 MK2, S4 MK3) are supported via read-only input learning.")
                    .size(11.0)
                    .color(TraktorTheme::TEXT_SECONDARY()),
            );
        });

    ui.add_space(8.0);

    // 2. Hardware Devices List
    let devices = mgr.discovered.lock().map(|d| d.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Connected hardware").size(14.0).strong());
        if devices.is_empty() {
            ui.label(
                RichText::new("— No generic MIDI or NI HID controllers detected")
                    .size(12.0)
                    .color(TraktorTheme::TEXT_MUTED()),
            );
        }
    });

    if !devices.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
            for d in &devices {
                let is_selected = mgr.active_device_slug.as_deref() == Some(&d.slug);
                let (lamp_col, tip) = if d.connected {
                    (TraktorTheme::PLAY_ACTIVE(), format!("{} connected at {}", d.name, d.dev_node))
                } else {
                    (TraktorTheme::TEXT_MUTED(), format!("{} disconnected", d.name))
                };

                let btn_text = format!("● {}", d.name);
                let btn = egui::Button::new(RichText::new(btn_text).color(if is_selected { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_SECONDARY() }))
                    .fill(if is_selected { TraktorTheme::ROW_SELECTED() } else { TraktorTheme::BG_PANEL() })
                    .min_size(Vec2::new(130.0, 26.0));

                let resp = ui.add(btn).on_hover_text(tip);
                ui.painter().circle_filled(resp.rect.left_center() + Vec2::new(8.0, 0.0), 3.0, lamp_col);

                if resp.clicked() {
                    mgr.select_device(&d.slug, &d.name);
                }
            }
        });
    }

    ui.add_space(10.0);

    // 3. Active Controller Mappings & Learn Section
    let active_name = if let Some(ref slug) = mgr.active_device_slug {
        slug.clone()
    } else {
        // Default to DDJ-FLX4 preset view if no device selected yet
        "ddj_flx4".to_string()
    };

    if mgr.active_mapping.entries.is_empty() && (active_name.contains("flx4") || active_name.contains("ddj")) {
        mgr.active_mapping = flx4_default_mapping();
    }

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Mapping: {}", mgr.active_mapping.device_name))
                .size(14.0)
                .strong(),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Save Button
            if ui
                .add(
                    egui::Button::new(RichText::new("Save mapping").strong())
                        .fill(TraktorTheme::ROW_SELECTED())
                        .min_size(Vec2::new(100.0, 26.0)),
                )
                .clicked()
            {
                let _ = mgr.active_mapping.save(&active_name);
            }

            // Reset to factory DDJ-FLX4 preset button if applicable
            if (active_name.contains("flx4") || active_name.contains("ddj"))
                && ui
                    .add(
                        egui::Button::new("Load FLX4 default")
                            .fill(TraktorTheme::BG_PANEL())
                            .min_size(Vec2::new(120.0, 26.0)),
                    )
                    .clicked()
            {
                mgr.active_mapping = flx4_default_mapping();
                let _ = mgr.active_mapping.save(&active_name);
            }

            // LEARN Button
            let learn_btn_label = if mgr.learn_mode {
                "● WAITING FOR INPUT…"
            } else {
                "● LEARN"
            };
            let learn_btn_col = if mgr.learn_mode {
                TraktorTheme::METER_RED()
            } else {
                TraktorTheme::ACCENT()
            };
            let learn_btn = egui::Button::new(
                RichText::new(learn_btn_label)
                    .color(TraktorTheme::TEXT_PRIMARY())
                    .strong(),
            )
            .fill(if mgr.learn_mode { TraktorTheme::ROW_SELECTED() } else { TraktorTheme::BG_PANEL() })
            .stroke(Stroke::new(1.2_f32, learn_btn_col))
            .min_size(Vec2::new(130.0, 26.0));

            if ui
                .add(learn_btn)
                .on_hover_text("Click Learn, then touch or turn any control on your hardware")
                .clicked()
            {
                mgr.learn_mode = !mgr.learn_mode;
            }
        });
    });

    if let Some(t) = last_touched_ctl {
        ui.label(
            RichText::new(format!("Last hardware event: {t}"))
                .size(11.0)
                .color(TraktorTheme::TEXT_MUTED()),
        );
    }

    ui.add_space(6.0);

    // 4. Mapped Controls Table
    let mut remove_idx = None;
    let mut changed = false;

    egui::Frame::none()
        .fill(TraktorTheme::BG_PANEL())
        .rounding(4.0)
        .inner_margin(8.0)
        .show(ui, |ui| {
            if mgr.active_mapping.entries.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new("No controls mapped yet. Click LEARN and touch a control on your controller.")
                            .size(12.5)
                            .color(TraktorTheme::TEXT_SECONDARY()),
                    );
                    ui.add_space(16.0);
                });
                return;
            }

            // Header Row
            ui.horizontal(|ui| {
                // Same widths as the row cells below.
                ui.allocate_ui(Vec2::new(195.0, 16.0), |ui| {
                    ui.set_width(195.0);
                    ui.label(RichText::new("CONTROL ID").size(11.0).color(TraktorTheme::TEXT_MUTED()).monospace());
                });
                ui.allocate_ui(Vec2::new(170.0, 16.0), |ui| {
                    ui.set_width(170.0);
                    ui.label(RichText::new("LABEL").size(11.0).color(TraktorTheme::TEXT_MUTED()));
                });
                ui.label(RichText::new("MAPPED ACTION").size(11.0).color(TraktorTheme::TEXT_MUTED()));
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("generic_mapping_table_scroll")
                .max_height(ui.available_height().max(280.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (idx, entry) in mgr.active_mapping.entries.iter_mut().enumerate() {
                        let is_selected = mgr.selected_control_id.as_deref() == Some(&entry.control_id);

                        ui.horizontal(|ui| {
                            // Control ID
                            let id_text = RichText::new(&entry.control_id)
                                .font(egui::FontId::monospace(11.0))
                                .color(if is_selected { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_SECONDARY() });
                            ui.allocate_ui(Vec2::new(195.0, 24.0), |ui| {
                                ui.set_width(195.0);
                                ui.label(id_text);
                            });

                            // Editable Friendly Label
                            ui.allocate_ui(Vec2::new(170.0, 24.0), |ui| {
                                ui.set_width(170.0);
                                if ui.add(egui::TextEdit::singleline(&mut entry.friendly_name).desired_width(160.0)).changed() {
                                    changed = true;
                                }
                            });

                            // Action ComboBox
                            ui.allocate_ui(Vec2::new(200.0, 24.0), |ui| {
                                ui.set_width(200.0);
                                let cur_label = action_label(&entry.action);
                                egui::ComboBox::from_id_salt(("act_combo", idx))
                                    .selected_text(cur_label)
                                    .width(190.0)
                                    .show_ui(ui, |ui| {
                                        // Standard button actions
                                        ui.label(RichText::new("Transport & Pad Actions").size(10.5).color(TraktorTheme::TEXT_MUTED()));
                                        for d in 1..=4 {
                                            for (id, label, _) in BUTTON_ACTIONS {
                                                let concrete = id.replace("{d}", &d.to_string());
                                                if ui.selectable_label(entry.action == concrete, format!("{label} Deck {d}")).clicked() {
                                                    entry.action = concrete;
                                                    changed = true;
                                                }
                                            }
                                        }

                                        ui.separator();
                                        ui.label(RichText::new("Fader & Knob Actions").size(10.5).color(TraktorTheme::TEXT_MUTED()));
                                        for d in 1..=4 {
                                            for (id, label, _) in CONTINUOUS_ACTIONS {
                                                let concrete = id.replace("{d}", &d.to_string());
                                                if ui.selectable_label(entry.action == concrete, format!("{label} Deck {d}")).clicked() {
                                                    entry.action = concrete;
                                                    changed = true;
                                                }
                                            }
                                        }

                                        ui.separator();
                                        // Extra Jog & Browse actions
                                        for d in 1..=2 {
                                            let jog_act = format!("jog:{d}");
                                            if ui.selectable_label(entry.action == jog_act, format!("Jogwheel Turn Deck {d}")).clicked() {
                                                entry.action = jog_act;
                                                changed = true;
                                            }
                                            let touch_act = format!("jog_touch:{d}");
                                            if ui.selectable_label(entry.action == touch_act, format!("Jogwheel Touch Deck {d}")).clicked() {
                                                entry.action = touch_act;
                                                changed = true;
                                            }
                                        }
                                        if ui.selectable_label(entry.action == "browse", "Browse Library").clicked() {
                                            entry.action = "browse".to_string();
                                            changed = true;
                                        }
                                        if ui.selectable_label(entry.action == "none", "Do nothing").clicked() {
                                            entry.action = "none".to_string();
                                            changed = true;
                                        }
                                    });
                            });

                            // Per-row Clear Button (hit target >= 24px)
                            let (clear_rect, clear_resp) = ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                            let clear_hover = clear_resp.hovered();
                            ui.painter().rect_filled(
                                clear_rect,
                                3.0,
                                if clear_hover { TraktorTheme::ROW_SELECTED() } else { TraktorTheme::BG_WINDOW() },
                            );
                            ui.painter().text(
                                clear_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                "x",
                                egui::FontId::monospace(12.0),
                                if clear_hover { TraktorTheme::METER_RED() } else { TraktorTheme::TEXT_MUTED() },
                            );
                            if clear_resp.on_hover_text("Clear this control mapping").clicked() {
                                remove_idx = Some(idx);
                                changed = true;
                            }
                        });
                        ui.add_space(2.0);
                    }
                });
        });

    if let Some(idx) = remove_idx {
        mgr.active_mapping.entries.remove(idx);
    }
    if changed {
        let _ = mgr.active_mapping.save(&active_name);
    }
}

// ============================================================================
// Unit Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_midi_parser_running_status() {
        let mut state = MidiParserState::new();
        // 0x90 (Note On ch1), note 60, vel 100, then running status: note 62, vel 100
        let bytes = [0x90, 60, 100, 62, 100];
        let evs = parse_midi_bytes(&mut state, "test", &bytes);
        assert_eq!(evs.len(), 2);
        assert_eq!(
            evs[0],
            SurfaceEvent::Button {
                control: "midi.test.ch1.note60".into(),
                pressed: true,
            }
        );
        assert_eq!(
            evs[1],
            SurfaceEvent::Button {
                control: "midi.test.ch1.note62".into(),
                pressed: true,
            }
        );
    }

    #[test]
    fn test_midi_parser_interleaved_realtime() {
        let mut state = MidiParserState::new();
        // Interleave timing clock (0xF8) and active sensing (0xFE) inside Note On bytes
        let bytes = [0x90, 0xF8, 60, 0xFE, 127];
        let evs = parse_midi_bytes(&mut state, "test", &bytes);
        assert_eq!(evs.len(), 1);
        assert_eq!(
            evs[0],
            SurfaceEvent::Button {
                control: "midi.test.ch1.note60".into(),
                pressed: true,
            }
        );
    }

    #[test]
    fn test_midi_parser_note_on_velocity_zero_is_off() {
        let mut state = MidiParserState::new();
        // Note On ch2 with velocity 0 is treated as Note Off
        let bytes = [0x91, 54, 0];
        let evs = parse_midi_bytes(&mut state, "test", &bytes);
        assert_eq!(evs.len(), 1);
        assert_eq!(
            evs[0],
            SurfaceEvent::Button {
                control: "midi.test.ch2.note54".into(),
                pressed: false,
            }
        );
    }

    #[test]
    fn test_midi_parser_14bit_cc_pair() {
        let mut state = MidiParserState::new();
        // Channel 1, CC 0 (MSB) = 64 (0x40), CC 32 (LSB) = 0 (0x00)
        // 14-bit value = (64 << 7) | 0 = 8192
        // Normalized = 8192 / 16383 ≈ 0.50003
        let bytes = [0xB0, 0, 64, 0xB0, 32, 0];
        let evs = parse_midi_bytes(&mut state, "test", &bytes);
        assert_eq!(evs.len(), 2); // 7-bit MSB fallback + 14-bit combined event
        match &evs[1] {
            SurfaceEvent::Fader { control, value } => {
                assert_eq!(control, "midi.test.ch1.cc0");
                assert!((*value - 0.5).abs() < 0.001);
            }
            _ => panic!("Expected Fader event for 14-bit CC pair"),
        }
    }

    #[test]
    fn test_midi_parser_sysex_skip() {
        let mut state = MidiParserState::new();
        // Sysex start (0xF0), junk payload, Sysex end (0xF7), followed by CC 19
        let bytes = [0xF0, 0x00, 0x20, 0x7F, 0xF7, 0xB0, 19, 127];
        let evs = parse_midi_bytes(&mut state, "test", &bytes);
        assert_eq!(evs.len(), 1);
        match &evs[0] {
            SurfaceEvent::Fader { control, value } => {
                assert_eq!(control, "midi.test.ch1.cc19");
                assert!((*value - 1.0).abs() < 0.001);
            }
            _ => panic!("Expected Fader event after sysex"),
        }
    }

    #[test]
    fn test_generic_ni_hid_diff_button_and_analog() {
        let mut tracker = HidAnalogTracker::default();
        let pid = 0x2200; // X1 MK3

        let prev = [0x01, 0x00, 0x00, 0x10, 0x00];
        // Flip bit 2 of byte 0 in payload (pressed) and smooth ADC change on 16-bit pair
        let curr = [0x01, 0x04, 0x00, 0x15, 0x00];

        let evs = diff_hid_reports(pid, &prev, &curr, &mut tracker);
        assert!(evs.iter().any(|e| matches!(
            e,
            SurfaceEvent::Button { control, pressed }
                if control == "hid.2200.r1.b0.2" && *pressed
        )));
        assert!(evs.iter().any(|e| matches!(
            e,
            SurfaceEvent::Fader { control, .. } if control == "hid.2200.r1.a2"
        )));
    }

    #[test]
    fn test_mapping_roundtrip_persistence() {
        let mut mapping = GenericControllerMapping::new("Test Controller");
        mapping.entries.push(MappingEntry {
            control_id: "midi.test.ch1.note11".into(),
            friendly_name: "Play / Pause".into(),
            action: "play:1".into(),
        });
        mapping.entries.push(MappingEntry {
            control_id: "midi.test.ch7.cc31".into(),
            friendly_name: "Crossfader".into(),
            action: "xfader".into(),
        });

        let serialized = mapping.to_text();
        let restored = GenericControllerMapping::parse("Test Controller", &serialized);
        assert_eq!(mapping, restored);
    }

    #[test]
    fn test_ddj_flx4_preset_covers_required_controls() {
        let preset = flx4_default_mapping();
        let cids: Vec<&str> = preset.entries.iter().map(|e| e.control_id.as_str()).collect();

        // Play, cue, sync
        assert!(cids.contains(&"midi.ddj_flx4.ch1.note11"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.note11"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.note12"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.note12"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.note88"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.note88"));

        // Jog touch & turn
        assert!(cids.contains(&"midi.ddj_flx4.ch1.note54"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc34"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.cc34"));

        // Tempo & channel faders & crossfader
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc0"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.cc0"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc19"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.cc19"));
        assert!(cids.contains(&"midi.ddj_flx4.ch7.cc31"));

        // EQ, Trim, Filter
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc7"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc11"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc15"));
        assert!(cids.contains(&"midi.ddj_flx4.ch1.cc4"));
        assert!(cids.contains(&"midi.ddj_flx4.ch7.cc23"));
        assert!(cids.contains(&"midi.ddj_flx4.ch7.cc24"));

        // Headphone cue
        assert!(cids.contains(&"midi.ddj_flx4.ch1.note84"));
        assert!(cids.contains(&"midi.ddj_flx4.ch2.note84"));

        // Hot cue pads 1-8
        for p in 0..8 {
            assert!(cids.contains(&format!("midi.ddj_flx4.ch8.note{p}").as_str()));
            assert!(cids.contains(&format!("midi.ddj_flx4.ch10.note{p}").as_str()));
        }

        // Browse encoder + load A/B
        assert!(cids.contains(&"midi.ddj_flx4.ch7.cc64"));
        assert!(cids.contains(&"midi.ddj_flx4.ch7.note70"));
        assert!(cids.contains(&"midi.ddj_flx4.ch7.note71"));
    }
}
