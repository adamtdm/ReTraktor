//! Key lock: change tempo without changing pitch.
//!
//! WSOLA time-stretch over the track already in memory. Every hop it takes a windowed grain at
//! the deck's playhead, nudged (within `tolerance`) to the offset that best continues the previous
//! grain, and overlap-adds it. Grains always start at the playhead, so seeks, loops and sync work
//! unchanged, and a jump simply crossfades over one hop.

use crate::audio::sample_stereo_hermite;

/// Grain length in output frames (about 43 ms at 48 kHz).
const GRAIN: usize = 2048;
const HOP: usize = GRAIN / 2;
/// Up to the main mix plus four stems, all aligned the same way.
pub const MAX_SOURCES: usize = 5;

pub struct KeyLock {
    acc: Vec<[f32; MAX_SOURCES * 2]>,
    window: Vec<f32>,
    pos: usize,
    /// Source frame where the previous grain would naturally continue.
    next_cont: Option<f64>,
    reference: Vec<f32>,
}

impl Default for KeyLock {
    fn default() -> Self {
        // Periodic Hann: two half-overlapping windows sum to exactly 1.
        let window = (0..GRAIN).map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / GRAIN as f32).cos()).collect();
        Self { acc: vec![[0.0; MAX_SOURCES * 2]; GRAIN], window, pos: 0, next_cont: None, reference: Vec::new() }
    }
}

/// Where in the source to read, folded back into an active loop.
fn wrap(p: f64, loop_frames: Option<(f64, f64)>) -> f64 {
    match loop_frames {
        Some((start, len)) if len > 1.0 && p >= start + len => start + (p - start).rem_euclid(len),
        _ => p,
    }
}

impl KeyLock {
    /// Re-primes from the playhead on the next frame (after a scratch or when key lock turns on).
    pub fn reset(&mut self) {
        self.pos = 0;
        self.next_cont = None;
        self.acc.iter_mut().for_each(|f| *f = [0.0; MAX_SOURCES * 2]);
    }

    /// The next output frame for each source.
    /// `playhead` is the deck position in source frames; `step` is source frames per output frame at 1×
    /// (track rate / output rate); `high_quality` widens the alignment search.
    pub fn next(
        &mut self,
        sources: &[&[f32]],
        playhead: f64,
        step: f64,
        loop_frames: Option<(f64, f64)>,
        high_quality: bool,
    ) -> [(f32, f32); MAX_SOURCES] {
        let n_src = sources.len().min(MAX_SOURCES);
        if self.pos == 0 && n_src > 0 {
            self.acc.copy_within(HOP.., 0);
            self.acc[HOP..].iter_mut().for_each(|f| *f = [0.0; MAX_SOURCES * 2]);
            let start = match self.next_cont {
                Some(cont) => self.best_start(sources[0], cont, playhead, step, loop_frames, high_quality),
                None => {
                    // Prime with the second half of a grain that would have started one hop earlier.
                    let prev = playhead - HOP as f64 * step;
                    self.add_grain(sources, n_src, prev, step, loop_frames, HOP);
                    playhead
                }
            };
            self.add_grain(sources, n_src, start, step, loop_frames, 0);
            self.next_cont = Some(start + HOP as f64 * step);
        }
        let frame = self.acc[self.pos];
        self.pos = (self.pos + 1) % HOP;
        std::array::from_fn(|s| (frame[s * 2], frame[s * 2 + 1]))
    }

    /// Adds window[from..] of a grain starting at source frame `start` into acc[0..].
    fn add_grain(&mut self, sources: &[&[f32]], n_src: usize, start: f64, step: f64, loop_frames: Option<(f64, f64)>, from: usize) {
        for i in from..GRAIN {
            let w = self.window[i];
            let p = wrap(start + i as f64 * step, loop_frames);
            let slot = &mut self.acc[i - from];
            for (s, src) in sources.iter().take(n_src).enumerate() {
                let (l, r) = sample_stereo_hermite(src, p);
                slot[s * 2] += w * l;
                slot[s * 2 + 1] += w * r;
            }
        }
    }

    /// The grain start near `target` whose opening best matches the natural continuation `cont`.
    /// Searches every 4th frame of a mono copy, then refines the winner frame by frame.
    fn best_start(&mut self, src: &[f32], cont: f64, target: f64, step: f64, loop_frames: Option<(f64, f64)>, hq: bool) -> f64 {
        const D: usize = 4;
        let tolerance: usize = if hq { 480 } else { 240 };
        let frames = src.len() / 2;
        let mono = |p: f64| {
            let i = (wrap(p, loop_frames).max(0.0) as usize).min(frames.saturating_sub(1));
            src.get(i * 2).map_or(0.0, |l| l + src[i * 2 + 1])
        };
        let span = ((HOP as f64 * step) as usize / D).max(1);
        self.reference.clear();
        self.reference.extend((0..span).map(|j| mono(cont + (j * D) as f64)));
        if self.reference.iter().all(|x| x.abs() < 1e-6) {
            return target;
        }
        let score = |reference: &[f32], base: f64| {
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for (j, &r) in reference.iter().enumerate() {
                let c = mono(base + (j * D) as f64);
                dot += r * c;
                energy += c * c;
            }
            dot / energy.sqrt()
        };
        let lo = target - tolerance as f64;
        let candidates: Vec<f32> = (0..(2 * tolerance / D + span)).map(|i| mono(lo + (i * D) as f64)).collect();
        let mut best = (f32::MIN, 0usize);
        for lag in 0..=2 * tolerance / D {
            let (mut dot, mut energy) = (0.0f32, 1e-9f32);
            for (r, c) in self.reference.iter().zip(&candidates[lag..]) {
                dot += r * c;
                energy += c * c;
            }
            let sc = dot / energy.sqrt();
            if sc > best.0 {
                best = (sc, lag);
            }
        }
        let coarse = lo + (best.1 * D) as f64;
        let mut fine = (f32::MIN, coarse);
        for off in -(D as i64)..=D as i64 {
            let base = coarse + off as f64;
            let sc = score(&self.reference, base);
            if sc > fine.0 {
                fine = (sc, base);
            }
        }
        fine.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, sr: u32, secs: f32) -> Vec<f32> {
        (0..(sr as f32 * secs) as usize)
            .flat_map(|i| {
                let v = (i as f32 * hz * std::f32::consts::TAU / sr as f32).sin() * 0.5;
                [v, v]
            })
            .collect()
    }

    /// Zero crossings per second of the left channel.
    fn pitch_hz(out: &[f32], sr: u32) -> f32 {
        let crossings = out.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count();
        crossings as f32 * sr as f32 / out.len() as f32
    }

    fn render(rate: f64) -> (Vec<f32>, f64) {
        let sr = 48_000;
        let src = sine(440.0, sr, 4.0);
        let mut kl = KeyLock::default();
        let mut playhead = sr as f64 * 0.5;
        let start = playhead;
        let mut out = Vec::new();
        for _ in 0..sr {
            out.push(kl.next(&[&src], playhead, 1.0, None, true)[0].0);
            playhead += rate;
        }
        (out, playhead - start)
    }

    #[test]
    fn keeps_pitch_while_tempo_changes() {
        for rate in [0.92, 1.0, 1.08] {
            let (out, advanced) = render(rate);
            let hz = pitch_hz(&out[HOP..], 48_000);
            assert!((hz - 440.0).abs() < 4.0, "rate {rate}: pitch {hz} Hz");
            assert!((advanced - 48_000.0 * rate).abs() < 1.0, "playhead must follow the tempo");
            // No dropouts: the level stays steady once primed.
            let quiet = out[HOP..].chunks(256).filter(|c| c.iter().fold(0.0f32, |m, x| m.max(x.abs())) < 0.3).count();
            assert_eq!(quiet, 0, "rate {rate}: level dips");
        }
    }

    #[test]
    fn unity_rate_reproduces_the_source() {
        let sr = 48_000;
        let src = sine(440.0, sr, 2.0);
        let mut kl = KeyLock::default();
        let start = 10_000.0;
        for i in 0..20_000 {
            let got = kl.next(&[&src], start + i as f64, 1.0, None, false)[0].0;
            let want = src[(10_000 + i) * 2];
            assert!((got - want).abs() < 1e-3, "frame {i}: {got} vs {want}");
        }
    }
}
