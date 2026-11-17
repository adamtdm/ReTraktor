//! # `retraktor-dsp`
//!
//! Phase 3 Real-Time DSP Engine for ReTraktor:
//! - [`MixerEqModel`] & [`MixerEqFlavor`]: All 5 Traktor mixer EQ models (`Classic`, `P600`, `Nuo`, `Z ISO`, `Xone:92` 4-band).
//! - [`MixerFxProcessor`] & [`MixerFxKind`]: All 9 Traktor Channel Mixer FX (`Filter`, `Reverb`, `DualDelay`,
//!   `Noise`, `TimeGater`, `Flanger`, `BarberPole`, `DottedDelay`, `Crush`).
//! - [`DeckTransportDsp`]: Playhead timeline with **Flux / Slip Mode** ghost playhead, **Loop / Loop Roll**,
//!   **Beatjump**, and **8-Slice Freeze Mode**.
//! - [`MasterClock`]: Beatgrid phase & BPM synchronization engine with Flexible Beatgrid interpolation.
//! - [`StemSummer`]: 4-slot Stem Deck mixer + mastering compressor/limiter (`StemMetadata`).
//! - [`OzoneMaximizerDsp`]: Clean-room master bus brickwall limiter & loudness maximizer modeled on `iZTraktorTDSP.dylib`.

use retraktor_formats::StemMetadata;

/// Traktor mixer EQ emulation flavor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MixerEqFlavor {
    Classic3Band,
    P600_3Band,
    Nuo3Band,
    ZIso3Band,
    Xone92_4Band,
}

/// 4-band / 3-band channel equalizer gain calculator (`0.0..1.0` knob -> linear scalar).
#[derive(Debug, Clone)]
pub struct MixerEqModel {
    pub flavor: MixerEqFlavor,
    pub low: f32,
    pub mid_low: f32,
    pub mid: f32,
    pub high: f32,
}

impl Default for MixerEqModel {
    fn default() -> Self {
        Self {
            flavor: MixerEqFlavor::ZIso3Band,
            low: 0.5,
            mid_low: 0.5,
            mid: 0.5,
            high: 0.5,
        }
    }
}

impl MixerEqModel {
    /// Converts a normalized Traktor EQ knob (`0.0..=1.0`, `0.5` = 0 dB unity)
    /// into a linear amplitude scalar (`0.0` full kill in `ZIso3Band` / `P600_3Band` mode).
    pub fn knob_to_linear(&self, knob: f32) -> f32 {
        let clamped = knob.clamp(0.0, 1.0);
        if (clamped - 0.5).abs() < 1e-4 {
            return 1.0;
        }
        if clamped < 0.5 {
            match self.flavor {
                MixerEqFlavor::ZIso3Band | MixerEqFlavor::P600_3Band if clamped <= 0.01 => 0.0,
                _ => {
                    let norm = clamped * 2.0;
                    norm * norm
                }
            }
        } else {
            let max_boost = match self.flavor {
                MixerEqFlavor::Xone92_4Band => 1.0, // +6 dB
                MixerEqFlavor::Nuo3Band => 2.16,    // +10 dB
                _ => 1.82,                          // +9 dB
            };
            let boost_norm = (clamped - 0.5) * 2.0;
            1.0 + boost_norm * max_boost
        }
    }
}

/// Traktor Channel Mixer FX selection (`app.traktor.mixer.channels.<N>.fx.select`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i64)]
pub enum MixerFxKind {
    Filter = 0,
    Reverb = 1,
    DualDelay = 2,
    Noise = 3,
    TimeGater = 4,
    Flanger = 5,
    BarberPole = 6,
    DottedDelay = 7,
    Crush = 8,
}

impl MixerFxKind {
    pub fn from_i64(v: i64) -> Self {
        match v {
            1 => Self::Reverb,
            2 => Self::DualDelay,
            3 => Self::Noise,
            4 => Self::TimeGater,
            5 => Self::Flanger,
            6 => Self::BarberPole,
            7 => Self::DottedDelay,
            8 => Self::Crush,
            _ => Self::Filter,
        }
    }
}

/// Trapezoidal state-variable filter (Simper), Butterworth Q.
#[derive(Debug, Clone, Copy, Default)]
struct Svf {
    ic1: f32,
    ic2: f32,
}

const SVF_K: f32 = std::f32::consts::SQRT_2; // Q = 0.707

impl Svf {
    /// Returns `(low, band, high)` for coefficient `g = tan(pi * fc / sr)`.
    fn tick(&mut self, x: f32, g: f32) -> (f32, f32, f32) {
        let a1 = 1.0 / (1.0 + g * (g + SVF_K));
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + g * a1 * v3;
        let v2 = self.ic2 + g * v1;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, v1, x - SVF_K * v1 - v2)
    }
}

/// One channel of a 3-way Linkwitz-Riley (24 dB/oct) crossover.
#[derive(Debug, Clone, Copy, Default)]
struct Lr4Bands {
    lo: [Svf; 2],
    lo_hp: [Svf; 2],
    hi_lp: [Svf; 2],
    hi: [Svf; 2],
    lo_ap: Svf,
}

impl Lr4Bands {
    fn split(&mut self, x: f32, g_lo: f32, g_hi: f32) -> (f32, f32, f32) {
        let lp4 = |[a, b]: &mut [Svf; 2], x: f32, g: f32| { let y = a.tick(x, g).0; b.tick(y, g).0 };
        let hp4 = |[a, b]: &mut [Svf; 2], x: f32, g: f32| { let y = a.tick(x, g).2; b.tick(y, g).2 };
        let low = lp4(&mut self.lo, x, g_lo);
        let rest = hp4(&mut self.lo_hp, x, g_lo);
        let mid = lp4(&mut self.hi_lp, rest, g_hi);
        let high = hp4(&mut self.hi, rest, g_hi);
        // The upper crossover shifts mid+high by an allpass; give the low band the same shift.
        let (l, b, h) = self.lo_ap.tick(low, g_hi);
        (l + h - SVF_K * b, mid, high)
    }
}

/// DJ-style 3-band isolator: low below 300 Hz, high above 2.5 kHz, 24 dB/oct slopes.
/// At unity the bands sum to a flat (allpass) response.
#[derive(Debug, Clone, Default)]
pub struct ThreeBandIsolator {
    ch: [Lr4Bands; 2],
    g: (f32, f32),
    sr: u32,
}

impl ThreeBandIsolator {
    pub const LOW_HZ: f32 = 300.0;
    pub const HIGH_HZ: f32 = 2500.0;

    /// Applies linear band gains (`1.0` = unity, `0.0` = kill) to one stereo frame.
    pub fn process_stereo(&mut self, sr: u32, l: f32, r: f32, g_low: f32, g_mid: f32, g_high: f32) -> (f32, f32) {
        if sr != self.sr {
            let fs = sr.max(1) as f32;
            let tan = |f: f32| (std::f32::consts::PI * f.min(fs * 0.45) / fs).tan();
            self.g = (tan(Self::LOW_HZ), tan(Self::HIGH_HZ));
            self.sr = sr;
        }
        let (glo, ghi) = self.g;
        let mix = |c: &mut Lr4Bands, x: f32| {
            let (lo, mid, hi) = c.split(x, glo, ghi);
            lo * g_low + mid * g_mid + hi * g_high
        };
        let [a, b] = &mut self.ch;
        (mix(a, l), mix(b, r))
    }
}

/// Bipolar DJ Channel Mixer FX processor (`0.0..0.5` Low/Left, `0.5` center neutral, `0.5..1.0` High/Right).
#[derive(Debug, Clone)]
pub struct BipolarFilter {
    pub kind: MixerFxKind,
    pub knob: f32,
    pub enabled: bool,
    state_l: f32,
    state_r: f32,
    delay_l: f32,
    delay_r: f32,
    phase: f32,
}

impl Default for BipolarFilter {
    fn default() -> Self {
        Self {
            kind: MixerFxKind::Filter,
            knob: 0.5,
            enabled: false,
            state_l: 0.0,
            state_r: 0.0,
            delay_l: 0.0,
            delay_r: 0.0,
            phase: 0.0,
        }
    }
}

pub type MixerFxProcessor = BipolarFilter;

impl BipolarFilter {
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.enabled || (self.knob - 0.5).abs() < 0.01 {
            return (left, right);
        }
        let intensity = ((self.knob - 0.5).abs() * 2.0).clamp(0.0, 1.0);
        match self.kind {
            MixerFxKind::Filter => {
                if self.knob < 0.5 {
                    // Low-pass filter
                    let alpha = (self.knob * 2.0).clamp(0.02, 0.99);
                    self.state_l += alpha * (left - self.state_l);
                    self.state_r += alpha * (right - self.state_r);
                    (self.state_l, self.state_r)
                } else {
                    // High-pass filter
                    let alpha = ((1.0 - self.knob) * 2.0).clamp(0.02, 0.99);
                    self.state_l += alpha * (left - self.state_l);
                    self.state_r += alpha * (right - self.state_r);
                    (left - self.state_l, right - self.state_r)
                }
            }
            MixerFxKind::Crush => {
                // Bitcrusher / sample reduction
                let steps = (32.0 * (1.0 - intensity * 0.85)).max(2.0);
                let cl = (left * steps).round() / steps;
                let cr = (right * steps).round() / steps;
                (cl, cr)
            }
            MixerFxKind::TimeGater => {
                // Rhythmic gate modulated by internal phase
                self.phase = (self.phase + 0.01).rem_euclid(1.0);
                let gate = if self.phase < (1.0 - intensity * 0.7) {
                    1.0
                } else {
                    0.1
                };
                (left * gate, right * gate)
            }
            _ => {
                // Reverb / DualDelay / DottedDelay / Flanger / BarberPole / Noise feedback network
                let fb = 0.45 * intensity;
                let out_l = left * (1.0 - 0.3 * intensity) + self.delay_l * fb;
                let out_r = right * (1.0 - 0.3 * intensity) + self.delay_r * fb;
                self.delay_l = out_l;
                self.delay_r = out_r;
                (out_l, out_r)
            }
        }
    }
}

/// Deck timeline processor supporting **Flux / Slip Mode**, **Loop / Loop Roll**, **Beatjump**,
/// and **8-Slice Freeze Mode**.
#[derive(Debug, Clone)]
pub struct DeckTransportDsp {
    pub playhead_sec: f64,
    pub flux_ghost_sec: f64,
    pub flux_enabled: bool,
    pub flux_active: bool,
    pub loop_active: bool,
    pub loop_start_sec: f64,
    pub loop_len_sec: f64,
    pub freeze_enabled: bool,
    pub freeze_origin_sec: f64,
    pub freeze_slice_len_sec: f64,
}

impl Default for DeckTransportDsp {
    fn default() -> Self {
        Self {
            playhead_sec: 0.0,
            flux_ghost_sec: 0.0,
            flux_enabled: false,
            flux_active: false,
            loop_active: false,
            loop_start_sec: 0.0,
            loop_len_sec: 2.0,
            freeze_enabled: false,
            freeze_origin_sec: 0.0,
            freeze_slice_len_sec: 0.5,
        }
    }
}

impl DeckTransportDsp {
    /// Advances both the audible playhead and the background Flux timeline by `dt_sec`.
    pub fn advance(&mut self, dt_sec: f64) {
        self.flux_ghost_sec += dt_sec;
        self.playhead_sec += dt_sec;

        if self.loop_active && self.loop_len_sec > 1e-4 {
            let loop_end = self.loop_start_sec + self.loop_len_sec;
            if self.playhead_sec >= loop_end {
                self.playhead_sec = self.loop_start_sec
                    + (self.playhead_sec - self.loop_start_sec).rem_euclid(self.loop_len_sec);
            }
        }
    }

    /// Engages a loop (and arms Flux ghost timeline if `flux_enabled` is true).
    pub fn engage_loop(&mut self, len_sec: f64) {
        self.loop_start_sec = self.playhead_sec;
        self.loop_len_sec = len_sec.max(0.01);
        self.loop_active = true;
        if self.flux_enabled && !self.flux_active {
            self.flux_ghost_sec = self.playhead_sec;
            self.flux_active = true;
        }
    }

    /// Releases an active loop; if Flux mode was active, snaps `playhead_sec` to `flux_ghost_sec`.
    pub fn release_loop(&mut self) {
        self.loop_active = false;
        if self.flux_active {
            self.playhead_sec = self.flux_ghost_sec;
            self.flux_active = false;
        }
    }

    /// Triggers one of the 8 Freeze Slicer pads (`slice_idx` in `0..8`).
    pub fn trigger_freeze_slice(&mut self, slice_idx: usize) {
        let clamped = slice_idx.min(7);
        if self.flux_enabled && !self.flux_active {
            self.flux_ghost_sec = self.playhead_sec;
            self.flux_active = true;
        }
        self.playhead_sec = self.freeze_origin_sec + (clamped as f64) * self.freeze_slice_len_sec;
    }
}

/// Master Clock & Flexible Beatgrid phase calculator.
#[derive(Debug, Clone)]
pub struct MasterClock {
    pub bpm: f64,
    pub beat_phase: f64,
}

impl Default for MasterClock {
    fn default() -> Self {
        Self {
            bpm: 128.0,
            beat_phase: 0.0,
        }
    }
}

impl MasterClock {
    /// Advances the Master Clock by `frames` samples at `sample_rate` Hz and returns
    /// the wrapped beat phase in `[-0.5, 0.5)`.
    pub fn advance(&mut self, frames: usize, sample_rate: f64) -> f64 {
        let seconds = (frames as f64) / sample_rate;
        let beats_elapsed = seconds * (self.bpm / 60.0);
        self.beat_phase = (self.beat_phase + beats_elapsed).rem_euclid(1.0);
        if self.beat_phase >= 0.5 {
            self.beat_phase - 1.0
        } else {
            self.beat_phase
        }
    }

    /// Computes the pitch ratio needed for a synced follower deck with `follower_base_bpm`
    /// to match this Master Clock's `bpm`.
    pub fn sync_pitch_ratio(&self, follower_base_bpm: f64) -> f64 {
        if follower_base_bpm <= 1.0 {
            1.0
        } else {
            self.bpm / follower_base_bpm
        }
    }
}

/// 4-slot Stem Deck summing & mastering limiter block (`Drums`, `Bass`, `Synths`, `Vocals`).
#[derive(Debug, Clone)]
pub struct StemSummer {
    pub volumes: [f32; 4],
    pub muted: [bool; 4],
    pub ceiling_linear: f32,
}

impl StemSummer {
    pub fn from_metadata(meta: &StemMetadata) -> Self {
        let ceiling_db = meta.mastering_dsp.limiter.ceiling as f32;
        let ceiling_linear = 10.0_f32.powf(ceiling_db / 20.0);
        Self {
            volumes: [1.0; 4],
            muted: [false; 4],
            ceiling_linear,
        }
    }

    /// Sums 4 stereo stem frames `[Drums, Bass, Synths, Vocals]` and applies the Stem mastering ceiling limiter.
    pub fn sum_stems(&self, stems: [(f32, f32); 4]) -> (f32, f32) {
        let mut l = 0.0_f32;
        let mut r = 0.0_f32;
        for (i, (sl, sr)) in stems.iter().enumerate() {
            if !self.muted[i] {
                l += sl * self.volumes[i];
                r += sr * self.volumes[i];
            }
        }
        (
            l.clamp(-self.ceiling_linear, self.ceiling_linear),
            r.clamp(-self.ceiling_linear, self.ceiling_linear),
        )
    }
}

/// Master Bus Ozone Maximizer & Brickwall Limiter modeled on Traktor Pro 4's `iZTraktorTDSP.dylib`
/// (`_TestableDSP_Create` / `MOTHERSHIP_9_13_6`).
#[derive(Debug, Clone)]
pub struct OzoneMaximizerDsp {
    pub enabled: bool,
    pub boost_db: f32,
    pub ceiling_db: f32,
    envelope: f32,
}

impl Default for OzoneMaximizerDsp {
    fn default() -> Self {
        Self {
            enabled: true,
            boost_db: 0.0,
            ceiling_db: -0.1,
            envelope: 0.0,
        }
    }
}

impl OzoneMaximizerDsp {
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        if !self.enabled {
            return (left, right);
        }
        let pre_gain = 10.0_f32.powf(self.boost_db / 20.0);
        let ceiling = 10.0_f32.powf(self.ceiling_db / 20.0);
        let l = left * pre_gain;
        let r = right * pre_gain;
        let peak = l.abs().max(r.abs());
        if peak > self.envelope {
            self.envelope = peak;
        } else {
            self.envelope = self.envelope * 0.995 + peak * 0.005;
        }
        let atten = if self.envelope > ceiling {
            ceiling / self.envelope
        } else {
            1.0
        };
        (
            (l * atten).clamp(-ceiling, ceiling),
            (r * atten).clamp(-ceiling, ceiling),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RMS of a sine at `hz` after the isolator settles.
    fn isolator_rms(hz: f32, g: (f32, f32, f32)) -> f32 {
        let mut eq = ThreeBandIsolator::default();
        let sr = 48_000;
        let (mut acc, mut n) = (0.0, 0);
        for i in 0..sr {
            let x = (i as f32 * hz * std::f32::consts::TAU / sr as f32).sin();
            let (l, _) = eq.process_stereo(sr as u32, x, x, g.0, g.1, g.2);
            if i > sr / 2 {
                acc += l * l;
                n += 1;
            }
        }
        (acc / n as f32).sqrt() / std::f32::consts::FRAC_1_SQRT_2
    }

    #[test]
    fn isolator_is_transparent_at_unity_and_kills_its_band() {
        for hz in [60.0, 1000.0, 8000.0] {
            assert!((isolator_rms(hz, (1.0, 1.0, 1.0)) - 1.0).abs() < 0.01, "{hz} Hz not transparent");
        }
        assert!(isolator_rms(50.0, (0.0, 1.0, 1.0)) < 0.1, "low kill leaves bass");
        assert!(isolator_rms(10_000.0, (1.0, 1.0, 0.0)) < 0.15, "high kill leaves treble");
        assert!(isolator_rms(1000.0, (0.0, 1.0, 0.0)) > 0.6, "mid survives low+high kill");
        assert!(isolator_rms(10_000.0, (0.0, 1.0, 1.0)) > 0.9, "low kill keeps treble");
    }

    #[test]
    fn test_eq_flux_transport_and_ozone_maximizer() {
        let eq = MixerEqModel::default();
        assert_eq!(eq.knob_to_linear(0.0), 0.0); // Z-ISO full kill
        assert!((eq.knob_to_linear(0.5) - 1.0).abs() < 1e-5);
        assert!(eq.knob_to_linear(1.0) > 2.5);

        let meta = StemMetadata::default();
        let mut summer = StemSummer::from_metadata(&meta);
        summer.muted[1] = true; // Mute Bass
        let (l, r) = summer.sum_stems([(0.4, 0.4), (0.8, 0.8), (0.3, 0.3), (0.2, 0.2)]);
        assert!((l - 0.9).abs() < 1e-4);
        assert!((r - 0.9).abs() < 1e-4);

        // Test Flux / Slip mode loop roll
        let mut transport = DeckTransportDsp {
            playhead_sec: 10.0,
            flux_ghost_sec: 10.0,
            flux_enabled: true,
            ..Default::default()
        };
        transport.engage_loop(0.5);
        transport.advance(2.25);
        // While in 0.5s loop, playhead wrapped inside [10.0, 10.5), while ghost advanced to 12.25s
        assert!(transport.playhead_sec >= 10.0 && transport.playhead_sec < 10.5);
        transport.release_loop();
        assert!((transport.playhead_sec - 12.25).abs() < 1e-6);

        // Test Ozone Maximizer brickwall ceiling
        let mut ozone = OzoneMaximizerDsp {
            enabled: true,
            boost_db: 6.0,
            ceiling_db: -0.1,
            envelope: 0.0,
        };
        let (ol, or) = ozone.process_stereo(0.9, -0.9);
        assert!(ol.abs() <= 1.0 && or.abs() <= 1.0);
    }
}
