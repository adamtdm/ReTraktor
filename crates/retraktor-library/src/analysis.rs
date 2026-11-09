//! Clean-room offline track analysis: BPM, beatgrid phase offset, musical key (0..=23),
//! perceived energy (1..=10), loudness/peak dBFS, and structural cue points.

use crate::KeyWheel;
use std::f32::consts::TAU;

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

    // 3. Onset envelope, BPM, Beatgrid offset, and Structural cue points
    let (bpm, beatgrid_offset_sec, onset_density, cue_points_sec) =
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

fn estimate_rhythm_and_cues(ds: &[f32], ds_sr: f64) -> (f64, f64, f32, Vec<f64>) {
    // Hop size 64 samples at ~11025 Hz -> 172.27 Hz onset envelope frame rate
    let hop = 64usize;
    let env_sr = ds_sr / (hop as f64);
    let n_frames = ds.len() / hop;
    if n_frames < 32 {
        return (120.0, 0.0, 0.0, Vec::new());
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

    (refined_bpm, best_phase_sec, onset_density, cue_points_sec)
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
}
