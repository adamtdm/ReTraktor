use cpal::traits::{DeviceTrait, HostTrait};
#[cfg(not(test))]
use cpal::traits::StreamTrait;
use retraktor_audio::FourDeckAudioEngine;
use crate::keylock::KeyLock;
use crate::recorder::RecorderTap;
use std::f32::consts::PI;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
#[cfg(not(test))]
use std::thread;
#[cfg(not(test))]
use std::time::{Duration, Instant};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Number of waveform peak columns stored per second of audio for real-time scrolling & stripe rendering.
pub const PEAKS_PER_SEC: usize = 60;

/// Real-time audio track loaded into a deck (either decoded from disk via Symphonia or synthesized).
#[derive(Clone)]
pub struct LoadedTrackAudio {
    pub title: String,
    pub artist: String,
    pub bpm: f64,
    /// Time of the first beat; every beat is `grid_offset_sec + n * 60 / bpm`.
    pub grid_offset_sec: f64,
    pub key: String,
    pub duration_sec: f64,
    /// Interleaved [L, R, L, R, ...]. Shared, never copied: the audio callback holds its own reference.
    pub pcm_stereo: Arc<Vec<f32>>,
    pub sample_rate: u32,
    /// Precomputed 3-band `[low, mid, high]` amplitude peaks at `PEAKS_PER_SEC` Hz.
    pub waveform_peaks: Arc<[[f32; 3]]>,
    /// Precomputed 4-stem `[drums, bass, instruments, vocals]` amplitude peaks at `PEAKS_PER_SEC` Hz.
    pub stem_peaks: Arc<[[f32; 4]]>,
    pub stems_ready: bool,
    /// True isolated AI stems (from Demucs or synthesis). If Some, played with zero bleed.
    pub pcm_drums: Option<Arc<Vec<f32>>>,
    pub pcm_bass: Option<Arc<Vec<f32>>>,
    pub pcm_other: Option<Arc<Vec<f32>>>,
    pub pcm_vocals: Option<Arc<Vec<f32>>>,
    pub phrases: Vec<retraktor_library::Phrase>,
}

impl LoadedTrackAudio {
    /// Computes 3-band `[low, mid, high]` waveform peaks and 4-stem `[drums, bass, inst, vocals]` peaks.
    pub fn compute_waveform_peaks(
        pcm_stereo: &[f32],
        sample_rate: u32,
    ) -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
        let total_frames = pcm_stereo.len() / 2;
        if total_frames == 0 || sample_rate == 0 {
            return (vec![[0.1, 0.1, 0.1]; 120], vec![[0.1, 0.1, 0.1, 0.1]; 120]);
        }
        let frames_per_bucket = ((sample_rate as usize) / PEAKS_PER_SEC).max(64);
        let num_buckets = (total_frames / frames_per_bucket).max(1);
        let mut peaks = Vec::with_capacity(num_buckets);
        let mut stems = Vec::with_capacity(num_buckets);

        let mut lp_state = 0.0_f32;
        let mut mp_state = 0.0_f32;
        let mut fast_env = 0.0_f32;
        let mut slow_env = 0.0_f32;

        for b in 0..num_buckets {
            let start_f = b * frames_per_bucket;
            let end_f = ((b + 1) * frames_per_bucket).min(total_frames);
            let mut max_low = 0.0_f32;
            let mut max_mid = 0.0_f32;
            let mut max_high = 0.0_f32;

            let mut max_drums = 0.0_f32;
            let mut max_bass = 0.0_f32;
            let mut max_inst = 0.0_f32;
            let mut max_vocals = 0.0_f32;

            let n = (end_f - start_f).max(1) as f32;
            let mut sum_low = 0.0_f32;
            let mut sum_mid = 0.0_f32;
            let mut sum_high = 0.0_f32;

            for f in start_f..end_f {
                let l = pcm_stereo[f * 2];
                let r = pcm_stereo[f * 2 + 1];
                let mid_ch = (l + r) * 0.5;
                let side_ch = (l - r) * 0.5;

                // 1-pole crossover estimation for 3-band & 4-stem decomposition
                lp_state += 0.032 * (mid_ch - lp_state);
                mp_state += 0.20 * (mid_ch - mp_state);
                let low = lp_state.abs();
                let mid = (mp_state - lp_state).abs();
                let high = (mid_ch - mp_state).abs();

                let abs_in = mid_ch.abs();
                fast_env = if abs_in > fast_env {
                    fast_env + 0.35 * (abs_in - fast_env)
                } else {
                    fast_env * 0.990
                };
                slow_env += 0.006 * (abs_in - slow_env);
                let transient = (fast_env - slow_env).max(0.0);

                let drums_val = (transient * 2.1 + high * 0.65).min(1.0);
                let bass_val = (low * 1.15).min(1.0);
                let inst_val = (side_ch.abs() * 1.6 + mid * 0.55).min(1.0);
                let vocal_val = ((mid - side_ch.abs() * 0.5).max(0.0) * 1.45).min(1.0);

                sum_low += low;
                sum_mid += mid;
                sum_high += high;

                if low > max_low {
                    max_low = low;
                }
                if mid > max_mid {
                    max_mid = mid;
                }
                if high > max_high {
                    max_high = high;
                }
                if drums_val > max_drums {
                    max_drums = drums_val;
                }
                if bass_val > max_bass {
                    max_bass = bass_val;
                }
                if inst_val > max_inst {
                    max_inst = inst_val;
                }
                if vocal_val > max_vocals {
                    max_vocals = vocal_val;
                }
            }
            // Blend RMS and Peak with power-curve shaping for realistic Traktor Pro 4 transient silhouettes
            let avg_low = sum_low / n;
            let avg_mid = sum_mid / n;
            let avg_high = sum_high / n;
            let shaped_low = ((max_low * 0.65 + avg_low * 0.75) * 1.15).clamp(0.02, 0.96).powf(0.85);
            let shaped_mid = ((max_mid * 0.60 + avg_mid * 0.70) * 1.25).clamp(0.02, 0.92).powf(0.95);
            let shaped_high = ((max_high * 0.55 + avg_high * 0.65) * 1.35).clamp(0.01, 0.88).powf(1.05);

            peaks.push([shaped_low, shaped_mid, shaped_high]);
            stems.push([
                (max_drums * 0.88).clamp(0.02, 0.95).powf(0.9),
                (max_bass * 0.85).clamp(0.02, 0.92).powf(0.9),
                (max_inst * 0.78).clamp(0.02, 0.86).powf(1.05),
                (max_vocals * 0.80).clamp(0.02, 0.88).powf(1.0),
            ]);
        }
        (peaks, stems)
    }

    /// Decodes a real audio file (`.mp3`, `.flac`, `.wav`, `.ogg`, `.m4a`, `.aac`) from disk using Symphonia.
    pub fn from_file(
        path: &Path,
        fallback_title: &str,
        fallback_artist: &str,
        fallback_bpm: f64,
        fallback_key: &str,
    ) -> Result<Self, String> {
        let file = File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let mut probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| format!("Unsupported audio format in {}: {e}", path.display()))?;

        let mut title = fallback_title.to_string();
        let mut artist = fallback_artist.to_string();

        // Extract ID3 / Vorbis / MP4 metadata tags if present
        if let Some(rev) = probed.format.metadata().current() {
            for tag in rev.tags() {
                let k = tag.key.to_ascii_lowercase();
                if k == "title" || k == "tit2" {
                    let val = tag.value.to_string();
                    if !val.trim().is_empty() {
                        title = val;
                    }
                } else if k == "artist" || k == "tpe1" {
                    let val = tag.value.to_string();
                    if !val.trim().is_empty() {
                        artist = val;
                    }
                }
            }
        }

        let track = probed
            .format
            .default_track()
            .ok_or_else(|| "No audio track found in file".to_string())?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let sr = codec_params.sample_rate.unwrap_or(48_000);

        let mut decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| format!("Codec error: {e}"))?;

        let mut pcm_stereo = Vec::new();
        // Limit max decoded duration to 15 minutes to prevent excessive RAM usage on huge mixes
        let max_samples = (sr as usize) * 2 * 900;

        while let Ok(packet) = probed.format.next_packet() {
            if packet.track_id() != track_id {
                continue;
            }
            if let Ok(decoded) = decoder.decode(&packet) {
                let spec = *decoded.spec();
                let ch_count = spec.channels.count().max(1);
                let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sample_buf.copy_interleaved_ref(decoded);
                let samples = sample_buf.samples();

                if ch_count == 1 {
                    for &s in samples {
                        pcm_stereo.push(s);
                        pcm_stereo.push(s);
                    }
                } else {
                    for frame in samples.chunks(ch_count) {
                        pcm_stereo.push(frame[0]);
                        pcm_stereo.push(frame[1]);
                    }
                }

                if pcm_stereo.len() >= max_samples {
                    break;
                }
            }
        }

        if pcm_stereo.is_empty() {
            return Err("Decoded 0 audio samples from file".to_string());
        }

        let duration_sec = (pcm_stereo.len() / 2) as f64 / (sr as f64);
        let (waveform_peaks, stem_peaks) = Self::compute_waveform_peaks(&pcm_stereo, sr);
        let phrases = retraktor_library::detect_phrases(&waveform_peaks, fallback_bpm, 0.0, duration_sec);

        let mut loaded = Self {
            title,
            artist,
            bpm: fallback_bpm,
            grid_offset_sec: 0.0,
            key: fallback_key.to_string(),
            duration_sec,
            pcm_stereo: Arc::new(pcm_stereo),
            sample_rate: sr,
            waveform_peaks: waveform_peaks.into(),
            stem_peaks: stem_peaks.into(),
            stems_ready: false,
            pcm_drums: None,
            pcm_bass: None,
            pcm_other: None,
            pcm_vocals: None,
            phrases,
        };

        if let Some(dir) = crate::demucs::DemucsManager::get_cached_stem_dir(path) {
            loaded.load_stems_from_dir(&dir);
        }

        Ok(loaded)
    }

    /// Decodes an arbitrary audio file to stereo interleaved f32 PCM samples and sample rate.
    pub fn decode_file_to_pcm(path: &Path) -> Result<(Vec<f32>, u32), String> {
        let file = File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());

        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }

        let mut probed = symphonia::default::get_probe()
            .format(
                &hint,
                mss,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|e| format!("Probe error for {}: {e}", path.display()))?;

        let track = probed
            .format
            .default_track()
            .ok_or_else(|| "No audio track found in file".to_string())?;

        let track_id = track.id;
        let codec_params = track.codec_params.clone();
        let sr = codec_params.sample_rate.unwrap_or(48_000);

        let mut decoder = symphonia::default::get_codecs()
            .make(&codec_params, &DecoderOptions::default())
            .map_err(|e| format!("Codec error: {e}"))?;

        let mut pcm_stereo = Vec::new();
        let max_samples = (sr as usize) * 2 * 900;

        while let Ok(packet) = probed.format.next_packet() {
            if packet.track_id() != track_id {
                continue;
            }
            if let Ok(decoded) = decoder.decode(&packet) {
                let spec = *decoded.spec();
                let ch_count = spec.channels.count().max(1);
                let mut sample_buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
                sample_buf.copy_interleaved_ref(decoded);
                let samples = sample_buf.samples();

                if ch_count == 1 {
                    for &s in samples {
                        pcm_stereo.push(s);
                        pcm_stereo.push(s);
                    }
                } else {
                    for frame in samples.chunks(ch_count) {
                        pcm_stereo.push(frame[0]);
                        pcm_stereo.push(frame[1]);
                    }
                }

                if pcm_stereo.len() >= max_samples {
                    break;
                }
            }
        }

        if pcm_stereo.is_empty() {
            return Err("Decoded 0 audio samples from file".to_string());
        }

        Ok((pcm_stereo, sr))
    }

    /// Loads the 4 true isolated AI stems (`drums`, `bass`, `other`, `vocals`) from a directory.
    pub fn load_stems_from_dir(&mut self, stem_dir: &Path) -> bool {
        let find_stem = |name: &str| -> Option<std::path::PathBuf> {
            for ext in &["flac", "wav", "mp3", "ogg"] {
                let p = stem_dir.join(format!("{name}.{ext}"));
                if p.is_file() {
                    return Some(p);
                }
            }
            None
        };

        let drums_p = find_stem("drums");
        let bass_p = find_stem("bass");
        let other_p = find_stem("other");
        let vocals_p = find_stem("vocals");

        if let (Some(dp), Some(bp), Some(op), Some(vp)) = (drums_p, bass_p, other_p, vocals_p) {
            let drums_pcm = Self::decode_file_to_pcm(&dp).ok().map(|(p, _)| p);
            let bass_pcm = Self::decode_file_to_pcm(&bp).ok().map(|(p, _)| p);
            let other_pcm = Self::decode_file_to_pcm(&op).ok().map(|(p, _)| p);
            let vocals_pcm = Self::decode_file_to_pcm(&vp).ok().map(|(p, _)| p);

            if let (Some(d), Some(b), Some(o), Some(v)) = (drums_pcm, bass_pcm, other_pcm, vocals_pcm) {
                // Compute true stem waveform peaks from the isolated audio buffers
                let total_frames = self.pcm_stereo.len() / 2;
                let frames_per_bucket = ((self.sample_rate as usize) / PEAKS_PER_SEC).max(64);
                let num_buckets = (total_frames / frames_per_bucket).max(1);
                let mut real_peaks = Vec::with_capacity(num_buckets);

                for b_idx in 0..num_buckets {
                    let start = b_idx * frames_per_bucket * 2;
                    let end = ((b_idx + 1) * frames_per_bucket * 2).min(self.pcm_stereo.len());
                    let mut max_d = 0.0_f32;
                    let mut max_b = 0.0_f32;
                    let mut max_o = 0.0_f32;
                    let mut max_v = 0.0_f32;

                    for i in (start..end).step_by(2) {
                        if let Some(&s) = d.get(i) { max_d = max_d.max(s.abs()); }
                        if let Some(&s) = b.get(i) { max_b = max_b.max(s.abs()); }
                        if let Some(&s) = o.get(i) { max_o = max_o.max(s.abs()); }
                        if let Some(&s) = v.get(i) { max_v = max_v.max(s.abs()); }
                    }

                    real_peaks.push([
                        max_d.clamp(0.04, 1.0),
                        max_b.clamp(0.04, 1.0),
                        max_o.clamp(0.04, 1.0),
                        max_v.clamp(0.04, 1.0),
                    ]);
                }

                self.pcm_drums = Some(Arc::new(d));
                self.pcm_bass = Some(Arc::new(b));
                self.pcm_other = Some(Arc::new(o));
                self.pcm_vocals = Some(Arc::new(v));
                self.stem_peaks = real_peaks.into();
                self.stems_ready = true;
                return true;
            }
        }
        false
    }
}

/// Whether a playhead has reached the end of a track `duration_sec` long (its decoded length). The nanosecond of
/// slack absorbs the roundoff of a playhead summed frame by frame, far less than any output frame.
fn at_track_end(playhead_sec: f64, duration_sec: f64) -> bool {
    playhead_sec >= duration_sec - 1e-9
}

/// Mixxx / xwax-based Alpha-Beta tracking filter for authentic turntable physics simulation.
#[derive(Clone, Copy, Debug)]
pub struct AlphaBetaFilter {
    pub initialized: bool,
    pub dt: f64,
    pub x: f64,
    pub v: f64,
    pub alpha: f64,
    pub beta: f64,
}

impl Default for AlphaBetaFilter {
    fn default() -> Self {
        Self {
            initialized: false,
            dt: 0.001,
            x: 0.0,
            v: 0.0,
            alpha: 1.0 / 8.0,         // 0.125
            beta: (1.0 / 8.0) / 32.0, // 0.00390625
        }
    }
}

impl AlphaBetaFilter {
    pub fn init(&mut self, dt: f64, initial_v: f64, alpha: f64, beta: f64) {
        self.initialized = true;
        self.dt = dt.max(0.0001);
        self.x = 0.0;
        self.v = initial_v;
        self.alpha = alpha;
        self.beta = beta;
    }

    pub fn observation(&mut self, dx: f64, dt: f64) {
        if !self.initialized {
            self.init(dt, 0.0, 1.0 / 8.0, (1.0 / 8.0) / 32.0);
        }
        self.dt = dt.max(0.0001);
        let predicted_x = self.x + self.v * self.dt;
        let predicted_v = self.v;
        let residual_x = dx - predicted_x;

        self.x = predicted_x + residual_x * self.alpha;
        self.v = predicted_v + residual_x * self.beta / self.dt;

        self.x -= dx;
    }

    #[inline(always)]
    pub fn predicted_velocity(&self) -> f64 {
        self.v
    }
}

/// 4-point Hermite cubic spline interpolation for anti-aliased vinyl scratching.
#[inline(always)]
pub fn hermite4(y0: f32, y1: f32, y2: f32, y3: f32, frac: f32) -> f32 {
    let c0 = y1;
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * frac + c2) * frac + c1) * frac + c0
}

#[inline(always)]
pub fn sample_stereo_hermite(pcm: &[f32], sample_pos: f64) -> (f32, f32) {
    let total_frames = pcm.len() / 2;
    if total_frames < 2 {
        return (0.0, 0.0);
    }
    let clamped_pos = sample_pos.clamp(0.0, (total_frames - 1) as f64);
    let f0 = clamped_pos.floor() as usize;
    let frac = (clamped_pos - (f0 as f64)) as f32;

    if total_frames < 4 || f0 == 0 || f0 >= total_frames - 2 {
        let idx0 = f0 * 2;
        let idx1 = ((f0 + 1).min(total_frames - 1)) * 2;
        let l = pcm[idx0] * (1.0 - frac) + pcm[idx1] * frac;
        let r = pcm[idx0 + 1] * (1.0 - frac) + pcm[idx1 + 1] * frac;
        (l, r)
    } else {
        let im1 = (f0 - 1) * 2;
        let i0 = f0 * 2;
        let i1 = (f0 + 1) * 2;
        let i2 = (f0 + 2) * 2;

        let l = hermite4(pcm[im1], pcm[i0], pcm[i1], pcm[i2], frac);
        let r = hermite4(pcm[im1 + 1], pcm[i0 + 1], pcm[i1 + 1], pcm[i2 + 1], frac);
        (l, r)
    }
}

/// Where the headphone (cue) mix goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeadphoneOut {
    /// Output channels 3 and 4 (a controller's headphone socket).
    Channels34,
    /// One stereo output split in mono: left = headphones, right = main.
    Split,
    Off,
}

/// How long a beat jump eases from what was playing into the new spot.
const JUMP_EASE_SEC: f64 = 0.003;

/// Eases one playing deck across a beat jump, before the mixer. For `JUMP_EASE_SEC` the deck moves from
/// the last frame it actually played to the audio at the new playhead, always a blend of the two, so the
/// seam has no step. A jump during an ease starts a new one from whatever was last heard.
/// shortcut: holds the last frame for 3 ms, read from both positions if jump transients need their original attack.
#[derive(Clone, Copy, Default)]
struct JumpEase {
    /// The deck's last output frame before channel gain; silence while it plays nothing.
    last: (f32, f32),
    /// The frame the current ease starts from, and how many frames of it have played.
    from: Option<(f32, f32)>,
    done: u32,
    requested: bool,
}

impl JumpEase {
    /// The deck's output frame. `key_lock` drops a request: its time-stretch already crossfades a jump.
    fn next(&mut self, new: (f32, f32), sample_rate: u32, key_lock: bool) -> (f32, f32) {
        if std::mem::take(&mut self.requested) && !key_lock {
            self.from = Some(self.last);
            self.done = 0;
        }
        let out = match self.from {
            None => new,
            Some(from) => {
                let len = (JUMP_EASE_SEC * sample_rate as f64).round().max(1.0) as u32;
                self.done += 1;
                if self.done >= len {
                    self.from = None;
                }
                // Raised cosine with 0 < w <= 1, so every frame lies between `from` and the new audio.
                let w = 0.5 - 0.5 * (PI * self.done.min(len) as f32 / len as f32).cos();
                (from.0 + (new.0 - from.0) * w, from.1 + (new.1 - from.1) * w)
            }
        };
        self.last = out;
        out
    }
}

/// A playhead move the UI asked for, applied by the callback at its next handoff.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Seek {
    /// Go to this spot (cue, hot cue, a click on the track, a new track).
    To(f64),
    /// Move by this much from wherever the deck has got to (beat jump, nudge, drag, sync), so beat phase holds.
    By(f64),
}

/// Thread-safe shared state for UI <-> Audio Engine communication.
///
/// The UI and workers edit it under the mutex. The audio callback keeps its own copy (see [`Renderer`]) and
/// trades with this one once per block, so it plays on while someone else holds the lock. On `engine`, only
/// the controls the UI sets are read from here; filter and EQ state live in the callback's copy.
pub struct AudioSharedState {
    pub engine: FourDeckAudioEngine,
    pub is_playing: [bool; 4],
    /// Where each deck is, as of the callback's last handoff. Read it freely; move it only with
    /// [`AudioSharedState::seek_to`] or [`AudioSharedState::seek_by`], which also tell the callback.
    pub playhead_sec: [f64; 4],
    pending_seek: [Option<Seek>; 4],
    pub pitch_ratio: [f64; 4],
    pub loop_active: [bool; 4],
    pub loop_start_sec: [f64; 4],
    pub loop_len_sec: [f64; 4],
    pub channel_gain: [f32; 4],
    pub loaded_tracks: [Option<LoadedTrackAudio>; 4],
    pub channel_peaks: [f32; 4],
    pub master_peak_l: f32,
    pub master_peak_r: f32,
    pub cue_enabled: [bool; 4],
    /// Headphone blend: 0 = cue only, 1 = main only.
    pub cue_mix: f32,
    pub cue_volume: f32,
    pub headphone_out: HeadphoneOut,
    pub keylock: [bool; 4],
    pub keylock_hq: bool,
    keylocks: [KeyLock; 4],
    keylock_engaged: [bool; 4],
    /// The callback's eases; on the shared side only their `requested` flags are used, as requests.
    jump_ease: [JumpEase; 4],
    /// Shared side only: forget the deck's ease at the next handoff (applied before a later request).
    jump_ease_reset: [bool; 4],
    /// Callback side only: decks this copy stopped at the end of their track and hasn't yet told the shared state.
    stopped_at_end: [bool; 4],
    /// Plays asked for per deck ([`AudioSharedState::start_playing`]); the callback's copy is the count it last took,
    /// so it can tell a Play pressed while it was locked out from a play flag it has outlived.
    play_requests: [u64; 4],
    /// Callback side only: per deck, the playhead its last step left and the rounding that step lost (see
    /// [`advance_playhead`]).
    playhead_residual: [(f64, f64); 4],
    pub crossfader: f32,
    pub crossfader_curve: f32,

    // Mixxx / xwax-based Vinyl Scratching Physics
    pub scratch_active: [bool; 4],
    pub scratch_rate: [f64; 4],
    pub scratch_ticks_accum: [i64; 4],
    pub scratch_dx_per_tick: [f64; 4],
    pub scratch_filter: [AlphaBetaFilter; 4],
    pub scratch_ramping: [bool; 4],
    pub scratch_ramp_target: [f64; 4],
    /// Shared side only: a jog touch or release for the callback to pick up (its physics runs there, every block).
    scratch_changed: [bool; 4],

    // Real-Time 4-Stem Mixing per Deck: [Drums, Bass, Instruments, Vocals]
    pub stem_volumes: [[f32; 4]; 4],
    pub stem_muted: [[bool; 4]; 4],
    pub stem_fx_send: [[bool; 4]; 4],
    stem_lp_l: [f32; 4],
    stem_lp_r: [f32; 4],
    stem_mp_l: [f32; 4],
    stem_mp_r: [f32; 4],
    stem_fast_env: [f32; 4],
    stem_slow_env: [f32; 4],

    // 16-Step Pattern Player (Drum Machine) State
    pub pattern_enabled: bool,
    pub pattern_steps: [[bool; 16]; 4], // 0: Kick, 1: Clap, 2: Closed Hat, 3: Rave Perc
    pub pattern_volume: f32,
    pub pattern_pitch: f32,
    pub pattern_decay: f32,
    pub pattern_current_step: usize,
    pattern_phase_sec: f64,

    /// Track audio the callback let go of, freed by the UI thread ([`AudioSharedState::take_retired`])
    /// so a deallocation never runs in the callback. Its capacity is reserved up front.
    retired: Vec<Arc<Vec<f32>>>,
    /// Which callback state is the current one; an older one, closing late, must not write positions.
    renderer_generation: u64,
}

/// Room for this many released track buffers between two UI frames (5 per deck with stems).
const RETIRED_CAPACITY: usize = 64;

impl Default for AudioSharedState {
    fn default() -> Self {
        let mut default_steps = [[false; 16]; 4];
        // Default Techno/House groove pattern
        default_steps[0] = [
            true, false, false, false, true, false, false, false, true, false, false, false, true,
            false, false, false,
        ]; // 4x4 Kick
        default_steps[1] = [
            false, false, false, false, true, false, false, false, false, false, false, false,
            true, false, false, false,
        ]; // 2 & 4 Clap
        default_steps[2] = [
            false, false, true, false, false, false, true, false, false, false, true, false, false,
            false, true, true,
        ]; // Offbeat 909 Hat
        default_steps[3] = [
            false, false, false, true, false, false, false, false, false, true, false, false,
            false, false, true, false,
        ]; // Syncopated Rave Perc

        Self {
            engine: FourDeckAudioEngine::default(),
            is_playing: [false, false, false, false],
            playhead_sec: [0.0, 0.0, 0.0, 0.0],
            pending_seek: [None; 4],
            pitch_ratio: [1.0, 1.0, 1.0, 1.0],
            loop_active: [false, false, false, false],
            loop_start_sec: [0.0, 0.0, 0.0, 0.0],
            loop_len_sec: [8.0, 8.0, 8.0, 8.0],
            channel_gain: [0.5, 0.5, 0.5, 0.5],
            loaded_tracks: [None, None, None, None],
            channel_peaks: [0.0, 0.0, 0.0, 0.0],
            master_peak_l: 0.0,
            master_peak_r: 0.0,
            cue_enabled: [false, false, false, false],
            cue_mix: 0.0,
            cue_volume: 0.8,
            headphone_out: HeadphoneOut::Channels34,
            keylock: [false; 4],
            keylock_hq: true,
            keylocks: Default::default(),
            keylock_engaged: [false; 4],
            jump_ease: [JumpEase::default(); 4],
            jump_ease_reset: [false; 4],
            stopped_at_end: [false; 4],
            play_requests: [0; 4],
            playhead_residual: [(0.0, 0.0); 4],
            crossfader: 0.5,
            crossfader_curve: 0.5,
            scratch_active: [false; 4],
            scratch_rate: [0.0; 4],
            scratch_ticks_accum: [0; 4],
            scratch_dx_per_tick: [1.0 / 568.888889; 4],
            scratch_filter: [AlphaBetaFilter::default(); 4],
            scratch_ramping: [false; 4],
            scratch_ramp_target: [1.0; 4],
            scratch_changed: [false; 4],
            stem_volumes: [[1.0; 4]; 4],
            stem_muted: [[false; 4]; 4],
            stem_fx_send: [[false; 4]; 4],
            stem_lp_l: [0.0; 4],
            stem_lp_r: [0.0; 4],
            stem_mp_l: [0.0; 4],
            stem_mp_r: [0.0; 4],
            stem_fast_env: [0.0; 4],
            stem_slow_env: [0.0; 4],
            pattern_enabled: false,
            pattern_steps: default_steps,
            pattern_volume: 0.75,
            pattern_pitch: 0.5,
            pattern_decay: 0.6,
            pattern_current_step: 0,
            pattern_phase_sec: 0.0,
            retired: Vec::with_capacity(RETIRED_CAPACITY),
            renderer_generation: 0,
        }
    }
}

impl AudioSharedState {
    /// Deck `d` (0..4) just jumped while playing: the next frames ease across the seam.
    pub fn ease_jump(&mut self, d: usize) {
        self.jump_ease[d].requested = true;
    }

    /// Forgets deck `d`'s last frame and any pending or running ease, for a stop, a new track or a paused jump,
    /// so nothing stale is heard if it plays again before the next callback.
    pub fn reset_jump_ease(&mut self, d: usize) {
        self.jump_ease[d] = JumpEase::default();
        self.jump_ease_reset[d] = true;
    }

    /// Puts deck `d` at `sec` (a cue, a hot cue, a click on the track, a new track's start).
    pub fn seek_to(&mut self, d: usize, sec: f64) {
        self.playhead_sec[d] = sec;
        self.pending_seek[d] = Some(Seek::To(sec));
    }

    /// Moves deck `d` by `delta_sec` from wherever it has got to when the callback applies it, so a move
    /// worked out from a playhead the callback has since played past still keeps its beat phase.
    pub fn seek_by(&mut self, d: usize, delta_sec: f64) {
        self.playhead_sec[d] += delta_sec;
        self.pending_seek[d] = Some(match self.pending_seek[d] {
            Some(Seek::To(sec)) => Seek::To(sec + delta_sec),
            Some(Seek::By(sum)) => Seek::By(sum + delta_sec),
            None => Seek::By(delta_sec),
        });
    }

    /// Starts deck `d` (Play, CUP, Sync + Play Both) if it has a track. A deck parked at the end of its track
    /// starts again from the top; one moved off the end first (a cue, a click on the track) starts there, and one
    /// in an active loop that runs past the end carries on round the loop.
    pub fn start_playing(&mut self, d: usize) {
        let Some(duration) = self.loaded_tracks[d].as_ref().map(|t| t.duration_sec) else {
            self.is_playing[d] = false;
            return;
        };
        self.play_requests[d] = self.play_requests[d].wrapping_add(1);
        let looping = self.loop_active[d] && self.loop_len_sec[d] > 0.05; // as the callback judges a loop
        if !looping && at_track_end(self.playhead_sec[d], duration) {
            self.seek_to(d, 0.0);
        }
        self.is_playing[d] = true;
    }

    /// Track audio the callback no longer plays. Drop it after releasing the lock.
    pub fn take_retired(&mut self) -> Vec<Arc<Vec<f32>>> {
        self.retired.drain(..).collect()
    }

    pub fn enable_scratch(
        &mut self,
        deck: usize,
        intervals_per_rev: u32,
        rpm: f64,
        alpha: f64,
        beta: f64,
    ) {
        let d = deck.min(3);
        let intervals_per_sec = (rpm * (intervals_per_rev as f64)) / 60.0;
        self.scratch_dx_per_tick[d] = if intervals_per_sec > 0.0 {
            1.0 / intervals_per_sec
        } else {
            1.0 / 568.888889
        };
        let init_v = if self.is_playing[d] { self.pitch_ratio[d] } else { 0.0 };
        self.scratch_filter[d].init(0.002, init_v, alpha, beta);
        self.scratch_rate[d] = init_v;
        self.scratch_ticks_accum[d] = 0;
        self.scratch_active[d] = true;
        self.scratch_ramping[d] = false;
        self.scratch_changed[d] = true;
    }

    pub fn scratch_tick(&mut self, deck: usize, ticks: i64) {
        let d = deck.min(3);
        self.scratch_ticks_accum[d] += ticks;
    }

    pub fn disable_scratch(&mut self, deck: usize) {
        let d = deck.min(3);
        self.scratch_active[d] = false;
        let target = if self.is_playing[d] { self.pitch_ratio[d] } else { 0.0 };
        self.scratch_ramping[d] = true;
        self.scratch_ramp_target[d] = target;
        self.scratch_changed[d] = true;
    }

    /// Ends a scratch at once, with no release ramp (the jog leaves turntable mode).
    pub fn cancel_scratch(&mut self, deck: usize) {
        let d = deck.min(3);
        self.scratch_active[d] = false;
        self.scratch_ramping[d] = false;
        self.scratch_changed[d] = true;
    }
}

/// Headphone signal: the cue bus blended toward the main mix, then the headphone level.
fn headphone_mix(cue: (f32, f32), main: (f32, f32), mix: f32, volume: f32) -> (f32, f32) {
    let m = mix.clamp(0.0, 1.0);
    let v = volume.clamp(0.0, 1.0);
    let ch = |c: f32, o: f32| ((c * (1.0 - m) + o * m) * v).clamp(-1.0, 1.0);
    (ch(cue.0, main.0), ch(cue.1, main.1))
}

pub struct AudioSystem {
    pub shared: Arc<Mutex<AudioSharedState>>,
    _stream: Option<cpal::Stream>,
    /// What the open stream actually runs at, for the Audio status line.
    pub output_info: Option<String>,
    /// Channels of the open stream (0 when none is open).
    pub output_channels: u16,
    pub running: Arc<AtomicBool>,
    /// True while a sound card drives the playheads; otherwise a timer thread does.
    hw_active: Arc<AtomicBool>,
    pub stats: Arc<AudioStats>,
    /// The REC tap while recording ([`crate::recorder`]). Its own lock, not `shared`'s, so a busy UI never costs
    /// the take a block: the callback tries it once per block, REC start and stop hold it only to swap the tap.
    pub recorder: Arc<Mutex<Option<RecorderTap>>>,
    /// Underruns and other stream errors already logged, and when.
    logged: (u64, u64, std::time::Instant),
    #[cfg(test)]
    test_renderer: Mutex<Option<Renderer>>,
}

/// An output the user can pick; `id` is what Preferences stores ("" = system default).
#[derive(Clone, Debug, PartialEq)]
pub struct OutputChoice {
    pub id: String,
    pub label: String,
    pub channels: u16,
}

/// Outputs to offer: PipeWire/Pulse sinks (`pw:<sink name>`) when a sound server runs (from `pactl`, else from
/// PipeWire's own graph via `pw-dump`), otherwise raw ALSA devices (their cpal ID, `alsa:<device>`).
pub fn list_outputs() -> Vec<OutputChoice> {
    let sinks = std::process::Command::new("pactl")
        .args(["-f", "json", "list", "sinks"])
        .output()
        .ok()
        .and_then(|o| serde_json::from_slice::<serde_json::Value>(&o.stdout).ok());
    if let Some(serde_json::Value::Array(sinks)) = sinks {
        let found: Vec<OutputChoice> = sinks
            .iter()
            .filter_map(|v| {
                let name = v["name"].as_str()?;
                Some(OutputChoice {
                    id: format!("pw:{name}"),
                    label: v["description"].as_str().unwrap_or(name).to_string(),
                    channels: v["channel_map"].as_str().map_or(2, |m| m.split(',').count() as u16),
                })
            })
            .collect();
        if !found.is_empty() {
            return found;
        }
    }
    // PipeWire without its Pulse server: the graph itself knows each sink's width (cpal may not).
    let graph = std::process::Command::new("pw-dump").output().ok().and_then(|o| serde_json::from_slice(&o.stdout).ok());
    let found = graph.as_ref().map(graph_sinks).unwrap_or_default();
    if !found.is_empty() {
        return found;
    }
    // Raw ALSA devices, asked of ALSA itself: cpal's default host is PipeWire whenever its server runs.
    let Ok(host) = alsa_host() else { return Vec::new() };
    host.output_devices()
        .map(|devs| {
            devs.filter_map(|d| {
                Some(OutputChoice {
                    id: d.id().ok()?.to_string(),
                    label: d.description().map(|x| x.name().to_string()).unwrap_or_else(|_| d.to_string()),
                    channels: d.supported_output_configs().ok()?.map(|c| c.channels()).max()?,
                })
            })
            .collect()
        })
        .unwrap_or_default()
}

/// The audio sinks in a `pw-dump` of the PipeWire graph, with the channel count the graph gives them: the
/// negotiated or offered format, else `audio.channels`, else the length of `audio.position`. Stereo when
/// none says, never more than the node states.
fn graph_sinks(dump: &serde_json::Value) -> Vec<OutputChoice> {
    let objects = dump.as_array().map(Vec::as_slice).unwrap_or_default();
    objects
        .iter()
        .filter(|o| o["type"].as_str().is_some_and(|t| t.ends_with(":Node")))
        .filter_map(|o| {
            let props = &o["info"]["props"];
            if props["media.class"].as_str() != Some("Audio/Sink") {
                return None;
            }
            let name = props["node.name"].as_str()?;
            let params = &o["info"]["params"];
            let from_format = ["Format", "EnumFormat"]
                .iter()
                .flat_map(|k| params[*k].as_array().into_iter().flatten())
                .filter_map(|f| f["channels"].as_u64())
                .max();
            let from_props = props["audio.channels"]
                .as_u64()
                .or_else(|| props["audio.channels"].as_str()?.parse().ok())
                .or_else(|| {
                    let position = props["audio.position"].as_str()?;
                    Some(position.split(|c: char| c == ',' || c.is_whitespace() || c == '[' || c == ']').filter(|p| !p.is_empty()).count() as u64)
                })
                .filter(|&n| n > 0);
            let label = props["node.description"].as_str().or(props["node.nick"].as_str()).unwrap_or(name);
            let channels = from_format.or(from_props).unwrap_or(2).min(u16::MAX as u64) as u16;
            Some(OutputChoice { id: format!("pw:{name}"), label: label.to_string(), channels })
        })
        .collect()
}

/// One way to reach an output, tried in order by [`AudioSystem::open`].
#[derive(Clone, Debug, PartialEq)]
enum Route {
    /// PipeWire's own API, following the system default output.
    NativeDefault,
    /// A PipeWire sink by node name, with this many channels (None: as many as the node offers, up to the cue's 4).
    NativeNode { node: String, channels: Option<u16> },
    /// A sound-server sink by name through one ALSA plugin, `"pipewire"` or `"pulse"`, each tried on its own.
    AlsaBridge { plugin: &'static str, node: String, channels: u16 },
    /// A raw ALSA device, by its cpal ID.
    Alsa { id: String },
    /// ALSA's default device (on a desktop, usually the sound server's ALSA plugin).
    AlsaDefault,
}

impl Route {
    fn describe(&self) -> &'static str {
        match self {
            Route::NativeDefault | Route::NativeNode { .. } => "PipeWire",
            Route::AlsaBridge { plugin: "pipewire", .. } => "PipeWire via ALSA",
            Route::AlsaBridge { .. } => "PulseAudio via ALSA",
            Route::Alsa { .. } => "ALSA",
            Route::AlsaDefault => "ALSA default device",
        }
    }
}

/// The routes to try for `device_id`, best first: a sound-server sink (`pw:<name>`) natively when PipeWire
/// runs (`native`), then through each ALSA plugin; any other ID as the raw ALSA device it names, whatever
/// else is listed; then the system default, natively first. `listed` is the sink's channel count when the
/// sound server listed it; `sink_absent` means the server's sinks are known and this one isn't among them,
/// so only the default is tried (the ALSA plugins would quietly play to another sink).
fn routes(device_id: &str, listed: Option<u16>, sink_absent: bool, native: bool, headphones_34: bool) -> Vec<Route> {
    let width = |channels: u16| if headphones_34 && channels >= 4 { 4 } else { 2 };
    let mut list = Vec::new();
    if let Some(node) = device_id.strip_prefix("pw:").filter(|_| !sink_absent) {
        if native {
            list.push(Route::NativeNode { node: node.to_string(), channels: listed.map(width) });
        }
        for plugin in ["pipewire", "pulse"] {
            list.push(Route::AlsaBridge { plugin, node: node.to_string(), channels: listed.map_or(2, width) });
        }
    } else if !device_id.is_empty() && !device_id.starts_with("pw:") {
        list.push(Route::Alsa { id: device_id.to_string() });
    }
    if native {
        list.push(Route::NativeDefault);
    }
    list.push(Route::AlsaDefault);
    list
}

/// ALSA itself, on the platforms where cpal has it; elsewhere the default host, as before.
fn alsa_host() -> Result<cpal::Host, String> {
    #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd"))]
    return cpal::host_from_id(cpal::HostId::Alsa).map_err(|e| e.to_string());
    #[cfg(not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd")))]
    Ok(cpal::default_host())
}

/// Sets `var` to `value` while `f` runs, then restores what was there (or its absence). For an ALSA plugin,
/// which reads its target from the environment when the stream opens.
fn with_env<T>(var: &str, value: &str, f: impl FnOnce() -> T) -> T {
    let before = std::env::var_os(var);
    std::env::set_var(var, value);
    let out = f();
    match before {
        Some(v) => std::env::set_var(var, v),
        None => std::env::remove_var(var),
    }
    out
}

impl AudioSystem {
    pub fn output_info(&self) -> Option<&str> {
        self.output_info.as_deref()
    }

    /// The open output for Preferences > Audio, with the underruns the backend has reported, if any.
    pub fn status_line(&self) -> Option<String> {
        let (xruns, errors) = (self.stats.xruns.load(Ordering::Relaxed), self.stats.stream_errors.load(Ordering::Relaxed));
        self.output_info().map(|info| {
            let mut line = info.to_string();
            if xruns > 0 {
                line += &format!(" · {xruns} underruns");
            }
            if errors > 0 {
                line += &format!(" · stream error: {}", self.last_error_kind());
            }
            line
        })
    }

    /// For the UI thread each frame: after new underruns or stream errors, one stderr line, at most once a second.
    pub fn log_stream_trouble(&mut self) {
        let (xruns, errors) = (self.stats.xruns.load(Ordering::Relaxed), self.stats.stream_errors.load(Ordering::Relaxed));
        if (xruns, errors) != (self.logged.0, self.logged.1) && self.logged.2.elapsed() >= std::time::Duration::from_secs(1) {
            let busy = self.stats.busy_blocks.load(Ordering::Relaxed);
            eprintln!("Audio stream: {xruns} underruns or overruns so far ({} new); {busy} blocks played on while the UI was busy", xruns - self.logged.0);
            if errors > self.logged.1 {
                eprintln!("Audio stream error: {} ({errors} so far)", self.last_error_kind());
            }
            self.logged = (xruns, errors, std::time::Instant::now());
        }
    }

    fn last_error_kind(&self) -> String {
        let index = self.stats.last_error_kind.load(Ordering::Relaxed);
        STREAM_ERROR_KINDS.get(index).map_or_else(|| "other".to_string(), |kind| format!("{kind:?}"))
    }

    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        let running = Arc::new(AtomicBool::new(true));
        let hw_active = Arc::new(AtomicBool::new(false));
        #[cfg(not(test))]
        Self::spawn_fallback_clock(Arc::clone(&shared), Arc::clone(&running), Arc::clone(&hw_active));
        Self {
            shared,
            _stream: None,
            output_info: None,
            output_channels: 0,
            running,
            hw_active,
            stats: Arc::default(),
            recorder: Arc::default(),
            logged: (0, 0, std::time::Instant::now()),
            #[cfg(test)]
            test_renderer: Mutex::new(None),
        }
    }

    /// (Re)opens the output. `device_id` comes from [`list_outputs`] ("" = system default) or is a raw ALSA ID
    /// saved earlier. `headphones_34` asks for four channels so the cue mix can use outputs 3–4.
    /// Tries [`routes`] in order; the status line names the route that opened and any that didn't.
    #[cfg(not(test))]
    pub fn open(&mut self, device_id: &str, sample_rate: u32, buffer: u32, headphones_34: bool) {
        self._stream = None;
        self.hw_active.store(false, Ordering::Relaxed);
        self.output_info = None;
        self.output_channels = 0;

        let choices = if device_id.is_empty() { Vec::new() } else { list_outputs() };
        let choice = choices.iter().find(|c| c.id == device_id);
        #[cfg(target_os = "linux")]
        let pipewire = cpal::host_from_id(cpal::HostId::PipeWire).ok();
        #[cfg(not(target_os = "linux"))]
        let pipewire: Option<cpal::Host> = None;
        let mut failed = Vec::new();
        // The sound server's own listing decides when there is one (a sink it lists is tried, through Pulse if
        // need be, whatever another PipeWire graph holds); without it, PipeWire's nodes do.
        let sink_absent = device_id.strip_prefix("pw:").is_some_and(|node| {
            if choices.iter().any(|c| c.id.starts_with("pw:")) {
                choice.is_none()
            } else {
                pipewire.as_ref().is_some_and(|host| {
                    host.output_devices().is_ok_and(|mut devices| !devices.any(|d| d.id().is_ok_and(|id| id.id() == node)))
                })
            }
        });
        if sink_absent {
            failed.push(format!("{} is not connected", &device_id[3..]));
        }
        for route in routes(device_id, choice.map(|c| c.channels), sink_absent, pipewire.is_some(), headphones_34) {
            let attempt = match &route {
                Route::AlsaBridge { plugin, node, .. } => {
                    let var = if *plugin == "pipewire" { "PIPEWIRE_NODE" } else { "PULSE_SINK" };
                    with_env(var, node, || self.open_route(&route, pipewire.as_ref(), choice, sample_rate, buffer, headphones_34))
                }
                _ => self.open_route(&route, pipewire.as_ref(), choice, sample_rate, buffer, headphones_34),
            };
            match attempt {
                Ok(info) => {
                    let fell_back = if failed.is_empty() { String::new() } else { format!(" · fallback after {}", failed.join("; ")) };
                    self.output_info = Some(info + &fell_back);
                    return;
                }
                Err(why) => failed.push(format!("{} failed ({why})", route.describe())),
            }
        }
        eprintln!("Could not open any audio output: {}", failed.join("; "));
    }

    /// Opens one route and starts its stream; the status line on success, the reason otherwise.
    #[cfg(not(test))]
    fn open_route(
        &mut self,
        route: &Route,
        pipewire: Option<&cpal::Host>,
        choice: Option<&OutputChoice>,
        sample_rate: u32,
        buffer: u32,
        headphones_34: bool,
    ) -> Result<String, String> {
        let alsa = alsa_host;
        let chosen_label = choice.map_or_else(|| "System default output".to_string(), |c| c.label.clone());
        let native = || pipewire.ok_or_else(|| "PipeWire unavailable".to_string());
        let (device, want_ch, label) = match route {
            Route::NativeDefault => (native()?.default_output_device(), 2, "System default output".to_string()),
            Route::NativeNode { node, channels } => {
                let found = native()?.output_devices().map_err(|e| e.to_string())?.find(|d| d.id().is_ok_and(|id| id.id() == node));
                // Not listed by the sound server: as wide as the node itself says it is.
                let widest = || found.as_ref().and_then(|d| d.supported_output_configs().ok()?.map(|c| c.channels()).max()).unwrap_or(2);
                let channels = channels.unwrap_or_else(|| if headphones_34 && widest() >= 4 { 4 } else { 2 });
                (found, channels, chosen_label)
            }
            Route::AlsaBridge { plugin, channels, .. } => {
                let host = alsa()?;
                let found = host.output_devices().ok().and_then(|mut d| d.find(|d| d.id().is_ok_and(|id| id.id() == *plugin)));
                (found, *channels, chosen_label)
            }
            Route::Alsa { id } => {
                let device = id.parse().ok().and_then(|id| alsa().ok()?.device_by_id(&id));
                let widest = device.as_ref().and_then(|d| d.supported_output_configs().ok()?.map(|c| c.channels()).max()).unwrap_or(2);
                let label = choice.map_or_else(|| id.clone(), |c| c.label.clone());
                (device, if headphones_34 && widest >= 4 { 4 } else { 2 }, label)
            }
            Route::AlsaDefault => (alsa()?.default_output_device(), 2, "System default output".to_string()),
        };
        let device = device.ok_or("no such device")?;
        let default = device.default_output_config().map_err(|e| e.to_string())?;
        let ranges: Vec<_> = device.supported_output_configs().map(|r| r.collect()).unwrap_or_default();
        let is_native = matches!(route, Route::NativeDefault | Route::NativeNode { .. });

        let (format, channels, sr, buffer_range) = if is_native {
            // PipeWire converts any width and position for the sink: ask for exactly what the mix needs, in f32.
            let sr = if ranges.iter().any(|r| r.contains_rate(sample_rate)) { sample_rate } else { default.sample_rate() };
            (cpal::SampleFormat::F32, want_ch as usize, sr, *default.buffer_size())
        } else {
            let usable = |r: &&cpal::SupportedStreamConfigRange| matches!(r.sample_format(), cpal::SampleFormat::F32 | cpal::SampleFormat::I16);
            let pick = ranges
                .iter()
                .filter(usable)
                .filter(|r| r.channels() == want_ch)
                .find(|r| r.contains_rate(sample_rate))
                .or_else(|| ranges.iter().filter(usable).find(|r| r.channels() == want_ch))
                .or_else(|| ranges.iter().filter(usable).find(|r| r.channels() == 2 && r.contains_rate(sample_rate)));
            let config = match pick {
                Some(r) => {
                    let rate = if r.contains_rate(sample_rate) { sample_rate } else { default.sample_rate().clamp(r.min_sample_rate(), r.max_sample_rate()) };
                    r.clone().with_sample_rate(rate)
                }
                None => default,
            };
            (config.sample_format(), config.channels() as usize, config.sample_rate(), *config.buffer_size())
        };
        let fixed_ok = match buffer_range {
            // A PipeWire node that hasn't reported its quantum range yet says 0..=0.
            cpal::SupportedBufferSize::Range { min, max } => max == 0 || (min..=max).contains(&buffer),
            cpal::SupportedBufferSize::Unknown => true,
        };

        let build = |buffer_size: cpal::BufferSize| {
            let stream_config = cpal::StreamConfig { channels: channels as u16, sample_rate: sr, buffer_size };
            let mut renderer = Renderer::new(Arc::clone(&self.shared), Arc::clone(&self.stats), Arc::clone(&self.recorder));
            let stats = Arc::clone(&self.stats);
            // Runs on the audio thread: only counts; the UI thread shows and logs it.
            let err_fn = move |err: cpal::Error| stats.stream_error(err.kind());
            match format {
                cpal::SampleFormat::F32 => device.build_output_stream(
                    stream_config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| renderer.render(data, channels, sr, |x| x),
                    err_fn,
                    None,
                ),
                _ => device.build_output_stream(
                    stream_config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| renderer.render(data, channels, sr, pcm_i16),
                    err_fn,
                    None,
                ),
            }
        };
        let built = match fixed_ok.then(|| build(cpal::BufferSize::Fixed(buffer))) {
            Some(Ok(s)) => Ok((s, format!("{buffer}-sample buffer"))),
            _ => build(cpal::BufferSize::Default).map(|s| (s, "device's own buffer".to_string())),
        };
        let (stream, buffer_text) = built.map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        self.output_channels = channels as u16;
        self.hw_active.store(true, Ordering::Relaxed);
        self._stream = Some(stream);
        let narrower = if channels < want_ch as usize { " (outputs 3–4 not available this way)" } else { "" };
        Ok(format!("{label} · {} · {sr} Hz · {channels} ch{narrower} · {buffer_text}", route.describe()))
    }

    #[cfg(test)]
    pub fn open(&mut self, _device_id: &str, _sample_rate: u32, _buffer: u32, _headphones_34: bool) {
        *self.test_renderer.lock().unwrap() = None;
    }

    /// Advances the playheads on a timer while no sound card is running.
    #[cfg(not(test))]
    fn spawn_fallback_clock(shared: Arc<Mutex<AudioSharedState>>, running: Arc<AtomicBool>, hw_active: Arc<AtomicBool>) {
        thread::spawn(move || {
            let mut last_tick = Instant::now();
            while running.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(16));
                let dt = last_tick.elapsed().as_secs_f64();
                last_tick = Instant::now();
                if hw_active.load(Ordering::Relaxed) {
                    continue;
                }
                if let Ok(mut state) = shared.try_lock() {
                    fallback_tick(&mut state, dt);
                }
            }
        });
    }

    /// Test fixture: interleaved stereo `i16` PCM as a WAV, with the recorder's header.
    #[cfg(test)]
    pub fn write_stereo_wav_file(path: &Path, samples: &[i16], sample_rate: u32) -> std::io::Result<()> {
        let mut bytes = crate::recorder::wav_header(samples.len() as u64 / 2, sample_rate).to_vec();
        bytes.extend(samples.iter().flat_map(|s| s.to_le_bytes()));
        std::fs::write(path, bytes)
    }

    /// The test build's stand-in for the device callback: the same [`Renderer`], kept across calls like a
    /// stream keeps its own, and replaced by [`AudioSystem::open`] as a reopen replaces it.
    #[cfg(test)]
    pub fn render_with<T: Copy>(&self, output: &mut [T], channels: usize, sr: u32, to_sample: fn(f32) -> T) {
        let mut slot = self.test_renderer.lock().unwrap();
        slot.get_or_insert_with(|| Renderer::new(Arc::clone(&self.shared), Arc::clone(&self.stats), Arc::clone(&self.recorder)))
            .render(output, channels, sr, to_sample);
    }

    #[cfg(test)]
    pub fn render_f32(&self, output: &mut [f32], channels: usize, sr: u32) {
        self.render_with(output, channels, sr, |x| x);
    }
}

/// One tick of the clock that moves the playheads while no sound card runs, `dt` seconds after the last.
/// Plain playback stops at the end of the track as in the callback; loops and scratches keep going as before.
fn fallback_tick(state: &mut AudioSharedState, dt: f64) {
    state.pending_seek = [None; 4]; // with no callback, `playhead_sec` itself is the position
    if state.pattern_enabled {
        let bpm = state.engine.clock.bpm.max(40.0);
        let step_dur = (60.0 / bpm) / 4.0;
        state.pattern_phase_sec += dt;
        state.pattern_current_step = ((state.pattern_phase_sec / step_dur).floor() as usize) % 16;
    }
    for d in 0..4 {
        if !state.scratch_active[d] && state.scratch_ramping[d] {
            release_step(state, d, dt); // the callback's jog physics isn't running to end the ramp
        }
        let scratching = state.scratch_active[d] || state.scratch_ramping[d];
        if state.is_playing[d] || scratching {
            let ratio = if scratching { state.scratch_rate[d] } else { state.pitch_ratio[d] };
            state.playhead_sec[d] += dt * ratio;
            let duration = state.loaded_tracks[d].as_ref().map(|t| t.duration_sec);
            if state.loop_active[d] && state.loop_len_sec[d] > 0.05 {
                let end = state.loop_start_sec[d] + state.loop_len_sec[d];
                if state.playhead_sec[d] >= end {
                    state.playhead_sec[d] = state.loop_start_sec[d];
                }
            } else if let Some(duration) = duration {
                if !scratching && at_track_end(state.playhead_sec[d], duration) {
                    state.playhead_sec[d] = duration;
                    state.is_playing[d] = false;
                    state.reset_jump_ease(d);
                } else if (scratching && ratio > 0.0 && state.playhead_sec[d] >= duration) || state.playhead_sec[d] < 0.0 {
                    state.playhead_sec[d] = 0.0;
                }
            }
        }
    }
}

/// Counters the audio callback and the stream's error handler bump; read and reported by the UI thread.
#[derive(Default)]
pub struct AudioStats {
    /// Blocks played while the UI or a worker held the shared lock (played from the callback's own state, not silence).
    pub busy_blocks: AtomicU64,
    /// Underruns and overruns the audio backend reported.
    pub xruns: AtomicU64,
    /// Other stream errors, and the kind of the latest as an index into [`STREAM_ERROR_KINDS`] (its length = other).
    pub stream_errors: AtomicU64,
    pub last_error_kind: AtomicUsize,
}

/// Stream error kinds the UI can name; anything else is reported as "other".
pub const STREAM_ERROR_KINDS: [cpal::ErrorKind; 6] = [
    cpal::ErrorKind::DeviceNotAvailable,
    cpal::ErrorKind::DeviceBusy,
    cpal::ErrorKind::DeviceChanged,
    cpal::ErrorKind::HostUnavailable,
    cpal::ErrorKind::BackendError,
    cpal::ErrorKind::UnsupportedConfig,
];

impl AudioStats {
    /// The stream's error handler. It runs on the audio thread, so it only counts: no printing, no allocation.
    pub fn stream_error(&self, kind: cpal::ErrorKind) {
        if kind == cpal::ErrorKind::Xrun {
            self.xruns.fetch_add(1, Ordering::Relaxed);
        } else {
            let index = STREAM_ERROR_KINDS.iter().position(|k| *k == kind).unwrap_or(STREAM_ERROR_KINDS.len());
            self.last_error_kind.store(index, Ordering::Relaxed);
            self.stream_errors.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One deck's audio as the callback holds it: references to the loaded track's PCM, never copies.
struct DeckPcm {
    pcm_stereo: Arc<Vec<f32>>,
    pcm_drums: Option<Arc<Vec<f32>>>,
    pcm_bass: Option<Arc<Vec<f32>>>,
    pcm_other: Option<Arc<Vec<f32>>>,
    pcm_vocals: Option<Arc<Vec<f32>>>,
    sample_rate: u32,
    duration_sec: f64,
}

impl DeckPcm {
    fn of(t: &LoadedTrackAudio) -> Self {
        Self {
            pcm_stereo: Arc::clone(&t.pcm_stereo),
            pcm_drums: t.pcm_drums.clone(),
            pcm_bass: t.pcm_bass.clone(),
            pcm_other: t.pcm_other.clone(),
            pcm_vocals: t.pcm_vocals.clone(),
            sample_rate: t.sample_rate,
            duration_sec: t.duration_sec,
        }
    }

    fn buffers(&self) -> [Option<&Arc<Vec<f32>>>; 5] {
        [Some(&self.pcm_stereo), self.pcm_drums.as_ref(), self.pcm_bass.as_ref(), self.pcm_other.as_ref(), self.pcm_vocals.as_ref()]
    }

    fn same_audio(&self, t: &LoadedTrackAudio) -> bool {
        let theirs = [Some(&t.pcm_stereo), t.pcm_drums.as_ref(), t.pcm_bass.as_ref(), t.pcm_other.as_ref(), t.pcm_vocals.as_ref()];
        self.buffers().iter().zip(theirs).all(|(a, b)| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        })
    }
}

/// The audio callback's own state: its DSP, playheads, last applied controls and references to the track PCM.
///
/// Each block it tries the shared lock once, without waiting. Got it: it takes the UI's controls, seeks and
/// tracks, renders, and publishes playheads and meters while still holding it, so the UI sees one consistent
/// instant. Busy (the UI or a worker holds it): it renders the block from its own state, as it was. The mix
/// recorder has its own lock ([`AudioSystem::recorder`]). Allocation-free per block: track buffers it lets go of are handed
/// to the UI thread to free.
pub(crate) struct Renderer {
    shared: Arc<Mutex<AudioSharedState>>,
    stats: Arc<AudioStats>,
    recorder: Arc<Mutex<Option<RecorderTap>>>,
    /// The callback's copy. Its `loaded_tracks` stay empty; `tracks` holds the audio.
    state: AudioSharedState,
    tracks: [Option<DeckPcm>; 4],
    generation: u64,
}

impl Renderer {
    /// Built on a control thread (it waits for the lock): starts from the published playheads and the current
    /// controls and tracks, so a new or reopened stream carries on where the last one left off.
    pub(crate) fn new(shared: Arc<Mutex<AudioSharedState>>, stats: Arc<AudioStats>, recorder: Arc<Mutex<Option<RecorderTap>>>) -> Self {
        let mut renderer =
            Self { shared, stats, recorder, state: AudioSharedState::default(), tracks: Default::default(), generation: 0 };
        let shared = Arc::clone(&renderer.shared);
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.renderer_generation += 1;
        renderer.generation = s.renderer_generation;
        // `playhead_sec` already includes any seek still pending.
        s.pending_seek = [None; 4];
        renderer.state.playhead_sec = s.playhead_sec;
        renderer.state.pattern_phase_sec = s.pattern_phase_sec;
        renderer.take_from(&mut s);
        drop(s);
        renderer
    }

    /// Fills one device buffer. `to_sample` converts the finished f32 mix to the device format.
    pub(crate) fn render<T: Copy>(&mut self, output: &mut [T], channels: usize, sr: u32, to_sample: fn(f32) -> T) {
        let channels = channels.max(1);
        let shared = Arc::clone(&self.shared);
        let mut guard = shared.try_lock().ok();
        match guard.as_deref_mut() {
            Some(s) => self.take_from(s),
            None => {
                self.stats.busy_blocks.fetch_add(1, Ordering::Relaxed);
            }
        }
        // While held, REC can't start or stop: this block is wholly in the take or wholly out. Busy only while
        // REC is being pressed right at this boundary, so the take starts or ends one block later or earlier.
        let mut slot = self.recorder.try_lock().ok();
        let mut tap = slot.as_deref_mut().and_then(Option::as_mut);
        if let Some(tap) = tap.as_deref_mut() {
            tap.begin_block(sr);
        }
        jog_physics(&mut self.state, (output.len() / channels) as f64 / (sr as f64).max(1.0));
        Self::mix(&mut self.state, &self.tracks, tap.as_deref_mut(), output, channels, sr, to_sample);
        if let Some(tap) = tap {
            tap.end_block();
        }
        drop(slot);
        if let Some(s) = guard.as_deref_mut() {
            self.publish_to(s);
        }
    }

    /// Handoff in: everything the UI or a worker changed since the last one.
    fn take_from(&mut self, s: &mut AudioSharedState) {
        let r = &mut self.state;
        r.is_playing = s.is_playing;
        r.pitch_ratio = s.pitch_ratio;
        r.loop_active = s.loop_active;
        r.loop_start_sec = s.loop_start_sec;
        r.loop_len_sec = s.loop_len_sec;
        r.channel_gain = s.channel_gain;
        r.cue_mix = s.cue_mix;
        r.cue_volume = s.cue_volume;
        r.headphone_out = s.headphone_out;
        r.keylock = s.keylock;
        r.keylock_hq = s.keylock_hq;
        r.crossfader = s.crossfader;
        r.crossfader_curve = s.crossfader_curve;
        r.stem_volumes = s.stem_volumes;
        r.stem_muted = s.stem_muted;
        r.pattern_enabled = s.pattern_enabled;
        r.pattern_steps = s.pattern_steps;
        r.pattern_volume = s.pattern_volume;
        r.pattern_pitch = s.pattern_pitch;
        r.pattern_decay = s.pattern_decay;
        r.engine.master_volume = s.engine.master_volume;
        r.engine.clock.bpm = s.engine.clock.bpm;
        for (mine, ui) in r.engine.strips.iter_mut().zip(&s.engine.strips) {
            mine.volume = ui.volume;
            mine.cue_enabled = ui.cue_enabled;
            mine.xfader_left = ui.xfader_left;
            mine.xfader_right = ui.xfader_right;
            mine.eq = ui.eq.clone();
            mine.filter.kind = ui.filter.kind;
            mine.filter.knob = ui.filter.knob;
            mine.filter.enabled = ui.filter.enabled;
        }

        // Jog gestures and ticks; the physics itself runs in `render`, every block, so it never waits for the lock.
        for d in 0..4 {
            if std::mem::take(&mut s.scratch_changed[d]) {
                r.scratch_active[d] = s.scratch_active[d];
                r.scratch_ramping[d] = s.scratch_ramping[d];
                r.scratch_ramp_target[d] = s.scratch_ramp_target[d];
                if s.scratch_active[d] {
                    // A touch starts a fresh gesture; a release ramps from the speed the callback has reached.
                    r.scratch_filter[d] = s.scratch_filter[d];
                    r.scratch_rate[d] = s.scratch_rate[d];
                    r.scratch_dx_per_tick[d] = s.scratch_dx_per_tick[d];
                    r.scratch_ticks_accum[d] = 0;
                }
            }
            r.scratch_ticks_accum[d] += std::mem::take(&mut s.scratch_ticks_accum[d]);
        }

        for d in 0..4 {
            settle_end(r, &self.tracks[d], s, d);
            if !same_track(&self.tracks[d], &s.loaded_tracks[d]) {
                let releasing = self.tracks[d].as_ref().map_or(0, |t| t.buffers().iter().flatten().count());
                // shortcut: with no room to retire the old buffers, the deck keeps its old audio (and its seek waits)
                // until the UI frees some, which it does every frame.
                if s.retired.capacity() - s.retired.len() < releasing {
                    continue;
                }
                if let Some(old) = self.tracks[d].take() {
                    s.retired.extend(old.buffers().into_iter().flatten().cloned());
                }
                self.tracks[d] = s.loaded_tracks[d].as_ref().map(DeckPcm::of);
            } else if let (Some(mine), Some(theirs)) = (&mut self.tracks[d], &s.loaded_tracks[d]) {
                mine.duration_sec = theirs.duration_sec;
                mine.sample_rate = theirs.sample_rate;
            }
            apply_seek(&mut r.playhead_sec[d], s, d);
            if std::mem::take(&mut s.jump_ease_reset[d]) {
                r.jump_ease[d] = JumpEase::default();
            }
            if std::mem::take(&mut s.jump_ease[d].requested) {
                r.jump_ease[d].requested = true;
            }
        }
        r.play_requests = s.play_requests;
    }

    /// Handoff out: playheads and meters as of the end of this block, and decks that reached the end of their
    /// track in it. The lock was held since `take_from`, so only a newer track the callback couldn't take yet
    /// (its old audio still waiting to be retired) has a play flag of its own to keep.
    fn publish_to(&mut self, s: &mut AudioSharedState) {
        let r = &mut self.state;
        for d in 0..4 {
            if std::mem::take(&mut r.stopped_at_end[d]) && same_track(&self.tracks[d], &s.loaded_tracks[d]) {
                s.is_playing[d] = false;
            }
        }
        s.playhead_sec = r.playhead_sec;
        s.channel_peaks = r.channel_peaks;
        s.master_peak_l = r.master_peak_l;
        s.master_peak_r = r.master_peak_r;
        s.pattern_current_step = r.pattern_current_step;
        s.pattern_phase_sec = r.pattern_phase_sec;
        s.scratch_rate = r.scratch_rate;
        s.scratch_ramping = r.scratch_ramping;
        s.scratch_filter = r.scratch_filter;
    }
}

impl Drop for Renderer {
    /// When the stream closes (a reopen, a device change, quitting), off the real-time path: hand the latest
    /// playheads to whoever plays next, the fallback clock or a new stream, even if the last blocks found the lock
    /// busy, and give back the track audio to be freed by the UI thread.
    fn drop(&mut self) {
        let shared = Arc::clone(&self.shared);
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        // Unless a newer stream already took over: seeks the UI made since the last handoff land here as they would
        // have in the callback (a move by an amount from where the deck really got to, a jump to a spot exactly),
        // after a deck that ran out during busy blocks is settled as there.
        if s.renderer_generation == self.generation {
            for d in 0..4 {
                settle_end(&mut self.state, &self.tracks[d], &mut s, d);
                apply_seek(&mut self.state.playhead_sec[d], &mut s, d);
                s.playhead_sec[d] = self.state.playhead_sec[d];
            }
            s.pattern_phase_sec = self.state.pattern_phase_sec;
        }
        for t in self.tracks.iter_mut().filter_map(Option::take) {
            s.retired.extend(t.buffers().into_iter().flatten().cloned());
        }
    }
}

/// Whether the callback's audio for a deck is the track the UI has loaded there (both empty counts).
fn same_track(mine: &Option<DeckPcm>, theirs: &Option<LoadedTrackAudio>) -> bool {
    match (mine, theirs) {
        (Some(mine), Some(theirs)) => mine.same_audio(theirs),
        (mine, theirs) => mine.is_none() && theirs.is_none(),
    }
}

/// At a handoff (or the stream's close) after deck `d` ran out while the lock was busy, the UI's play flag for it
/// may be one the deck has outlived. Whatever the UI did since stands: a new track, a move (cue, jump), and a Play,
/// which at the end of the track starts from the top. With none, the deck stays stopped at the end.
fn settle_end(r: &mut AudioSharedState, mine: &Option<DeckPcm>, s: &mut AudioSharedState, d: usize) {
    if !std::mem::take(&mut r.stopped_at_end[d]) || !same_track(mine, &s.loaded_tracks[d]) || s.pending_seek[d].is_some() {
        return;
    }
    if s.play_requests[d] != r.play_requests[d] && s.is_playing[d] {
        r.playhead_sec[d] = 0.0;
    } else {
        s.is_playing[d] = false;
    }
    r.is_playing[d] = s.is_playing[d];
}

/// One block of jog physics in the callback's state: the scratch filter observes the ticks that arrived (none
/// is an observation too), and a released platter eases back to playing speed.
fn jog_physics(r: &mut AudioSharedState, block_dt: f64) {
    for d in 0..4 {
        if r.scratch_active[d] {
            let ticks = std::mem::take(&mut r.scratch_ticks_accum[d]);
            r.scratch_filter[d].observation(r.scratch_dx_per_tick[d] * ticks as f64, block_dt);
            r.scratch_rate[d] = r.scratch_filter[d].predicted_velocity();
        } else if r.scratch_ramping[d] {
            release_step(r, d, block_dt);
        }
    }
}

/// `dt` seconds of a released platter easing back to the speed it was let go towards; the ramp ends there.
fn release_step(r: &mut AudioSharedState, d: usize, dt: f64) {
    let target = r.scratch_ramp_target[d];
    // Smooth exponential decay towards playback speed (tau = 0.065s)
    let next = target + (r.scratch_rate[d] - target) * (-dt / 0.065).exp();
    r.scratch_rate[d] = next;
    if (next - target).abs() < 0.005 {
        r.scratch_rate[d] = target;
        r.scratch_ramping[d] = false;
    }
}

/// Moves deck `d`'s playhead by `step` seconds with compensated (Kahan) summation: a plain sum of millions of
/// per-frame steps falls nanoseconds short, enough to play a track's last frame twice. The lost rounding is carried
/// only while the playhead is where the last step left it, so a seek, wrap or stop starts afresh.
fn advance_playhead(r: &mut AudioSharedState, d: usize, step: f64) {
    let (left_at, lost) = r.playhead_residual[d];
    let from = r.playhead_sec[d];
    let step = if left_at == from { step - lost } else { step };
    let to = from + step;
    r.playhead_residual[d] = (to, (to - from) - step);
    r.playhead_sec[d] = to;
}

/// Plain playback of deck `d` reached the end of its track: park it exactly there, silent, with no key lock
/// history or jump ease left over to be heard if it plays again, and mark it for the next handoff.
fn stop_at_track_end(r: &mut AudioSharedState, d: usize, end: f64) {
    r.playhead_sec[d] = end;
    r.playhead_residual[d] = (0.0, 0.0); // a hand resting on the parked deck holds it exactly here
    r.is_playing[d] = false;
    r.stopped_at_end[d] = true;
    r.keylock_engaged[d] = false; // the next key-locked start re-primes the stretcher
    r.jump_ease[d] = JumpEase::default();
}

/// Applies (and clears) deck `d`'s pending seek to `playhead`; a move by an amount stays within the loaded track.
fn apply_seek(playhead: &mut f64, s: &mut AudioSharedState, d: usize) {
    match s.pending_seek[d].take() {
        Some(Seek::To(sec)) => *playhead = sec,
        Some(Seek::By(delta)) => {
            let end = s.loaded_tracks[d].as_ref().map_or(f64::MAX, |t| t.duration_sec);
            *playhead = (*playhead + delta).clamp(0.0, end);
        }
        None => {}
    }
}

impl Renderer {
    /// One block of the mix into `output`; with `rec`, its main output (before the cue routing) goes to the recorder too.
    fn mix<T: Copy>(
        state: &mut AudioSharedState,
        tracks: &[Option<DeckPcm>; 4],
        mut rec: Option<&mut RecorderTap>,
        output: &mut [T],
        channels: usize,
        sr: u32,
        to_sample: fn(f32) -> T,
    ) {
        let frames = output.len() / channels;
        let dt = 1.0 / (sr as f64).max(1.0);
        output.fill(to_sample(0.0));
        state.engine.routing.sample_rate = sr;

        for f in 0..frames {
            let mut deck_samples: [(f32, f32); 4] = [(0.0, 0.0); 4];

            for d in 0..4 {
                state.engine.strips[d].playing = state.is_playing[d];
                if !state.is_playing[d] && !state.scratch_active[d] && !state.scratch_ramping[d] {
                    state.jump_ease[d] = JumpEase::default(); // silent: no stale frame or request survives a stop
                    continue;
                }
                let mut advance_dt = 0.0_f64;
                let mut track_dur = 120.0_f64;
                let mut raw_l = 0.0_f32;
                let mut raw_r = 0.0_f32;
                let mut has_sample = false;

                let effective_rate = if state.scratch_active[d] || state.scratch_ramping[d] {
                    state.scratch_rate[d]
                } else {
                    state.pitch_ratio[d]
                };

                let mut true_stems = None;
                let scratching = state.scratch_active[d] || state.scratch_ramping[d];
                let looping = state.loop_active[d] && state.loop_len_sec[d] > 0.05;
                // Plain playback ends after the last decoded frame; a loop or a hand on the platter carries on as before.
                let track_end = tracks[d].as_ref().filter(|_| !scratching && !looping).map(|t| t.duration_sec);
                if let Some(end) = track_end.filter(|&end| at_track_end(state.playhead_sec[d], end)) {
                    // Already there: not one more frame, nor one from the channel's EQ and filter fed silence.
                    stop_at_track_end(state, d, end);
                    state.engine.strips[d].playing = false;
                    continue;
                }
                let keylock_on = state.keylock[d] && !scratching;
                if keylock_on && !state.keylock_engaged[d] {
                    state.keylocks[d].reset();
                }
                state.keylock_engaged[d] = keylock_on;

                if let Some(track) = &tracks[d] {
                    track_dur = track.duration_sec.max(0.1);
                    let tsr = track.sample_rate as f64;
                    let sample_pos = state.playhead_sec[d] * tsr;
                    let stems = [&track.pcm_drums, &track.pcm_bass, &track.pcm_other, &track.pcm_vocals];
                    let has_stems = stems.iter().all(|s| s.is_some());
                    let locked = keylock_on.then(|| {
                        let mut srcs: [&[f32]; 5] = [&track.pcm_stereo, &[], &[], &[], &[]];
                        for (i, s) in stems.iter().enumerate() {
                            srcs[i + 1] = s.as_deref().map_or(&[], |v| v.as_slice());
                        }
                        let n = if has_stems { 5 } else { 1 };
                        let lp = (state.loop_active[d] && state.loop_len_sec[d] > 0.05)
                            .then(|| (state.loop_start_sec[d] * tsr, state.loop_len_sec[d] * tsr));
                        state.keylocks[d].next(&srcs[..n], sample_pos, tsr / sr as f64, lp, state.keylock_hq)
                    });
                    let (mut s_l, mut s_r) = match locked {
                        Some(k) => k[0],
                        None => sample_stereo_hermite(&track.pcm_stereo, sample_pos),
                    };

                    // Phono cartridge magnetic induction scaling (output proportional to needle velocity)
                    // Attenuates frozen sample buzz/clicks when platter is stopped
                    let mut phono_gain = 1.0_f32;
                    if state.scratch_active[d] {
                        let speed_abs = effective_rate.abs();
                        if speed_abs < 0.06 {
                            phono_gain = (speed_abs / 0.06) as f32;
                        }
                        s_l *= phono_gain;
                        s_r *= phono_gain;
                    }

                    // Check for true isolated AI stems (Demucs or pre-synthesized)
                    if let (Some(dp), Some(bp), Some(op), Some(vp)) = (
                        &track.pcm_drums,
                        &track.pcm_bass,
                        &track.pcm_other,
                        &track.pcm_vocals,
                    ) {
                        let read = |i: usize, pcm: &[f32]| match locked {
                            Some(k) => k[i],
                            None => sample_stereo_hermite(pcm, sample_pos),
                        };
                        let (mut d_l, mut d_r) = read(1, dp);
                        let (mut b_l, mut b_r) = read(2, bp);
                        let (mut o_l, mut o_r) = read(3, op);
                        let (mut v_l, mut v_r) = read(4, vp);
                        if phono_gain < 1.0 {
                            d_l *= phono_gain; d_r *= phono_gain;
                            b_l *= phono_gain; b_r *= phono_gain;
                            o_l *= phono_gain; o_r *= phono_gain;
                            v_l *= phono_gain; v_r *= phono_gain;
                        }
                        true_stems = Some(((d_l, d_r), (b_l, b_r), (o_l, o_r), (v_l, v_r)));
                    }

                    raw_l = s_l;
                    raw_r = s_r;
                    has_sample = true;
                    advance_dt = dt * effective_rate;
                }

                if has_sample {
                    let g_drums = if state.stem_muted[d][0] {
                        0.0
                    } else {
                        state.stem_volumes[d][0]
                    };
                    let g_bass = if state.stem_muted[d][1] {
                        0.0
                    } else {
                        state.stem_volumes[d][1]
                    };
                    let g_inst = if state.stem_muted[d][2] {
                        0.0
                    } else {
                        state.stem_volumes[d][2]
                    };
                    let g_voc = if state.stem_muted[d][3] {
                        0.0
                    } else {
                        state.stem_volumes[d][3]
                    };

                    let (stem_l, stem_r) = if let Some(((d_l, d_r), (b_l, b_r), (o_l, o_r), (v_l, v_r))) = true_stems {
                        // True Demucs AI Stem Isolation — 100% pristine isolation with ZERO bleed!
                        (
                            d_l * g_drums + b_l * g_bass + o_l * g_inst + v_l * g_voc,
                            d_r * g_drums + b_r * g_bass + o_r * g_inst + v_r * g_voc,
                        )
                    } else {
                        // Real-Time 4-Stem Frequency Decomposition Fallback ([Drums, Bass, Instruments, Vocals])
                        state.stem_lp_l[d] += 0.035 * (raw_l - state.stem_lp_l[d]);
                        state.stem_lp_r[d] += 0.035 * (raw_r - state.stem_lp_r[d]);
                        state.stem_mp_l[d] += 0.22 * (raw_l - state.stem_mp_l[d]);
                        state.stem_mp_r[d] += 0.22 * (raw_r - state.stem_mp_r[d]);

                        let bass_l = state.stem_lp_l[d];
                        let bass_r = state.stem_lp_r[d];
                        let mid_band_l = state.stem_mp_l[d] - bass_l;
                        let mid_band_r = state.stem_mp_r[d] - bass_r;
                        let high_band_l = raw_l - state.stem_mp_l[d];
                        let high_band_r = raw_r - state.stem_mp_r[d];

                        // Center vocal extraction from mid-band + transient percussion separation
                        let mid_center = (mid_band_l + mid_band_r) * 0.5;
                        let mid_side = (mid_band_l - mid_band_r) * 0.5;
                        let abs_in = ((raw_l + raw_r) * 0.5).abs();
                        state.stem_fast_env[d] = if abs_in > state.stem_fast_env[d] {
                            state.stem_fast_env[d] + 0.35 * (abs_in - state.stem_fast_env[d])
                        } else {
                            state.stem_fast_env[d] * 0.992
                        };
                        state.stem_slow_env[d] += 0.008 * (abs_in - state.stem_slow_env[d]);
                        let transient_gate =
                            ((state.stem_fast_env[d] - state.stem_slow_env[d]).max(0.0) * 3.5).min(1.0);

                        let drums_l = high_band_l * 0.65 + mid_band_l * transient_gate * 0.45;
                        let drums_r = high_band_r * 0.65 + mid_band_r * transient_gate * 0.45;
                        let vocal_l = mid_center * (1.0 - transient_gate * 0.45);
                        let vocal_r = mid_center * (1.0 - transient_gate * 0.45);
                        let inst_l = mid_side + high_band_l * 0.35;
                        let inst_r = -mid_side + high_band_r * 0.35;

                        (
                            drums_l * g_drums + bass_l * g_bass + inst_l * g_inst + vocal_l * g_voc,
                            drums_r * g_drums + bass_r * g_bass + inst_r * g_inst + vocal_r * g_voc,
                        )
                    };

                    let (stem_l, stem_r) = state.jump_ease[d].next((stem_l, stem_r), sr, keylock_on);

                    // 2. Channel gain (the 3-band EQ runs in the mixer engine).
                    let gain = (state.channel_gain[d] * 2.0).clamp(0.0, 2.0);
                    deck_samples[d] = (stem_l * gain, stem_r * gain);
                } else {
                    state.jump_ease[d] = JumpEase::default();
                }

                advance_playhead(state, d, advance_dt);

                if looping {
                    let loop_start = state.loop_start_sec[d];
                    let loop_end = loop_start + state.loop_len_sec[d];
                    if state.playhead_sec[d] >= loop_end {
                        state.playhead_sec[d] = loop_start + (state.playhead_sec[d] - loop_end);
                    } else if state.playhead_sec[d] < loop_start {
                        state.playhead_sec[d] = loop_end - (loop_start - state.playhead_sec[d]);
                    }
                } else if let Some(end) = track_end.filter(|&end| at_track_end(state.playhead_sec[d], end)) {
                    stop_at_track_end(state, d, end);
                } else if track_end.is_none() && advance_dt > 0.0 && state.playhead_sec[d] >= track_dur {
                    // Only moving forward past the end comes round: a hand resting on a parked deck stays put.
                    state.playhead_sec[d] = 0.0;
                } else if state.playhead_sec[d] < 0.0 {
                    state.playhead_sec[d] = 0.0;
                }
            }

            // 3. 16-Step Pattern Player (Master Clock Synced Drum Machine)
            let mut pat_sample = 0.0_f32;
            if state.pattern_enabled {
                let bpm = state.engine.clock.bpm.max(40.0);
                let step_dur = (60.0 / bpm) / 4.0;
                state.pattern_phase_sec += dt;
                let step_idx = ((state.pattern_phase_sec / step_dur).floor() as usize) % 16;
                state.pattern_current_step = step_idx;
                let step_t = (state.pattern_phase_sec % step_dur) as f32;
                let decay_scale = (0.3 + state.pattern_decay * 1.4).max(0.2);
                let pitch_scale = 0.75 + state.pattern_pitch * 0.5;

                // Voice 0: 808 Sub Kick
                if state.pattern_steps[0][step_idx] {
                    let env = (-step_t * 18.0 / decay_scale).exp();
                    let freq = (52.0 + 130.0 * (-step_t * 42.0).exp()) * pitch_scale;
                    pat_sample += (step_t * freq * 2.0 * PI).sin() * env * 0.75;
                }
                // Voice 1: 909 Clap
                if state.pattern_steps[1][step_idx] {
                    let env = (-step_t * 24.0 / decay_scale).exp();
                    let noise = (((f * 1664525 + 1013904223) & 0x7FFF) as f32 / 16384.0) - 1.0;
                    pat_sample += noise * env * 0.45;
                }
                // Voice 2: 909 Closed/Open Hat
                if state.pattern_steps[2][step_idx] {
                    let env = (-step_t * 36.0 / decay_scale).exp();
                    let metallic = (step_t * 6400.0 * pitch_scale * 2.0 * PI).sin()
                        * (step_t * 9100.0 * 2.0 * PI).cos();
                    pat_sample += metallic * env * 0.35;
                }
                // Voice 3: Rave Rim / Tom
                if state.pattern_steps[3][step_idx] {
                    let env = (-step_t * 28.0 / decay_scale).exp();
                    let freq = 240.0 * pitch_scale;
                    pat_sample += (step_t * freq * 2.0 * PI).sin() * env * 0.45;
                }
                pat_sample *= state.pattern_volume;
            }

            state.engine.crossfader = state.crossfader;
            state.engine.crossfader_curve = state.crossfader_curve;
            let (mut ml, mut mr, cl, cr) = state.engine.process_frame(deck_samples);
            ml = (ml + pat_sample).clamp(-1.0, 1.0);
            mr = (mr + pat_sample).clamp(-1.0, 1.0);

            if let Some(tap) = rec.as_deref_mut() {
                tap.push(ml, mr);
            }

            let out = &mut output[f * channels..(f + 1) * channels];
            let (hl, hr) = headphone_mix((cl, cr), (ml, mr), state.cue_mix, state.cue_volume);
            match (state.headphone_out, channels) {
                (_, 1) => out[0] = to_sample((ml + mr) * 0.5),
                (HeadphoneOut::Split, _) => {
                    out[0] = to_sample((hl + hr) * 0.5);
                    out[1] = to_sample((ml + mr) * 0.5);
                }
                (HeadphoneOut::Channels34, 4..) => {
                    out[..4].copy_from_slice(&[ml, mr, hl, hr].map(to_sample));
                }
                _ => out[..2].copy_from_slice(&[ml, mr].map(to_sample)),
            }
        }

        // Update real-time peak meter state
        for d in 0..4 {
            state.channel_peaks[d] = state.engine.strips[d].last_prefader_peak;
        }
        state.master_peak_l = state.engine.last_master_peak.0;
        state.master_peak_r = state.engine.last_master_peak.1;
    }
}

/// A finished f32 sample as 16-bit PCM, for i16 devices and the mix recorder.
pub(crate) fn pcm_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0) as i16
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A jump eases from the last frame heard to the new audio in 3 ms at any rate, never leaving the
    /// span between the two on either channel. A second jump mid-ease starts from where the first had got
    /// to. Key lock drops the request.
    #[test]
    fn jump_ease_stays_between_old_and_new() {
        let between = |x: f32, a: f32, b: f32| x >= a.min(b) - 1e-6 && x <= a.max(b) + 1e-6;
        for sr in [44_100u32, 48_000, 96_000] {
            let len = (0.003 * sr as f64).round() as usize;
            let mut ease = JumpEase::default();
            ease.next((0.5, -0.5), sr, false);
            ease.requested = true;
            let mut prev = (0.5f32, -0.5f32);
            for i in 1..=len {
                let out = ease.next((-0.5, 0.5), sr, false);
                assert!(between(out.0, prev.0, -0.5) && between(out.1, prev.1, 0.5), "{sr} Hz frame {i}: {out:?} after {prev:?}");
                prev = out;
            }
            assert_eq!(prev, (-0.5, 0.5), "{sr} Hz: not at the new audio after {len} frames");
            assert!(ease.from.is_none());

            // A second jump, then a third a third of the way into its ease: that one starts from the frame last heard.
            ease.requested = true;
            for _ in 0..len / 3 {
                prev = ease.next((0.5, -0.5), sr, false);
            }
            let heard = prev;
            ease.requested = true;
            let first = ease.next((-0.25, 0.25), sr, false);
            assert!((first.0 - heard.0).abs() < 0.01 && (first.1 - heard.1).abs() < 0.01, "{sr} Hz: {first:?} after {heard:?}");
            for _ in 1..len {
                let out = ease.next((-0.25, 0.25), sr, false);
                assert!(between(out.0, heard.0, -0.25) && between(out.1, heard.1, 0.25));
            }
            assert_eq!(ease.next((0.1, 0.2), sr, false), (0.1, 0.2));
        }
        let mut ease = JumpEase { last: (0.5, -0.5), requested: true, ..Default::default() };
        assert_eq!(ease.next((-0.5, 0.5), 48_000, true), (-0.5, 0.5), "key lock's crossfade covers the jump");
        assert!(!ease.requested && ease.from.is_none());
    }

    fn tone_track(hz: f32, sr: u32, secs: f32) -> LoadedTrackAudio {
        let pcm_stereo = (0..(sr as f32 * secs) as usize)
            .flat_map(|i| {
                let v = (i as f32 * hz * std::f32::consts::TAU / sr as f32).sin() * 0.5;
                [v, v]
            })
            .collect::<Vec<_>>()
            .into();
        LoadedTrackAudio {
            title: "tone".into(),
            artist: String::new(),
            duration_sec: secs as f64,
            bpm: 120.0,
            grid_offset_sec: 0.0,
            key: String::new(),
            sample_rate: sr,
            waveform_peaks: Vec::new().into(),
            stem_peaks: Vec::new().into(),
            pcm_stereo,
            stems_ready: false,
            pcm_drums: None,
            pcm_bass: None,
            pcm_other: None,
            pcm_vocals: None,
            phrases: Vec::new(),
        }
    }

    /// Peak of each output channel over half a second of the real callback.
    fn render_peaks(setup: impl FnOnce(&mut AudioSharedState), channels: usize) -> Vec<f32> {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut st = shared.lock().unwrap();
            st.loaded_tracks[0] = Some(tone_track(1000.0, 48_000, 2.0));
            st.is_playing[0] = true;
            setup(&mut st);
        }
        let mut peaks = vec![0.0f32; channels];
        let mut buf = vec![0.0f32; 512 * channels];
        let mut renderer = Renderer::new(shared, Arc::default(), Arc::default());
        for block in 0..47 {
            renderer.render(&mut buf, channels, 48_000, |x| x);
            if block > 4 {
                for (i, x) in buf.iter().enumerate() {
                    peaks[i % channels] = peaks[i % channels].max(x.abs());
                }
            }
        }
        peaks
    }

    /// `cargo test --release -p retraktor-gui keylock_cpu -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn keylock_cpu() {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut st = shared.lock().unwrap();
            for d in 0..4 {
                let mut t = tone_track(440.0, 44_100, 30.0);
                for s in [&mut t.pcm_drums, &mut t.pcm_bass, &mut t.pcm_other, &mut t.pcm_vocals] {
                    *s = Some(t.pcm_stereo.clone());
                }
                st.loaded_tracks[d] = Some(t);
                st.is_playing[d] = true;
                st.keylock[d] = true;
                st.pitch_ratio[d] = 1.04;
            }
        }
        let mut buf = vec![0.0f32; 256 * 2];
        let mut renderer = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        for locked in [false, true] {
            shared.lock().unwrap().keylock = [locked; 4];
            let t0 = std::time::Instant::now();
            for _ in 0..(48_000 * 10 / 256) {
                renderer.render(&mut buf, 2, 48_000, |x| x);
            }
            println!("10 s of 4 decks with stems, key lock {locked}: {:?}", t0.elapsed());
        }
    }

    /// Counts heap allocations and frees made by the current thread while `COUNTING` is set.
    struct CountingAlloc;
    thread_local! {
        static COUNTING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        static HEAP_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    fn note_heap_call() {
        let _ = COUNTING.try_with(|on| if on.get() { HEAP_CALLS.with(|n| n.set(n.get() + 1)) });
    }
    unsafe impl std::alloc::GlobalAlloc for CountingAlloc {
        unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
            note_heap_call();
            unsafe { std::alloc::System.alloc(l) }
        }
        unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
            note_heap_call();
            unsafe { std::alloc::System.dealloc(p, l) }
        }
        unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, size: usize) -> *mut u8 {
            note_heap_call();
            unsafe { std::alloc::System.realloc(p, l, size) }
        }
    }
    #[global_allocator]
    static ALLOC: CountingAlloc = CountingAlloc;

    /// The callback never touches the heap: four decks (stems, key lock at three rates, a 96 kHz track, then
    /// a 384 kHz one on a 22.05 kHz card, a 17x step), recording to disk (and its stop at that rate change),
    /// the pattern player, a scratch release, a jump
    /// ease, seeks, track swaps that release audio, busy and free blocks, f32 and i16, 128 and 1024-frame
    /// buffers, and a fresh callback state as after a reopen.
    #[test]
    fn callback_never_allocates_or_frees() {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        let mut stemmed = tone_track(440.0, 44_100, 4.0);
        for s in [&mut stemmed.pcm_drums, &mut stemmed.pcm_bass, &mut stemmed.pcm_other, &mut stemmed.pcm_vocals] {
            *s = Some(Arc::new(stemmed.pcm_stereo.iter().map(|v| v * 0.25).collect()));
        }
        {
            let mut st = shared.lock().unwrap();
            st.loaded_tracks = [Some(stemmed), Some(tone_track(330.0, 48_000, 4.0)), Some(tone_track(220.0, 96_000, 4.0)), Some(tone_track(110.0, 44_100, 4.0))];
            st.is_playing = [true; 4];
            st.keylock = [true, true, true, false];
            st.pitch_ratio = [1.04, 0.92, 1.08, 1.0];
            st.pattern_enabled = true;
            st.ease_jump(3);
            st.scratch_rate[3] = 0.5;
            st.disable_scratch(3);
        }
        let (stats, recorder) = (Arc::new(AudioStats::default()), Arc::default());
        let rec = start_take(&recorder, "alloc");
        let mut calls = 0;
        let mut counted = |renderer: &mut Renderer, frames: usize, i16_out: bool, sr: u32| {
            let (mut f, mut i) = (vec![0.0f32; frames * 2], vec![0i16; frames * 2]);
            COUNTING.with(|c| c.set(true));
            if i16_out { renderer.render(&mut i, 2, sr, pcm_i16) } else { renderer.render(&mut f, 2, sr, |x| x) }
            COUNTING.with(|c| c.set(false));
            calls += 1;
        };
        for round in 0..3 {
            if round == 2 {
                let mut st = shared.lock().unwrap();
                st.loaded_tracks[2] = Some(tone_track(220.0, 384_000, 1.0));
                st.seek_to(2, 0.1);
            }
            let sr = if round == 2 { 22_050 } else { 48_000 };
            let mut renderer = Renderer::new(Arc::clone(&shared), Arc::clone(&stats), Arc::clone(&recorder));
            for block in 0..120 {
                if block % 30 == 10 {
                    // A new track on deck D: the callback lets go of the old one's buffer.
                    let mut st = shared.lock().unwrap();
                    st.loaded_tracks[3] = Some(tone_track(110.0 + block as f32, 44_100, 4.0));
                    st.seek_to(3, 0.5);
                    st.seek_by(0, 0.25);
                }
                let busy = (block / 7) % 3 == 1;
                let held = busy.then(|| shared.lock().unwrap());
                counted(&mut renderer, if block % 5 == 0 { 1024 } else { 128 }, (block + round) % 2 == 0, sr);
                drop(held);
                if block % 40 == 39 {
                    drop(shared.lock().unwrap().take_retired()); // a UI frame
                }
            }
        }
        assert_eq!(HEAP_CALLS.with(|n| n.get()), 0, "heap calls in {calls} callbacks");
        assert!(stats.busy_blocks.load(Ordering::Relaxed) > 50);
        let (saved, take) = finish_take(&recorder, rec);
        assert_eq!(saved.reason, Some(crate::recorder::StopReason::RateChanged));
        assert!(take.len() > 2 * 240 * 128 && take.iter().any(|&s| s != 0), "nothing recorded");
    }

    /// A recording streaming to a fresh temp dir, its tap installed in `slot` as REC does.
    fn start_take(slot: &Mutex<Option<RecorderTap>>, name: &str) -> crate::recorder::Recording {
        let dir = std::env::temp_dir().join(format!("retraktor-audio-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (rec, tap) = crate::recorder::Recording::start(&dir, std::time::SystemTime::now()).unwrap();
        *slot.lock().unwrap() = Some(tap);
        rec
    }

    /// Stops as REC does, waits for the writer, and returns what was saved with the WAV's samples (the dir goes).
    fn finish_take(slot: &Mutex<Option<RecorderTap>>, mut rec: crate::recorder::Recording) -> (crate::recorder::Saved, Vec<i16>) {
        let tap = slot.lock().unwrap().take();
        rec.stop(tap);
        let t0 = std::time::Instant::now();
        let saved = loop {
            if let Some(outcome) = rec.poll() {
                break outcome.unwrap();
            }
            assert!(t0.elapsed() < std::time::Duration::from_secs(10), "the writer never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let bytes = std::fs::read(&rec.wav).unwrap();
        assert_eq!(bytes[..80], crate::recorder::wav_header(saved.frames, saved.sample_rate));
        std::fs::remove_dir_all(rec.wav.parent().unwrap()).unwrap();
        (saved, bytes[80..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect())
    }

    const PERIOD: f64 = 128.0 / 48_000.0;

    /// One 48 kHz deck playing a tone from 1 s, and the callback state that plays it.
    fn playing_deck() -> (Arc<Mutex<AudioSharedState>>, Arc<Mutex<Option<RecorderTap>>>, Renderer) {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut s = shared.lock().unwrap();
            s.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 12.0));
            s.playhead_sec[0] = 1.0;
            s.is_playing[0] = true;
        }
        let recorder = Arc::default();
        let renderer = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::clone(&recorder));
        (shared, recorder, renderer)
    }

    fn block(r: &mut Renderer) -> Vec<f32> {
        let mut out = vec![0.0f32; 256];
        r.render(&mut out, 2, 48_000, |x| x);
        out
    }

    /// After the UI held the lock for 100 callbacks (the deck played on to 1.27 s): a hot cue or click to 5 s
    /// lands on 5 s, also when 5 s is the very spot last published; a move by +2 s (beat jump, nudge, drag,
    /// sync) goes from where the deck had got to; a new track starts at 0 whatever the old one had played; a
    /// cue then a nudge in the same hold adds up. Each is checked after the first block back.
    #[test]
    fn seeks_after_a_long_ui_hold_land_as_asked() {
        let held_for_100 = |act: &dyn Fn(&mut AudioSharedState)| -> f64 {
            let (shared, _, mut r) = playing_deck();
            block(&mut r);
            {
                let mut s = shared.lock().unwrap();
                for _ in 0..100 {
                    block(&mut r);
                }
                act(&mut s);
            }
            block(&mut r);
            let p = shared.lock().unwrap().playhead_sec[0];
            p
        };
        let close = |got: f64, want: f64, what: &str| assert!((got - want).abs() < 1e-9, "{what}: at {got}, expected {want}");
        close(held_for_100(&|s| s.seek_to(0, 5.0)), 5.0 + PERIOD, "hot cue to 5 s");
        close(held_for_100(&|s| s.seek_to(0, 1.0 + PERIOD)), 1.0 + 2.0 * PERIOD, "cue to the spot last published");
        close(held_for_100(&|s| s.seek_by(0, 2.0)), 1.0 + 102.0 * PERIOD + 2.0, "a +2 s move from where the deck is");
        close(held_for_100(&|s| { s.seek_to(0, 5.0); s.seek_by(0, -0.5) }), 4.5 + PERIOD, "a cue, then a nudge");
        close(held_for_100(&|s| { s.seek_by(0, 2.0); s.seek_to(0, 3.0) }), 3.0 + PERIOD, "a nudge, then a cue");
        close(
            held_for_100(&|s| {
                s.loaded_tracks[0] = Some(tone_track(330.0, 48_000, 12.0));
                s.seek_to(0, 0.0);
            }),
            PERIOD,
            "a new track",
        );
    }

    /// A reopen right after blocks that found the lock busy: the next stream (or the fallback clock) carries on
    /// from where the deck really is, not from the last published playhead. Seeks still pending when the stream
    /// closes land as the callback would have applied them: a +2 s move from the deck's real spot, a cue exactly.
    /// A stream that closes late, after a newer one started, writes nothing.
    #[test]
    fn reopen_after_busy_blocks_keeps_the_real_position() {
        let (shared, recorder, mut r) = playing_deck();
        block(&mut r);
        {
            let _ui = shared.lock().unwrap();
            for _ in 0..50 {
                block(&mut r);
            }
        }
        drop(r); // the stream closes
        let p = shared.lock().unwrap().playhead_sec[0];
        assert!((p - (1.0 + 51.0 * PERIOD)).abs() < 1e-9, "after the reopen the deck is at {p}");
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::clone(&recorder));
        block(&mut r);
        assert!((shared.lock().unwrap().playhead_sec[0] - (1.0 + 52.0 * PERIOD)).abs() < 1e-9);

        // A cue made while the last callback was locked out survives the close.
        {
            let mut s = shared.lock().unwrap();
            block(&mut r);
            s.seek_to(0, 7.0);
        }
        drop(r);
        assert_eq!(shared.lock().unwrap().playhead_sec[0], 7.0);
        assert!(shared.lock().unwrap().retired.len() >= 1, "the closed stream's track audio wasn't handed back");

        // From 1 s: a block, 100 locked out, a +2 s move, the close, one block on the new stream.
        shared.lock().unwrap().seek_to(0, 1.0);
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::clone(&recorder));
        block(&mut r);
        {
            let mut s = shared.lock().unwrap();
            for _ in 0..100 {
                block(&mut r);
            }
            s.seek_by(0, 2.0);
        }
        drop(r);
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::clone(&recorder));
        block(&mut r);
        let p = shared.lock().unwrap().playhead_sec[0];
        assert!((p - (3.0 + 102.0 * PERIOD)).abs() < 1e-9, "a move pending at the close landed at {p}");

        // The old stream closes only after a new one has played a block and the UI moved the deck again.
        let mut newer = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::clone(&recorder));
        block(&mut newer);
        shared.lock().unwrap().seek_by(0, 1.0);
        let published = shared.lock().unwrap().playhead_sec[0];
        drop(r);
        assert_eq!(shared.lock().unwrap().playhead_sec[0], published, "the late close wrote over the newer stream");
        block(&mut newer);
        assert!((shared.lock().unwrap().playhead_sec[0] - (published + PERIOD)).abs() < 1e-9, "the newer stream lost the move");
    }

    /// A jog touch, 20 blocks of ticks scratching forward at about 2x, then release; then 75 blocks (200 ms)
    /// with no new input, the UI holding the lock or not, and 75 more. The release ramp runs in the callback
    /// either way: the deck is at the same spot at the same speed, and once back at playing speed the UI is
    /// told the ramp is over.
    #[test]
    fn jog_release_eases_back_while_the_ui_holds_the_lock() {
        let run = |busy: bool| -> (f64, f64, f64, bool) {
            let (shared, _, mut r) = playing_deck();
            block(&mut r);
            shared.lock().unwrap().enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
            for _ in 0..20 {
                shared.lock().unwrap().scratch_tick(0, 3); // 3 ticks a 2.67 ms block: about 2x at 33 1/3 rpm
                block(&mut r);
            }
            shared.lock().unwrap().disable_scratch(0);
            block(&mut r);
            {
                let _ui = busy.then(|| shared.lock().unwrap());
                for _ in 0..75 {
                    block(&mut r);
                }
            }
            block(&mut r);
            let after_hold = shared.lock().unwrap().playhead_sec[0];
            for _ in 0..75 {
                block(&mut r);
            }
            let s = shared.lock().unwrap();
            (after_hold, s.playhead_sec[0], s.scratch_rate[0], s.scratch_ramping[0])
        };
        let (calm, busy) = (run(false), run(true));
        assert_eq!(calm, busy, "the release played differently while the UI held the lock");
        assert_eq!((busy.2, busy.3), (1.0, false), "not back at playing speed");
    }

    /// `frames` frames of steady 0.25 at `sr`: every frame of it is audible at the output, even through the stem split.
    fn flat_track(frames: usize, sr: u32) -> LoadedTrackAudio {
        let mut t = tone_track(0.0, sr, 0.0);
        t.pcm_stereo = Arc::new(vec![0.25; frames * 2]);
        t.duration_sec = frames as f64 / sr as f64;
        t
    }

    fn render_at(r: &mut Renderer, sr: u32) -> Vec<f32> {
        let mut out = vec![0.0f32; 256];
        r.render(&mut out, 2, sr, |x| x);
        out
    }

    /// Plain playback stops after the last decoded frame: the deck sounds for exactly the output frames that read
    /// the track, whether they end inside a block, on a block's last frame, or within 100 ms of the start, at 44.1,
    /// 48 and 96 kHz, with the track and the card at different rates both ways, and pitched down. The block that ends
    /// it already tells the UI the deck stopped, exactly at the end; the next block is silent; Play then starts it
    /// from the top and it sounds again. A pause in mid-track resumes where it was.
    #[test]
    fn plain_playback_stops_after_the_last_frame() {
        // (track rate, card rate, pitch, track frames, output frames that read them)
        let mut cases: Vec<(u32, u32, f64, usize, usize)> = vec![
            (48_000, 96_000, 1.0, 384, 768),  // the last one reads half a frame past the track's last frame
            (96_000, 48_000, 1.0, 421, 211),  // every other track frame, the last one included
            (44_100, 48_000, 1.0, 441, 480),
            (48_000, 48_000, 0.9, 384, 427),
        ];
        for sr in [44_100u32, 48_000, 96_000] {
            for frames in [3 * 128, 3 * 128 + 37, (0.05 * sr as f64) as usize] {
                cases.push((sr, sr, 1.0, frames, frames));
            }
        }
        for (track_sr, sr, pitch, frames, heard_frames) in cases {
            let case = format!("{frames} frames at {track_sr} Hz on {sr} Hz at {pitch}x");
            let end = frames as f64 / track_sr as f64;
            let shared = Arc::new(Mutex::new(AudioSharedState::default()));
            {
                let mut s = shared.lock().unwrap();
                s.loaded_tracks[0] = Some(flat_track(frames, track_sr));
                s.pitch_ratio[0] = pitch;
                s.start_playing(0);
            }
            let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
            let heard: Vec<f32> = (0..heard_frames.div_ceil(128)).flat_map(|_| render_at(&mut r, sr)).step_by(2).collect();
            assert!(heard[..heard_frames].iter().all(|&x| x != 0.0), "{case}: a frame of the track went unheard");
            assert!(heard[heard_frames..].iter().all(|&x| x == 0.0), "{case}: sound after the last frame");
            {
                let s = shared.lock().unwrap();
                assert!(!s.is_playing[0], "{case}: the block that ended it didn't say so");
                assert_eq!(s.playhead_sec[0], end, "{case}: not parked at the end");
            }
            assert!(render_at(&mut r, sr).iter().all(|&x| x == 0.0), "{case}: the next block isn't silent");
            assert_eq!(shared.lock().unwrap().playhead_sec[0], end, "{case}: moved after the end");

            shared.lock().unwrap().start_playing(0);
            assert_eq!(shared.lock().unwrap().playhead_sec[0], 0.0, "{case}: Play at the end didn't go to the top");
            let again = render_at(&mut r, sr);
            assert!(again[..2 * 128.min(heard_frames)].iter().all(|&x| x != 0.0), "{case}: silent after Play");
            assert!(shared.lock().unwrap().is_playing[0] || heard_frames <= 128, "{case}: stopped again early");
        }

        let (shared, _, mut r) = playing_deck();
        block(&mut r);
        shared.lock().unwrap().is_playing[0] = false;
        block(&mut r);
        shared.lock().unwrap().start_playing(0);
        block(&mut r);
        assert!((shared.lock().unwrap().playhead_sec[0] - (1.0 + 2.0 * PERIOD)).abs() < 1e-9, "a mid-track pause didn't resume in place");
    }

    /// Ending exactly on a block's last frame leaves nothing stale to hear on a restart: a key-locked deck re-primes
    /// its time-stretch, and a jump ease still running on another deck is dropped. A third deck plays on throughout.
    #[test]
    fn track_end_on_a_block_boundary_clears_key_lock_and_jump_ease() {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut s = shared.lock().unwrap();
            s.loaded_tracks[0] = Some(flat_track(3 * 128, 48_000));
            s.loaded_tracks[1] = Some(flat_track(3 * 128, 48_000));
            s.loaded_tracks[2] = Some(tone_track(330.0, 48_000, 12.0));
            s.keylock[0] = true;
            s.is_playing = [true, true, true, false];
        }
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        block(&mut r);
        block(&mut r);
        shared.lock().unwrap().ease_jump(1); // its 3 ms ease outlasts the last 128 frames
        block(&mut r);
        {
            let s = shared.lock().unwrap();
            assert_eq!((s.is_playing, s.playhead_sec[0], s.playhead_sec[1]), ([false, false, true, false], 0.008, 0.008));
        }
        assert!(!r.state.keylock_engaged[0], "key lock would carry on from before the end");
        let ease = r.state.jump_ease[1];
        assert!(ease.from.is_none() && ease.last == (0.0, 0.0) && !ease.requested, "a jump ease survived the end");

        {
            let mut s = shared.lock().unwrap();
            s.start_playing(0);
            s.start_playing(1);
        }
        block(&mut r);
        assert!(r.state.keylock_engaged[0] && r.state.jump_ease[1].from.is_none());
        let s = shared.lock().unwrap();
        assert!(s.is_playing[0] && s.is_playing[1] && (s.playhead_sec[2] - 4.0 * PERIOD).abs() < 1e-9);
    }

    /// The deck runs out while the UI holds the lock, the UI still thinking it plays. With no command meanwhile,
    /// the first block back says it stopped, at the end, and plays nothing; Play then starts it from the top. A cue,
    /// a beat jump back, or a new track played during the hold are newer commands and stand; so is a Play (pause then
    /// Play, or Sync + Play) from the stale position the UI last saw, which starts from the top as the deck had ended.
    /// A new track whose audio must wait for the old one's to be retired keeps its play flag through the wait. A
    /// stream closing (a reopen) right after such a hold leaves the deck stopped too, unless a cue is pending, and
    /// the next stream follows.
    #[test]
    fn track_end_during_a_ui_hold_stays_stopped_unless_the_ui_moved_on() {
        let near_end = |act: &dyn Fn(&mut AudioSharedState)| {
            let (shared, recorder, mut r) = playing_deck();
            shared.lock().unwrap().seek_to(0, 11.99); // 480 frames to go
            block(&mut r);
            {
                let mut s = shared.lock().unwrap();
                for _ in 0..10 {
                    block(&mut r);
                }
                assert!(s.is_playing[0]);
                act(&mut s);
            }
            (shared, recorder, r)
        };
        let silent = |out: &[f32]| out.iter().all(|&x| x == 0.0);
        let state = |shared: &Arc<Mutex<AudioSharedState>>| {
            let s = shared.lock().unwrap();
            (s.is_playing[0], s.playhead_sec[0])
        };
        let close = |(playing, at): (bool, f64), want: f64, what: &str| {
            assert!(playing && (at - want).abs() < 1e-9, "{what}: playing {playing} at {at}, expected {want}");
        };

        let (shared, _, mut r) = near_end(&|_| {});
        assert!(silent(&block(&mut r)), "sound after the end");
        assert_eq!(state(&shared), (false, 12.0), "the stale play flag won");
        assert!(silent(&block(&mut r)) && state(&shared) == (false, 12.0));
        shared.lock().unwrap().start_playing(0);
        assert!(!silent(&block(&mut r)));
        close(state(&shared), PERIOD, "Play at the end");

        for (act, want, what) in [
            (&(|s: &mut AudioSharedState| s.seek_to(0, 5.0)) as &dyn Fn(&mut AudioSharedState), 5.0, "a cue"),
            (&|s: &mut AudioSharedState| s.seek_by(0, -2.0), 10.0, "a jump back"),
            (
                &|s: &mut AudioSharedState| {
                    s.loaded_tracks[0] = Some(tone_track(330.0, 48_000, 12.0));
                    s.seek_to(0, 0.0);
                    s.start_playing(0);
                },
                0.0,
                "a new track",
            ),
            (&|s: &mut AudioSharedState| { s.is_playing[0] = false; s.start_playing(0) }, 0.0, "pause then Play"),
            (&|s: &mut AudioSharedState| s.start_playing(0), 0.0, "Sync + Play"),
        ] {
            let (shared, _, mut r) = near_end(act);
            assert!(!silent(&block(&mut r)), "{what}: silent");
            close(state(&shared), want + PERIOD, what);
        }

        let (shared, _, mut r) = near_end(&|s| {
            while s.retired.len() < s.retired.capacity() {
                s.retired.push(Arc::default());
            }
            s.loaded_tracks[0] = Some(tone_track(330.0, 48_000, 12.0));
            s.seek_to(0, 0.0);
            s.start_playing(0);
        });
        assert!(silent(&block(&mut r)), "the old track played on");
        assert!(shared.lock().unwrap().is_playing[0], "the old track's end stopped the new one");
        drop(shared.lock().unwrap().take_retired()); // a UI frame
        assert!(!silent(&block(&mut r)));
        close(state(&shared), PERIOD, "the new track once taken");

        // Reopens: no blocks between the hold and the close.
        let (shared, recorder, r) = near_end(&|_| {});
        drop(r);
        assert_eq!(state(&shared), (false, 12.0), "the close let the stale play flag stand");
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), recorder);
        assert!(silent(&block(&mut r)) && state(&shared) == (false, 12.0), "the new stream played on");

        let (shared, recorder, r) = near_end(&|s| s.seek_to(0, 5.0));
        drop(r);
        assert_eq!(state(&shared), (true, 5.0), "the close dropped the cue");
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), recorder);
        assert!(!silent(&block(&mut r)));
        close(state(&shared), 5.0 + PERIOD, "a cue across a reopen");
    }

    /// At the end of the track an active loop keeps the deck going round it, also a loop that runs past the end; a
    /// held platter dragged forward past the end comes round to the start, and a paused one dragged back holds at 0,
    /// as before. None of them changes whether the deck plays.
    #[test]
    fn loops_and_scratches_cross_the_track_end_as_before() {
        for (start, len) in [(11.9, 0.08), (11.97, 0.1)] {
            let (shared, _, mut r) = playing_deck();
            {
                let mut s = shared.lock().unwrap();
                s.seek_to(0, start);
                (s.loop_active[0], s.loop_start_sec[0], s.loop_len_sec[0]) = (true, start, len);
            }
            for _ in 0..200 {
                block(&mut r);
            }
            let (playing, at) = {
                let s = shared.lock().unwrap();
                (s.is_playing[0], s.playhead_sec[0])
            };
            assert!(playing && at >= start && at < start + len, "loop {start}+{len}: playing {playing} at {at}");
            assert!(block(&mut r).iter().any(|&x| x != 0.0));
        }

        let scratch = |from: f64, playing: bool, ticks: i64, blocks: usize| {
            let (shared, _, mut r) = playing_deck();
            {
                let mut s = shared.lock().unwrap();
                s.seek_to(0, from);
                s.is_playing[0] = playing;
            }
            block(&mut r);
            shared.lock().unwrap().enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
            for _ in 0..blocks {
                shared.lock().unwrap().scratch_tick(0, ticks);
                block(&mut r);
            }
            let s = shared.lock().unwrap();
            (s.is_playing[0], s.playhead_sec[0])
        };
        let (playing, at) = scratch(11.98, true, 3, 20);
        assert!(playing && at < 0.2, "forward past the end: playing {playing} at {at}");
        assert_eq!(scratch(0.01, false, -3, 60), (false, 0.0), "back past the start");
    }

    /// With no sound card the timer clock follows the same rule: plain playback of a 44.1 kHz, a 96 kHz and a
    /// sub-100 ms track parks exactly at the end and stays there, and Play starts it from the top; a loop past the
    /// end and a held platter go round as before.
    #[test]
    fn fallback_clock_stops_at_the_track_end() {
        for (sr, secs) in [(44_100, 2.0), (96_000, 2.0), (48_000, 0.05)] {
            let mut s = AudioSharedState::default();
            let t = tone_track(440.0, sr, secs);
            let end = t.duration_sec;
            s.loaded_tracks[0] = Some(t);
            s.seek_to(0, end - 0.03);
            s.start_playing(0);
            fallback_tick(&mut s, 0.016);
            assert!(s.is_playing[0] && s.playhead_sec[0] < end, "{sr} Hz: stopped early");
            fallback_tick(&mut s, 0.016);
            assert_eq!((s.is_playing[0], s.playhead_sec[0]), (false, end), "{sr} Hz: not parked at the end");
            fallback_tick(&mut s, 0.016);
            assert_eq!((s.is_playing[0], s.playhead_sec[0]), (false, end), "{sr} Hz: moved after the end");
            s.start_playing(0);
            fallback_tick(&mut s, 0.016);
            assert!(s.is_playing[0] && (s.playhead_sec[0] - 0.016).abs() < 1e-9, "{sr} Hz: Play at the end");
        }

        let mut s = AudioSharedState::default();
        s.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 2.0));
        s.seek_to(0, 1.97);
        s.start_playing(0);
        (s.loop_active[0], s.loop_start_sec[0], s.loop_len_sec[0]) = (true, 1.97, 0.1);
        for _ in 0..20 {
            fallback_tick(&mut s, 0.016);
        }
        assert!(s.is_playing[0] && s.playhead_sec[0] >= 1.97 && s.playhead_sec[0] < 2.07, "loop past the end");

        s.loop_active[0] = false;
        s.seek_to(0, 1.99);
        (s.scratch_active[0], s.scratch_rate[0]) = (true, 1.0);
        fallback_tick(&mut s, 0.016);
        assert!(s.is_playing[0] && s.playhead_sec[0] == 0.0, "a held platter past the end");
    }

    /// With no sound card, a jog released while the deck plays eases up to the pitched speed and the ramp ends;
    /// plain playback then stops at the end of the track, and Play starts it from the top. Released while paused,
    /// the platter eases to a stop and stays there.
    #[test]
    fn fallback_clock_ends_a_jog_release_ramp() {
        let mut s = AudioSharedState::default();
        s.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 2.0));
        s.seek_to(0, 1.0);
        s.pitch_ratio[0] = 0.92;
        s.enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0); // a hand on the paused platter
        s.start_playing(0);
        s.disable_scratch(0); // and let go
        for _ in 0..100 {
            fallback_tick(&mut s, 0.016);
        }
        assert!(!s.scratch_ramping[0] && s.scratch_rate[0] == 0.92, "the release ramp never ended");
        assert_eq!((s.is_playing[0], s.playhead_sec[0]), (false, 2.0), "didn't stop at the end");
        s.start_playing(0);
        fallback_tick(&mut s, 0.016);
        assert!(s.is_playing[0] && (s.playhead_sec[0] - 0.016 * 0.92).abs() < 1e-9, "Play at the end");

        s.enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
        s.is_playing[0] = false;
        s.disable_scratch(0);
        for _ in 0..40 {
            fallback_tick(&mut s, 0.016);
        }
        let at = s.playhead_sec[0];
        fallback_tick(&mut s, 0.016);
        assert!(!s.scratch_ramping[0] && s.scratch_rate[0] == 0.0 && s.playhead_sec[0] == at, "a paused release kept moving");
    }

    /// A hand resting on a deck parked at the end of its track holds it there, on both clocks, so the DJ can drag
    /// back from the end to cue; it doesn't jump to the intro.
    #[test]
    fn a_hand_on_a_parked_deck_holds_the_end() {
        let (shared, _, mut r) = playing_deck();
        shared.lock().unwrap().seek_to(0, 11.99);
        for _ in 0..5 {
            block(&mut r);
        }
        assert_eq!(shared.lock().unwrap().playhead_sec[0], 12.0);
        shared.lock().unwrap().enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
        for _ in 0..10 {
            block(&mut r);
        }
        assert_eq!(shared.lock().unwrap().playhead_sec[0], 12.0, "a resting hand moved the parked deck");
        for _ in 0..30 {
            shared.lock().unwrap().scratch_tick(0, -3);
            block(&mut r);
        }
        let at = shared.lock().unwrap().playhead_sec[0];
        assert!(at > 11.5 && at < 12.0, "dragging back from the end landed at {at}");

        let mut s = AudioSharedState::default();
        s.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 2.0));
        s.seek_to(0, 1.99);
        s.start_playing(0);
        fallback_tick(&mut s, 0.016);
        assert_eq!((s.is_playing[0], s.playhead_sec[0]), (false, 2.0));
        s.enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
        for _ in 0..5 {
            fallback_tick(&mut s, 0.016);
        }
        assert_eq!(s.playhead_sec[0], 2.0, "a resting hand moved the parked deck without a sound card");
        s.scratch_rate[0] = -1.0; // the timer holds a touch's speed; this one drags back
        fallback_tick(&mut s, 0.016);
        assert!((s.playhead_sec[0] - (2.0 - 0.016)).abs() < 1e-9);
    }

    /// Paused inside an active loop that runs past the end of the track, Play carries on round the loop from where
    /// the deck was, not from the top.
    #[test]
    fn play_inside_a_loop_past_the_end_keeps_the_loop_phase() {
        let (shared, _, mut r) = playing_deck();
        {
            let mut s = shared.lock().unwrap();
            s.seek_to(0, 11.97);
            (s.loop_active[0], s.loop_start_sec[0], s.loop_len_sec[0]) = (true, 11.97, 0.1);
        }
        for _ in 0..15 {
            block(&mut r);
        }
        shared.lock().unwrap().is_playing[0] = false;
        block(&mut r);
        let paused_at = shared.lock().unwrap().playhead_sec[0];
        assert!(paused_at > 12.0 && paused_at < 12.07, "not paused past the end: {paused_at}");
        shared.lock().unwrap().start_playing(0);
        assert_eq!(shared.lock().unwrap().playhead_sec[0], paused_at, "Play left the loop");
        block(&mut r);
        let s = shared.lock().unwrap();
        assert!(s.is_playing[0] && (s.playhead_sec[0] - (paused_at + PERIOD)).abs() < 1e-9, "loop phase lost");
    }

    /// A plain sum of 1/48000 s steps over 60 s (and of 1/96000 s over 30 s) falls nanoseconds short of the end,
    /// which would play the last frame twice. The callback's compensated playhead, stepped as it steps it up to the
    /// last block, ends the track on its last frame: that block is audible to its end and says the deck stopped,
    /// exactly at the end; the next is silent.
    #[test]
    fn a_long_track_ends_on_its_last_frame() {
        for (sr, secs) in [(48_000u32, 60.0), (96_000, 30.0)] {
            let frames = (sr as f64 * secs) as usize;
            let dt = 1.0 / sr as f64;
            let plain = (0..frames).fold(0.0f64, |p, _| p + dt);
            assert!(plain < secs - 1e-9, "{sr} Hz: the plain sum ({plain}) reaches the end, so this case shows nothing");
            let shared = Arc::new(Mutex::new(AudioSharedState::default()));
            {
                let mut s = shared.lock().unwrap();
                let mut t = flat_track(256, sr); // reads past these frames hold the last one: no minutes of PCM
                t.duration_sec = secs;
                s.loaded_tracks[0] = Some(t);
                s.is_playing[0] = true;
            }
            let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
            for _ in 0..frames - 128 {
                advance_playhead(&mut r.state, 0, dt);
            }
            assert!(render_at(&mut r, sr).iter().all(|&x| x != 0.0), "{sr} Hz: the last block isn't all heard");
            {
                let s = shared.lock().unwrap();
                assert_eq!((s.is_playing[0], s.playhead_sec[0]), (false, secs), "{sr} Hz: still playing after the last frame");
            }
            assert!(render_at(&mut r, sr).iter().all(|&x| x == 0.0), "{sr} Hz: the last frame played again");
        }
    }

    /// Six seconds of UI hold while two decks play and REC streams to disk, in f32 and in i16: every block lands
    /// in the take as it plays (the recording clock moves by exactly its frames, nothing is caught up later), and
    /// the WAV is the main output exactly, partial last chunk included. Nothing is recorded after the stop. A REC
    /// press that lands while a callback runs costs the take only that edge block, never a wait in the callback.
    #[test]
    fn recording_is_gapless_through_a_long_ui_hold_and_cut_on_blocks() {
        for to_i16 in [false, true] {
            let (shared, slot, mut r) = playing_deck();
            {
                let mut s = shared.lock().unwrap();
                s.loaded_tracks[1] = Some(tone_track(330.0, 44_100, 12.0));
                s.is_playing[1] = true;
            }
            // One 128-frame block; its main output as the recorder quantizes it.
            let play = |r: &mut Renderer| -> Vec<i16> {
                if to_i16 {
                    let mut out = [0i16; 256];
                    r.render(&mut out, 2, 48_000, pcm_i16);
                    out.to_vec()
                } else {
                    block(r).into_iter().map(pcm_i16).collect()
                }
            };
            play(&mut r);
            let rec = start_take(&slot, &format!("gapless-{to_i16}"));
            let mut heard = Vec::new();
            {
                let _ui = shared.lock().unwrap();
                for n in 1..=2250 {
                    heard.extend(play(&mut r));
                    assert_eq!(rec.seconds(), (n * 128) as f64 / 48_000.0, "block {n} didn't land in the take as it played");
                }
            }
            heard.extend(play(&mut r));
            let at_stop = rec.seconds();
            let (saved, take) = finish_take(&slot, rec);
            play(&mut r); // after the stop
            assert_eq!((saved.frames, saved.sample_rate, saved.reason), (2251 * 128, 48_000, None));
            assert_eq!(at_stop, saved.seconds());
            assert!(heard.iter().step_by(2).any(|&v| v.abs() > 1000), "silent");
            assert!(take == heard, "{}: the WAV isn't what played", if to_i16 { "i16" } else { "f32" });
        }

        // REC pressed while a callback runs: that block plays on, outside the take; the take starts at the next.
        let (_shared, slot, mut r) = playing_deck();
        let pressing = slot.lock().unwrap();
        block(&mut r);
        drop(pressing);
        let rec = start_take(&slot, "edge");
        let first: Vec<i16> = block(&mut r).into_iter().map(pcm_i16).collect();
        let pressing = slot.lock().unwrap();
        block(&mut r); // and a stop pressed during this one ends the take before it
        drop(pressing);
        let (saved, take) = finish_take(&slot, rec);
        assert_eq!(saved.frames, 128);
        assert!(take == first, "the take isn't exactly its one block");
    }

    /// Which ways out `open` tries, in order: a named sound-server sink natively when PipeWire runs, then
    /// through the PipeWire and the Pulse ALSA plugin as separate attempts, with four channels when the sink
    /// has them and the cue asks (from the server's listing, else from the native node); a sink the server
    /// knows is gone goes straight to the default; a raw ALSA ID as that device even while sinks are listed;
    /// the system default last, natively first; ALSA only when PipeWire doesn't run.
    #[test]
    fn routes_try_native_first_and_keep_named_outputs() {
        use Route::*;
        let bridges = |channels| ["pipewire", "pulse"].map(|plugin| AlsaBridge { plugin, node: "usb.mx2".into(), channels });
        let native = |channels| NativeNode { node: "usb.mx2".into(), channels };
        let [pw4, pulse4] = bridges(4);
        assert_eq!(routes("pw:usb.mx2", Some(4), false, true, true), [native(Some(4)), pw4.clone(), pulse4.clone(), NativeDefault, AlsaDefault]);
        assert_eq!(routes("pw:usb.mx2", Some(4), false, false, true), [pw4, pulse4, AlsaDefault], "Pulse or plain ALSA");
        let [pw2, pulse2] = bridges(2);
        assert_eq!(routes("pw:usb.mx2", Some(4), false, true, false), [native(Some(2)), pw2.clone(), pulse2.clone(), NativeDefault, AlsaDefault], "split cue needs only stereo");
        assert_eq!(routes("pw:usb.mx2", Some(2), false, true, true)[0], native(Some(2)));
        assert_eq!(routes("pw:usb.mx2", None, false, true, true), [native(None), pw2, pulse2, NativeDefault, AlsaDefault], "no pactl: the node decides");
        assert_eq!(routes("pw:qa.gone", None, true, true, true), [NativeDefault, AlsaDefault], "a sink the server knows is gone");
        assert_eq!(routes("alsa:hw:CARD=MX2,DEV=0", None, false, true, true), [Alsa { id: "alsa:hw:CARD=MX2,DEV=0".into() }, NativeDefault, AlsaDefault]);
        assert_eq!(routes("", None, false, true, true), [NativeDefault, AlsaDefault]);
        assert_eq!(routes("", None, false, false, true), [AlsaDefault]);
    }

    /// Without pactl, the graph's own metadata gives each sink its width: four from the format a node offers
    /// even without `audio.channels` (as the four-output test sink has), four from `audio.position` alone, two
    /// for stereo and for a sink that states nothing; streams and other nodes aren't outputs.
    #[test]
    fn graph_sinks_take_their_width_from_the_graph() {
        let dump = serde_json::json!([
            { "type": "PipeWire:Interface:Node", "info": { "props": { "node.name": "Dummy-Driver" } } },
            { "type": "PipeWire:Interface:Node", "info": {
                "props": { "node.name": "test.stereo", "node.description": "Test Stereo", "media.class": "Audio/Sink", "audio.position": "[ FL FR ]" },
                "params": { "EnumFormat": [ { "mediaSubtype": "raw", "channels": 2, "position": ["FL", "FR"] } ] } } },
            { "type": "PipeWire:Interface:Node", "info": {
                "props": { "node.name": "test.four", "node.description": "Test Four", "media.class": "Audio/Sink", "audio.position": "[ FL FR RL RR ]" },
                "params": {
                    "EnumFormat": [ { "mediaSubtype": "raw", "channels": 4, "position": ["FL", "FR", "RL", "RR"] } ],
                    "Format": [ { "mediaSubtype": "raw", "channels": 4, "position": ["FL", "FR", "RL", "RR"] } ] } } },
            { "type": "PipeWire:Interface:Node", "info": { "props": { "node.name": "pro.aux", "media.class": "Audio/Sink", "audio.position": "AUX0,AUX1,AUX2,AUX3" } } },
            { "type": "PipeWire:Interface:Node", "info": { "props": { "node.name": "bare", "media.class": "Audio/Sink" } } },
            { "type": "PipeWire:Interface:Node", "info": {
                "props": { "node.name": "preview-final", "media.class": "Stream/Output/Audio" },
                "params": { "Format": [ { "channels": 2 } ] } } },
            { "type": "PipeWire:Interface:Port", "info": { "props": { "port.name": "playback_FL" } } }
        ]);
        let found: Vec<(String, String, u16)> = graph_sinks(&dump).into_iter().map(|c| (c.id, c.label, c.channels)).collect();
        assert_eq!(
            found,
            [
                ("pw:test.stereo".into(), "Test Stereo".into(), 2),
                ("pw:test.four".into(), "Test Four".into(), 4),
                ("pw:pro.aux".into(), "pro.aux".into(), 4),
                ("pw:bare".into(), "bare".into(), 2),
            ]
        );
        assert!(graph_sinks(&serde_json::json!({})).is_empty(), "not a dump");
    }

    /// The ALSA plugin's target is set only for its own attempt; whatever the user had set comes back.
    #[test]
    fn plugin_target_env_is_scoped_and_restored() {
        let var = "RETRAKTOR_TEST_SINK_TARGET";
        std::env::set_var(var, "users.own.sink");
        assert_eq!(with_env(var, "qa.four", || std::env::var(var).unwrap()), "qa.four");
        assert_eq!(std::env::var(var).unwrap(), "users.own.sink");
        std::env::remove_var(var);
        with_env(var, "qa.four", || ());
        assert!(std::env::var_os(var).is_none(), "an unset variable was left set");
    }

    /// Non-xrun stream errors are counted and named for the UI; underruns only counted.
    #[test]
    fn stream_errors_are_counted_not_printed() {
        let stats = AudioStats::default();
        stats.stream_error(cpal::ErrorKind::Xrun);
        stats.stream_error(cpal::ErrorKind::DeviceNotAvailable);
        stats.stream_error(cpal::ErrorKind::InvalidInput);
        assert_eq!((stats.xruns.load(Ordering::Relaxed), stats.stream_errors.load(Ordering::Relaxed)), (1, 2));
        assert_eq!(stats.last_error_kind.load(Ordering::Relaxed), STREAM_ERROR_KINDS.len(), "an unlisted kind is other");
    }

    #[test]
    fn cue_reaches_headphones_on_3_4_and_split() {
        // Fader down, CUE on: silent on the speakers, audible in the headphones.
        let cue_only = |st: &mut AudioSharedState| {
            st.engine.strips[0].volume = 0.0;
            st.engine.strips[0].cue_enabled = true;
        };
        let p = render_peaks(cue_only, 4);
        assert!(p[0] < 0.01 && p[1] < 0.01, "main should be silent: {p:?}");
        assert!(p[2] > 0.1 && p[3] > 0.1, "headphones should hear the cue: {p:?}");

        let p = render_peaks(|st| { cue_only(st); st.headphone_out = HeadphoneOut::Split; }, 2);
        assert!(p[0] > 0.1 && p[1] < 0.01, "split: left = cue, right = main: {p:?}");

        // CUE off: nothing in the headphones at the CUE end of the blend.
        let p = render_peaks(|st| st.engine.strips[0].volume = 0.0, 4);
        assert!(p[2] < 0.01, "no cue pressed, headphones silent: {p:?}");
    }

    #[test]
    fn mid_kill_removes_a_1khz_tone_and_key_lock_keeps_playing() {
        let open = render_peaks(|_| {}, 2);
        let killed = render_peaks(|st| st.engine.strips[0].eq.mid = 0.0, 2);
        assert!(killed[0] < open[0] * 0.2, "mid kill: {killed:?} vs {open:?}");
        let locked = render_peaks(|st| { st.keylock[0] = true; st.pitch_ratio[0] = 1.06; }, 2);
        assert!(locked[0] > open[0] * 0.7, "key lock output: {locked:?} vs {open:?}");
    }

    #[test]
    fn test_alpha_beta_filter_scratch_physics() {
        let mut filter = AlphaBetaFilter::default();
        let dt = 0.002; // 2ms
        filter.init(dt, 0.0, 0.125, 0.125 / 32.0);

        // Constant forward rotation at 1.0x vinyl speed (dx = dt * 1.0)
        for _ in 0..100 {
            filter.observation(dt * 1.0, dt);
        }
        let v = filter.predicted_velocity();
        assert!((v - 1.0).abs() < 0.05, "Velocity should track ~1.0, got {v}");

        // Stop platter (dx = 0)
        for _ in 0..100 {
            filter.observation(0.0, dt);
        }
        let v_stop = filter.predicted_velocity();
        assert!(v_stop.abs() < 0.05, "Velocity should settle near 0.0, got {v_stop}");

        // Reverse scratch at -2.0x vinyl speed (dx = dt * -2.0)
        for _ in 0..100 {
            filter.observation(dt * -2.0, dt);
        }
        let v_rev = filter.predicted_velocity();
        assert!((v_rev - -2.0).abs() < 0.1, "Velocity should track ~ -2.0, got {v_rev}");
    }

    #[test]
    fn test_hermite_cubic_interpolation() {
        // Test a linear ramp signal: y = 2.0 * x
        let pcm = vec![0.0, 0.0, 2.0, 2.0, 4.0, 4.0, 6.0, 6.0, 8.0, 8.0];
        // At frame position 2.5, exact value should be 5.0
        let (l, r) = sample_stereo_hermite(&pcm, 2.5);
        assert!((l - 5.0).abs() < 1e-4);
        assert!((r - 5.0).abs() < 1e-4);
    }

    #[test]
    fn test_waveform_and_stem_peak_crossover_bounds() {
        let silence = vec![0.0_f32; 48000 * 2];
        let (wf_peaks, stem_peaks) = LoadedTrackAudio::compute_waveform_peaks(&silence, 48000);
        assert!(!wf_peaks.is_empty());
        assert!(!stem_peaks.is_empty());
        for p in &wf_peaks {
            assert!(p[0] >= 0.0 && p[0] <= 1.0 && !p[0].is_nan());
            assert!(p[1] >= 0.0 && p[1] <= 1.0 && !p[1].is_nan());
            assert!(p[2] >= 0.0 && p[2] <= 1.0 && !p[2].is_nan());
        }
        for s in &stem_peaks {
            for &val in s {
                assert!(val >= 0.0 && val <= 1.0 && !val.is_nan());
            }
        }

        let square: Vec<f32> = (0..96000)
            .map(|i| if (i / 100) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let (wf_peaks_sq, stem_peaks_sq) = LoadedTrackAudio::compute_waveform_peaks(&square, 48000);
        assert!(!wf_peaks_sq.is_empty());
        assert!(!stem_peaks_sq.is_empty());
        for p in &wf_peaks_sq {
            assert!(p[0] <= 1.0 && p[1] <= 1.0 && p[2] <= 1.0);
        }
    }
}
