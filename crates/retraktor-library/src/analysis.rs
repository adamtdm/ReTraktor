//! Clean-room offline track analysis: BPM, beatgrid phase offset, musical key (0..=23),
//! perceived energy (1..=10), loudness/peak dBFS, and structural cue points.

use crate::KeyWheel;
use std::f32::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhraseKind {
    Intro,
    Build,
    Drop,
    Breakdown,
    Outro,
}

impl PhraseKind {
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Intro => "I",
            Self::Build => "B",
            Self::Drop => "D",
            Self::Breakdown => "BR",
            Self::Outro => "O",
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Intro => "Intro",
            Self::Build => "Build",
            Self::Drop => "Drop",
            Self::Breakdown => "Breakdown",
            Self::Outro => "Outro",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Phrase {
    pub start_sec: f64,
    pub kind: PhraseKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrackAnalysis {
    pub bpm: f64,                 // 60..=200 range, folded sensibly (prefer 70..=180 for dance music)
    pub beatgrid_offset_sec: f64, // time of first downbeat-aligned beat, seconds
    pub key_value: u8,            // Traktor MUSICAL_KEY id 0..=23 (0..11 major C..B, 12..23 minor Cm..Bm) — see KeyWheel::OPEN_KEY_TABLE
    pub key_open: String,         // Open Key text, via KeyWheel::to_open_key(key_value)
    pub energy: u8,               // 1..=10, Mixed-In-Key style perceived intensity
    pub loudness_db: f32,         // integrated loudness estimate (RMS-based or simplified LUFS), dBFS
    pub peak_db: f32,
    pub cue_points_sec: Vec<f64>, // up to 8 auto cue points at structural changes (intro end, breakdowns, drops), snapped to beatgrid, ascending
    pub phrases: Vec<Phrase>,
}

pub fn analyze_mono(samples: &[f32], sample_rate: u32) -> TrackAnalysis {
    let sr = sample_rate.max(1) as f64;
    if samples.is_empty() {
        return TrackAnalysis {
            bpm: 120.0,
            beatgrid_offset_sec: 0.0,
            key_value: 0,
            key_open: "1d".to_string(),
            energy: 1,
            loudness_db: -60.0,
            peak_db: -60.0,
            cue_points_sec: Vec::new(),
            phrases: Vec::new(),
        };
    }

    // 1. Loudness & Peak dBFS
    let (loudness_db, peak_db, hf_ratio) = compute_loudness_and_brightness(samples);

    // 2. Downsample to ~11025 Hz for fast spectral analysis
    let decim = ((sample_rate / 11_025).max(1)) as usize;
    let ds_sr = sr / (decim as f64);
    let mut ds = Vec::with_capacity(samples.len() / decim + 1);
    for chunk in samples.chunks(decim) {
        let sum: f32 = chunk.iter().copied().sum();
        ds.push(sum / (chunk.len() as f32));
    }

    // 3. Onset envelope, BPM, Beatgrid offset, Structural cue points, and Phrases
    let (bpm, beatgrid_offset_sec, onset_density, cue_points_sec, phrases) =
        estimate_rhythm_and_cues(&ds, ds_sr);

    // 4. Musical Key via multi-octave harmonic pitch-class profile (chromagram)
    let key_value = estimate_key(&ds, ds_sr as f32);
    let key_open = KeyWheel::to_open_key(key_value).unwrap_or("1d").to_string();

    // 5. Perceived Energy (1..=10) from loudness, onset density, and spectral brightness
    let loud_norm = ((loudness_db + 34.0) / 26.0).clamp(0.0, 1.0);
    let dens_norm = (onset_density / 6.0).clamp(0.0, 1.0);
    let bright_norm = (hf_ratio * 3.5).clamp(0.0, 1.0);
    let raw_energy = 1.0 + 9.0 * (0.55 * loud_norm + 0.30 * dens_norm + 0.15 * bright_norm);
    let energy = (raw_energy.round() as i32).clamp(1, 10) as u8;

    TrackAnalysis {
        bpm,
        beatgrid_offset_sec,
        key_value,
        key_open,
        energy,
        loudness_db,
        peak_db,
        cue_points_sec,
        phrases,
    }
}

fn compute_loudness_and_brightness(samples: &[f32]) -> (f32, f32, f32) {
    let mut peak = 0.0_f32;
    let mut sum_sq = 0.0_f64;
    let mut diff_sq = 0.0_f64;
    let mut prev = 0.0_f32;

    // Gate out digital silence blocks (100ms chunks) for integrated loudness
    let block_len = 4096;
    let mut gated_sq_sum = 0.0_f64;
    let mut gated_count = 0usize;

    for chunk in samples.chunks(block_len) {
        let mut b_sq = 0.0_f64;
        for &s in chunk {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
            let sq = (s as f64) * (s as f64);
            b_sq += sq;
            sum_sq += sq;
            let d = (s - prev) as f64;
            diff_sq += d * d;
            prev = s;
        }
        let b_rms = (b_sq / (chunk.len() as f64)).sqrt();
        if b_rms > 1e-4 {
            gated_sq_sum += b_sq;
            gated_count += chunk.len();
        }
    }

    let rms = if gated_count > 0 {
        (gated_sq_sum / (gated_count as f64)).sqrt() as f32
    } else {
        (sum_sq / (samples.len().max(1) as f64)).sqrt() as f32
    };
    let loudness_db = 20.0 * rms.max(1e-6).log10();
    let peak_db = 20.0 * peak.max(1e-6).log10();
    let hf_ratio = if sum_sq > 1e-12 {
        (diff_sq / sum_sq).sqrt() as f32
    } else {
        0.0
    };
    (loudness_db, peak_db, hf_ratio)
}

/// Radix-2 Cooley-Tukey in-place complex FFT. `re` and `im` must have power-of-two length.
fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && n == im.len());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let half = len >> 1;
        let ang = -TAU / (len as f32);
        let (w_sin, w_cos) = ang.sin_cos();
        let mut i = 0;
        while i < n {
            let mut u_re = 1.0_f32;
            let mut u_im = 0.0_f32;
            for k in 0..half {
                let even = i + k;
                let odd = even + half;
                let v_re = re[odd] * u_re - im[odd] * u_im;
                let v_im = re[odd] * u_im + im[odd] * u_re;
                re[odd] = re[even] - v_re;
                im[odd] = im[even] - v_im;
                re[even] += v_re;
                im[even] += v_im;
                let next_re = u_re * w_cos - u_im * w_sin;
                u_im = u_re * w_sin + u_im * w_cos;
                u_re = next_re;
            }
            i += len;
        }
        len <<= 1;
    }
}

fn estimate_rhythm_and_cues(ds: &[f32], ds_sr: f64) -> (f64, f64, f32, Vec<f64>, Vec<Phrase>) {
    // Hop size 64 samples at ~11025 Hz -> 172.27 Hz onset envelope frame rate
    let hop = 64usize;
    let env_sr = ds_sr / (hop as f64);
    let n_frames = ds.len() / hop;
    if n_frames < 32 {
        return (120.0, 0.0, 0.0, Vec::new(), Vec::new());
    }

    // 3-band energy envelopes: sub/kick (<150 Hz), mid/snare (150..1400 Hz), high/hat (>1400 Hz)
    let mut low_env = Vec::with_capacity(n_frames);
    let mut mid_env = Vec::with_capacity(n_frames);
    let mut high_env = Vec::with_capacity(n_frames);
    let mut rms_frames = Vec::with_capacity(n_frames);
    let mut lp1 = 0.0_f32;
    let mut lp2 = 0.0_f32;

    for f in 0..n_frames {
        let slice = &ds[f * hop..(f + 1) * hop];
        let mut e_low = 0.0_f32;
        let mut e_mid = 0.0_f32;
        let mut e_high = 0.0_f32;
        let mut e_full = 0.0_f32;
        for &x in slice {
            lp1 += 0.08 * (x - lp1);
            lp2 += 0.55 * (x - lp2);
            let low = lp1;
            let mid = lp2 - lp1;
            let high = x - lp2;
            e_low += low * low;
            e_mid += mid * mid;
            e_high += high * high;
            e_full += x * x;
        }
        let inv = 1.0 / (hop as f32);
        low_env.push((e_low * inv).sqrt());
        mid_env.push((e_mid * inv).sqrt());
        high_env.push((e_high * inv).sqrt());
        rms_frames.push((e_full * inv).sqrt());
    }

    // Half-wave rectified first-order difference across 3 bands
    let mut odf = vec![0.0_f32; n_frames];
    for i in 1..n_frames {
        let dl = (low_env[i] - low_env[i - 1]).max(0.0);
        let dm = (mid_env[i] - mid_env[i - 1]).max(0.0);
        let dh = (high_env[i] - high_env[i - 1]).max(0.0);
        odf[i] = (1.0 + 30.0 * (1.8 * dl + 1.3 * dm + 0.6 * dh)).ln();
    }

    // Local mean subtraction (adaptive threshold over ~0.32s window)
    let half_w = ((0.16 * env_sr) as usize).max(2);
    let mut clean_odf = vec![0.0_f32; n_frames];
    let mut running_sum = 0.0_f64;
    let mut count = 0usize;
    for &v in odf.iter().take(half_w.min(n_frames)) {
        running_sum += v as f64;
        count += 1;
    }
    let mut peak_odf = 0.0_f32;
    for i in 0..n_frames {
        if i + half_w < n_frames {
            running_sum += odf[i + half_w] as f64;
            count += 1;
        }
        if i > half_w {
            running_sum -= odf[i - half_w - 1] as f64;
            count -= 1;
        }
        let mean = (running_sum / (count.max(1) as f64)) as f32;
        let v = (odf[i] - mean).max(0.0);
        clean_odf[i] = v;
        if v > peak_odf {
            peak_odf = v;
        }
    }

    // Count salient onset peaks per second for energy estimation
    let thresh = peak_odf * 0.20;
    let mut onset_peaks = 0usize;
    for i in 1..n_frames.saturating_sub(1) {
        if clean_odf[i] > thresh
            && clean_odf[i] >= clean_odf[i - 1]
            && clean_odf[i] >= clean_odf[i + 1]
        {
            onset_peaks += 1;
        }
    }
    let duration_sec = (ds.len() as f64) / ds_sr;
    let onset_density = (onset_peaks as f32) / (duration_sec.max(1.0) as f32);

    // Multi-window autocorrelation across up to 4 active 30-second windows
    let win_len = ((30.0 * env_sr) as usize).min(n_frames);
    let mut win_starts = Vec::new();
    if n_frames <= win_len + 16 {
        win_starts.push(0);
    } else {
        let margin = (n_frames / 10).min((n_frames - win_len) / 2);
        let span = n_frames.saturating_sub(2 * margin + win_len);
        if span == 0 {
            win_starts.push((n_frames - win_len) / 2);
        } else {
            for k in 0..4 {
                win_starts.push(margin + (k * span) / 3);
            }
        }
    }

    let mut global_acf = vec![0.0_f64; win_len];
    let mut valid_wins = 0usize;
    let fft_len = (win_len * 2).next_power_of_two();
    let mut acf_re = vec![0.0_f32; fft_len];
    let mut acf_im = vec![0.0_f32; fft_len];

    for &ws in &win_starts {
        let slice = &clean_odf[ws..ws + win_len];
        acf_re.fill(0.0);
        acf_im.fill(0.0);
        acf_re[..win_len].copy_from_slice(slice);
        fft_in_place(&mut acf_re, &mut acf_im);
        for i in 0..fft_len {
            acf_re[i] = acf_re[i] * acf_re[i] + acf_im[i] * acf_im[i];
            acf_im[i] = 0.0;
        }
        fft_in_place(&mut acf_re, &mut acf_im);
        let acf_0 = ((acf_re[0] / (fft_len as f32)) / (win_len as f32)) as f64;
        if acf_0 > 1e-7 {
            for i in 0..win_len {
                let overlap = (win_len - i).max(1) as f64;
                global_acf[i] += (((acf_re[i] / (fft_len as f32)) as f64) / overlap) / acf_0;
            }
            valid_wins += 1;
        }
    }
    if valid_wins > 1 {
        for v in &mut global_acf {
            *v /= valid_wins as f64;
        }
    }

    // Subtract moving baseline from global_acf so only sharp periodic lag spikes contribute
    let acf_half_w = 18usize;
    let mut sharp_acf = vec![0.0_f64; win_len];
    for i in 1..win_len {
        let lo = i.saturating_sub(acf_half_w).max(1);
        let hi = (i + acf_half_w + 1).min(win_len);
        let local_mean: f64 = global_acf[lo..hi].iter().sum::<f64>() / ((hi - lo).max(1) as f64);
        sharp_acf[i] = (global_acf[i] - local_mean).max(0.0);
    }

    let sample_acf = |lag: f64| -> f64 {
        let idx = lag.floor() as usize;
        if idx + 1 >= sharp_acf.len() || idx == 0 {
            return 0.0;
        }
        let frac = lag - (idx as f64);
        let c = sharp_acf[idx] * (1.0 - frac) + sharp_acf[idx + 1] * frac;
        let l = sharp_acf[idx.saturating_sub(1)];
        let r = sharp_acf[(idx + 2).min(sharp_acf.len() - 1)];
        c.max(0.70 * l).max(0.70 * r)
    };

    // Comb-filter score across candidate BPMs (68.0..=180.0 in 0.1 BPM steps)
    let mut best_bpm = 120.0_f64;
    let mut best_score = -1.0_f64;
    let mut step = 680;
    while step <= 1800 {
        let cand_bpm = (step as f64) * 0.1;
        let base_lag = 60.0 * env_sr / cand_bpm;
        let s1 = sample_acf(base_lag);
        let s2 = sample_acf(base_lag * 2.0);
        let s3 = sample_acf(base_lag * 3.0);
        let s4 = sample_acf(base_lag * 4.0);
        let s8 = sample_acf(base_lag * 8.0);
        let s_half = sample_acf(base_lag * 0.5);
        let s_third = sample_acf(base_lag / 3.0);

        let comb = 1.00 * s1 + 0.60 * s2 + 0.25 * s3 + 0.45 * s4 + 0.25 * s8
            - 0.30 * s_half
            - 0.20 * s_third;

        // Perceptual tempo prior centered around 114 BPM
        let log_diff = (cand_bpm / 114.0).ln();
        let prior = (-0.5 * (log_diff / 0.52).powi(2)).exp();
        let score = comb * (0.86 + 0.14 * prior);
        if score > best_score {
            best_score = score;
            best_bpm = cand_bpm;
        }
        step += 1;
    }


    // Fine-tune BPM around best_bpm (±1.2 BPM in 0.02 BPM steps) using higher lag harmonics (up to 16 beats)
    let mut refined_bpm = best_bpm;
    let mut best_fine = -1.0_f64;
    for delta in -60..=60 {
        let cand = (best_bpm + (delta as f64) * 0.02).clamp(60.0, 200.0);
        let lag = 60.0 * env_sr / cand;
        let mut sum = 0.0_f64;
        for &m in &[1usize, 2, 3, 4, 6, 8, 12, 16] {
            let l = lag * (m as f64);
            let idx = l.floor() as usize;
            if idx + 1 < sharp_acf.len() {
                let frac = l - (idx as f64);
                let val = sharp_acf[idx] * (1.0 - frac) + sharp_acf[idx + 1] * frac;
                sum += val / (m as f64).sqrt();
            }
        }
        if sum > best_fine {
            best_fine = sum;
            refined_bpm = cand;
        }
    }

    // Snap integer tempos when within ±0.42 BPM if integer lag still has strong correlation
    if (refined_bpm - refined_bpm.round()).abs() < 0.42 {
        let rounded = refined_bpm.round();
        let lag_r = 60.0 * env_sr / rounded;
        let lag_f = 60.0 * env_sr / refined_bpm;
        if sample_acf(lag_r) + sample_acf(lag_r * 2.0) + sample_acf(lag_r * 4.0)
            >= 0.90 * (sample_acf(lag_f) + sample_acf(lag_f * 2.0) + sample_acf(lag_f * 4.0))
        {
            refined_bpm = rounded;
        }
    }

    // Beatgrid phase alignment: find phase in [0, beat_period) maximizing onset correlation
    let beat_period_sec = 60.0 / refined_bpm;
    let n_phases = 100usize;
    let mut best_phase_sec = 0.0_f64;
    let mut best_phase_score = -1.0_f64;
    for p in 0..n_phases {
        let phase_sec = ((p as f64) / (n_phases as f64)) * beat_period_sec;
        let mut t = phase_sec;
        let mut score = 0.0_f64;
        while t < duration_sec.min(180.0) {
            let f_idx = (t * env_sr).round() as usize;
            if f_idx < clean_odf.len() {
                score += clean_odf[f_idx] as f64;
            }
            t += beat_period_sec;
        }
        if score > best_phase_score {
            best_phase_score = score;
            best_phase_sec = phase_sec;
        }
    }

    // Structural cue points: evaluate phrase boundaries (every 16 beats) for energy novelty
    let phrase_beats = 16.0;
    let phrase_sec = beat_period_sec * phrase_beats;
    let mut candidates: Vec<(f64, f32)> = Vec::new();
    if duration_sec > beat_period_sec * 4.0 {
        let rms_max = rms_frames.iter().copied().fold(0.0_f32, f32::max).max(1e-6);
        let mut first_beat = best_phase_sec;
        while first_beat < duration_sec {
            let idx = (first_beat * env_sr).round() as usize;
            if idx < rms_frames.len() && rms_frames[idx] > rms_max * 0.12 {
                break;
            }
            first_beat += beat_period_sec;
        }
        candidates.push((first_beat, 100.0));

        let win_frames = ((phrase_sec * 0.5 * env_sr) as usize).max(4);
        let mut k = 1usize;
        while first_beat + (k as f64) * phrase_sec < duration_sec - beat_period_sec * 4.0 {
            let t = first_beat + (k as f64) * phrase_sec;
            let center = (t * env_sr).round() as usize;
            if center > win_frames && center + win_frames < rms_frames.len() {
                let before: f32 = rms_frames[center - win_frames..center].iter().sum::<f32>()
                    / (win_frames as f32);
                let after: f32 = rms_frames[center..center + win_frames].iter().sum::<f32>()
                    / (win_frames as f32);
                let novelty = (after - before).abs() / rms_max;
                candidates.push((t, novelty));
            }
            k += 1;
        }
    }

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut cue_points_sec: Vec<f64> = candidates.into_iter().take(8).map(|(t, _)| t).collect();
    cue_points_sec.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let env_peaks: Vec<[f32; 3]> = (0..low_env.len())
        .map(|i| [low_env[i], mid_env[i], high_env[i]])
        .collect();
    let phrases = detect_phrases(&env_peaks, refined_bpm, best_phase_sec, duration_sec);

    (refined_bpm, best_phase_sec, onset_density, cue_points_sec, phrases)
}

/// Pure function for dance music phrase detection:
/// Works on bars (4 beats) aligned to the beat grid; computes per-bar loudness and bass energy;
/// identifies boundaries snapped to 8-bar multiples from the first downbeat; classifies segments
/// into Intro, Build, Drop, Breakdown, Outro. Returns empty vector if the track lacks clear structure.
pub fn detect_phrases(
    peaks: &[[f32; 3]],
    bpm: f64,
    grid_offset_sec: f64,
    duration_sec: f64,
) -> Vec<Phrase> {
    if bpm.is_nan() || bpm <= 30.0 || bpm > 300.0 || duration_sec <= 0.0 || peaks.is_empty() {
        return Vec::new();
    }

    let beat_len_sec = 60.0 / bpm;
    let bar_len_sec = beat_len_sec * 4.0;
    if duration_sec < bar_len_sec * 16.0 {
        return Vec::new();
    }

    let downbeat_offset_sec = if grid_offset_sec.is_finite() && grid_offset_sec >= 0.0 {
        grid_offset_sec.rem_euclid(bar_len_sec)
    } else {
        0.0
    };

    let num_bars = ((duration_sec - downbeat_offset_sec) / bar_len_sec).floor() as usize;
    if num_bars < 16 {
        return Vec::new();
    }

    let peaks_per_sec = peaks.len() as f64 / duration_sec;
    let mut bar_bass = Vec::with_capacity(num_bars);
    let mut bar_loudness = Vec::with_capacity(num_bars);

    for b in 0..num_bars {
        let t_start = downbeat_offset_sec + (b as f64) * bar_len_sec;
        let t_end = t_start + bar_len_sec;
        let i_start = ((t_start * peaks_per_sec).round() as usize).min(peaks.len());
        let i_end = ((t_end * peaks_per_sec).round() as usize).min(peaks.len());

        if i_end > i_start {
            let count = (i_end - i_start) as f32;
            let mut sum_bass = 0.0_f32;
            let mut sum_loud = 0.0_f32;
            for p in &peaks[i_start..i_end] {
                sum_bass += p[0];
                sum_loud += 0.5 * p[0] + 0.3 * p[1] + 0.2 * p[2];
            }
            bar_bass.push(sum_bass / count);
            bar_loudness.push(sum_loud / count);
        } else if let Some(&last_bass) = bar_bass.last() {
            bar_bass.push(last_bass);
            bar_loudness.push(*bar_loudness.last().unwrap_or(&0.0));
        } else {
            bar_bass.push(0.0);
            bar_loudness.push(0.0);
        }
    }

    let min_bass = bar_bass.iter().copied().fold(f32::INFINITY, f32::min);
    let max_bass = bar_bass.iter().copied().fold(0.0_f32, f32::max);
    let min_loud = bar_loudness.iter().copied().fold(f32::INFINITY, f32::min);
    let max_loud = bar_loudness.iter().copied().fold(0.0_f32, f32::max);

    let bass_range = max_bass - min_bass;
    let loud_range = max_loud - min_loud;
    if bass_range < 0.15 && loud_range < 0.15 {
        return Vec::new();
    }

    let mean_bass: f32 = bar_bass.iter().sum::<f32>() / (bar_bass.len() as f32);
    let var_bass: f32 = bar_bass.iter().map(|&x| (x - mean_bass).powi(2)).sum::<f32>() / (bar_bass.len() as f32);
    if var_bass < 0.002 {
        return Vec::new();
    }

    let n_blocks = num_bars / 8;
    if n_blocks < 2 {
        return Vec::new();
    }

    let mut block_bass = Vec::with_capacity(n_blocks);
    let mut block_loud = Vec::with_capacity(n_blocks);
    let mut block_trend = Vec::with_capacity(n_blocks);

    for k in 0..n_blocks {
        let b_start = k * 8;
        let b_mid = b_start + 4;
        let b_end = b_start + 8;

        let b_slice = &bar_bass[b_start..b_end];
        let l_slice = &bar_loudness[b_start..b_end];

        let avg_bass = b_slice.iter().sum::<f32>() / 8.0;
        let avg_loud = l_slice.iter().sum::<f32>() / 8.0;

        let first_half_loud = bar_loudness[b_start..b_mid].iter().sum::<f32>() / 4.0;
        let second_half_loud = bar_loudness[b_mid..b_end].iter().sum::<f32>() / 4.0;
        let trend = second_half_loud - first_half_loud;

        block_bass.push(avg_bass);
        block_loud.push(avg_loud);
        block_trend.push(trend);
    }

    let bass_thresh_high = min_bass + 0.55 * bass_range;
    let bass_thresh_low = min_bass + 0.35 * bass_range;

    let first_drop_idx = block_bass.iter().position(|&b| b >= bass_thresh_high);
    let Some(first_drop) = first_drop_idx else {
        return Vec::new();
    };

    let last_drop_idx = block_bass.iter().rposition(|&b| b >= bass_thresh_high).unwrap_or(first_drop);
    let mut block_kinds = vec![PhraseKind::Intro; n_blocks];

    for k in 0..n_blocks {
        if k < first_drop {
            if k + 1 == first_drop && (block_trend[k] > 0.08 || block_loud[k] > block_loud[k.saturating_sub(1)] + 0.10) {
                block_kinds[k] = PhraseKind::Build;
            } else {
                block_kinds[k] = PhraseKind::Intro;
            }
        } else if k > last_drop_idx {
            if block_bass[k] <= bass_thresh_low || block_loud[k] <= min_loud + 0.40 * loud_range {
                block_kinds[k] = PhraseKind::Outro;
            } else {
                block_kinds[k] = PhraseKind::Breakdown;
            }
        } else {
            if block_bass[k] >= bass_thresh_high {
                block_kinds[k] = PhraseKind::Drop;
            } else if k + 1 < n_blocks && block_bass[k + 1] >= bass_thresh_high && (block_trend[k] > 0.08 || block_loud[k] > block_bass[k] + 0.15) {
                block_kinds[k] = PhraseKind::Build;
            } else {
                block_kinds[k] = PhraseKind::Breakdown;
            }
        }
    }

    let mut phrases = Vec::new();
    let mut cur_kind = block_kinds[0];
    let mut cur_bar = 0usize;

    for k in 1..n_blocks {
        if block_kinds[k] != cur_kind {
            let start_sec = downbeat_offset_sec + (cur_bar as f64) * bar_len_sec;
            phrases.push(Phrase {
                start_sec,
                kind: cur_kind,
            });
            cur_kind = block_kinds[k];
            cur_bar = k * 8;
        }
    }
    let start_sec = downbeat_offset_sec + (cur_bar as f64) * bar_len_sec;
    phrases.push(Phrase {
        start_sec,
        kind: cur_kind,
    });

    phrases
}

/// Proposes hotcues at key structural points (first downbeat, first Drop, first Breakdown,
/// second Drop, Outro start), only filling empty slots and never moving or overwriting existing cues.
pub fn auto_cues(phrases: &[Phrase], existing_cues: &[(u8, f64)]) -> Vec<(u8, f64, String)> {
    if phrases.is_empty() {
        return Vec::new();
    }

    let occupied: std::collections::HashSet<u8> = existing_cues.iter().map(|(s, _)| *s).collect();
    let mut free_slots: Vec<u8> = (1..=8).filter(|s| !occupied.contains(s)).collect();
    if free_slots.is_empty() {
        return Vec::new();
    }

    let mut candidates: Vec<(f64, String)> = Vec::new();

    // 1. First downbeat
    if let Some(first) = phrases.first() {
        candidates.push((first.start_sec, "Downbeat".to_string()));
    }

    // 2. First Drop and 4. Second Drop
    let mut drops = phrases.iter().filter(|p| p.kind == PhraseKind::Drop);
    if let Some(drop1) = drops.next() {
        candidates.push((drop1.start_sec, "Drop 1".to_string()));
    }

    // 3. First Breakdown
    if let Some(breakdown) = phrases.iter().find(|p| p.kind == PhraseKind::Breakdown) {
        candidates.push((breakdown.start_sec, "Breakdown".to_string()));
    }

    // 4. Second Drop
    if let Some(drop2) = drops.next() {
        candidates.push((drop2.start_sec, "Drop 2".to_string()));
    }

    // 5. Outro start
    if let Some(outro) = phrases.iter().find(|p| p.kind == PhraseKind::Outro) {
        candidates.push((outro.start_sec, "Outro".to_string()));
    }

    // Deduplicate candidate points by time (within 0.5s of each other or existing cues)
    let mut unique_candidates: Vec<(f64, String)> = Vec::new();
    for (t, label) in candidates {
        if unique_candidates.iter().any(|(existing_t, _)| (existing_t - t).abs() < 0.5) {
            continue;
        }
        if existing_cues.iter().any(|(_, existing_t)| (existing_t - t).abs() < 0.5) {
            continue;
        }
        unique_candidates.push((t, label));
    }

    let mut result = Vec::new();
    for (t, label) in unique_candidates {
        if let Some(slot) = free_slots.first().copied() {
            free_slots.remove(0);
            result.push((slot, t, label));
        } else {
            break;
        }
    }

    result
}

fn estimate_key(ds: &[f32], ds_sr: f32) -> u8 {
    // Use 8192-sample FFT (~0.74s at 11025 Hz -> 1.34 Hz/bin resolution)
    // so bass notes down to C2 (65.4 Hz, 3.8 Hz/semitone) are cleanly resolved.
    let fft_len = 8192usize;
    let hop = 4096usize;
    if ds.len() < fft_len {
        return 0;
    }

    let mut window = vec![0.0_f32; fft_len];
    for (i, w) in window.iter_mut().enumerate() {
        *w = 0.5 * (1.0 - (TAU * (i as f32) / (fft_len as f32)).cos());
    }

    // Precompute Constant-Q / log-frequency pitch kernels for 60 MIDI notes (C2=36 to B6=95)
    let max_bin = fft_len / 2;
    let bin_hz = ds_sr / (fft_len as f32);
    let mut note_bins: Vec<Vec<(usize, f32)>> = Vec::with_capacity(60);
    for n in 0..60usize {
        let midi = 36 + n;
        let center_hz = 440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0);
        let lo_hz = center_hz * 2.0_f32.powf(-0.45 / 12.0);
        let hi_hz = center_hz * 2.0_f32.powf(0.45 / 12.0);
        let b_lo = ((lo_hz / bin_hz).floor() as usize).max(2);
        let b_hi = ((hi_hz / bin_hz).ceil() as usize).min(max_bin - 2);
        let mut weights = Vec::new();
        let mut w_sum = 0.0_f32;
        for b in b_lo..=b_hi {
            let f = (b as f32) * bin_hz;
            let semitone_diff = 12.0 * (f / center_hz).log2();
            if semitone_diff.abs() <= 0.5 {
                let w = 0.5 * (1.0 + (std::f32::consts::PI * semitone_diff / 0.5).cos());
                weights.push((b, w));
                w_sum += w;
            }
        }
        if w_sum > 1e-6 {
            for item in &mut weights {
                item.1 /= w_sum;
            }
        }
        note_bins.push(weights);
    }

    let total_frames = (ds.len() - fft_len) / hop + 1;
    let (start_f, end_f) = if total_frames > 16 {
        (total_frames / 10, total_frames - total_frames / 10)
    } else {
        (0, total_frames)
    };
    let step_f = ((end_f - start_f) / 160).max(1);

    let mut re = vec![0.0_f32; fft_len];
    let mut im = vec![0.0_f32; fft_len];
    let mut mag = vec![0.0_f32; max_bin];
    let mut chroma_acc = [0.0_f64; 12];
    let mut bass_chroma_acc = [0.0_f64; 12];

    let mut frame_idx = start_f;
    while frame_idx < end_f {
        let offset = frame_idx * hop;
        let slice = &ds[offset..offset + fft_len];

        let mut frame_energy = 0.0_f32;
        for i in 0..fft_len {
            let s = slice[i] * window[i];
            re[i] = s;
            im[i] = 0.0;
            frame_energy += s * s;
        }

        if frame_energy > 1e-5 {
            fft_in_place(&mut re, &mut im);
            for b in 1..max_bin {
                mag[b] = (re[b] * re[b] + im[b] * im[b]).sqrt();
            }

            // Evaluate 60 semitone raw energies
            let mut raw_note = [0.0_f32; 60];
            for (n, kernel) in note_bins.iter().enumerate() {
                let mut sum = 0.0_f32;
                for &(b, w) in kernel {
                    if mag[b] >= 0.75 * mag[b - 1] && mag[b] >= 0.75 * mag[b + 1] {
                        sum += mag[b] * w;
                    }
                }
                raw_note[n] = sum;
            }

            // Spectral whitening per octave to suppress broadband percussion / drum transients
            for oct in 0..5 {
                let base = oct * 12;
                let oct_mean: f32 = raw_note[base..base + 12].iter().sum::<f32>() / 12.0;
                for c in 0..12 {
                    raw_note[base + c] = (raw_note[base + c] - 0.90 * oct_mean).max(0.0);
                }
            }

            // Harmonic summation & overtone subtraction:
            // A fundamental at note `n` produces overtones at `n + 12` (2nd), `n + 19` (3rd / +7 semitones), `n + 24` (4th).
            // Subtracting a fraction of `n`'s energy from `n + 19` prevents the 3rd harmonic from skewing the chroma toward the dominant (V)!
            let mut clean_note = raw_note;
            for n in 0..60 {
                if n + 19 < 60 {
                    clean_note[n + 19] = (clean_note[n + 19] - 0.30 * raw_note[n]).max(0.0);
                }
                if n + 12 < 60 {
                    clean_note[n] += 0.25 * raw_note[n + 12];
                }
                if n + 19 < 60 {
                    clean_note[n] += 0.15 * raw_note[n + 19];
                }
            }

            // Fold octaves into 12 pitch classes and measure tonalness (peakedness of chroma)
            let mut frame_chroma = [0.0_f64; 12];
            let mut frame_bass = [0.0_f64; 12];
            const OCT_WEIGHTS: [f32; 5] = [1.2, 1.4, 1.2, 0.8, 0.4];
            for oct in 0..5 {
                let w_oct = OCT_WEIGHTS[oct];
                for c in 0..12 {
                    let v = (1.0 + 18.0 * clean_note[oct * 12 + c]).ln() * w_oct;
                    frame_chroma[c] += v as f64;
                    if oct <= 1 {
                        frame_bass[c] += v as f64;
                    }
                }
            }

            // Weight frame contribution by its tonal crest factor (max / mean) so drum-only frames have low weight
            let sum_c: f64 = frame_chroma.iter().sum();
            let max_c: f64 = frame_chroma.iter().copied().fold(0.0_f64, f64::max);
            let norm = frame_chroma.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm > 1e-6 && sum_c > 1e-6 {
                let crest = (max_c / (sum_c / 12.0) - 1.0).clamp(0.0, 3.0);
                let w_tonal = crest * crest;
                for c in 0..12 {
                    chroma_acc[c] += (frame_chroma[c] / norm) * w_tonal;
                }
            }
            let b_norm = frame_bass.iter().map(|x| x * x).sum::<f64>().sqrt();
            if b_norm > 1e-6 {
                for c in 0..12 {
                    bass_chroma_acc[c] += frame_bass[c] / b_norm;
                }
            }
        }

        frame_idx += step_f;
    }

    let total_norm = chroma_acc.iter().map(|x| x * x).sum::<f64>().sqrt().max(1e-9);
    let bass_norm = bass_chroma_acc
        .iter()
        .map(|x| x * x)
        .sum::<f64>()
        .sqrt()
        .max(1e-9);
    let mut combined = [0.0_f64; 12];
    for c in 0..12 {
        combined[c] = 0.72 * (chroma_acc[c] / total_norm) + 0.28 * (bass_chroma_acc[c] / bass_norm);
    }

    // Correlate against Major and Minor key profiles (0..11 Major, 12..23 Minor)
    // Notice natural minor (b3=+3, b6=+8, b7=+10) vs major (3=+4, 6=+9, 7=+11)
    const MAJOR_PROFILE: [f64; 12] = [
        6.80, 2.00, 3.50, 2.00, 4.90, 4.10, 2.20, 5.50, 2.10, 3.70, 2.30, 3.10,
    ];
    const MINOR_PROFILE: [f64; 12] = [
        6.90, 2.30, 3.60, 5.70, 2.10, 3.80, 2.40, 5.40, 4.10, 2.60, 4.60, 2.80,
    ];

    let mean_c = combined.iter().sum::<f64>() / 12.0;
    let mut c_zero = [0.0_f64; 12];
    for i in 0..12 {
        c_zero[i] = combined[i] - mean_c;
    }

    let mut best_key = 0u8;
    let mut best_corr = f64::NEG_INFINITY;

    for (mode_offset, profile) in [(0u8, &MAJOR_PROFILE), (12u8, &MINOR_PROFILE)] {
        let mean_p = profile.iter().sum::<f64>() / 12.0;
        let mut p_zero = [0.0_f64; 12];
        let mut p_norm_sq = 0.0_f64;
        for i in 0..12 {
            p_zero[i] = profile[i] - mean_p;
            p_norm_sq += p_zero[i] * p_zero[i];
        }
        let p_norm = p_norm_sq.sqrt().max(1e-9);

        for (root, &bass_val) in bass_chroma_acc.iter().enumerate() {
            let mut dot = 0.0_f64;
            for (i, &pz) in p_zero.iter().enumerate() {
                let pc = (root + i) % 12;
                dot += c_zero[pc] * pz;
            }
            let bass_root_bonus = 0.14 * (bass_val / bass_norm);
            let corr = (dot / p_norm) + bass_root_bonus;
            if corr > best_corr {
                best_corr = corr;
                best_key = mode_offset + (root as u8);
            }
        }
    }

    best_key
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_click_track(bpm: f64, sr: u32, duration_sec: f64) -> Vec<f32> {
        let n = (sr as f64 * duration_sec) as usize;
        let mut out = vec![0.0_f32; n];
        let period = 60.0 / bpm;
        let mut t = 0.15; // slight non-zero offset
        while t < duration_sec {
            let start = (t * sr as f64) as usize;
            // Short damped 100 Hz kick + 1 kHz click transient
            for i in 0..(sr as usize / 25) {
                if start + i < n {
                    let dt = (i as f32) / (sr as f32);
                    let env = (-dt * 80.0).exp();
                    let wave = (TAU * 110.0 * dt).sin() + 0.6 * (TAU * 1200.0 * dt).sin();
                    out[start + i] += 0.8 * env * wave;
                }
            }
            t += period;
        }
        out
    }

    fn synth_chord_progression(midi_chords: &[[u8; 3]], sr: u32, sec_per_chord: f64) -> Vec<f32> {
        let n_per = (sr as f64 * sec_per_chord) as usize;
        let mut out = Vec::with_capacity(n_per * midi_chords.len());
        for chord in midi_chords {
            for i in 0..n_per {
                let t = (i as f32) / (sr as f32);
                let mut sample = 0.0_f32;
                for (idx, &midi) in chord.iter().enumerate() {
                    let freq = 440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0);
                    // Root weighted slightly stronger, with 2nd harmonic
                    let w = if idx == 0 { 1.3 } else { 0.85 };
                    sample += w * ((TAU * freq * t).sin() + 0.35 * (TAU * 2.0 * freq * t).sin());
                }
                out.push(sample * 0.2);
            }
        }
        out
    }

    #[test]
    fn test_bpm_synthetic_click_tracks() {
        let sr = 44_100;
        for &target_bpm in &[128.0, 174.0, 95.0] {
            let pcm = synth_click_track(target_bpm, sr, 20.0);
            let res = analyze_mono(&pcm, sr);
            assert!(
                (res.bpm - target_bpm).abs() <= 0.5,
                "Expected {} BPM (±0.5), got {:.2} BPM",
                target_bpm,
                res.bpm
            );
            assert!(res.beatgrid_offset_sec >= 0.0 && res.beatgrid_offset_sec < 60.0 / target_bpm);
        }
    }

    #[test]
    fn test_key_synthetic_triads() {
        let sr = 44_100;
        // C Major progression: I - IV - V - I (C, F, G, C) -> key_value = 0 (C Major, "1d")
        let c_maj = synth_chord_progression(
            &[[60, 64, 67], [65, 69, 72], [67, 71, 74], [60, 64, 67]],
            sr,
            2.0,
        );
        let res_c = analyze_mono(&c_maj, sr);
        assert_eq!(res_c.key_value, 0, "Expected C Major (0), got {}", res_c.key_value);
        assert_eq!(res_c.key_open, "1d");

        // A Minor progression: i - iv - v - i (Am, Dm, Em, Am) -> key_value = 21 (A Minor, "1m")
        let a_min = synth_chord_progression(
            &[[57, 60, 64], [62, 65, 69], [64, 67, 71], [57, 60, 64]],
            sr,
            2.0,
        );
        let res_am = analyze_mono(&a_min, sr);
        assert_eq!(res_am.key_value, 21, "Expected A Minor (21), got {}", res_am.key_value);
        assert_eq!(res_am.key_open, "1m");

        // F# Minor progression: i - iv - v - i (F#m, Bm, C#m, F#m) -> key_value = 18 (F# Minor, "4m")
        let fs_min = synth_chord_progression(
            &[[54, 57, 61], [59, 62, 66], [61, 64, 68], [54, 57, 61]],
            sr,
            2.0,
        );
        let res_fsm = analyze_mono(&fs_min, sr);
        assert_eq!(res_fsm.key_value, 18, "Expected F# Minor (18), got {}", res_fsm.key_value);
        assert_eq!(res_fsm.key_open, "4m");
    }

    #[test]
    fn test_energy_monotonicity() {
        let sr = 44_100;
        let quiet_sparse: Vec<f32> = (0..sr as usize * 10)
            .map(|i| 0.02 * (TAU * 220.0 * (i as f32 / sr as f32)).sin())
            .collect();
        let mut loud_dense = synth_click_track(140.0, sr, 10.0);
        for (i, s) in loud_dense.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            *s = (*s + 0.6 * (TAU * 440.0 * t).sin() + 0.4 * (TAU * 3520.0 * t).sin()).clamp(-1.0, 1.0);
        }

        let res_quiet = analyze_mono(&quiet_sparse, sr);
        let res_loud = analyze_mono(&loud_dense, sr);
        assert!(
            res_loud.energy > res_quiet.energy,
            "Expected loud_dense energy ({}) > quiet_sparse energy ({})",
            res_loud.energy,
            res_quiet.energy
        );
        assert!(res_loud.loudness_db > res_quiet.loudness_db);
    }

    #[test]
    fn test_detect_phrases_synthetic_envelope() {
        let bpm = 128.0;
        let bar_sec = (60.0 / bpm) * 4.0;
        // 80 bars total: 16 intro, 16 drop, 16 breakdown, 16 drop, 16 outro
        let total_bars = 80;
        let duration_sec = total_bars as f64 * bar_sec;
        let peaks_per_sec = 60.0;
        let n_peaks = (duration_sec * peaks_per_sec).round() as usize;
        let mut peaks = vec![[0.0_f32; 3]; n_peaks];

        for i in 0..n_peaks {
            let t = i as f64 / peaks_per_sec;
            let bar = (t / bar_sec).floor() as usize;
            if bar < 16 {
                // Intro: low bass, moderate mid/high
                peaks[i] = [0.15, 0.20, 0.10];
            } else if bar < 32 {
                // Drop 1: high bass and loud
                peaks[i] = [0.85, 0.80, 0.70];
            } else if bar < 48 {
                // Breakdown: clearly lower bass than drop neighbours
                peaks[i] = [0.10, 0.40, 0.30];
            } else if bar < 64 {
                // Drop 2: high bass and loud
                peaks[i] = [0.85, 0.80, 0.70];
            } else {
                // Outro: low bass and quiet
                peaks[i] = [0.15, 0.20, 0.10];
            }
        }

        let phrases = detect_phrases(&peaks, bpm, 0.0, duration_sec);
        assert_eq!(phrases.len(), 5, "Expected 5 phrases, got {:?}", phrases);
        assert_eq!(phrases[0].kind, PhraseKind::Intro);
        assert_eq!(phrases[1].kind, PhraseKind::Drop);
        assert_eq!(phrases[2].kind, PhraseKind::Breakdown);
        assert_eq!(phrases[3].kind, PhraseKind::Drop);
        assert_eq!(phrases[4].kind, PhraseKind::Outro);

        // Check boundaries within one bar:
        assert!((phrases[0].start_sec - 0.0).abs() <= bar_sec);
        assert!((phrases[1].start_sec - 16.0 * bar_sec).abs() <= bar_sec);
        assert!((phrases[2].start_sec - 32.0 * bar_sec).abs() <= bar_sec);
        assert!((phrases[3].start_sec - 48.0 * bar_sec).abs() <= bar_sec);
        assert!((phrases[4].start_sec - 64.0 * bar_sec).abs() <= bar_sec);

        // Verify boundaries are always snapped to the bar grid
        for p in &phrases {
            let bar_idx = (p.start_sec / bar_sec).round();
            assert!(
                (p.start_sec - bar_idx * bar_sec).abs() < 1e-4,
                "Boundary {} must be exactly on the bar grid",
                p.start_sec
            );
        }
    }

    #[test]
    fn test_detect_phrases_flat_noise_and_unknown_bpm() {
        let bpm = 128.0;
        let duration_sec = 120.0;
        let peaks_flat = vec![[0.45, 0.45, 0.45]; (duration_sec * 60.0) as usize];
        let res_flat = detect_phrases(&peaks_flat, bpm, 0.0, duration_sec);
        assert!(res_flat.is_empty(), "Flat noise must yield empty phrase list");

        let res_no_bpm = detect_phrases(&peaks_flat, 0.0, 0.0, duration_sec);
        assert!(res_no_bpm.is_empty(), "0 BPM must yield empty phrase list");

        let res_nan_bpm = detect_phrases(&peaks_flat, f64::NAN, 0.0, duration_sec);
        assert!(res_nan_bpm.is_empty(), "NaN BPM must yield empty phrase list");

        let res_too_short = detect_phrases(&peaks_flat[..30], bpm, 0.0, 0.5);
        assert!(res_too_short.is_empty(), "Too short track must yield empty phrase list");
    }

    #[test]
    fn test_auto_cues_empty_and_partial_and_full_slots() {
        let phrases = vec![
            Phrase { start_sec: 0.0, kind: PhraseKind::Intro },
            Phrase { start_sec: 30.0, kind: PhraseKind::Drop },
            Phrase { start_sec: 60.0, kind: PhraseKind::Breakdown },
            Phrase { start_sec: 90.0, kind: PhraseKind::Drop },
            Phrase { start_sec: 120.0, kind: PhraseKind::Outro },
        ];

        // 1. All empty -> takes slots 1..=5
        let empty_cues = Vec::new();
        let cues = auto_cues(&phrases, &empty_cues);
        assert_eq!(cues.len(), 5);
        assert_eq!(cues[0], (1, 0.0, "Downbeat".to_string()));
        assert_eq!(cues[1], (2, 30.0, "Drop 1".to_string()));
        assert_eq!(cues[2], (3, 60.0, "Breakdown".to_string()));
        assert_eq!(cues[3], (4, 90.0, "Drop 2".to_string()));
        assert_eq!(cues[4], (5, 120.0, "Outro".to_string()));

        // 2. All 8 slots taken -> nothing added
        let all_taken: Vec<(u8, f64)> = (1..=8).map(|i| (i, i as f64 * 10.0)).collect();
        let cues_all = auto_cues(&phrases, &all_taken);
        assert!(cues_all.is_empty(), "All slots taken must yield empty list");

        // 3. Some taken -> only free slots filled, never moving or overwriting existing
        let some_taken = vec![(1, 10.0), (2, 20.0), (4, 40.0)];
        let cues_some = auto_cues(&phrases, &some_taken);
        assert_eq!(cues_some.len(), 5);
        assert_eq!(cues_some[0].0, 3);
        assert_eq!(cues_some[1].0, 5);
        assert_eq!(cues_some[2].0, 6);
        assert_eq!(cues_some[3].0, 7);
        assert_eq!(cues_some[4].0, 8);
        for &(slot, _, _) in &cues_some {
            assert!(slot != 1 && slot != 2 && slot != 4, "Must not overwrite existing cues");
        }
    }
}
