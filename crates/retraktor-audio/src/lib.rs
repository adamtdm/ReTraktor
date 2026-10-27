//! # `retraktor-audio`
//!
//! Real-time 4-deck audio mixer engine, lock-free command queue, crossfader curve processor,
//! Ozone Maximizer master stage, `PropertyBus` telemetry bridge, and Linux PipeWire / ALSA
//! UAC2 multi-channel routing profiles.

use retraktor_core::{PropValue, PropertyBus};
use retraktor_dsp::{
    BipolarFilter, DeckTransportDsp, MasterClock, MixerEqModel, MixerFxKind, OzoneMaximizerDsp,
};
use std::collections::VecDeque;

/// Linux PipeWire / ALSA `pro-audio` multi-channel output/input routing map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxAudioRoutingProfile {
    pub profile_name: String,
    pub sample_rate: u32,
    pub period_frames: u32,
    pub master_out_channels: (usize, usize),
    pub headphone_cue_channels: (usize, usize),
    pub booth_out_channels: Option<(usize, usize)>,
}

impl LinuxAudioRoutingProfile {
    /// Standard 4-out USB Audio Class 2.0 layout for Kontrol S4 MK3, S3, S2 MK3, S8, Z1 MK2 on Linux PipeWire.
    pub fn ni_kontrol_uac2_default() -> Self {
        Self {
            profile_name: "PipeWire pro-audio (NI Kontrol UAC2)".to_string(),
            sample_rate: 48_000,
            period_frames: 128,
            master_out_channels: (0, 1),
            headphone_cue_channels: (2, 3),
            booth_out_channels: None,
        }
    }

    /// 4-Out / 2-In 24-bit 96kHz/48kHz USB Audio Class 2.0 routing profile for the
    /// **Traktor MX2** (`VID 0x17CC, PID 0x2420`) matching `<device id="Traktor MX2">`
    /// (`out_master_left=0`, `out_master_right=1`, `out_monitor_left=2`, `out_monitor_right=3`,
    /// `in_microphone_left=0`, `in_microphone_right=1`).
    pub fn traktor_mx2_default() -> Self {
        Self {
            profile_name: "Traktor MX2 (USB Audio Class 2.0 — 4-Out / 2-In)".to_string(),
            sample_rate: 48_000,
            period_frames: 128,
            master_out_channels: (0, 1),
            headphone_cue_channels: (2, 3),
            booth_out_channels: None,
        }
    }
}

/// Real-time command sent from the UI / CSI controller thread to the audio callback thread.
#[derive(Debug, Clone, Copy)]
pub enum AudioCommand {
    SetDeckPlay { deck: usize, play: bool },
    SetChannelVolume { channel: usize, volume: f32 },
    SetCrossfader { position: f32 },
    SetMasterTempo { bpm: f64 },
}

/// Bounded pre-allocated SPSC-style ring queue for real-time audio commands.
#[derive(Debug, Clone)]
pub struct AudioCommandQueue {
    buf: VecDeque<AudioCommand>,
    capacity: usize,
}

impl AudioCommandQueue {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buf: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, cmd: AudioCommand) -> bool {
        if self.buf.len() >= self.capacity {
            return false;
        }
        self.buf.push_back(cmd);
        true
    }

    pub fn pop(&mut self) -> Option<AudioCommand> {
        self.buf.pop_front()
    }
}

/// Per-channel strip state inside the real-time audio callback.
#[derive(Debug, Clone, Default)]
pub struct ChannelStrip {
    pub playing: bool,
    pub volume: f32,
    pub cue_enabled: bool,
    pub xfader_left: bool,
    pub xfader_right: bool,
    pub last_prefader_peak: f32,
    pub transport: DeckTransportDsp,
    pub eq: MixerEqModel,
    pub filter: BipolarFilter,
}

/// 4-Deck real-time audio mixing engine producing stereo `Master L/R` and `Headphone Cue L/R`.
#[derive(Debug, Clone)]
pub struct FourDeckAudioEngine {
    pub routing: LinuxAudioRoutingProfile,
    pub clock: MasterClock,
    pub strips: [ChannelStrip; 4],
    pub crossfader: f32,
    pub crossfader_curve: f32,
    pub master_volume: f32,
    pub maximizer: OzoneMaximizerDsp,
    pub last_master_peak: (f32, f32),
}

impl Default for FourDeckAudioEngine {
    fn default() -> Self {
        Self {
            routing: LinuxAudioRoutingProfile::ni_kontrol_uac2_default(),
            clock: MasterClock::default(),
            strips: std::array::from_fn(|i| ChannelStrip {
                playing: false,
                volume: 0.85,
                cue_enabled: false,
                xfader_left: i % 2 == 0,
                xfader_right: i % 2 == 1,
                last_prefader_peak: 0.0,
                transport: DeckTransportDsp::default(),
                eq: MixerEqModel::default(),
                filter: BipolarFilter::default(),
            }),
            crossfader: 0.5,
            crossfader_curve: 0.5,
            master_volume: 0.8,
            maximizer: OzoneMaximizerDsp::default(),
            last_master_peak: (0.0, 0.0),
        }
    }
}

impl FourDeckAudioEngine {
    pub fn apply_command(&mut self, cmd: AudioCommand) {
        match cmd {
            AudioCommand::SetDeckPlay { deck, play } if (1..=4).contains(&deck) => {
                self.strips[deck - 1].playing = play;
            }
            AudioCommand::SetChannelVolume { channel, volume } if (1..=4).contains(&channel) => {
                self.strips[channel - 1].volume = volume.clamp(0.0, 1.0);
            }
            AudioCommand::SetCrossfader { position } => {
                self.crossfader = position.clamp(0.0, 1.0);
            }
            AudioCommand::SetMasterTempo { bpm } => {
                self.clock.bpm = bpm.clamp(20.0, 300.0);
            }
            _ => {}
        }
    }

    /// Computes Crossfader Left/Right gains for `crossfader` in `0.0..=1.0` and `crossfader_curve`
    /// (`0.0 = Smooth`, `0.5 = Steep`, `1.0 = Scratch cut`).
    pub fn crossfader_gains(&self) -> (f32, f32) {
        let x = self.crossfader.clamp(0.0, 1.0);
        if self.crossfader_curve >= 0.9 {
            // Scratch sharp cut with 5% cut-in lag
            let left = if x <= 0.95 { 1.0 } else { (1.0 - x) * 20.0 };
            let right = if x >= 0.05 { 1.0 } else { x * 20.0 };
            (left.clamp(0.0, 1.0), right.clamp(0.0, 1.0))
        } else {
            // Constant-power / steep blend
            let left = (1.0 - x).sqrt();
            let right = x.sqrt();
            (left, right)
        }
    }

    /// Synchronizes deck/mixer state from [`PropertyBus`] into the audio engine and writes
    /// real-time channel & master VU meters and playhead elapsed times back to [`PropertyBus`].
    pub fn sync_with_property_bus(&mut self, bus: &mut PropertyBus) {
        if let Some(bpm) = bus.get("app.traktor.masterclock.tempo").and_then(|v| v.as_f64()) {
            self.clock.bpm = bpm;
        }
        if let Some(xf) = bus.get("app.traktor.mixer.xfader.adjust").and_then(|v| v.as_f64()) {
            self.crossfader = xf as f32;
        }
        if let Some(xc) = bus.get("app.traktor.mixer.xfader.curve").and_then(|v| v.as_f64()) {
            self.crossfader_curve = xc as f32;
        }
        if let Some(mv) = bus.get("app.traktor.mixer.master.volume").and_then(|v| v.as_f64()) {
            self.master_volume = mv as f32;
        }

        for idx in 1..=4 {
            let strip = &mut self.strips[idx - 1];
            if let Some(play) = bus.get(&format!("app.traktor.decks.{idx}.play")).and_then(|v| v.as_bool()) {
                strip.playing = play;
            }
            if let Some(flux) = bus.get(&format!("app.traktor.decks.{idx}.flux.enabled")).and_then(|v| v.as_bool()) {
                strip.transport.flux_enabled = flux;
            }
            if let Some(vol) = bus.get(&format!("app.traktor.mixer.channels.{idx}.volume")).and_then(|v| v.as_f64()) {
                strip.volume = vol as f32;
            }
            if let Some(cue) = bus.get(&format!("app.traktor.mixer.channels.{idx}.cue")).and_then(|v| v.as_bool()) {
                strip.cue_enabled = cue;
            }
            if let Some(fxs) = bus.get(&format!("app.traktor.mixer.channels.{idx}.fx.select")).and_then(|v| v.as_i64()) {
                strip.filter.kind = MixerFxKind::from_i64(fxs);
            }
            if let Some(fxa) = bus.get(&format!("app.traktor.mixer.channels.{idx}.fx.adjust")).and_then(|v| v.as_f64()) {
                strip.filter.knob = fxa as f32;
            }
            if let Some(fxo) = bus.get(&format!("app.traktor.mixer.channels.{idx}.fx.on")).and_then(|v| v.as_bool()) {
                strip.filter.enabled = fxo;
            }

            // Write telemetry back to PropertyBus
            bus.set(
                &format!("app.traktor.decks.{idx}.track.player.elapsed_time"),
                PropValue::Float(strip.transport.playhead_sec),
            );
            bus.set(
                &format!("app.traktor.mixer.channels.{idx}.level.prefader.linear.sum"),
                PropValue::Float(strip.last_prefader_peak as f64),
            );
            bus.set(
                &format!("app.traktor.mixer.channels.{idx}.level.prefader.linear.meter"),
                PropValue::Float(strip.last_prefader_peak as f64),
            );
        }

        bus.set(
            "app.traktor.mixer.master.level.left",
            PropValue::Float(self.last_master_peak.0 as f64),
        );
        bus.set(
            "app.traktor.mixer.master.level.right",
            PropValue::Float(self.last_master_peak.1 as f64),
        );
        bus.set(
            "app.traktor.mixer.master.level.clip.left",
            PropValue::Bool(self.last_master_peak.0 >= 0.99),
        );
        bus.set(
            "app.traktor.mixer.master.level.clip.right",
            PropValue::Bool(self.last_master_peak.1 >= 0.99),
        );
    }

    /// Processes a single stereo frame across all 4 decks, returning `(master_l, master_r, cue_l, cue_r)`.
    pub fn process_frame(&mut self, deck_inputs: [(f32, f32); 4]) -> (f32, f32, f32, f32) {
        let sr = self.routing.sample_rate as f64;
        let dt_sec = 1.0 / sr.max(1.0);
        let _phase = self.clock.advance(1, sr);
        let (xf_l, xf_r) = self.crossfader_gains();

        let mut master_l = 0.0_f32;
        let mut master_r = 0.0_f32;
        let mut cue_l = 0.0_f32;
        let mut cue_r = 0.0_f32;

        for (i, (in_l, in_r)) in deck_inputs.iter().copied().enumerate() {
            let strip = &mut self.strips[i];
            if !strip.playing {
                strip.last_prefader_peak = 0.0;
                continue;
            }
            strip.transport.advance(dt_sec);
            let (fl, fr) = strip.filter.process_stereo(in_l, in_r);
            let eq_gain = strip.eq.knob_to_linear(strip.eq.mid);
            let pre_l = fl * eq_gain;
            let pre_r = fr * eq_gain;
            strip.last_prefader_peak = pre_l.abs().max(pre_r.abs()).clamp(0.0, 1.0);

            if strip.cue_enabled {
                cue_l += pre_l;
                cue_r += pre_r;
            }

            let xf_scalar = if strip.xfader_left && !strip.xfader_right {
                xf_l
            } else if strip.xfader_right && !strip.xfader_left {
                xf_r
            } else {
                1.0
            };

            let post_gain = strip.volume * xf_scalar * self.master_volume;
            master_l += pre_l * post_gain;
            master_r += pre_r * post_gain;
        }

        let (lim_l, lim_r) = self.maximizer.process_stereo(master_l, master_r);
        self.last_master_peak = (lim_l.abs().clamp(0.0, 1.0), lim_r.abs().clamp(0.0, 1.0));
        (lim_l, lim_r, cue_l, cue_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_four_deck_audio_mix_and_bus_sync() {
        let mut bus = PropertyBus::new_traktor_pro4_default();
        bus.set("app.traktor.decks.1.play", PropValue::Bool(true));
        bus.set("app.traktor.mixer.channels.1.fx.select", PropValue::Int(8)); // Crush
        bus.set("app.traktor.mixer.channels.1.fx.adjust", PropValue::Float(0.8));
        bus.set("app.traktor.mixer.channels.1.fx.on", PropValue::Bool(true));

        let mut engine = FourDeckAudioEngine::default();
        engine.sync_with_property_bus(&mut bus);

        let (ml, mr, _cl, _cr) =
            engine.process_frame([(0.6, 0.6), (0.0, 0.0), (0.0, 0.0), (0.0, 0.0)]);
        assert!(ml > 0.1 && mr > 0.1);

        engine.sync_with_property_bus(&mut bus);
        let meter = bus
            .get("app.traktor.mixer.channels.1.level.prefader.linear.sum")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        assert!(meter > 0.1);
    }
}
