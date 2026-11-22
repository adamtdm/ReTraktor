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
        self.tick_k(x, g, SVF_K)
    }

    /// Same, with damping `k = 1/Q` (smaller = more resonant).
    fn tick_k(&mut self, x: f32, g: f32, k: f32) -> (f32, f32, f32) {
        let x = if x.is_finite() { x } else { 0.0 };
        let a1 = 1.0 / (1.0 + g * (g + k));
        let v3 = x - self.ic2;
        let v1 = a1 * self.ic1 + g * a1 * v3;
        let v2 = self.ic2 + g * v1;
        let flush = |v: f32| if !v.is_finite() || v.abs() < 1e-15 { 0.0 } else { v };
        self.ic1 = flush(2.0 * v1 - self.ic1);
        self.ic2 = flush(2.0 * v2 - self.ic2);
        (v2, v1, x - k * v1 - v2)
    }
}

/// SVF coefficient for cutoff `fc` Hz, kept below Nyquist.
fn svf_g(fc: f32, sr: f32) -> f32 {
    (std::f32::consts::PI * fc.clamp(10.0, sr * 0.45) / sr).tan()
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

/// Fractional ring-buffer delay line, allocated up front so the audio thread never allocates.
#[derive(Debug, Clone)]
struct DelayLine {
    buf: Vec<f32>,
    pos: usize,
}

impl DelayLine {
    fn new(len: usize) -> Self {
        Self { buf: vec![0.0; len], pos: 0 }
    }

    /// The sample written `d` writes ago (linear interpolation, clamped to the buffer).
    fn read(&self, d: f32) -> f32 {
        let len = self.buf.len();
        let d = if d.is_finite() { d.clamp(1.0, (len - 2) as f32) } else { 1.0 };
        let i = d as usize;
        let a = self.buf[(self.pos + len - i) % len];
        let b = self.buf[(self.pos + len - i - 1) % len];
        a + (b - a) * (d - i as f32)
    }

    fn write(&mut self, x: f32) {
        // Flush near-silence and non-finite inputs so decaying feedback never stalls or poisons the buffer.
        self.buf[self.pos] = if !x.is_finite() || x.abs() < 1e-15 { 0.0 } else { x };
        self.pos = (self.pos + 1) % self.buf.len();
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.pos = 0;
    }
}

/// Cubic soft clip: near-linear for small signals, saturates smoothly at ±1.
fn soft_clip(x: f32) -> f32 {
    if !x.is_finite() {
        return 0.0;
    }
    let x = x.clamp(-1.5, 1.5);
    x - x * x * x * (4.0 / 27.0)
}

/// Tempo multiplier from a `0..1` knob, snapped to powers of two (`0.5` = ×1, `0.25` = ×½, `1.0` = ×4).
fn sync_mult(amount: f32) -> f32 {
    2f32.powi(((amount.clamp(0.0, 1.0) - 0.5) * 4.0).round() as i32)
}

/// Bipolar tone coefficient: below 0 a dark LPF (20 kHz → 300 Hz), above 0 a bright HPF (20 Hz → 1.5 kHz).
fn tone_g(tone: f32, sr: f32) -> f32 {
    let t = tone.abs();
    svf_g(if tone < 0.0 { 20_000.0 * 0.015f32.powf(t) } else { 20.0 * 75f32.powf(t) }, sr)
}

fn tone_tick(svf: &mut Svf, x: f32, tone: f32, g: f32) -> f32 {
    if tone.abs() < 0.01 {
        return x;
    }
    let (lp, _, hp) = svf.tick(x, g);
    if tone < 0.0 { lp } else { hp }
}

/// Moves `v` toward `target` with a ~10 ms one-pole glide, snapping once close.
fn approach(v: f32, target: f32, sr: f32) -> f32 {
    let v = v + (target - v) * (100.0 / sr.max(1.0)).min(1.0);
    if (v - target).abs() < 1e-4 { target } else { v }
}

const DELAY_FRAMES: usize = 96_000; // 2 s at 48 kHz, 1 s at 96 kHz
const COMB_LENS: [f32; 4] = [1116.0, 1188.0, 1277.0, 1356.0]; // Freeverb tunings at 44.1 kHz
const ALLPASS_LENS: [f32; 2] = [556.0, 441.0];
const STEREO_SPREAD: f32 = 23.0;

/// Stereo FX engine shared by [`BipolarFilter`] and [`DeckFxUnit`].
#[derive(Debug, Clone)]
struct FxCore {
    delay: [DelayLine; 2],
    combs: [[DelayLine; 4]; 2],
    comb_lp: [[f32; 4]; 2],
    allpass: [[DelayLine; 2]; 2],
    svf: [Svf; 2],
    tone: [Svf; 2],
    beats: f64,
    gate: f32,
    hold: [f32; 2],
    hold_count: f32,
    rng: u32,
    quiet: usize,
    last_kind: MixerFxKind,
}

impl Default for FxCore {
    fn default() -> Self {
        Self {
            delay: std::array::from_fn(|_| DelayLine::new(DELAY_FRAMES)),
            // Sized for the largest comb/allpass (size 1.5) at 96 kHz.
            combs: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::new(4608))),
            comb_lp: [[0.0; 4]; 2],
            allpass: std::array::from_fn(|_| std::array::from_fn(|_| DelayLine::new(1280))),
            svf: Default::default(),
            tone: Default::default(),
            beats: 0.0,
            gate: 1.0,
            hold: [0.0; 2],
            hold_count: 0.0,
            rng: 0x9E37_79B9,
            quiet: DELAY_FRAMES,
            last_kind: MixerFxKind::Filter,
        }
    }
}

impl FxCore {
    /// Sends (reverb, delays, noise) add a tail to the dry signal; the rest replace it.
    fn is_send(kind: MixerFxKind) -> bool {
        matches!(kind, MixerFxKind::Reverb | MixerFxKind::DualDelay | MixerFxKind::DottedDelay | MixerFxKind::Noise)
    }

    /// Feeds sends at `send` level (`0` = off, so tails ring out) and returns the wet frame, or `None`
    /// once the FX is off and its tail has died, so callers pass dry audio without running anything.
    fn run(&mut self, kind: MixerFxKind, send: f32, x: (f32, f32), p: [f32; 3], sr: f32, bpm: f32) -> Option<(f32, f32)> {
        if kind != self.last_kind {
            // Avoid feeding stale delay-line audio into a newly selected delay/flanger effect.
            for d in &mut self.delay {
                d.clear();
            }
            self.svf = Default::default();
            self.tone = Default::default();
            self.last_kind = kind;
            self.quiet = DELAY_FRAMES;
        }
        let is_send = Self::is_send(kind);
        let send = if send.is_finite() { send.clamp(0.0, 1.0) } else { 0.0 };
        if send <= 0.0 && (!is_send || self.quiet >= DELAY_FRAMES) {
            return None;
        }
        let s = if is_send { send } else { 1.0 };
        let in_l = if x.0.is_finite() { x.0 * s } else { 0.0 };
        let in_r = if x.1.is_finite() { x.1 * s } else { 0.0 };
        let (a, b) = self.process(kind, (in_l, in_r), p, sr, bpm);
        // Noise ignores its input, so the send level scales its output instead.
        let y = if kind == MixerFxKind::Noise { (a * s, b * s) } else { (a, b) };
        self.quiet = if y.0.abs().max(y.1.abs()) > 1e-5 { 0 } else { (self.quiet + 1).min(DELAY_FRAMES) };
        Some(y)
    }

    /// One stereo frame of `kind`. `p` = `[amount, feedback, tone]`: `amount` 0..1 is size / tempo
    /// time / intensity, `feedback` 0..1 is feedback / resonance / depth, `tone` -1..1 is dark..bright
    /// (Filter: LPF..HPF cutoff, Flanger: polarity, BarberPole: falling..rising).
    fn process(&mut self, kind: MixerFxKind, x: (f32, f32), p: [f32; 3], sr: f32, bpm: f32) -> (f32, f32) {
        use std::f32::consts::{FRAC_1_SQRT_2, TAU};
        let sr = if sr.is_finite() { sr.max(8_000.0) } else { 48_000.0 };
        let bpm = if bpm.is_finite() { bpm.clamp(20.0, 300.0) } else { 128.0 };
        let clamp_or = |v: f32, lo: f32, hi: f32, def: f32| if v.is_finite() { v.clamp(lo, hi) } else { def };
        let [amount, feedback, tone] = [
            clamp_or(p[0], 0.0, 1.0, 0.5),
            clamp_or(p[1], 0.0, 1.0, 0.5),
            clamp_or(p[2], -1.0, 1.0, 0.0),
        ];
        let beat = 60.0 * sr / bpm; // samples per beat
        self.beats = (self.beats + 1.0 / beat as f64) % 256.0;
        let mult = sync_mult(amount);
        let tg = if matches!(kind, MixerFxKind::Reverb | MixerFxKind::DualDelay | MixerFxKind::DottedDelay | MixerFxKind::Crush) {
            tone_g(tone, sr)
        } else {
            0.0
        };
        let x = [if x.0.is_finite() { x.0 } else { 0.0 }, if x.1.is_finite() { x.1 } else { 0.0 }];
        let mut y = [0.0f32; 2];
        match kind {
            MixerFxKind::Filter => {
                let t = tone.abs();
                let fc = if tone < 0.0 { 20_000.0 * 0.001f32.powf(t) } else { 20.0 * 1000f32.powf(t) };
                let (g, k) = (svf_g(fc, sr), SVF_K * (1.0 - 0.8 * feedback));
                for c in 0..2 {
                    let (lp, _, hp) = self.svf[c].tick_k(x[c], g, k);
                    y[c] = if tone < 0.0 { lp } else { hp };
                }
            }
            MixerFxKind::Reverb => {
                let scale = (0.5 + amount) * sr / 44_100.0;
                let room = 0.72 + 0.24 * feedback;
                let damp_a = (1.0 - (-TAU * 12_000.0 / sr).exp()).clamp(0.1, 0.95);
                let input = (x[0] + x[1]) * 0.5 * 0.25;
                for c in 0..2 {
                    let spread = c as f32 * STEREO_SPREAD;
                    let mut acc = 0.0;
                    for (i, len) in COMB_LENS.iter().enumerate() {
                        let out = self.combs[c][i].read((len + spread) * scale);
                        let lp = &mut self.comb_lp[c][i];
                        *lp += (out - *lp) * damp_a;
                        if lp.abs() < 1e-15 {
                            *lp = 0.0;
                        }
                        self.combs[c][i].write(soft_clip(input + *lp * room));
                        acc += out;
                    }
                    for (i, len) in ALLPASS_LENS.iter().enumerate() {
                        let ap = &mut self.allpass[c][i];
                        let b = ap.read((len + spread) * sr / 44_100.0);
                        // Unity-magnitude Schroeder allpass: H(z) = (z^-M - g) / (1 - g z^-M), g = 0.5.
                        let v = acc + b * 0.5;
                        ap.write(v);
                        acc = b - 0.5 * v;
                    }
                    y[c] = tone_tick(&mut self.tone[c], acc, tone, tg);
                }
            }
            MixerFxKind::DualDelay => {
                let fb = 0.9 * feedback;
                for (c, beats) in [0.5f32, 0.75].into_iter().enumerate() {
                    y[c] = self.delay[c].read(beats * mult * beat);
                    let back = tone_tick(&mut self.tone[c], y[c], tone, tg);
                    self.delay[c].write(soft_clip(x[c] + back * fb));
                }
            }
            MixerFxKind::DottedDelay => {
                let (fb, d) = (0.9 * feedback, 0.75 * mult * beat);
                y = [self.delay[0].read(d), self.delay[1].read(d)];
                let back_l = tone_tick(&mut self.tone[0], y[0], tone, tg);
                let back_r = tone_tick(&mut self.tone[1], y[1], tone, tg);
                // Ping-pong: input enters on the left, every echo crosses to the other side.
                self.delay[0].write(soft_clip((x[0] + x[1]) * 0.5 + back_r * fb));
                self.delay[1].write(soft_clip(back_l * fb));
            }
            MixerFxKind::Noise => {
                let fc = (1_000.0 * 20f32.powf(tone)).min(sr * 0.45); // 50 Hz .. 20 kHz
                let (g, k) = (svf_g(fc, sr), 1.0 - 0.8 * feedback);
                // Keep loudness roughly even as the band narrows at low cutoffs.
                let level = amount * 0.25 * (sr / (fc * k)).sqrt().min(8.0);
                for c in 0..2 {
                    self.rng ^= self.rng << 13;
                    self.rng ^= self.rng >> 17;
                    self.rng ^= self.rng << 5;
                    let n = self.rng as i32 as f32 / i32::MAX as f32;
                    y[c] = self.svf[c].tick_k(n, g, k).1 * k * level;
                }
            }
            MixerFxKind::TimeGater => {
                // shortcut: free-running from engage, not locked to the master beat phase; pass the phase in if gates must land on the grid.
                let open = (self.beats / (0.5 * mult) as f64).fract() < 0.5; // 1/8 note at ×1
                let target = if open { 1.0 } else { 1.0 - feedback };
                self.gate += (target - self.gate) * (1.0 / (0.002 * sr)).min(1.0); // ~2 ms ramps, no clicks
                y = [x[0] * self.gate, x[1] * self.gate];
            }
            MixerFxKind::Flanger => {
                let sign = if tone < 0.0 { -1.0 } else { 1.0 };
                let phase = (self.beats / (8.0 * mult) as f64).fract() as f32;
                for c in 0..2 {
                    let lfo = 0.5 - 0.5 * (TAU * (phase + 0.25 * c as f32)).cos();
                    let tap = sign * self.delay[c].read((0.5 + 5.5 * lfo) * 0.001 * sr);
                    self.delay[c].write(soft_clip(x[c] + tap * 0.85 * feedback));
                    y[c] = (x[c] + tap) * FRAC_1_SQRT_2;
                }
            }
            MixerFxKind::BarberPole => {
                let phase = (self.beats / (2.0 * mult) as f64).fract() as f32;
                for c in 0..2 {
                    let mut wet = 0.0;
                    for offset in [0.0, 0.5] {
                        let p = (phase + offset + 0.25 * c as f32).fract();
                        let sweep = if tone < 0.0 { p } else { 1.0 - p }; // shrinking delay = rising notches
                        let window = 0.5 - 0.5 * (TAU * p).cos(); // the two voices' windows sum to 1 and hide each wrap
                        wet += window * self.delay[c].read((0.5 + 5.5 * sweep) * 0.001 * sr);
                    }
                    self.delay[c].write(soft_clip(x[c] + wet * 0.7 * feedback));
                    y[c] = (x[c] + wet) * FRAC_1_SQRT_2;
                }
            }
            MixerFxKind::Crush => {
                let steps = 2f32.powf(16.0 - 13.0 * amount); // 16 -> 3 bits
                let period = 1.0 + 31.0 * amount * (sr / 48_000.0); // sample-and-hold decimation, rate-normalized
                self.hold_count += 1.0;
                if self.hold_count >= period {
                    self.hold_count -= period;
                    self.hold = x.map(|s| (s * steps).round() / steps);
                }
                for c in 0..2 {
                    y[c] = tone_tick(&mut self.tone[c], self.hold[c], tone, tg);
                }
            }
        }
        (y[0], y[1])
    }
}

/// Bipolar DJ Channel Mixer FX: `knob` `0.5` is dry, below is the dark / LPF / short side,
/// above the bright / HPF / long side; the distance from center sets the depth.
#[derive(Debug, Clone)]
pub struct BipolarFilter {
    pub kind: MixerFxKind,
    pub knob: f32,
    pub enabled: bool,
    pos: f32,
    last_sign: f32,
    core: FxCore,
}

impl Default for BipolarFilter {
    fn default() -> Self {
        Self { kind: MixerFxKind::Filter, knob: 0.5, enabled: false, pos: 0.0, last_sign: 1.0, core: FxCore::default() }
    }
}

pub type MixerFxProcessor = BipolarFilter;

impl BipolarFilter {
    /// One frame at 48 kHz / 128 BPM; prefer [`Self::process_at`] with the real rate and tempo.
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.process_at(left, right, 48_000.0, 128.0)
    }

    pub fn process_at(&mut self, in_l: f32, in_r: f32, sample_rate: f32, bpm: f32) -> (f32, f32) {
        let in_l = if in_l.is_finite() { in_l } else { 0.0 };
        let in_r = if in_r.is_finite() { in_r } else { 0.0 };
        let knob = if self.knob.is_finite() { self.knob } else { 0.5 };
        let target = if self.enabled && (knob - 0.5).abs() >= 0.01 { ((knob - 0.5) * 2.0).clamp(-1.0, 1.0) } else { 0.0 };
        self.pos = approach(self.pos, target, if sample_rate.is_finite() { sample_rate } else { 48_000.0 });
        let (pos, d) = (self.pos, self.pos.abs());
        if pos.abs() > 0.01 {
            self.last_sign = pos.signum();
        }
        // Keep the active tap side while a send tail rings out back through center.
        let side = |below: f32, above: f32| if self.last_sign < 0.0 { below } else { above };
        let [amount, feedback] = match self.kind {
            MixerFxKind::Filter => [0.5, 0.35],
            // Fixed room size avoids pitch-scrubbing comb delays as the knob returns to center.
            MixerFxKind::Reverb => [0.6, 0.25 + 0.7 * d],
            MixerFxKind::DualDelay => [0.5, 0.3 + 0.55 * d],
            MixerFxKind::DottedDelay => [side(0.5, 0.75), 0.3 + 0.55 * d], // dotted 1/8 or dotted 1/4
            MixerFxKind::Noise => [1.0, 0.5],
            MixerFxKind::TimeGater => [side(0.5, 0.25), d], // 1/8 or 1/16 notes
            MixerFxKind::Flanger | MixerFxKind::BarberPole => [0.5, d],
            MixerFxKind::Crush => [d, 0.0],
        };
        let Some((wl, wr)) = self.core.run(self.kind, d, (in_l, in_r), [amount, feedback, pos], sample_rate, bpm) else {
            return (in_l, in_r);
        };
        if FxCore::is_send(self.kind) {
            (in_l + wl, in_r + wr)
        } else {
            let m = (4.0 * d).min(1.0);
            (in_l + (wl - in_l) * m, in_r + (wr - in_r) * m)
        }
    }
}

/// Stereo deck FX unit (FX1 / FX2) built on the same engine as the mixer FX.
/// `params[0]`: Filter cutoff (`0.5` open, below LPF, above HPF), Reverb size, delay / gate / LFO
/// time (snapped ×¼..×4), Noise / Crush amount. `params[1]`: feedback / resonance / decay / gate depth.
/// `params[2]`: tone (`0.5` neutral, below dark, above bright; Flanger polarity, BarberPole direction).
#[derive(Debug, Clone)]
pub struct DeckFxUnit {
    pub enabled: bool,
    pub kind: MixerFxKind,
    pub dry_wet: f32,
    pub params: [f32; 3],
    on: f32,
    core: FxCore,
}

impl Default for DeckFxUnit {
    fn default() -> Self {
        Self { enabled: false, kind: MixerFxKind::Reverb, dry_wet: 0.5, params: [0.5; 3], on: 0.0, core: FxCore::default() }
    }
}

impl DeckFxUnit {
    pub fn process(&mut self, in_l: f32, in_r: f32, sample_rate: f32, bpm: f32) -> (f32, f32) {
        let in_l = if in_l.is_finite() { in_l } else { 0.0 };
        let in_r = if in_r.is_finite() { in_r } else { 0.0 };
        let dw = if self.dry_wet.is_finite() { self.dry_wet.clamp(0.0, 1.0) } else { 0.0 };
        let sr = if sample_rate.is_finite() { sample_rate } else { 48_000.0 };
        let target = if self.enabled && dw > 0.001 { 1.0 } else { 0.0 };
        self.on = approach(self.on, target, sr);
        let [p0, p1, p2] = self.params.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 });
        let tone = (if self.kind == MixerFxKind::Filter { p0 } else { p2 } - 0.5) * 2.0;
        let Some((wl, wr)) = self.core.run(self.kind, self.on, (in_l, in_r), [p0, p1, tone], sr, bpm) else {
            return (in_l, in_r);
        };
        let mut m = self.on * dw;
        if self.kind == MixerFxKind::Filter {
            // Smoothly pass dry through center so crossing LPF <-> HPF never clicks.
            m *= (tone.abs() * 4.0).min(1.0);
        }
        if FxCore::is_send(self.kind) {
            // The tail keeps its level after switching off so it rings out instead of cutting.
            (in_l * (1.0 - m) + wl * dw, in_r * (1.0 - m) + wr * dw)
        } else {
            (in_l + (wl - in_l) * m, in_r + (wr - in_r) * m)
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
    /// `(boost_db, ceiling_db, sample_rate)` that `cache` = `(pre_gain, ceiling, release)` was computed for.
    cache_key: (f32, f32, f32),
    cache: (f32, f32, f32),
}

impl Default for OzoneMaximizerDsp {
    fn default() -> Self {
        Self {
            enabled: true,
            boost_db: 0.0,
            ceiling_db: -0.1,
            envelope: 0.0,
            cache_key: (f32::NAN, f32::NAN, f32::NAN),
            cache: (1.0, 1.0, 0.995),
        }
    }
}

impl OzoneMaximizerDsp {
    /// One frame at 48 kHz; prefer [`Self::process_stereo_at`] with the real rate.
    pub fn process_stereo(&mut self, left: f32, right: f32) -> (f32, f32) {
        self.process_stereo_at(left, right, 48_000.0)
    }

    pub fn process_stereo_at(&mut self, left: f32, right: f32, sample_rate: f32) -> (f32, f32) {
        let left = if left.is_finite() { left } else { 0.0 };
        let right = if right.is_finite() { right } else { 0.0 };
        if !self.enabled {
            return (left, right);
        }
        let boost_db = if self.boost_db.is_finite() { self.boost_db } else { 0.0 };
        let ceiling_db = if self.ceiling_db.is_finite() { self.ceiling_db } else { -0.3 };
        let sr = if sample_rate.is_finite() { sample_rate.max(1.0) } else { 48_000.0 };
        let key = (boost_db, ceiling_db, sr);
        if key != self.cache_key {
            // Release is 0.995 per sample at 48 kHz (~4 ms), kept the same length in time at other rates.
            let release = 0.995_f32.powf(48_000.0 / sr);
            self.cache = (10.0_f32.powf(boost_db / 20.0), 10.0_f32.powf(ceiling_db / 20.0), release);
            self.cache_key = key;
        }
        let (pre_gain, ceiling, release) = self.cache;
        let l = left * pre_gain;
        let r = right * pre_gain;
        let peak = l.abs().max(r.abs());
        if peak > self.envelope {
            self.envelope = peak;
        } else {
            self.envelope = self.envelope * release + peak * (1.0 - release);
            if self.envelope < 1e-15 {
                self.envelope = 0.0;
            }
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

    const SR: f32 = 48_000.0;
    const ALL_FX: [MixerFxKind; 9] = [
        MixerFxKind::Filter,
        MixerFxKind::Reverb,
        MixerFxKind::DualDelay,
        MixerFxKind::Noise,
        MixerFxKind::TimeGater,
        MixerFxKind::Flanger,
        MixerFxKind::BarberPole,
        MixerFxKind::DottedDelay,
        MixerFxKind::Crush,
    ];

    /// Two-tone test signal (220 Hz + 3.5 kHz).
    fn tones(i: usize) -> f32 {
        let t = i as f32 / SR;
        0.4 * (t * 220.0 * std::f32::consts::TAU).sin() + 0.2 * (t * 3500.0 * std::f32::consts::TAU).sin()
    }

    /// Runs a mixer FX at `knob` over `n` frames of mono `input`.
    fn mixer_fx(kind: MixerFxKind, knob: f32, n: usize, input: impl Fn(usize) -> f32) -> Vec<(f32, f32)> {
        let mut fx = BipolarFilter { kind, knob, enabled: true, ..Default::default() };
        (0..n).map(|i| fx.process_at(input(i), input(i), SR, 128.0)).collect()
    }

    fn energy(v: &[(f32, f32)]) -> f32 {
        v.iter().map(|(l, r)| l * l + r * r).sum()
    }

    #[test]
    fn mixer_fx_center_is_dry_and_every_kind_is_audible_bounded_and_distinct() {
        let n = SR as usize;
        let mut outs = Vec::new();
        for kind in ALL_FX {
            let dry = mixer_fx(kind, 0.5, 4096, tones);
            assert!(dry.iter().enumerate().all(|(i, &o)| o == (tones(i), tones(i))), "{kind:?} not dry at center");
            for knob in [0.15, 0.85] {
                let out = mixer_fx(kind, knob, n, tones);
                assert!(out.iter().all(|(l, r)| l.is_finite() && r.is_finite() && l.abs() < 3.0 && r.abs() < 3.0), "{kind:?} {knob} unbounded");
                let diff: f32 = out.iter().enumerate().map(|(i, (l, r))| (l - tones(i)).powi(2) + (r - tones(i)).powi(2)).sum();
                assert!(diff > 0.02 * energy(&out), "{kind:?} at {knob} barely changes the signal");
            }
            outs.push(mixer_fx(kind, 0.85, n, tones));
        }
        for a in 0..outs.len() {
            for b in a + 1..outs.len() {
                let d: f32 = outs[a].iter().zip(&outs[b]).map(|(x, y)| (x.0 - y.0).powi(2) + (x.1 - y.1).powi(2)).sum();
                assert!(d > 0.01 * energy(&outs[a]), "{:?} sounds like {:?}", ALL_FX[a], ALL_FX[b]);
            }
        }
    }

    #[test]
    fn mixer_fx_sends_leave_tails_and_noise_rises_from_silence() {
        // Impulse after the knob glide settles, then silence; listen 0.25..1.5 s later.
        let impulse = |i: usize| if i == 4800 { 1.0 } else { 0.0 };
        for kind in [MixerFxKind::Reverb, MixerFxKind::DualDelay, MixerFxKind::DottedDelay] {
            let out = mixer_fx(kind, 0.85, 4800 + 72_000, impulse);
            assert!(energy(&out[4800 + 12_000..]) > 1e-3, "{kind:?} has no tail");
            assert!(out.iter().all(|(l, r)| l.is_finite() && r.is_finite()));
        }
        // Dual delay: first echoes at 1/2 beat left, 3/4 beat right (128 BPM).
        let out = mixer_fx(MixerFxKind::DualDelay, 0.85, 4800 + 20_000, impulse);
        let peak = |ch: fn(&(f32, f32)) -> f32| (4801..out.len()).max_by(|&a, &b| ch(&out[a]).abs().total_cmp(&ch(&out[b]).abs())).unwrap() - 4800;
        assert!((peak(|f| f.0) as i32 - 11_250).abs() < 3 && (peak(|f| f.1) as i32 - 16_875).abs() < 3);

        let noise = mixer_fx(MixerFxKind::Noise, 0.9, 48_000, |_| 0.0);
        let rms = (energy(&noise[24_000..]) / 48_000.0).sqrt();
        assert!(rms > 0.05 && rms < 1.0, "noise rms {rms}");
    }

    #[test]
    fn time_gater_chops_at_tempo_rate() {
        // 1/8 notes below center, 1/16 above: at 128 BPM over 3 s that is 12.8 and 25.6 cycles.
        for (knob, cycles) in [(0.1, 12.8), (0.9, 25.6)] {
            let out = mixer_fx(MixerFxKind::TimeGater, knob, 3 * SR as usize, |_| 0.5);
            let opens = out.windows(2).filter(|w| w[0].0 < 0.3 && w[1].0 >= 0.3).count() as f32;
            assert!((opens - cycles).abs() <= 1.5, "knob {knob}: {opens} gates");
            // ~2 ms ramps: no sample-to-sample jump bigger than a small slice of the swing.
            assert!(out.windows(2).all(|w| (w[1].0 - w[0].0).abs() < 0.01), "gater clicks");
        }
    }

    #[test]
    fn filter_sweeps_lowpass_below_and_highpass_above() {
        let rms_of = |knob: f32, hz: f32| {
            let out = mixer_fx(MixerFxKind::Filter, knob, 24_000, |i| (i as f32 * hz * std::f32::consts::TAU / SR).sin());
            (energy(&out[12_000..]) / 24_000.0).sqrt()
        };
        assert!(rms_of(0.15, 8000.0) < 0.05 && rms_of(0.15, 100.0) > 0.6, "LPF side");
        assert!(rms_of(0.85, 100.0) < 0.05 && rms_of(0.85, 8000.0) > 0.6, "HPF side");
    }

    #[test]
    fn deck_fx_unit_passes_dry_when_off_and_adds_wet_tails_when_on() {
        let mut off = DeckFxUnit { enabled: false, dry_wet: 1.0, ..Default::default() };
        assert!((0..2000).all(|i| off.process(tones(i), -tones(i), SR, 128.0) == (tones(i), -tones(i))));

        for kind in [MixerFxKind::Reverb, MixerFxKind::DualDelay] {
            let mut fx = DeckFxUnit { enabled: true, kind, dry_wet: 0.5, ..Default::default() };
            let out: Vec<_> = (0..60_000).map(|i| fx.process(if i == 4800 { 1.0 } else { 0.0 }, 0.0, SR, 128.0)).collect();
            assert!(energy(&out[4800 + 12_000..]) > 1e-3, "{kind:?} has no tail");
            // Switching off lets the tail ring out, then the unit goes back to exact dry.
            fx.enabled = false;
            let rest: Vec<_> = (0..10 * SR as usize).map(|i| fx.process(tones(i), tones(i), SR, 128.0)).collect();
            assert!(rest.iter().all(|(l, r)| l.is_finite() && r.is_finite()));
            assert_eq!(fx.process(0.25, -0.25, SR, 128.0), (0.25, -0.25), "{kind:?} not dry after tail");
        }
    }

    #[test]
    fn dsp_guards_nan_and_filter_center_crossing_is_click_free() {
        let mut fx = DeckFxUnit { enabled: true, kind: MixerFxKind::Filter, dry_wet: 1.0, params: [0.5, 0.5, 0.5], ..Default::default() };
        let mut prev = 0.0f32;
        for i in 0..4800 {
            let x = (i as f32 * 440.0 * std::f32::consts::TAU / SR).sin();
            fx.params[0] = 0.45 + 0.10 * (i as f32 / 4800.0);
            let (l, r) = fx.process(x, x, SR, 128.0);
            assert!(l.is_finite() && r.is_finite());
            if i > 100 {
                assert!((l - prev).abs() < 0.15, "click across Filter center at step {i}: {} -> {}", prev, l);
            }
            prev = l;
        }
        let mut bf = BipolarFilter { enabled: true, kind: MixerFxKind::Reverb, knob: f32::NAN, ..Default::default() };
        let (l, r) = bf.process_at(f32::NAN, 0.5, f32::NAN, f32::NAN);
        assert!(l.is_finite() && r.is_finite());
        let mut oz = OzoneMaximizerDsp { boost_db: f32::NAN, ceiling_db: f32::NAN, ..Default::default() };
        let (l, r) = oz.process_stereo_at(f32::NAN, 0.5, f32::NAN);
        assert!(l.is_finite() && r.is_finite());
    }

    #[test]
    fn ozone_uses_cached_db_gains_and_holds_the_ceiling() {
        let mut ozone = OzoneMaximizerDsp { boost_db: 6.0, ..Default::default() };
        assert_eq!(ozone.process_stereo(0.1, -0.1), (0.1 * 10f32.powf(0.3), -0.1 * 10f32.powf(0.3)));
        ozone.boost_db = 3.0;
        assert_eq!(ozone.process_stereo(0.1, 0.1).0, 0.1 * 10f32.powf(0.15));
        let ceiling = 10f32.powf(-0.1 / 20.0);
        ozone.boost_db = 12.0;
        for i in 0..4800 {
            let (l, r) = ozone.process_stereo_at(tones(i) * 2.0, tones(i), 96_000.0);
            assert!(l.abs() <= ceiling && r.abs() <= ceiling);
        }
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
        let mut ozone = OzoneMaximizerDsp { boost_db: 6.0, ..Default::default() };
        let (ol, or) = ozone.process_stereo(0.9, -0.9);
        assert!(ol.abs() <= 1.0 && or.abs() <= 1.0);
    }
}
