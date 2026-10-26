//! # `retraktor-hid`
//!
//! Native Instruments Kontrol (`VID 0x17CC`) USB HID, Haptic Drive, & `OffscreenView` Display protocol layer:
//! - [`NI_DEVICES`]: Complete USB VID/PID registry for Kontrol S4 MK3, S3, S2 MK3, S8, S5, D2, X1 MK3, Z1 MK2, and F1.
//! - [`HidReportDecoder`]: Decodes Report `0x01` (Buttons, Encoders, Jog Wheels) and Report `0x02`
//!   (12-bit/16-bit ADC Faders/Pots) into [`retraktor_core::SurfaceEvent`]s.
//! - [`S4Mk3HapticPlatterConfig`]: Encodes Report `0x30`/`0x31` for S4 MK3 Haptic Drive motorized platters
//!   (`JogwheelMode`, platter speed `33.3`/`45` RPM, tension `0..=255`, haptic tick density `0..=2`, LED ring color).
//! - [`KontrolScreenFramebuffer`]: Renders 3-band [`WaveformStripe`] + playhead + hotcues into an offscreen
//!   RGB565 framebuffer (`480x272` for S8/S5/D2 `OffscreenView.qml`, `320x240` for S4 MK3) and packetizes
//!   dirty rectangles into USB Bulk frames (`0x8400` header).
//! - [`AlsaMidiTsiRouter`]: Routes raw 3-byte MIDI messages through a [`TsiFile`] into [`PropertyBus`].
//! - [`generate_linux_udev_rules`]: Generates `/etc/udev/rules.d/99-retraktor-ni.rules`.

use retraktor_core::{JogwheelMode, PropValue, PropertyBus, ScreenFlavor, SurfaceEvent, TraktorColor};
use retraktor_formats::{TsiFile, WaveformStripe};

/// Native Instruments GmbH USB Vendor ID.
pub const NI_USB_VID: u16 = 0x17CC;

/// Supported Native Instruments Kontrol hardware models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NiDeviceDescriptor {
    pub model: &'static str,
    pub vid: u16,
    pub pid: u16,
    pub csi_folder: &'static str,
    pub has_color_screen: bool,
    pub has_motorized_jog: bool,
}

/// Registry of Native Instruments Traktor Kontrol hardware USB IDs.
pub const NI_DEVICES: [NiDeviceDescriptor; 11] = [
    NiDeviceDescriptor {
        model: "Traktor MX2",
        vid: NI_USB_VID,
        pid: 0x2420,
        csi_folder: "MX2",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor MX4",
        vid: NI_USB_VID,
        pid: 0x2430,
        csi_folder: "MX4",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S4 MK3",
        vid: NI_USB_VID,
        pid: 0x1720,
        csi_folder: "S4MK3",
        has_color_screen: true,
        has_motorized_jog: true,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S3",
        vid: NI_USB_VID,
        pid: 0x1900,
        csi_folder: "S3",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S2 MK3",
        vid: NI_USB_VID,
        pid: 0x1730,
        csi_folder: "S2MK3",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S8",
        vid: NI_USB_VID,
        pid: 0x1370,
        csi_folder: "S8",
        has_color_screen: true,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S5",
        vid: NI_USB_VID,
        pid: 0x1420,
        csi_folder: "S5",
        has_color_screen: true,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol D2",
        vid: NI_USB_VID,
        pid: 0x1400,
        csi_folder: "D2",
        has_color_screen: true,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol X1 MK3",
        vid: NI_USB_VID,
        pid: 0x2200,
        csi_folder: "X1MK3",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol Z1 MK2",
        vid: NI_USB_VID,
        pid: 0x2400,
        csi_folder: "Z1MK2",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol F1",
        vid: NI_USB_VID,
        pid: 0x1120,
        csi_folder: "F1",
        has_color_screen: false,
        has_motorized_jog: false,
    },
];

/// Looks up a Native Instruments controller descriptor by `(vid, pid)`.
pub fn lookup_ni_device(vid: u16, pid: u16) -> Option<&'static NiDeviceDescriptor> {
    NI_DEVICES.iter().find(|d| d.vid == vid && d.pid == pid)
}

/// Stateful NI HID input report decoder that detects button edges and ADC fader deltas.
#[derive(Debug, Clone)]
pub struct HidReportDecoder {
    pub surface_prefix: String,
    prev_buttons: Vec<u8>,
    prev_adc: Vec<u16>,
}

impl HidReportDecoder {
    pub fn new(surface_prefix: impl Into<String>, button_bytes: usize, adc_channels: usize) -> Self {
        Self {
            surface_prefix: surface_prefix.into(),
            prev_buttons: vec![0; button_bytes],
            prev_adc: vec![0; adc_channels],
        }
    }

    /// Decodes a raw NI HID input report (`0x01` buttons or `0x02` 12-bit Little-Endian ADCs)
    /// into [`SurfaceEvent`]s.
    pub fn decode_report(&mut self, report: &[u8]) -> Vec<SurfaceEvent> {
        let mut events = Vec::new();
        if report.is_empty() {
            return events;
        }
        match report[0] {
            0x01 => {
                let payload = &report[1..];
                for (byte_idx, &b) in payload.iter().enumerate() {
                    let prev = self.prev_buttons.get(byte_idx).copied().unwrap_or(0);
                    let diff = b ^ prev;
                    if diff != 0 {
                        for bit in 0..8 {
                            if (diff & (1 << bit)) != 0 {
                                let pressed = (b & (1 << bit)) != 0;
                                let btn_id = byte_idx * 8 + bit + 1;
                                events.push(SurfaceEvent::Button {
                                    control: format!("{}.buttons.{btn_id}", self.surface_prefix),
                                    pressed,
                                });
                            }
                        }
                        if byte_idx < self.prev_buttons.len() {
                            self.prev_buttons[byte_idx] = b;
                        }
                    }
                }
            }
            0x02 => {
                let payload = &report[1..];
                for (ch_idx, pair) in payload.chunks_exact(2).enumerate() {
                    let raw = u16::from_le_bytes([pair[0], pair[1]]) & 0x0FFF;
                    let prev = self.prev_adc.get(ch_idx).copied().unwrap_or(u16::MAX);
                    if raw != prev {
                        let norm = (raw as f64) / 4095.0;
                        events.push(SurfaceEvent::Fader {
                            control: format!("{}.faders.{}", self.surface_prefix, ch_idx + 1),
                            value: norm,
                        });
                        if ch_idx < self.prev_adc.len() {
                            self.prev_adc[ch_idx] = raw;
                        }
                    }
                }
            }
            _ => {}
        }
        events
    }
}

/// S4 MK3 Haptic Drive Motorized Platter Report (`0x30` / `0x31`) matching `CSI/S4MK3/S4MK3.qml` & `S4MK3Deck.qml`.
#[derive(Debug, Clone, PartialEq)]
pub struct S4Mk3HapticPlatterConfig {
    pub deck_side: u8,
    pub mode: JogwheelMode,
    pub motor_on: bool,
    pub platter_speed_45rpm: bool,
    pub tension: u8,
    pub ticks_density: u8,
    pub tempo_ratio: f32,
    pub led_color: TraktorColor,
}

impl Default for S4Mk3HapticPlatterConfig {
    fn default() -> Self {
        Self {
            deck_side: 0,
            mode: JogwheelMode::Jogwheel,
            motor_on: false,
            platter_speed_45rpm: false,
            tension: 50,
            ticks_density: 0,
            tempo_ratio: 1.0,
            led_color: TraktorColor::Blue,
        }
    }
}

impl S4Mk3HapticPlatterConfig {
    /// Packs the S4 MK3 Haptic Drive configuration into a 16-byte HID Output Report (`0x30`).
    pub fn to_hid_report(&self) -> [u8; 16] {
        let mut r = [0u8; 16];
        r[0] = 0x30;
        r[1] = self.deck_side;
        r[2] = self.mode as u8;
        r[3] = u8::from(self.motor_on);
        r[4] = u8::from(self.platter_speed_45rpm);
        r[5] = self.tension;
        r[6] = self.ticks_density.min(2);
        let rpm_base: f32 = if self.platter_speed_45rpm { 45.0 } else { 33.3333 };
        let effective_rpm = rpm_base * self.tempo_ratio;
        let rpm_bytes = effective_rpm.to_le_bytes();
        r[7..11].copy_from_slice(&rpm_bytes);
        let (red, green, blue) = self.led_color.to_rgb8();
        r[11] = red >> 1;
        r[12] = green >> 1;
        r[13] = blue >> 1;
        r[14] = self.led_color as u8;
        r
    }
}

/// Offscreen RGB565 framebuffer matching `Resources/qml/Screens/OffscreenView.qml`
/// (`480x272` for S8/S5/D2, `320x240` for S4 MK3) with USB Bulk packet framing (`0x8400`).
#[derive(Debug, Clone)]
pub struct KontrolScreenFramebuffer {
    pub flavor: ScreenFlavor,
    pub screen_index: u8,
    pub width: u16,
    pub height: u16,
    pub pixels_rgb565: Vec<u16>,
}

impl KontrolScreenFramebuffer {
    pub fn new(flavor: ScreenFlavor, screen_index: u8) -> Self {
        let (width, height) = match flavor {
            ScreenFlavor::S8 | ScreenFlavor::D2 => (480, 272),
            ScreenFlavor::S4Mk3 => (320, 240),
            ScreenFlavor::X1Mk3Mode | ScreenFlavor::Z1Mk2Main => (128, 64),
        };
        Self {
            flavor,
            screen_index,
            width,
            height,
            pixels_rgb565: vec![0; (width as usize) * (height as usize)],
        }
    }

    #[inline]
    pub fn rgb8_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
        let r5 = ((r as u16) >> 3) & 0x1F;
        let g6 = ((g as u16) >> 2) & 0x3F;
        let b5 = ((b as u16) >> 3) & 0x1F;
        (r5 << 11) | (g6 << 5) | b5
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, rgb565: u16) {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            self.pixels_rgb565[idx] = rgb565;
        }
    }

    /// Renders a Deck HUD + 3-band [`WaveformStripe`] + Playhead cursor into the offscreen framebuffer.
    pub fn render_deck_waveform(
        &mut self,
        deck_idx: usize,
        stripe: &WaveformStripe,
        playhead_norm: f32,
    ) {
        // Clear background to deep charcoal
        let bg = Self::rgb8_to_rgb565(12, 14, 18);
        self.pixels_rgb565.fill(bg);

        // Top deck header bar (Blue for Decks 1/2, Orange for Decks 3/4)
        let header_color = if deck_idx <= 2 {
            TraktorColor::Blue
        } else {
            TraktorColor::LightOrange
        };
        let (hr, hg, hb) = header_color.to_rgb8();
        let header_px = Self::rgb8_to_rgb565(hr, hg, hb);
        let header_h = (self.height / 10).max(4);
        for y in 0..header_h {
            for x in 0..self.width {
                self.set_pixel(x, y, header_px);
            }
        }

        // Render waveform columns centered vertically
        if !stripe.columns.is_empty() {
            let center_y = (self.height / 2) as i32;
            let max_half_h = ((self.height as i32) * 3 / 8).max(8);
            for x in 0..self.width {
                let col_idx =
                    ((x as usize) * stripe.columns.len()) / (self.width as usize).max(1);
                let col = stripe.columns[col_idx.min(stripe.columns.len() - 1)];
                let amp = (col.low as u16 + col.mid as u16 + col.high as u16) / 3;
                let bar_h = ((amp as i32) * max_half_h) / 255;
                let wf_px = Self::rgb8_to_rgb565(col.low, col.mid, col.high);
                for dy in -bar_h..=bar_h {
                    let y = (center_y + dy).clamp(header_h as i32, (self.height - 1) as i32) as u16;
                    self.set_pixel(x, y, wf_px);
                }
            }
        }

        // Draw playhead vertical red/white cursor
        let play_x = ((playhead_norm.clamp(0.0, 1.0) * (self.width.saturating_sub(1) as f32))
            .round() as u16)
            .min(self.width.saturating_sub(1));
        let cursor_px = Self::rgb8_to_rgb565(255, 255, 255);
        for y in header_h..self.height {
            self.set_pixel(play_x, y, cursor_px);
        }
    }

    /// Encodes a rectangular region `(x, y, w, h)` of the framebuffer into a NI USB Bulk Display
    /// packet (`[0x84, 0x00, screen_index, 0x00, x_be, y_be, w_be, h_be, ...rgb565_be_payload]`).
    pub fn encode_usb_bulk_rect(&self, x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
        let w_clamped = w.min(self.width.saturating_sub(x));
        let h_clamped = h.min(self.height.saturating_sub(y));
        let pixel_count = (w_clamped as usize) * (h_clamped as usize);
        let mut pkt = Vec::with_capacity(12 + pixel_count * 2);
        pkt.extend_from_slice(&[0x84, 0x00, self.screen_index, 0x00]);
        pkt.extend_from_slice(&x.to_be_bytes());
        pkt.extend_from_slice(&y.to_be_bytes());
        pkt.extend_from_slice(&w_clamped.to_be_bytes());
        pkt.extend_from_slice(&h_clamped.to_be_bytes());

        for row in y..(y + h_clamped) {
            let row_start = (row as usize) * (self.width as usize) + (x as usize);
            for &px in &self.pixels_rgb565[row_start..row_start + (w_clamped as usize)] {
                pkt.extend_from_slice(&px.to_be_bytes());
            }
        }
        pkt
    }
}

/// Evaluates raw 3-byte ALSA MIDI messages against a loaded `.tsi` [`TsiFile`] and updates [`PropertyBus`].
pub struct AlsaMidiTsiRouter;

impl AlsaMidiTsiRouter {
    /// Routes a 3-byte MIDI message (`status`, `data1`, `data2`) through `tsi` into `bus`,
    /// returning the number of property updates applied.
    pub fn route_midi_message(midi: [u8; 3], tsi: &TsiFile, bus: &mut PropertyBus) -> usize {
        let msg_type = midi[0] & 0xF0;
        let binding_id = midi[1] as u32;
        let val_norm = (midi[2] as f64) / 127.0;
        let mut updates = 0;

        for dev in &tsi.devices {
            for m in &dev.mappings {
                if m.is_output_led || m.midi_binding_id != binding_id {
                    continue;
                }
                let deck = m.target_assignment.clamp(1, 4);
                match (m.traktor_control_id, msg_type) {
                    // 102 = Deck Play/Pause
                    (102, 0x90) if midi[2] > 0 => {
                        let path = format!("app.traktor.decks.{deck}.play");
                        let cur = bus.get(&path).and_then(|v| v.as_bool()).unwrap_or(false);
                        if bus.set(&path, PropValue::Bool(!cur)) {
                            updates += 1;
                        }
                    }
                    // 201 = Channel Volume CC
                    (201, 0xB0) => {
                        let path = format!("app.traktor.mixer.channels.{deck}.volume");
                        if bus.set(&path, PropValue::Float(val_norm)) {
                            updates += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        updates
    }
}

/// Generates the Linux `udev` rules file (`/etc/udev/rules.d/99-retraktor-ni.rules`)
/// granting unprivileged user-space access (`TAG+="uaccess"`) to all NI Kontrol HID & USB display endpoints.
pub fn generate_linux_udev_rules() -> String {
    let mut rules = String::new();
    rules.push_str("# /etc/udev/rules.d/99-retraktor-ni.rules\n");
    rules.push_str("# Unprivileged hidraw and USB bulk display access for Native Instruments Traktor controllers\n\n");
    for dev in &NI_DEVICES {
        rules.push_str(&format!("# {}\n", dev.model));
        rules.push_str(&format!(
            "SUBSYSTEM==\"hidraw\", ATTRS{{idVendor}}==\"{:04x}\", ATTRS{{idProduct}}==\"{:04x}\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n",
            dev.vid, dev.pid
        ));
        rules.push_str(&format!(
            "SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{:04x}\", ATTR{{idProduct}}==\"{:04x}\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n\n",
            dev.vid, dev.pid
        ));
    }
    // Also include DFU PID 0x0400 for Traktor MX2 / MX4 / Z1 MK2 firmware endpoints
    rules.push_str("# Traktor MX2 / MX4 / Z1 MK2 DFU Mode\n");
    rules.push_str("SUBSYSTEM==\"usb\", ATTR{idVendor}==\"17cc\", ATTR{idProduct}==\"0400\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n");
    rules
}

/// USB Product ID for the Native Instruments Traktor MX2 (`NI::NHL2::USBDeviceDB::TraktorMX2`).
pub const TRAKTOR_MX2_PID: u16 = 0x2420;
/// USB Product ID for the Native Instruments Traktor MX4 (`NI::NHL2::USBDeviceDB::TraktorMX4`).
pub const TRAKTOR_MX4_PID: u16 = 0x2430;
/// Jogwheel resolution for Traktor MX2 / MX4 (`0x3C0 = 960` ticks per revolution).
pub const TRAKTOR_MX2_JOG_TICKS_PER_REV: i32 = 960;

/// Exact 82 CSI button names (`0..=81`) registered by `csi::communication::nhl::MX2PortDescriptor`.
pub const MX2_BUTTON_NAMES: [&str; 82] = [
    // Left Deck FX buttons (0..=3)
    "surface.left.fx.buttons.1",
    "surface.left.fx.buttons.2",
    "surface.left.fx.buttons.3",
    "surface.left.fx.buttons.4",
    // Left Deck Browse buttons (4..=7)
    "surface.left.browse.favorite",
    "surface.left.browse.add_to_list",
    "surface.left.browse.preview",
    "surface.left.browse.view",
    // Left Deck Transport & Jog Mode (8..=15)
    "surface.left.flux",
    "surface.left.reverse",
    "surface.left.mode.turntable",
    "surface.left.mode.jogwheel",
    "surface.left.shift",
    "surface.left.sync",
    "surface.left.master",
    "surface.left.key_lock",
    // Left Deck Pad Mode Selectors (16..=19)
    "surface.left.hotcues",
    "surface.left.stems",
    "surface.left.pattern",
    "surface.left.flux_loop",
    // Left Deck 8 RGB Performance Pads (20..=27)
    "surface.left.pads.1",
    "surface.left.pads.2",
    "surface.left.pads.3",
    "surface.left.pads.4",
    "surface.left.pads.5",
    "surface.left.pads.6",
    "surface.left.pads.7",
    "surface.left.pads.8",
    // Left Deck Cue & Play (28..=29)
    "surface.left.cue",
    "surface.left.play",
    // Right Deck FX buttons (30..=33)
    "surface.right.fx.buttons.1",
    "surface.right.fx.buttons.2",
    "surface.right.fx.buttons.3",
    "surface.right.fx.buttons.4",
    // Right Deck Browse buttons (34..=37)
    "surface.right.browse.favorite",
    "surface.right.browse.add_to_list",
    "surface.right.browse.preview",
    "surface.right.browse.view",
    // Right Deck Transport & Jog Mode (38..=45)
    "surface.right.flux",
    "surface.right.reverse",
    "surface.right.mode.turntable",
    "surface.right.mode.jogwheel",
    "surface.right.shift",
    "surface.right.sync",
    "surface.right.master",
    "surface.right.key_lock",
    // Right Deck Pad Mode Selectors (46..=49)
    "surface.right.hotcues",
    "surface.right.stems",
    "surface.right.pattern",
    "surface.right.flux_loop",
    // Right Deck 8 RGB Performance Pads (50..=57)
    "surface.right.pads.1",
    "surface.right.pads.2",
    "surface.right.pads.3",
    "surface.right.pads.4",
    "surface.right.pads.5",
    "surface.right.pads.6",
    "surface.right.pads.7",
    "surface.right.pads.8",
    // Right Deck Cue & Play (58..=59)
    "surface.right.cue",
    "surface.right.play",
    // Mixer Channel 1 & 2 buttons (60..=67)
    "surface.mixer.channels.1.deck_fx_assign.1",
    "surface.mixer.channels.1.deck_fx_assign.2",
    "surface.mixer.channels.1.channel_fx.on",
    "surface.mixer.channels.1.cue",
    "surface.mixer.channels.2.deck_fx_assign.1",
    "surface.mixer.channels.2.deck_fx_assign.2",
    "surface.mixer.channels.2.channel_fx.on",
    "surface.mixer.channels.2.cue",
    // Mixer FX Selector & Mic buttons (68..=73)
    "surface.mixer.channel_fx.fx1",
    "surface.mixer.channel_fx.fx2",
    "surface.mixer.channel_fx.fx3",
    "surface.mixer.channel_fx.fx4",
    "surface.mixer.channel_fx.filter",
    "surface.mixer.mic",
    // Push-Encoder switches (74..=79)
    "surface.left.browse.encoder.push",
    "surface.left.loop_move.push",
    "surface.left.loop_size.push",
    "surface.right.browse.encoder.push",
    "surface.right.loop_move.push",
    "surface.right.loop_size.push",
    // Capacitive Jogwheel Touch sensors (80..=81)
    "surface.left.jogwheel.touch",
    "surface.right.jogwheel.touch",
];

/// Exact 6 rotary encoder names (`0..=5`) in Input Report `0x01` bytes `11..=13` (6 nibbles).
pub const MX2_ENCODER_NAMES: [&str; 6] = [
    "surface.left.browse.encoder",
    "surface.left.loop_move",
    "surface.left.loop_size",
    "surface.right.browse.encoder",
    "surface.right.loop_move",
    "surface.right.loop_size",
];

/// Exact 26 12-bit analog pot/fader names (`0..=25`) in Input Report `0x02` (`I-2 W1A`).
pub const MX2_POT_FADER_NAMES: [&str; 26] = [
    // Left FX Knobs 1..4 (0..=3)
    "surface.left.fx.knobs.1",
    "surface.left.fx.knobs.2",
    "surface.left.fx.knobs.3",
    "surface.left.fx.knobs.4",
    // Right FX Knobs 1..4 (4..=7)
    "surface.right.fx.knobs.1",
    "surface.right.fx.knobs.2",
    "surface.right.fx.knobs.3",
    "surface.right.fx.knobs.4",
    // Mixer Channel 1 Knobs (8..=12)
    "surface.mixer.channels.1.gain",
    "surface.mixer.channels.1.eq.high",
    "surface.mixer.channels.1.eq.mid",
    "surface.mixer.channels.1.eq.low",
    "surface.mixer.channels.1.channel_fx.amount",
    // Mixer Channel 2 Knobs (13..=17)
    "surface.mixer.channels.2.gain",
    "surface.mixer.channels.2.eq.high",
    "surface.mixer.channels.2.eq.mid",
    "surface.mixer.channels.2.eq.low",
    "surface.mixer.channels.2.channel_fx.amount",
    // Master / Cue Knobs (18..=20)
    "surface.mixer.main.volume",
    "surface.mixer.cue_mix",
    "surface.mixer.cue_vol",
    // 5 Faders (21..=25)
    "surface.mixer.channels.1.volume",
    "surface.mixer.channels.2.volume",
    "surface.mixer.xfader",
    "surface.left.pitch.fader",
    "surface.right.pitch.fader",
];

/// Bottom-shell LED illumination state for a Traktor MX2 deck (`bottom.leds.1..6`),
/// matching `Resources/qml/CSI/MX2/MX2Deck.qml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mx2BottomShellState {
    /// Default ambient glow (`Blue` for Deck A, `Cyan` for Deck B).
    DefaultGlow,
    /// Active Loop engaged (`Green` on all 6 bottom shell LEDs).
    ActiveLoopGreen,
    /// Track End Warning (`Red` blinking on all 6 bottom shell LEDs).
    TrackEndWarningRed,
    /// Out-of-phase Sync Warning (`Red` solid).
    SyncWarningRed,
}

/// Complete NHL2 HID driver for the **Native Instruments Traktor MX2** (`VID 0x17CC, PID 0x2420`)
/// matching `NI::NHL2::Traktor::MX2::Device` and `CSI/MX2/MX2.qml`:
/// - Input Report `0x01` (`14` bytes): 83 buttons (`b58`) + 6 4-bit encoders (`n6` at byte offset `11`).
/// - Input Report `0x02` (`52` bytes): 21 12-bit pots + 5 12-bit faders (`W1A`, `0..=4095`).
/// - Input Report `0x03` (`19` bytes): 2 jogwheels (`960` ticks/rev at byte offset `3`: `(timestamp_u32, ticks_i32)`).
/// - Output Report `0x80` (`106` bytes): Report ID `0x80` + `105` LED bytes (`0..=73` backlit buttons/pads,
///   `74..=79` `left.bottom.leds.1..6`, `80..=85` `right.bottom.leds.1..6`, `86` `mixer.main.clip`,
///   `87..=95` Ch1 9-seg meter, `96..=104` Ch2 9-seg meter).
/// - Mode Report `0xA0` (`3` bytes): `[0xA0, 0x00, 0xFF]` enables Native NHL2 HID mode.
#[derive(Debug, Clone)]
pub struct Mx2HidDriver {
    prev_buttons: [u8; 11],
    prev_encoders: Option<[u8; 6]>,
    prev_adc: [u16; 26],
    adc_initialized: bool,
    prev_jog_ticks: [Option<i32>; 2],
    pub leds: [u8; 105],
    pub leds_dirty: bool,
}

impl Default for Mx2HidDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl Mx2HidDriver {
    pub fn new() -> Self {
        Self {
            prev_buttons: [0u8; 11],
            prev_encoders: None,
            prev_adc: [0u16; 26],
            adc_initialized: false,
            prev_jog_ticks: [None, None],
            // Matches `movi.16b v0, #0x7c` in `NI::NHL2::Traktor::MX2::Device::Device` (0x100d61114)
            leds: [0x7C; 105],
            leds_dirty: true,
        }
    }

    /// Returns the 3-byte NHL2 mode activation report (`[0xA0, 0x00, 0xFF]`) from
    /// `NI::NHL2::Traktor::MX2::Device::setDeviceMode('PAP ')` (`0x100d61604`).
    pub fn mode_enable_nhl_report() -> [u8; 3] {
        [0xA0, 0x00, 0xFF]
    }

    /// Returns the 3-byte MIDI fallback mode report (`[0xA0, 0xFF, 0x00]`) from
    /// `NI::NHL2::Traktor::MX2::Device::setDeviceMode('MIDI')` (`0x100d615a4`).
    pub fn mode_fallback_midi_report() -> [u8; 3] {
        [0xA0, 0xFF, 0x00]
    }

    /// Decodes any Traktor MX2 HID Input Report (`0x01`, `0x02`, or `0x03`) into [`SurfaceEvent`]s.
    pub fn decode_input_report(&mut self, report: &[u8]) -> Vec<SurfaceEvent> {
        let mut events = Vec::new();
        if report.is_empty() {
            return events;
        }
        match report[0] {
            // Report 0x01: 14 bytes (byte 0 = 0x01, bytes 1..=11 = 83 buttons, bytes 11..=13 = 6 4-bit encoders)
            0x01 if report.len() >= 14 => {
                for bit_idx in 0..MX2_BUTTON_NAMES.len() {
                    let byte_pos = 1 + (bit_idx / 8);
                    let bit_mask = 1u8 << (bit_idx % 8);
                    let cur = (report[byte_pos] & bit_mask) != 0;
                    let prev = (self.prev_buttons[bit_idx / 8] & bit_mask) != 0;
                    if cur != prev {
                        events.push(SurfaceEvent::Button {
                            control: MX2_BUTTON_NAMES[bit_idx].to_string(),
                            pressed: cur,
                        });
                    }
                }
                self.prev_buttons.copy_from_slice(&report[1..12]);

                // 6 4-bit nibbles at byte offset 11..=13 (`EncoderHandler` offset 0x0B)
                let nibbles = [
                    report[11] & 0x0F,
                    (report[11] >> 4) & 0x0F,
                    report[12] & 0x0F,
                    (report[12] >> 4) & 0x0F,
                    report[13] & 0x0F,
                    (report[13] >> 4) & 0x0F,
                ];
                if let Some(prev_n) = self.prev_encoders {
                    for i in 0..6 {
                        let raw_diff = (nibbles[i] as i8) - (prev_n[i] as i8);
                        let delta = if raw_diff > 7 {
                            raw_diff - 16
                        } else if raw_diff < -7 {
                            raw_diff + 16
                        } else {
                            raw_diff
                        };
                        if delta != 0 {
                            events.push(SurfaceEvent::Encoder {
                                control: MX2_ENCODER_NAMES[i].to_string(),
                                delta: delta as i64,
                            });
                        }
                    }
                }
                self.prev_encoders = Some(nibbles);
            }
            // Report 0x02: 52 bytes (byte 0 = 0x02, 26 Little-Endian 12-bit ADC words 0..=4095)
            0x02 if report.len() >= 53 || report.len() >= 52 => {
                let payload = &report[1..];
                for (ch_idx, name) in MX2_POT_FADER_NAMES.iter().enumerate() {
                    let off = ch_idx * 2;
                    if off + 2 > payload.len() {
                        break;
                    }
                    let raw = u16::from_le_bytes([payload[off], payload[off + 1]]) & 0x0FFF;
                    let changed = if !self.adc_initialized {
                        true
                    } else {
                        self.prev_adc[ch_idx].abs_diff(raw) >= 4
                    };
                    if changed {
                        self.prev_adc[ch_idx] = raw;
                        let norm = (raw as f64) / 4095.0;
                        events.push(SurfaceEvent::Fader {
                            control: (*name).to_string(),
                            value: norm,
                        });
                    }
                }
                self.adc_initialized = true;
            }
            // Report 0x03: 19 bytes (byte 0 = 0x03, byte offset 3 has two 8-byte `(timestamp_u32, ticks_i32)` structs)
            0x03 if report.len() >= 19 => {
                for wheel_idx in 0..2 {
                    let base = 3 + wheel_idx * 8;
                    let _timestamp = u32::from_le_bytes([
                        report[base],
                        report[base + 1],
                        report[base + 2],
                        report[base + 3],
                    ]);
                    let ticks = i32::from_le_bytes([
                        report[base + 4],
                        report[base + 5],
                        report[base + 6],
                        report[base + 7],
                    ]);
                    if let Some(prev) = self.prev_jog_ticks[wheel_idx] {
                        let mut delta = ticks.wrapping_sub(prev);
                        let half = TRAKTOR_MX2_JOG_TICKS_PER_REV / 2;
                        if delta > half {
                            delta -= TRAKTOR_MX2_JOG_TICKS_PER_REV;
                        } else if delta < -half {
                            delta += TRAKTOR_MX2_JOG_TICKS_PER_REV;
                        }
                        if delta != 0 {
                            let ctrl = if wheel_idx == 0 {
                                "surface.left.jogwheel.rotation"
                            } else {
                                "surface.right.jogwheel.rotation"
                            };
                            events.push(SurfaceEvent::Encoder {
                                control: ctrl.to_string(),
                                delta: delta as i64,
                            });
                        }
                    }
                    self.prev_jog_ticks[wheel_idx] = Some(ticks);
                }
            }
            _ => {}
        }
        events
    }

    /// Synchronizes the 105 LEDs (`0..=104`) from [`PropertyBus`] and builds the 106-byte
    /// Output Report `0x80` (`[0x80, ...105_led_bytes]`) matching `NI::NHL2::Traktor::MX2::Device::updateLEDs`.
    pub fn encode_led_report(
        &mut self,
        bus: &PropertyBus,
        left_shell: Mx2BottomShellState,
        right_shell: Mx2BottomShellState,
    ) -> [u8; 106] {
        let on_val = 0x7Fu8;
        let dim_val = 0x18u8;

        // Deck 1 (Left) transport & pad LEDs
        let d1_play = bus.get("app.traktor.decks.1.play").and_then(|v| v.as_bool()).unwrap_or(false);
        let d1_sync = bus.get("app.traktor.decks.1.sync.enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let d1_flux = bus.get("app.traktor.decks.1.flux.enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        self.leds[8] = if d1_flux { on_val } else { dim_val }; // left.flux
        self.leds[11] = on_val; // left.mode.jogwheel
        self.leds[13] = if d1_sync { on_val } else { dim_val }; // left.sync
        self.leds[16] = on_val; // left.hotcues
        for pad in 0..8 {
            self.leds[20 + pad] = on_val; // left.pads.1..8
        }
        self.leds[28] = on_val; // left.cue
        self.leds[29] = if d1_play { on_val } else { dim_val }; // left.play

        // Deck 2 (Right) transport & pad LEDs
        let d2_play = bus.get("app.traktor.decks.2.play").and_then(|v| v.as_bool()).unwrap_or(false);
        let d2_sync = bus.get("app.traktor.decks.2.sync.enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        let d2_flux = bus.get("app.traktor.decks.2.flux.enabled").and_then(|v| v.as_bool()).unwrap_or(false);
        self.leds[38] = if d2_flux { on_val } else { dim_val }; // right.flux
        self.leds[41] = on_val; // right.mode.jogwheel
        self.leds[43] = if d2_sync { on_val } else { dim_val }; // right.sync
        self.leds[46] = on_val; // right.hotcues
        for pad in 0..8 {
            self.leds[50 + pad] = on_val; // right.pads.1..8
        }
        self.leds[58] = on_val; // right.cue
        self.leds[59] = if d2_play { on_val } else { dim_val }; // right.play

        // Mixer channel 1 & 2 Cue & Filter/FX On LEDs
        let c1_cue = bus.get("app.traktor.mixer.channels.1.cue").and_then(|v| v.as_bool()).unwrap_or(false);
        let c1_fx = bus.get("app.traktor.mixer.channels.1.fx.on").and_then(|v| v.as_bool()).unwrap_or(true);
        let c2_cue = bus.get("app.traktor.mixer.channels.2.cue").and_then(|v| v.as_bool()).unwrap_or(false);
        let c2_fx = bus.get("app.traktor.mixer.channels.2.fx.on").and_then(|v| v.as_bool()).unwrap_or(true);
        self.leds[62] = if c1_fx { on_val } else { dim_val };
        self.leds[63] = if c1_cue { on_val } else { dim_val };
        self.leds[66] = if c2_fx { on_val } else { dim_val };
        self.leds[67] = if c2_cue { on_val } else { dim_val };
        self.leds[72] = on_val; // mixer.channel_fx.filter

        // Left & Right 6 Illuminated Bottom Shell LEDs (`74..=79` and `80..=85`)
        let map_shell = |state: Mx2BottomShellState, default_val: u8| -> u8 {
            match state {
                Mx2BottomShellState::DefaultGlow => default_val,
                Mx2BottomShellState::ActiveLoopGreen => 0x38, // Green palette index
                Mx2BottomShellState::TrackEndWarningRed | Mx2BottomShellState::SyncWarningRed => 0x04, // Red palette index
            }
        };
        let l_val = map_shell(left_shell, 0x48);
        let r_val = map_shell(right_shell, 0x44);
        for i in 74..=79 {
            self.leds[i] = l_val;
        }
        for i in 80..=85 {
            self.leds[i] = r_val;
        }

        // Main Clip LED (86)
        let clip_l = bus.get("app.traktor.mixer.master.level.clip.left").and_then(|v| v.as_bool()).unwrap_or(false);
        let clip_r = bus.get("app.traktor.mixer.master.level.clip.right").and_then(|v| v.as_bool()).unwrap_or(false);
        self.leds[86] = if clip_l || clip_r { on_val } else { 0 };

        // 9-segment Channel 1 Level Meter (`87..=95`) & Channel 2 Level Meter (`96..=104`)
        let ch1_lvl = bus
            .get("app.traktor.mixer.channels.1.level.prefader.linear.sum")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let ch2_lvl = bus
            .get("app.traktor.mixer.channels.2.level.prefader.linear.sum")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let ch1_segs = ((ch1_lvl.clamp(0.0, 1.0) * 9.0).round() as usize).min(9);
        let ch2_segs = ((ch2_lvl.clamp(0.0, 1.0) * 9.0).round() as usize).min(9);
        for s in 0..9 {
            self.leds[87 + s] = if s < ch1_segs { on_val } else { 0 };
            self.leds[96 + s] = if s < ch2_segs { on_val } else { 0 };
        }

        let mut report = [0u8; 106];
        report[0] = 0x80;
        report[1..106].copy_from_slice(&self.leds);
        self.leds_dirty = false;
        report
    }
}

/// Discovered Linux `/dev/hidraw*` Native Instruments controller endpoint.
#[derive(Debug, Clone)]
pub struct DetectedNiHidrawDevice {
    pub dev_node: String,
    pub descriptor: &'static NiDeviceDescriptor,
}

/// Scans `/sys/class/hidraw` on Linux for any connected Native Instruments controller (`VID 0x17CC`),
/// including **Traktor MX2** (`17CC:2420`), **Traktor MX4** (`17CC:2430`), S4 MK3, S3, S2 MK3, X1 MK3, Z1 MK2, etc.
pub fn scan_linux_ni_hidraw_devices() -> Vec<DetectedNiHidrawDevice> {
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
        for line in uevent.lines() {
            // Format: HID_ID=0003:000017CC:00002420
            if let Some(rest) = line.strip_prefix("HID_ID=") {
                let parts: Vec<&str> = rest.split(':').collect();
                if parts.len() == 3 {
                    let vid = u16::from_str_radix(parts[1].trim_start_matches('0'), 16).unwrap_or(0);
                    let pid = u16::from_str_radix(parts[2].trim_start_matches('0'), 16).unwrap_or(0);
                    if let Some(desc) = lookup_ni_device(vid, pid) {
                        found.push(DetectedNiHidrawDevice {
                            dev_node: format!("/dev/{name}"),
                            descriptor: desc,
                        });
                    }
                }
            }
        }
    }
    found.sort_by(|a, b| a.dev_node.cmp(&b.dev_node));
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use retraktor_formats::{StripeColumn, TsiControlMapping, TsiDevice};

    #[test]
    fn test_hid_haptic_screen_and_midi_tsi() {
        assert_eq!(
            lookup_ni_device(0x17CC, 0x1720).map(|d| d.model),
            Some("Traktor Kontrol S4 MK3")
        );

        // S4 MK3 Haptic Platter report
        let platter = S4Mk3HapticPlatterConfig {
            deck_side: 0,
            mode: JogwheelMode::Turntable,
            motor_on: true,
            platter_speed_45rpm: false,
            tension: 80,
            ticks_density: 1,
            tempo_ratio: 1.0,
            led_color: TraktorColor::Blue,
        };
        let report = platter.to_hid_report();
        assert_eq!(report[0], 0x30);
        assert_eq!(report[3], 1); // motor_on

        // OffscreenView 480x272 RGB565 waveform rendering & USB Bulk packetization
        let mut fb = KontrolScreenFramebuffer::new(ScreenFlavor::S8, 0);
        let stripe = WaveformStripe {
            header_tag: 0x06,
            version: 2,
            scale_factor: 3.5,
            block_size: 1024,
            block_count: 64,
            populated_columns: 64,
            duration_sec: 180.0,
            columns: vec![
                StripeColumn {
                    low: 200,
                    mid: 120,
                    high: 80,
                    total: 133,
                };
                64
            ],
            transients_sec: vec![0.0, 0.5, 1.0],
        };
        fb.render_deck_waveform(1, &stripe, 0.25);
        let bulk_pkt = fb.encode_usb_bulk_rect(0, 0, 480, 16);
        assert_eq!(&bulk_pkt[0..2], &[0x84, 0x00]);
        assert_eq!(bulk_pkt.len(), 12 + 480 * 16 * 2);

        // ALSA MIDI .tsi router test
        let tsi = TsiFile::from_devices(vec![TsiDevice {
            device_name: "Generic MIDI".to_string(),
            comment: None,
            in_port: None,
            out_port: None,
            mappings: vec![TsiControlMapping {
                midi_binding_id: 60, // Note C4
                is_output_led: false,
                traktor_control_id: 102, // Deck Play
                target_assignment: 1,
            }],
        }]);
        let mut bus = PropertyBus::new_traktor_pro4_default();
        let writes = AlsaMidiTsiRouter::route_midi_message([0x90, 60, 127], &tsi, &mut bus);
        assert_eq!(writes, 1);
        assert_eq!(
            bus.get("app.traktor.decks.1.play"),
            Some(&PropValue::Bool(true))
        );

        // Traktor MX2 (0x17CC:0x2420) HID input/output verification
        assert_eq!(
            lookup_ni_device(0x17CC, TRAKTOR_MX2_PID).map(|d| d.model),
            Some("Traktor MX2")
        );
        let mut mx2 = Mx2HidDriver::new();
        assert_eq!(Mx2HidDriver::mode_enable_nhl_report(), [0xA0, 0x00, 0xFF]);

        // Test Report 0x01: Press left.play (button 29 -> byte 1 + 3 = byte 4, bit 5)
        let mut r1 = [0u8; 14];
        r1[0] = 0x01;
        r1[1 + (29 / 8)] |= 1 << (29 % 8);
        let ev1 = mx2.decode_input_report(&r1);
        assert!(ev1.contains(&SurfaceEvent::Button {
            control: "surface.left.play".to_string(),
            pressed: true,
        }));

        // Test Report 0x02: Move mixer.xfader (channel 23) to 4095 (1.0)
        let mut r2 = [0u8; 53];
        r2[0] = 0x02;
        let xf_bytes = 4095u16.to_le_bytes();
        r2[1 + 23 * 2] = xf_bytes[0];
        r2[1 + 23 * 2 + 1] = xf_bytes[1];
        let ev2 = mx2.decode_input_report(&r2);
        assert!(ev2.iter().any(|e| matches!(
            e,
            SurfaceEvent::Fader { control, value }
                if control == "surface.mixer.xfader" && (*value - 1.0).abs() < 1e-6
        )));

        // Test Report 0x03: Jogwheel 0 (Left) rotates +24 ticks
        let mut r3_a = [0u8; 19];
        r3_a[0] = 0x03;
        r3_a[7..11].copy_from_slice(&100i32.to_le_bytes());
        let _ = mx2.decode_input_report(&r3_a);
        let mut r3_b = [0u8; 19];
        r3_b[0] = 0x03;
        r3_b[7..11].copy_from_slice(&124i32.to_le_bytes());
        let ev3 = mx2.decode_input_report(&r3_b);
        assert_eq!(
            ev3,
            vec![SurfaceEvent::Encoder {
                control: "surface.left.jogwheel.rotation".to_string(),
                delta: 24,
            }]
        );

        // Test Output Report 0x80 (106 bytes) with Active Loop Green bottom shell LEDs
        let led_pkt = mx2.encode_led_report(
            &bus,
            Mx2BottomShellState::ActiveLoopGreen,
            Mx2BottomShellState::DefaultGlow,
        );
        assert_eq!(led_pkt[0], 0x80);
        assert_eq!(led_pkt[1 + 29], 0x7F); // left.play lit
        assert_eq!(led_pkt[1 + 74], 0x38); // left.bottom.leds.1 green
    }
}
