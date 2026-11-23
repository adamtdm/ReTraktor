use cpal::traits::{DeviceTrait, HostTrait};
#[cfg(not(test))]
use cpal::traits::StreamTrait;
use retraktor_audio::FourDeckAudioEngine;
use crate::keylock::KeyLock;
use crate::recorder::RecorderTap;
use retraktor_dsp::{DeckFxUnit, MixerFxKind};
use std::collections::HashMap;
use std::f32::consts::PI;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
#[cfg(not(test))]
use std::thread;
#[cfg(not(test))]
use std::time::Duration;
use std::time::Instant;
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
    /// The whole-track overview drawn under the deck: at first from `waveform_peaks`, then the track's stripe as the
    /// cache keeps it (this load's or a browser row's, whichever was kept first), as its browser row draws it.
    pub overview: crate::overview::Overview,
    pub stems_ready: bool,
    /// True isolated AI stems (from Demucs or synthesis). If Some, played with zero bleed.
    pub pcm_drums: Option<Arc<Vec<f32>>>,
    pub pcm_bass: Option<Arc<Vec<f32>>>,
    pub pcm_other: Option<Arc<Vec<f32>>>,
    pub pcm_vocals: Option<Arc<Vec<f32>>>,
    pub phrases: Vec<retraktor_library::Phrase>,
    /// Auto gain trim (linear), played after the stems beside the channel GAIN. `from_file`
    /// measures it; a deck load with Auto gain off sets it to 1.0.
    pub auto_gain: f32,
    /// `waveform_peaks` and `stem_peaks` as if the audio were `auto_gain` times louder, so deck
    /// waveforms show the level the deck plays. The same arrays as the analysed ones at unity.
    pub played_peaks: Arc<[[f32; 3]]>,
    pub played_stem_peaks: Arc<[[f32; 4]]>,
    /// `overview` at the played (auto-gain-matched) level, for deck overview heights.
    pub played_overview: crate::overview::Overview,
}

/// One waveform bucket's linear levels, before the display floors, ceilings and curves.
#[derive(Clone, Copy)]
pub struct BucketLevels {
    band_peak: [f32; 3],
    band_mean: [f32; 3],
    stem_peak: [f32; 4],
}

impl LoadedTrackAudio {
    /// Computes 3-band `[low, mid, high]` waveform peaks and 4-stem `[drums, bass, inst, vocals]` peaks.
    pub fn compute_waveform_peaks(
        pcm_stereo: &[f32],
        sample_rate: u32,
    ) -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
        if pcm_stereo.len() < 2 || sample_rate == 0 {
            return (vec![[0.1, 0.1, 0.1]; 120], vec![[0.1, 0.1, 0.1, 0.1]; 120]);
        }
        Self::shape_peaks(&Self::measure_buckets(pcm_stereo, sample_rate), 1.0)
    }

    /// Display peaks from `buckets` as if the audio were `gain` times louder: the gain scales the
    /// linear levels before the floors, ceilings and curves, as louder audio would.
    pub fn shape_peaks(buckets: &[BucketLevels], gain: f32) -> (Vec<[f32; 3]>, Vec<[f32; 4]>) {
        buckets.iter().map(|b| {
            let [max_low, max_mid, max_high] = b.band_peak.map(|v| v * gain);
            let [avg_low, avg_mid, avg_high] = b.band_mean.map(|v| v * gain);
            let [max_drums, max_bass, max_inst, max_vocals] = b.stem_peak.map(|v| (v * gain).min(1.0));
            // Blend RMS and Peak with power-curve shaping for realistic Traktor Pro 4 transient silhouettes
            let shaped_low = ((max_low * 0.65 + avg_low * 0.75) * 1.15).clamp(0.02, 0.96).powf(0.85);
            let shaped_mid = ((max_mid * 0.60 + avg_mid * 0.70) * 1.25).clamp(0.02, 0.92).powf(0.95);
            let shaped_high = ((max_high * 0.55 + avg_high * 0.65) * 1.35).clamp(0.01, 0.88).powf(1.05);
            ([shaped_low, shaped_mid, shaped_high], [
                (max_drums * 0.88).clamp(0.02, 0.95).powf(0.9),
                (max_bass * 0.85).clamp(0.02, 0.92).powf(0.9),
                (max_inst * 0.78).clamp(0.02, 0.86).powf(1.05),
                (max_vocals * 0.80).clamp(0.02, 0.88).powf(1.0),
            ])
        }).unzip()
    }

    /// Measures the linear levels of every `PEAKS_PER_SEC` bucket of `pcm_stereo` in one pass.
    pub fn measure_buckets(pcm_stereo: &[f32], sample_rate: u32) -> Vec<BucketLevels> {
        let total_frames = pcm_stereo.len() / 2;
        let frames_per_bucket = ((sample_rate as usize) / PEAKS_PER_SEC).max(64);
        let num_buckets = (total_frames / frames_per_bucket).max(1);
        let mut buckets = Vec::with_capacity(num_buckets);

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

                // Uncapped here; `shape_peaks` caps them at 1.0 after the gain.
                let drums_val = transient * 2.1 + high * 0.65;
                let bass_val = low * 1.15;
                let inst_val = side_ch.abs() * 1.6 + mid * 0.55;
                let vocal_val = (mid - side_ch.abs() * 0.5).max(0.0) * 1.45;

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
            buckets.push(BucketLevels {
                band_peak: [max_low, max_mid, max_high],
                band_mean: [sum_low / n, sum_mid / n, sum_high / n],
                stem_peak: [max_drums, max_bass, max_inst, max_vocals],
            });
        }
        buckets
    }

    /// Decodes a real audio file (`.mp3`, `.flac`, `.wav`, `.ogg`, `.m4a`, `.aac`) from disk using Symphonia.
    /// Cached AI stems are not loaded here ([`Self::load_stems_from_dir`]), so a deck can play before they arrive.
    pub fn from_file(
        path: &Path,
        fallback_title: &str,
        fallback_artist: &str,
        fallback_bpm: f64,
        fallback_key: &str,
    ) -> Result<Self, String> {
        Self::from_file_with_autogain(path, fallback_title, fallback_artist, fallback_bpm, fallback_key, true)
    }

    /// Like [`Self::from_file`], skipping EBU R128 metering and second peak shaping when `autogain` is false.
    pub fn from_file_with_autogain(
        path: &Path,
        fallback_title: &str,
        fallback_artist: &str,
        fallback_bpm: f64,
        fallback_key: &str,
        autogain: bool,
    ) -> Result<Self, String> {
        type CacheEntry = (crate::overview::FileIdentity, f32);
        static AUTOGAIN_CACHE: LazyLock<Mutex<HashMap<PathBuf, CacheEntry>>> = LazyLock::new(Mutex::default);

        let cache_key = if autogain { path.canonicalize().ok().zip(file_identity(path)) } else { None };
        let mut file = AudioFile::open(path)?;
        let (tag_title, tag_artist) = file.tags();
        let pcm_stereo = file.decode_all(&|| false)?;
        let cache_key = cache_key.filter(|(_, identity)| file_identity(path) == Some(*identity));
        let cached_gain = cache_key.as_ref().and_then(|(path, identity)| {
            let (cached_identity, gain) = AUTOGAIN_CACHE.lock().unwrap_or_else(PoisonError::into_inner).get(path).copied()?;
            (cached_identity == *identity).then_some(gain)
        });
        let mut loaded = Self::from_pcm_with_gain(
            pcm_stereo, file.sample_rate, fallback_title, fallback_artist, fallback_bpm, fallback_key,
            if autogain { cached_gain } else { Some(1.0) },
        );
        if let Some((key, identity)) = cache_key.filter(|(_, identity)| cached_gain.is_none() && file_identity(path) == Some(*identity)) {
            let mut cache = AUTOGAIN_CACHE.lock().unwrap_or_else(PoisonError::into_inner);
            if cache.len() >= 2048 {
                cache.clear();
            }
            cache.insert(key, (identity, loaded.auto_gain));
        }
        loaded.title = tag_title.unwrap_or(loaded.title);
        loaded.artist = tag_artist.unwrap_or(loaded.artist);
        Ok(loaded)
    }

    /// A deck track from stereo PCM already decoded: its waveform, stem peaks and rough phrases.
    pub fn from_pcm(pcm_stereo: Vec<f32>, sr: u32, title: &str, artist: &str, bpm: f64, key: &str) -> Self {
        Self::from_pcm_with_autogain(pcm_stereo, sr, title, artist, bpm, key, true)
    }

    pub fn from_pcm_with_autogain(pcm_stereo: Vec<f32>, sr: u32, title: &str, artist: &str, bpm: f64, key: &str, autogain: bool) -> Self {
        Self::from_pcm_with_gain(pcm_stereo, sr, title, artist, bpm, key, (!autogain).then_some(1.0))
    }

    fn from_pcm_with_gain(pcm_stereo: Vec<f32>, sr: u32, title: &str, artist: &str, bpm: f64, key: &str, gain: Option<f32>) -> Self {
        let duration_sec = (pcm_stereo.len() / 2) as f64 / (sr as f64);
        let (buckets, auto_gain) = match gain {
            Some(gain) => (Self::measure_buckets(&pcm_stereo, sr), gain),
            None => std::thread::scope(|scope| {
                let meter = scope.spawn(|| retraktor_library::autogain_multiplier(&pcm_stereo, sr));
                (Self::measure_buckets(&pcm_stereo, sr), meter.join().unwrap())
            }),
        };
        let (waveform_peaks_vec, stem_peaks_vec) = Self::shape_peaks(&buckets, 1.0);
        let phrases = retraktor_library::detect_phrases(&waveform_peaks_vec, bpm, 0.0, duration_sec);
        let overview = crate::overview::from_peaks(&waveform_peaks_vec);
        let waveform_peaks: Arc<[[f32; 3]]> = waveform_peaks_vec.into();
        let stem_peaks: Arc<[[f32; 4]]> = stem_peaks_vec.into();
        let (played_peaks, played_stem_peaks, played_overview) = if (auto_gain - 1.0).abs() < 1e-4 {
            (Arc::clone(&waveform_peaks), Arc::clone(&stem_peaks), Arc::clone(&overview))
        } else {
            let (pp, psp) = Self::shape_peaks(&buckets, auto_gain);
            let po = crate::overview::from_peaks(&pp);
            (pp.into(), psp.into(), po)
        };
        Self {
            title: title.to_string(),
            artist: artist.to_string(),
            bpm,
            grid_offset_sec: 0.0,
            key: key.to_string(),
            duration_sec,
            pcm_stereo: Arc::new(pcm_stereo),
            sample_rate: sr,
            overview,
            waveform_peaks,
            stem_peaks,
            stems_ready: false,
            pcm_drums: None,
            pcm_bass: None,
            pcm_other: None,
            pcm_vocals: None,
            phrases,
            auto_gain,
            played_peaks,
            played_stem_peaks,
            played_overview,
        }
    }

    /// Decodes an arbitrary audio file to stereo interleaved f32 PCM samples and sample rate.
    pub fn decode_file_to_pcm(path: &Path) -> Result<(Vec<f32>, u32), String> {
        let mut file = AudioFile::open(path)?;
        Ok((file.decode_all(&|| false)?, file.sample_rate))
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
            let decode_stem = |path: &Path| {
                let (pcm, stem_sr) = Self::decode_file_to_pcm(path).ok()?;
                if stem_sr == self.sample_rate {
                    return Some(pcm);
                }
                let source_step = f64::from(stem_sr) / f64::from(self.sample_rate);
                let source_frames = pcm.len() / 2;
                let frames = (source_frames as f64 / source_step).round() as usize;
                if source_frames == 1 {
                    return Some(pcm.repeat(frames));
                }
                Some((0..frames).flat_map(|i| {
                    let (l, r) = sample_stereo_hermite(&pcm, i as f64 * source_step);
                    [l, r]
                }).collect())
            };
            let [drums_pcm, bass_pcm, other_pcm, vocals_pcm] = [&dp, &bp, &op, &vp].map(|p| decode_stem(p));

            if let (Some(d), Some(b), Some(o), Some(v)) = (drums_pcm, bass_pcm, other_pcm, vocals_pcm) {
                // Compute true stem waveform peaks from the isolated audio buffers
                let total_frames = self.pcm_stereo.len() / 2;
                let frames_per_bucket = ((self.sample_rate as usize) / PEAKS_PER_SEC).max(64);
                let num_buckets = (total_frames / frames_per_bucket).max(1);
                let mut real_peaks = Vec::with_capacity(num_buckets);
                let mut played_peaks = Vec::with_capacity(num_buckets);

                for b_idx in 0..num_buckets {
                    let start = b_idx * frames_per_bucket * 2;
                    let end = ((b_idx + 1) * frames_per_bucket * 2).min(self.pcm_stereo.len());
                    let maxima = [&d, &b, &o, &v].map(|pcm| {
                        pcm.get(start..end.min(pcm.len())).unwrap_or_default().iter()
                            .fold(0.0_f32, |peak, sample| peak.max(sample.abs()))
                    });
                    real_peaks.push(maxima.map(|m| m.clamp(0.04, 1.0)));
                    // Stems that arrive after a deck load use its latched gain; on load, `finish_loads` swaps in unity when Auto gain is off.
                    played_peaks.push(maxima.map(|m| (m * self.auto_gain).clamp(0.04, 1.0)));
                }

                self.pcm_drums = Some(Arc::new(d));
                self.pcm_bass = Some(Arc::new(b));
                self.pcm_other = Some(Arc::new(o));
                self.pcm_vocals = Some(Arc::new(v));
                self.stem_peaks = real_peaks.into();
                self.played_stem_peaks = played_peaks.into();
                self.stems_ready = true;
                return true;
            }
        }
        false
    }
}

/// The most audio decoded from one file: 15 minutes, so a huge mix can't exhaust RAM.
const MAX_DECODE_SEC: u64 = 900;

/// An audio file's default track, opened for decoding packet by packet into interleaved stereo.
pub(crate) struct AudioFile {
    source: AudioSource,
    track_id: u32,
    time_base: Option<symphonia::core::units::TimeBase>,
    /// The timestamp of the track's first frame (not zero in some containers).
    start_ts: u64,
    pub sample_rate: u32,
    /// Frames in the track as the file states them. Only an estimate for some files (raw AAC, an MP3 without a
    /// Xing or VBRI header), so nothing may end or be cut there.
    pub header_frames: Option<u64>,
    /// Reused for every packet, so decoding allocates only when a packet is larger than any before.
    samples: Option<SampleBuffer<f32>>,
}

/// Where an [`AudioFile`]'s packets come from.
enum AudioSource {
    Symphonia { format: Box<dyn symphonia::core::formats::FormatReader>, decoder: Box<dyn symphonia::core::codecs::Decoder> },
    /// Tests only: a scripted stream ([`TestStream`]) and the next packet in it.
    #[cfg(test)]
    Script(Arc<TestStream>, usize),
}

/// Tests only: a stream as a decoder would deliver it, packet by packet (first frame, interleaved stereo), to run
/// the real preview worker on what no small file can easily show: a header that misstates the length, a stream
/// that starts late, gaps, no seeking.
#[cfg(test)]
pub(crate) struct TestStream {
    pub rate: u32,
    pub header_frames: Option<u64>,
    pub packets: Vec<(u64, Vec<f32>)>,
    pub seekable: bool,
}

#[cfg(test)]
static TEST_STREAMS: Mutex<Vec<(std::path::PathBuf, Arc<TestStream>)>> = Mutex::new(Vec::new());

/// Tests only: [`AudioFile::open`] of `path` reads `stream` instead.
#[cfg(test)]
pub(crate) fn register_test_stream(path: &Path, stream: TestStream) {
    let mut streams = TEST_STREAMS.lock().unwrap_or_else(PoisonError::into_inner);
    streams.retain(|(p, _)| p != path);
    streams.push((path.to_path_buf(), Arc::new(stream)));
}

impl AudioFile {
    pub fn open(path: &Path) -> Result<Self, String> {
        #[cfg(test)]
        if let Some((_, stream)) = TEST_STREAMS.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(p, _)| p == path) {
            let (rate, header_frames) = (stream.rate, stream.header_frames);
            return Ok(Self { source: AudioSource::Script(Arc::clone(stream), 0), track_id: 0, time_base: None, start_ts: 0, sample_rate: rate, header_frames, samples: None });
        }
        let file = File::open(path).map_err(|e| format!("Failed to open {}: {e}", path.display()))?;
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let format = symphonia::default::get_probe()
            .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
            .map_err(|e| format!("Unsupported audio format in {}: {e}", path.display()))?
            .format;
        let (track_id, params) = {
            let track = format.default_track().ok_or_else(|| "No audio track found in file".to_string())?;
            (track.id, track.codec_params.clone())
        };
        let decoder = symphonia::default::get_codecs().make(&params, &DecoderOptions::default()).map_err(|e| format!("Codec error: {e}"))?;
        let sample_rate = params.sample_rate.filter(|&r| r > 0).unwrap_or(48_000);
        if sample_rate > 768_000 {
            return Err(format!("Unsupported sample rate {sample_rate} Hz in {}", path.display()));
        }
        let source = AudioSource::Symphonia { format, decoder };
        let mut file = Self { source, track_id, time_base: params.time_base, start_ts: params.start_ts, sample_rate, header_frames: None, samples: None };
        // The stated length is in the container's own ticks (an MP4's media duration, for one).
        file.header_frames = params.n_frames.map(|n| file.ts_to_frames(n)).filter(|&n| n > 0);
        Ok(file)
    }

    /// Title and artist from the file's own tags (ID3, Vorbis, MP4), when present and not blank.
    pub fn tags(&mut self) -> (Option<String>, Option<String>) {
        let (mut title, mut artist) = (None, None);
        let format = match &mut self.source {
            AudioSource::Symphonia { format, .. } => format,
            #[cfg(test)]
            AudioSource::Script(..) => return (title, artist),
        };
        if let Some(rev) = format.metadata().current() {
            for tag in rev.tags() {
                let (key, value) = (tag.key.to_ascii_lowercase(), tag.value.to_string());
                if value.trim().is_empty() {
                    continue;
                }
                if key == "title" || key == "tit2" {
                    title = Some(value);
                } else if key == "artist" || key == "tpe1" {
                    artist = Some(value);
                }
            }
        }
        (title, artist)
    }

    /// The most frames decoded from this file.
    pub fn max_frames(&self) -> u64 {
        self.sample_rate as u64 * MAX_DECODE_SEC
    }

    /// A packet's timestamp as a frame of the track, counted from its first frame.
    fn frame_of(&self, ts: u64) -> u64 {
        self.ts_to_frames(ts.saturating_sub(self.start_ts))
    }

    /// Converts between timestamps (or durations) and frames, for containers whose time base isn't one frame.
    fn ts_to_frames(&self, ts: u64) -> u64 {
        match self.time_base {
            Some(tb) if (tb.numer, tb.denom) != (1, self.sample_rate) => (ts as u128 * tb.numer as u128 * self.sample_rate as u128 / tb.denom.max(1) as u128) as u64,
            _ => ts,
        }
    }

    fn frames_to_ts(&self, frames: u64) -> u64 {
        match self.time_base {
            Some(tb) if (tb.numer, tb.denom) != (1, self.sample_rate) => (frames as u128 * tb.denom as u128 / (tb.numer.max(1) as u128 * self.sample_rate.max(1) as u128)) as u64,
            _ => frames,
        }
    }

    /// Decodes the next packet of the track onto `out` as interleaved stereo (mono doubled, channels past two
    /// dropped). Returns the frame it starts at, or `None` at the end or on a read error. Packets that fail to
    /// decode are skipped, as before.
    pub fn next_stereo(&mut self, out: &mut Vec<f32>) -> Option<u64> {
        let (format, decoder) = match &mut self.source {
            AudioSource::Symphonia { format, decoder } => (format, decoder),
            #[cfg(test)]
            AudioSource::Script(stream, next) => {
                let (frame, pcm) = stream.packets.get(*next)?;
                *next += 1;
                out.extend_from_slice(pcm);
                return Some(*frame);
            }
        };
        loop {
            let packet = format.next_packet().ok()?;
            if packet.track_id() != self.track_id {
                continue;
            }
            let Ok(decoded) = decoder.decode(&packet) else { continue };
            let spec = *decoded.spec();
            let ch = spec.channels.count().max(1);
            if !self.samples.as_ref().is_some_and(|b| b.capacity() >= decoded.capacity() * ch) {
                self.samples = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
            }
            let buf = self.samples.as_mut()?;
            buf.copy_interleaved_ref(decoded);
            let samples = buf.samples();
            match ch {
                2 => out.extend_from_slice(samples), // already [L, R, L, R, ...]
                1 => out.extend(samples.iter().flat_map(|&s| [s, s])),
                _ => out.extend(samples.chunks(ch).flat_map(|f| [f[0], f[1]])),
            }
            return Some(self.frame_of(packet.ts()));
        }
    }

    /// The whole track (up to [`MAX_DECODE_SEC`]) as interleaved stereo; gives up when `cancelled` says so.
    pub fn decode_all(&mut self, cancelled: &dyn Fn() -> bool) -> Result<Vec<f32>, String> {
        let max_samples = self.max_frames() as usize * 2;
        // Room for what the header promises (bounded by the cap), so the buffer isn't copied as it grows.
        let mut pcm = Vec::with_capacity(self.header_frames.map_or(0, |n| n.min(self.max_frames()) as usize * 2 + 8192));
        while pcm.len() < max_samples {
            if cancelled() {
                return Err("Cancelled".to_string());
            }
            if self.next_stereo(&mut pcm).is_none() {
                break;
            }
        }
        if pcm.is_empty() {
            return Err("Decoded 0 audio samples from file".to_string());
        }
        Ok(pcm)
    }

    /// Moves to just before `frame`, so the next packets lead up to it; the decoder starts afresh.
    pub fn seek(&mut self, frame: u64) -> Result<(), String> {
        let ts = self.frames_to_ts(frame).saturating_add(self.start_ts);
        match &mut self.source {
            AudioSource::Symphonia { format, decoder } => {
                format
                    .seek(symphonia::core::formats::SeekMode::Accurate, symphonia::core::formats::SeekTo::TimeStamp { ts, track_id: self.track_id })
                    .map_err(|e| e.to_string())?;
                decoder.reset();
            }
            #[cfg(test)]
            AudioSource::Script(stream, next) => {
                if !stream.seekable {
                    return Err("not seekable".into());
                }
                *next = stream.packets.partition_point(|(at, _)| *at <= frame).saturating_sub(1);
            }
        }
        Ok(())
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

/// The most adjacent stereo pairs offered or saved for one output (channels 1–64).
pub const MAX_OUTPUT_PAIRS: u8 = 32;

/// Which output channels the main mix and the headphone (cue) mix are meant for. The UI, the stream width
/// and the audio callback all read it through [`OutputRoute::place`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputRoute {
    /// Each bus on its own adjacent stereo pair, by pair index (0 = channels 1–2, 1 = 3–4, …).
    /// `headphones: None` plays the main mix alone.
    Pairs { main: u8, headphones: Option<u8> },
    /// One stereo output split in mono on 1–2: left = headphones, right = main.
    Split,
}

impl Default for OutputRoute {
    /// Main on 1–2, headphones on 3–4, as ReTraktor always did.
    fn default() -> Self {
        OutputRoute::Pairs { main: 0, headphones: Some(1) }
    }
}

/// Where the mix actually lands on a stream of a given width.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Placement {
    /// Fewer than two channels: the main mix in mono, no headphones.
    Mono,
    /// Channel 1 = headphones in mono, channel 2 = main in mono.
    Split,
    /// The main mix from channel index `main`, the headphones from `phones` if they have a pair of their own.
    Pairs { main: usize, phones: Option<usize> },
}

impl OutputRoute {
    /// Where this route plays on a `channels`-wide stream. A main pair the stream lacks falls back to 1–2, so
    /// the main mix is never lost; headphones the stream lacks, or that would land on the main pair, are dropped.
    pub fn place(self, channels: usize) -> Placement {
        let fits = |pair: u8| Some(2 * pair as usize).filter(|first| first + 2 <= channels);
        match self {
            _ if channels < 2 => Placement::Mono,
            OutputRoute::Split => Placement::Split,
            OutputRoute::Pairs { main, headphones } => {
                let main = fits(main).unwrap_or(0);
                Placement::Pairs { main, phones: headphones.and_then(fits).filter(|&h| h != main) }
            }
        }
    }

    /// Channels a stream needs to reach every pair this route uses.
    pub fn width(self) -> u16 {
        match self {
            OutputRoute::Split => 2,
            OutputRoute::Pairs { main, headphones } => 2 * (main.max(headphones.unwrap_or(0)) as u16 + 1),
        }
    }

    /// The route on a system-default output, whose wiring is unknown: the main mix on 1–2 and no separate
    /// headphones, so a controller's cue never lands on laptop speakers. Split cue stays as chosen.
    pub fn on_system_default(self) -> Self {
        match self {
            OutputRoute::Split => self,
            OutputRoute::Pairs { .. } => OutputRoute::Pairs { main: 0, headphones: None },
        }
    }
}

impl Placement {
    /// Whether the cue mix (and so a preview) is heard anywhere.
    pub fn headphones_heard(self) -> bool {
        matches!(self, Placement::Split | Placement::Pairs { phones: Some(_), .. })
    }
}

/// "3–4" for the pair starting at channel index 2.
pub fn pair_label(first: usize) -> String {
    format!("{}–{}", first + 1, first + 2)
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
    /// Phase lock, set by the UI for each synced deck: the deck it follows, and both beatgrids as
    /// `(bpm, first beat sec)` with the follower's bpm already folded to the leader's octave.
    pub sync_follow: [Option<SyncLock>; 4],
    /// UI to callback: forget the deck's phase offset and correction (SYNC pressed, or it snapped on play).
    pub sync_reset: [bool; 4],
    /// Callback side: the speed factor phase correction applies, and the beat offset the DJ chose by hand.
    sync_adjust: [f64; 4],
    sync_user_offset: [f64; 4],
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
    /// Set only through [`AudioSystem`], which knows the open stream (tests set it directly).
    pub output_route: OutputRoute,
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
    scratch_target_rate: [f64; 4],
    scratch_last_tick_at: [Option<Instant>; 4],
    scratch_burst_ticks: [i64; 4],
    scratch_burst_dt: [f64; 4],
    scratch_burst_base: [f64; 4],
    scratch_tick_serial: [u64; 4],
    scratch_idle_sec: [f64; 4],
    pub scratch_dx_per_tick: [f64; 4],
    pub scratch_ramping: [bool; 4],
    pub scratch_ramp_target: [f64; 4],
    /// Shared side only: a jog touch or release for the callback to pick up (its physics runs there, every block).
    scratch_changed: [bool; 4],
    pub jog_nudge_rate: [f64; 4],
    jog_nudge_delta: [f64; 4],

    pub fx_enabled: [bool; 2],
    pub fx_kind: [MixerFxKind; 2],
    pub fx_dry_wet: [f32; 2],
    pub fx_params: [[f32; 3]; 2],
    pub deck_fx_assign: [[bool; 2]; 4],
    pub limiter_enabled: bool,

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

    /// Track and preview audio the callback let go of, freed by the UI thread ([`AudioSharedState::take_retired`])
    /// so a deallocation never runs in the callback. Its capacity is reserved up front.
    retired: Vec<Retired>,
    /// Which callback state is the current one; an older one, closing late, must not write positions.
    renderer_generation: u64,

    /// The library preview the UI wants in the headphones, and where in it to start (a fraction of the track).
    /// `preview_serial` counts every start and stop, so the callback swaps once per press, even to the same audio.
    preview: Option<(Arc<PreviewPcm>, f64)>,
    preview_serial: u64,
    /// A click on the overview while this serial plays: move there (a fraction of the track).
    preview_seek: Option<f64>,
    /// Published by the callback: how far it has got through the preview of that serial.
    pub preview_progress: PreviewProgress,
}

/// Audio handed back by the callback, of whatever kind, to be dropped off the real-time path.
pub type Retired = Arc<dyn std::any::Any + Send + Sync>;

/// How far the callback has played one preview, as of its last handoff.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PreviewProgress {
    pub serial: u64,
    /// Source frames played.
    pub frame: f64,
    /// `frame` is a real place in the track: the start (or last seek) has been found once its length was known.
    pub started: bool,
    /// It ran out, or a newer press is waiting to replace it: silent either way.
    pub ended: bool,
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
            sync_follow: [None; 4],
            sync_reset: [false; 4],
            sync_adjust: [1.0; 4],
            sync_user_offset: [0.0; 4],
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
            output_route: OutputRoute::default(),
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
            scratch_target_rate: [0.0; 4],
            scratch_last_tick_at: [None; 4],
            scratch_burst_ticks: [0; 4],
            scratch_burst_dt: [0.016; 4],
            scratch_burst_base: [0.0; 4],
            scratch_tick_serial: [0; 4],
            scratch_idle_sec: [0.0; 4],
            scratch_dx_per_tick: [1.0 / 568.888889; 4],
            scratch_ramping: [false; 4],
            scratch_ramp_target: [1.0; 4],
            scratch_changed: [false; 4],
            jog_nudge_rate: [0.0; 4],
            jog_nudge_delta: [0.0; 4],
            fx_enabled: [false; 2],
            fx_kind: [MixerFxKind::Reverb, MixerFxKind::DualDelay],
            fx_dry_wet: [0.3; 2],
            fx_params: [[0.5; 3]; 2],
            deck_fx_assign: [[true, false], [false, true], [true, false], [false, true]],
            limiter_enabled: true,
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
            preview: None,
            preview_serial: 0,
            preview_seek: None,
            preview_progress: PreviewProgress::default(),
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
        let next = self.playhead_sec[d] + delta_sec;
        self.playhead_sec[d] = match &self.loaded_tracks[d] {
            Some(t) => next.clamp(0.0, t.duration_sec.max(0.0)),
            None => next.max(0.0),
        };
        self.pending_seek[d] = Some(match self.pending_seek[d] {
            Some(Seek::To(_)) => Seek::To(self.playhead_sec[d]),
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
    pub fn take_retired(&mut self) -> Vec<Retired> {
        self.retired.drain(..).collect()
    }

    /// Plays `pcm` on the cue bus only from `at` (a fraction of the track), replacing any preview. Until that
    /// part is decoded the preview waits in silence. Returns its serial, to match against
    /// [`AudioSharedState::preview_progress`].
    pub fn start_preview(&mut self, pcm: Arc<PreviewPcm>, at: f64) -> u64 {
        self.preview = Some((pcm, at));
        self.preview_seek = None;
        self.preview_serial += 1;
        self.preview_serial
    }

    /// Moves the playing preview to `at` (a fraction of the track) at the callback's next handoff.
    pub fn seek_preview(&mut self, at: f64) {
        if self.preview.is_some() {
            self.preview_seek = Some(at);
        }
    }

    /// Silences the preview from the callback's next handoff; its audio comes back through `take_retired`.
    pub fn stop_preview(&mut self) {
        self.preview_seek = None;
        if self.preview.take().is_some() {
            self.preview_serial += 1;
        }
    }

    pub fn enable_scratch(
        &mut self,
        deck: usize,
        intervals_per_rev: u32,
        rpm: f64,
        _alpha: f64,
        _beta: f64,
    ) {
        let d = deck.min(3);
        let intervals_per_sec = (rpm * (intervals_per_rev as f64)) / 60.0;
        self.scratch_dx_per_tick[d] = if intervals_per_sec > 0.0 {
            1.0 / intervals_per_sec
        } else {
            1.0 / 568.888889
        };
        let init_v = if self.is_playing[d] { self.pitch_ratio[d] } else { 0.0 };
        self.scratch_rate[d] = init_v;
        self.scratch_target_rate[d] = init_v;
        self.scratch_last_tick_at[d] = None;
        self.scratch_burst_ticks[d] = 0;
        self.scratch_burst_dt[d] = 0.016;
        self.scratch_burst_base[d] = init_v;
        self.scratch_idle_sec[d] = 0.0;
        self.scratch_active[d] = true;
        self.scratch_ramping[d] = false;
        self.scratch_changed[d] = true;
    }

    pub fn scratch_tick(&mut self, deck: usize, ticks: i64) {
        self.scratch_tick_scaled(deck, ticks, 1.0);
    }

    /// Accumulates raw platter ticks within a burst before scaling by `sensitivity`, so fractional
    /// sensitivity gives the same speed whether ticks arrive as many tiny reports or one larger report.
    pub fn scratch_tick_scaled(&mut self, deck: usize, ticks: i64, sensitivity: f64) {
        let d = deck.min(3);
        if ticks == 0 || !self.scratch_active[d] {
            return;
        }
        let now = Instant::now();
        // Coalesce multi-report HID bursts drained in the same UI frame (<4 ms apart) so N reports
        // of +1 tick yield the exact same velocity as 1 report of +N ticks.
        let same_burst = self.scratch_last_tick_at[d]
            .is_some_and(|last| now.saturating_duration_since(last).as_secs_f64() < 0.004);
        if same_burst {
            self.scratch_burst_ticks[d] = self.scratch_burst_ticks[d].saturating_add(ticks);
        } else {
            let elapsed = self.scratch_last_tick_at[d].map_or(0.016, |last| now.saturating_duration_since(last).as_secs_f64());
            let already_held = self.scratch_idle_sec[d].max(0.045);
            self.scratch_burst_base[d] = self.scratch_target_rate[d] * (-(elapsed - already_held).max(0.0) / 0.035).exp();
            self.scratch_burst_dt[d] = elapsed.clamp(0.004, 0.050);
            self.scratch_burst_ticks[d] = ticks;
            self.scratch_last_tick_at[d] = Some(now);
        }
        let scaled = self.scratch_burst_ticks[d] as f64 * sensitivity.clamp(0.1, 4.0);
        let velocity = (scaled * self.scratch_dx_per_tick[d] / self.scratch_burst_dt[d]).clamp(-6.0, 6.0);
        self.scratch_target_rate[d] = self.scratch_burst_base[d] * 0.45 + velocity * 0.55;
        self.scratch_idle_sec[d] = 0.0;
        self.scratch_tick_serial[d] = self.scratch_tick_serial[d].wrapping_add(1);
    }

    pub fn jog_nudge(&mut self, d: usize, delta_ticks: i64, sensitivity: f64) {
        let delta = delta_ticks as f64 / 1024.0 * sensitivity;
        if self.is_playing[d] {
            let next = (self.jog_nudge_rate[d] + delta).clamp(-0.18, 0.18);
            self.jog_nudge_delta[d] += next - self.jog_nudge_rate[d];
            self.jog_nudge_rate[d] = next;
        } else {
            self.seek_by(d, delta * 0.08);
        }
    }

    pub fn fast_scrub(&mut self, d: usize, delta_ticks: i64, sensitivity: f64) {
        self.seek_by(d, delta_ticks as f64 / 960.0 * 6.0 * sensitivity);
        if self.is_playing[d] {
            self.ease_jump(d);
        }
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
    /// The routing the open stream plays: what was asked for, or less on a system-default output.
    pub output_route: OutputRoute,
    /// The open stream (or the one being tried) is a system default, not the chosen output.
    on_system_default: bool,
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
    /// Test builds: how many times the output was (re)opened.
    #[cfg(test)]
    pub opens: usize,
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
                .filter(|&n| n > 0)
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
    /// A PipeWire sink by node name, with this many channels (None: as the node offers, up to what the pairs need).
    NativeNode { node: String, channels: Option<u16> },
    /// A raw ALSA device, by its cpal ID.
    Alsa { id: String },
    /// ALSA's default device (on a desktop, usually the sound server's ALSA plugin).
    AlsaDefault,
}

impl Route {
    fn describe(&self) -> &'static str {
        match self {
            Route::NativeDefault | Route::NativeNode { .. } => "PipeWire",
            Route::Alsa { .. } => "ALSA",
            Route::AlsaDefault => "ALSA default device",
        }
    }
}

/// Try the named PipeWire sink or raw ALSA device first, then the system default.
/// Only native PipeWire can target a graph sink directly through CPAL.
fn routes(device_id: &str, listed: Option<u16>, sink_absent: bool, native: bool, want: u16) -> Vec<Route> {
    let width = |channels: u16| want_width(want, channels);
    let mut list = Vec::new();
    if let Some(node) = device_id.strip_prefix("pw:").filter(|_| !sink_absent) {
        if native {
            list.push(Route::NativeNode { node: node.to_string(), channels: listed.map(width) });
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

/// Channels to ask of an output that has `channels`: enough for the `want`ed pairs if it has them, as many
/// as it has otherwise (odd counts and mono too, so a mono speaker never gets a split cue downmixed into
/// it); stereo when it doesn't say (0).
fn want_width(want: u16, channels: u16) -> u16 {
    if channels == 0 { 2 } else { want.min(channels) }
}

/// Select a writable ALSA configuration: the requested width and rate, a fixed wider layout, or stereo.
fn pick_config(ranges: &[cpal::SupportedStreamConfigRange], want: u16, most: u16, rate: u32) -> Option<&cpal::SupportedStreamConfigRange> {
    let usable = || ranges.iter().filter(|r| matches!(r.sample_format(), cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16 | cpal::SampleFormat::I32) && r.channels() <= most);
    let wider = || usable().filter(|r| r.channels() > want);
    usable()
        .filter(|r| r.channels() == want)
        .find(|r| r.contains_rate(rate))
        .or_else(|| usable().find(|r| r.channels() == want))
        .or_else(|| wider().filter(|r| r.contains_rate(rate)).min_by_key(|r| r.channels()))
        .or_else(|| wider().min_by_key(|r| r.channels()))
        .or_else(|| usable().find(|r| r.channels() == 2 && r.contains_rate(rate)))
}

/// ALSA itself, on the platforms where cpal has it; elsewhere the default host, as before.
fn alsa_host() -> Result<cpal::Host, String> {
    #[cfg(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd"))]
    return cpal::host_from_id(cpal::HostId::Alsa).map_err(|e| e.to_string());
    #[cfg(not(any(target_os = "linux", target_os = "dragonfly", target_os = "freebsd", target_os = "netbsd")))]
    Ok(cpal::default_host())
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
            output_route: OutputRoute::default(),
            on_system_default: false,
            running,
            hw_active,
            stats: Arc::default(),
            recorder: Arc::default(),
            logged: (0, 0, std::time::Instant::now()),
            #[cfg(test)]
            test_renderer: Mutex::new(None),
            #[cfg(test)]
            opens: 0,
        }
    }

    /// (Re)opens the output. `device_id` comes from [`list_outputs`] ("" = system default) or is a raw ALSA ID
    /// saved earlier. A named output is asked for enough channels to reach `route`'s pairs.
    /// Tries [`routes`] in order; the status line names the route that opened and any that didn't.
    #[cfg(not(test))]
    pub fn open(&mut self, device_id: &str, sample_rate: u32, buffer: u32, route: OutputRoute) {
        self._stream = None;
        self.hw_active.store(false, Ordering::Relaxed);
        self.output_info = None;
        self.output_channels = 0;
        self.stats = Arc::default();
        self.logged = (0, 0, std::time::Instant::now());
        let want = route.width();

        let choices = if device_id.is_empty() { Vec::new() } else { list_outputs() };
        let choice = choices.iter().find(|c| c.id == device_id);
        #[cfg(target_os = "linux")]
        let pipewire = cpal::host_from_id(cpal::HostId::PipeWire).ok();
        #[cfg(not(target_os = "linux"))]
        let pipewire: Option<cpal::Host> = None;
        let mut failed = Vec::new();
        // Prefer the sound server's listing for sink width and use CPAL's graph when no listing is available.
        let sink_absent = device_id.strip_prefix("pw:").is_some_and(|node| {
            if choices.iter().any(|c| c.id.starts_with("pw:")) {
                choice.is_none()
            } else {
                pipewire.as_ref().is_some_and(|host| {
                    host.output_devices().is_ok_and(|mut devices| !devices.any(|d| d.id().is_ok_and(|id| id.id() == node)))
                })
            }
        });
        if let Some(node) = device_id.strip_prefix("pw:") {
            if pipewire.is_none() {
                failed.push(format!("native PipeWire unavailable for {node}"));
            } else if sink_absent {
                failed.push(format!("{node} is not connected"));
            }
        }
        for way in routes(device_id, choice.map(|c| c.channels), sink_absent, pipewire.is_some(), want) {
            // The old stream is gone and the new one not built yet: its first block plays this routing.
            self.on_system_default = matches!(way, Route::NativeDefault | Route::AlsaDefault);
            self.reroute(route);
            match self.open_route(&way, pipewire.as_ref(), choice, sample_rate, buffer, want) {
                Ok(info) => {
                    let fell_back = if failed.is_empty() { String::new() } else { format!(" · fallback after {}", failed.join("; ")) };
                    self.output_info = Some(info + &fell_back);
                    return;
                }
                Err(why) => failed.push(format!("{} failed ({why})", way.describe())),
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
        want: u16,
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
                let channels = channels.unwrap_or_else(|| want_width(want, widest()));
                (found, channels, chosen_label)
            }
            Route::Alsa { id } => {
                let device = id.parse().ok().and_then(|id| alsa().ok()?.device_by_id(&id));
                let widest = device.as_ref().and_then(|d| d.supported_output_configs().ok()?.map(|c| c.channels()).max()).unwrap_or(2);
                let label = choice.map_or_else(|| id.clone(), |c| c.label.clone());
                (device, want_width(want, widest), label)
            }
            Route::AlsaDefault => (alsa()?.default_output_device(), 2, "System default output".to_string()),
        };
        let device = device.ok_or("no such device")?;
        let default = device.default_output_config().map_err(|e| e.to_string())?;
        let ranges: Vec<_> = device.supported_output_configs().map(|r| r.collect()).unwrap_or_default();
        let is_native = matches!(route, Route::NativeDefault | Route::NativeNode { .. });

        let (format, channels, sr, buffer_range) = if is_native {
            // PipeWire converts any width and position for the sink: ask for what the mix needs, in f32,
            // never wider than a mono system-default sink so split cue / preview cannot downmix to mono speakers.
            let ch = if matches!(route, Route::NativeDefault) { want_width(want_ch, default.channels()) } else { want_ch };
            let sr = if ranges.iter().any(|r| r.contains_rate(sample_rate)) { sample_rate } else { default.sample_rate() };
            (cpal::SampleFormat::F32, ch as usize, sr, *default.buffer_size())
        } else {
            let most = u16::MAX;
            let config = match pick_config(&ranges, want_ch, most, sample_rate) {
                Some(r) => {
                    let rate = if r.contains_rate(sample_rate) { sample_rate } else { default.sample_rate().clamp(r.min_sample_rate(), r.max_sample_rate()) };
                    r.clone().with_sample_rate(rate)
                }
                None if default.channels() <= most => default,
                None => return Err(format!("no layout of {most} channels or fewer")),
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
                cpal::SampleFormat::I16 => device.build_output_stream(
                    stream_config,
                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| renderer.render(data, channels, sr, pcm_i16),
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::U16 => device.build_output_stream(
                    stream_config,
                    move |data: &mut [u16], _: &cpal::OutputCallbackInfo| renderer.render(data, channels, sr, cpal::Sample::from_sample),
                    err_fn,
                    None,
                ),
                cpal::SampleFormat::I32 => device.build_output_stream(
                    stream_config,
                    move |data: &mut [i32], _: &cpal::OutputCallbackInfo| renderer.render(data, channels, sr, cpal::Sample::from_sample),
                    err_fn,
                    None,
                ),
                _ => Err(cpal::ErrorKind::UnsupportedConfig.into()),
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
        let narrower = if channels < want_ch as usize { format!(" (of {want_ch} asked for)") } else { String::new() };
        Ok(format!("{label} · {} · {sr} Hz · {channels} ch{narrower} · {buffer_text}", route.describe()))
    }

    #[cfg(test)]
    pub fn open(&mut self, _device_id: &str, _sample_rate: u32, _buffer: u32, route: OutputRoute) {
        *self.test_renderer.lock().unwrap() = None;
        self.opens += 1;
        self.on_system_default = false;
        self.reroute(route);
    }

    /// Whether the open stream (or the last one tried) is a system default rather than a named output.
    pub fn on_system_default(&self) -> bool {
        self.on_system_default
    }

    /// Plays `route` from the stream's next block, both pairs switching together, or, from `open`, from the
    /// new stream's first block. A system-default output keeps to [`OutputRoute::on_system_default`].
    pub fn reroute(&mut self, route: OutputRoute) {
        let route = if self.on_system_default { route.on_system_default() } else { route };
        if route != self.output_route {
            self.output_route = route;
            self.shared.lock().unwrap_or_else(PoisonError::into_inner).output_route = route;
        }
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
    state.jog_nudge_delta = [0.0; 4];
    jog_physics(state, dt);
    for d in 0..4 {
        let scratching = state.scratch_active[d] || state.scratch_ramping[d];
        if state.is_playing[d] || scratching {
            let ratio = if scratching { state.scratch_rate[d] } else { state.pitch_ratio[d] + state.jog_nudge_rate[d] };
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
    auto_gain: f32,
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
            auto_gain: t.auto_gain,
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

/// The callback's library preview: its own cursor through audio it only borrows, at the source's rate.
#[derive(Default)]
struct PreviewPlayer {
    pcm: Option<Arc<PreviewPcm>>,
    /// Where to start (a fraction of the track) until the track's length is known and `frame` can be set.
    start: Option<f64>,
    /// A start placed by the header's estimated length, and the frame it gave, until the preview moves on.
    estimated: Option<(f64, f64)>,
    frame: f64,
    serial: u64,
    ended: bool,
}

impl PreviewPlayer {
    /// The next output frame at device rate `sr`. Silent, without moving on, while the audio it needs isn't
    /// decoded yet; silent for good once it has ended.
    fn next(&mut self, sr: u32) -> (f32, f32) {
        let Some(pcm) = self.pcm.as_ref().filter(|_| !self.ended) else { return (0.0, 0.0) };
        if let Some(at) = self.start {
            let Some(frame) = pcm.frame_at(at) else { return (0.0, 0.0) };
            self.frame = frame;
            self.start = None;
            self.estimated = (!pcm.end_exact()).then_some((at, frame));
        }
        let Some(rate) = pcm.layout().map(|l| l.sample_rate) else { return (0.0, 0.0) };
        match pcm.sample(self.frame) {
            PreviewSample::Ready(out) => {
                self.frame += rate as f64 / (sr as f64).max(1.0);
                self.estimated = None;
                out
            }
            PreviewSample::Waiting => (0.0, 0.0),
            // Placed past the real end by an overestimated length, before playing anything: place it again in
            // the real length, now known.
            PreviewSample::Ended if self.estimated.is_some_and(|(_, f)| f == self.frame) => {
                self.start = self.estimated.take().map(|(at, _)| at);
                (0.0, 0.0)
            }
            PreviewSample::Ended => {
                self.ended = true;
                (0.0, 0.0)
            }
        }
    }

    fn progress(&self) -> PreviewProgress {
        PreviewProgress { serial: self.serial, frame: self.frame, started: self.start.is_none(), ended: self.ended }
    }
}

/// A library track decoded for the headphone preview and the browser overview, about a second at a time, on the
/// preview worker ([`PreviewLoader`]): from where the listener wants to be first, then the rest. Each second is
/// written once and never moves or changes, so the audio callback reads what has landed without a lock.
pub struct PreviewPcm {
    pub path: std::path::PathBuf,
    /// The file as it was when this preview was made (modified time, byte size), to tell a replaced file. A
    /// peaks-only decode takes it on the worker, when it opens the file, so a background row never makes the GUI wait on a disk.
    identity: std::sync::OnceLock<Option<(std::time::SystemTime, u64)>>,
    layout: std::sync::OnceLock<PreviewLayout>,
    failed: std::sync::OnceLock<String>,
    /// Title and artist from the file's tags, read when it was opened.
    tags: std::sync::OnceLock<(Option<String>, Option<String>)>,
    /// Frames in the track once decoding has reached its real end (or an upper bound on it); `u64::MAX` until then.
    end: AtomicU64,
    /// True once `end` is the exact track end (or cap), not an intermediate upper bound from past-EOF seeking.
    end_exact: AtomicBool,
    /// Where the listener wants to be, as a fraction of the track (`f64` bits): decoded first.
    wanted: AtomicU64,
    filled: AtomicUsize,
    /// Decoded in one uninterrupted run from the start, every packet where the previous one ended, so it is exactly
    /// what [`LoadedTrackAudio::from_file`] would decode. Cleared by a seek, a gap or overlap, or the cap.
    exact: AtomicBool,
    /// Keeps each second's overview peaks and drops its audio ([`PreviewPcm::peaks_only`]).
    peaks_only: bool,
    /// The finished overview and length ([`PreviewPcm::cached`]).
    cached: std::sync::OnceLock<(crate::overview::Overview, f64)>,
}

/// A file's modified time and byte size, to tell when it has been replaced.
pub(crate) fn file_identity(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// A preview's shape, known once its file is open.
pub struct PreviewLayout {
    pub sample_rate: u32,
    /// Frames the file's header promises (within the cap), if it says; an estimate for some MP3s.
    header_frames: Option<u64>,
    chunk_frames: usize,
    chunks: Box<[std::sync::OnceLock<PreviewChunk>]>,
}

/// One decoded second: interleaved stereo and its overview peaks.
struct PreviewChunk {
    pcm: Box<[f32]>,
    peaks: Box<[[f32; 3]]>,
}

/// What the preview has at one spot.
enum PreviewSample {
    Ready((f32, f32)),
    /// Not decoded yet.
    Waiting,
    Ended,
}

impl PreviewPcm {
    /// A preview of `path`, to be decoded first from `at` (a fraction of the track).
    pub fn new(path: std::path::PathBuf, at: f64) -> Arc<Self> {
        Self::build(path, at, false)
    }

    /// Only the overview peaks of `path`, for a browser row's waveform: each second's audio is dropped as soon as
    /// its peaks are worked out, so a whole track's audio is never held. Silent: never to be played or loaded.
    pub fn peaks_only(path: std::path::PathBuf) -> Arc<Self> {
        Self::build(path, 0.0, true)
    }

    fn build(path: std::path::PathBuf, at: f64, peaks_only: bool) -> Arc<Self> {
        let pcm = Self {
            identity: if peaks_only { Default::default() } else { file_identity(&path).into() },
            path,
            layout: Default::default(),
            failed: Default::default(),
            tags: Default::default(),
            end: AtomicU64::new(u64::MAX),
            end_exact: AtomicBool::new(false),
            wanted: AtomicU64::new(0),
            filled: AtomicUsize::new(0),
            exact: AtomicBool::new(!peaks_only),
            peaks_only,
            cached: Default::default(),
        };
        pcm.want(at);
        Arc::new(pcm)
    }

    pub fn layout(&self) -> Option<&PreviewLayout> {
        self.layout.get()
    }

    /// Why the file can't be previewed, once the worker found out.
    pub fn failed(&self) -> Option<&str> {
        self.failed.get().map(String::as_str)
    }

    /// The file at `path` is still the one this preview was made from (same modified time and size).
    pub fn same_file(&self) -> bool {
        self.identity().is_some() && self.identity() == file_identity(&self.path)
    }

    /// The file as it was when this preview was made (modified time, byte size), once known.
    pub fn identity(&self) -> Option<(std::time::SystemTime, u64)> {
        self.identity.get().copied().flatten()
    }

    /// Title and artist from the file's own tags, once it has been opened.
    pub fn tags(&self) -> Option<&(Option<String>, Option<String>)> {
        self.tags.get()
    }

    /// Decoded straight through from the start, exactly as a deck's decode would be (see `exact`).
    pub fn exact(&self) -> bool {
        self.exact.load(Ordering::Acquire)
    }

    /// Decode from `at` (a fraction of the track) first; tell the worker with [`PreviewLoader::nudge`].
    pub fn want(&self, at: f64) {
        self.wanted.store(at.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn end(&self) -> Option<u64> {
        Some(self.end.load(Ordering::Acquire)).filter(|&e| e != u64::MAX)
    }

    fn end_exact(&self) -> bool {
        self.end_exact.load(Ordering::Acquire)
    }

    fn set_exact_end(&self, frames: u64) {
        self.end.store(frames, Ordering::Release);
        self.end_exact.store(true, Ordering::Release);
    }

    /// Frames a listener can play through: exact once decoding reached the end, else what the header says.
    pub fn playable_frames(&self) -> Option<u64> {
        self.end().or_else(|| self.layout()?.header_frames)
    }

    pub fn duration_sec(&self) -> Option<f64> {
        Some(self.playable_frames()? as f64 / self.layout()?.sample_rate.max(1) as f64)
    }

    /// The frame at `at` (a fraction of the track), once the track's length is known (the start needs none);
    /// never past its last frame.
    pub fn frame_at(&self, at: f64) -> Option<f64> {
        if at <= 0.0 {
            return Some(0.0);
        }
        let frames = self.playable_frames()?;
        Some((at.clamp(0.0, 1.0) * frames as f64).min(frames.saturating_sub(1) as f64))
    }

    /// Seconds decoded so far; changes whenever another lands.
    pub fn filled(&self) -> usize {
        self.filled.load(Ordering::Acquire)
    }

    /// Chunks up to the end, once it is known.
    fn chunks_to_end(&self, layout: &PreviewLayout) -> Option<usize> {
        Some((self.end()?.div_ceil(layout.chunk_frames as u64) as usize).min(layout.chunks.len()))
    }

    /// Everything up to the end has been decoded.
    pub fn complete(&self) -> bool {
        let Some(layout) = self.layout() else { return false };
        self.chunks_to_end(layout).is_some_and(|n| layout.chunks[..n].iter().all(|c| c.get().is_some()))
    }

    /// The worker has nothing left to decode for it: a preview's audio is decoded whatever the cache holds.
    fn done(&self) -> bool {
        self.failed().is_some() || self.complete() || self.peaks_only && self.cached().is_some()
    }

    /// The track's finished overview and length, as the rows draw it: its stripe found in the cache when the file was
    /// opened (a peaks-only job then decodes nothing), or else the one kept once decoding completed. Never audio.
    pub fn cached(&self) -> Option<&(crate::overview::Overview, f64)> {
        self.cached.get()
    }

    /// One stereo frame, if decoded.
    fn frame(&self, layout: &PreviewLayout, i: u64) -> Option<(f32, f32)> {
        let chunk = layout.chunks.get(i as usize / layout.chunk_frames)?.get()?;
        let at = i as usize % layout.chunk_frames * 2;
        Some((*chunk.pcm.get(at)?, *chunk.pcm.get(at + 1)?))
    }

    /// The audio at `pos` (frames), read exactly as [`sample_stereo_hermite`] reads a whole buffer.
    fn sample(&self, pos: f64) -> PreviewSample {
        let Some(layout) = self.layout() else { return PreviewSample::Waiting };
        let end = self.end.load(Ordering::Acquire);
        let f0 = pos.max(0.0).floor() as u64;
        if f0 >= end {
            return PreviewSample::Ended;
        }
        if end < 2 {
            return PreviewSample::Ready((0.0, 0.0));
        }
        let frac = (pos.max(0.0) - f0 as f64) as f32;
        let get = |i: u64| self.frame(layout, i);
        let out = if f0 == 0 || f0 + 2 >= end {
            get(f0).zip(get((f0 + 1).min(end - 1))).map(|(a, b)| (a.0 * (1.0 - frac) + b.0 * frac, a.1 * (1.0 - frac) + b.1 * frac))
        } else {
            match (get(f0 - 1), get(f0), get(f0 + 1), get(f0 + 2)) {
                (Some(a), Some(b), Some(c), Some(d)) => Some((hermite4(a.0, b.0, c.0, d.0, frac), hermite4(a.1, b.1, c.1, d.1, frac))),
                _ => None,
            }
        };
        out.map_or(PreviewSample::Waiting, PreviewSample::Ready)
    }

    /// The overview's peaks over the whole track, zero (not drawn) where not decoded yet.
    pub fn overview_peaks(&self) -> Option<Vec<[f32; 3]>> {
        let (layout, frames) = (self.layout()?, self.playable_frames()?);
        let (cf, per_bucket) = (layout.chunk_frames as u64, (layout.sample_rate as usize / PEAKS_PER_SEC).max(64) as u64);
        let mut peaks = Vec::with_capacity((frames / per_bucket) as usize + 64);
        for i in 0..(frames.div_ceil(cf) as usize).min(layout.chunks.len()) {
            match layout.chunks[i].get() {
                Some(c) => peaks.extend_from_slice(&c.peaks),
                None => peaks.resize(peaks.len() + ((frames - i as u64 * cf).min(cf) / per_bucket).max(1) as usize, [0.0; 3]),
            }
        }
        Some(peaks)
    }

    /// The whole track as one interleaved stereo buffer, for a deck load instead of decoding again: only once it
    /// is complete, exact (decoded as a deck would) and its file hasn't been replaced since.
    pub fn to_vec(&self) -> Option<Vec<f32>> {
        // Assemble first so its Acquire loads happen-before checking `exact` (cleared before the last chunk lands).
        let pcm = self.assembled()?;
        if !(self.exact() && self.same_file()) {
            return None;
        }
        Some(pcm)
    }

    /// Every decoded chunk up to the end, in one buffer, once complete.
    fn assembled(&self) -> Option<Vec<f32>> {
        let layout = self.layout()?;
        let (n, end) = (self.chunks_to_end(layout)?, self.end()? as usize);
        let mut pcm = Vec::with_capacity(end * 2);
        for c in &layout.chunks[..n] {
            pcm.extend_from_slice(&c.get()?.pcm);
        }
        pcm.truncate(end * 2);
        Some(pcm).filter(|p| !p.is_empty())
    }
}

#[cfg(test)]
impl PreviewPcm {
    /// A preview of `pcm` (interleaved stereo at `rate`) laid out in chunks of `chunk_frames`, with the chunks
    /// `have` picks already decoded, its end known if `ended`, and the header's length if `header`.
    pub(crate) fn decoded(pcm: &[f32], rate: u32, chunk_frames: usize, have: impl Fn(usize) -> bool, ended: bool, header: bool) -> Arc<Self> {
        let preview = Self::new("/test/preview".into(), 0.0);
        let frames = pcm.len() / 2;
        let n = frames.div_ceil(chunk_frames).max(1);
        let header_frames = header.then_some(frames as u64);
        let _ = preview.layout.set(PreviewLayout { sample_rate: rate, header_frames, chunk_frames, chunks: (0..n).map(|_| Default::default()).collect() });
        for i in (0..n).filter(|&i| have(i)) {
            preview.land(pcm, i);
        }
        if ended {
            preview.end.store(frames as u64, Ordering::Release);
        }
        preview
    }

    /// Lands chunk `i` of `pcm`, as the worker does once it is decoded.
    pub(crate) fn land(&self, pcm: &[f32], i: usize) {
        let layout = self.layout().unwrap();
        let cf = layout.chunk_frames;
        let chunk: Box<[f32]> = pcm[i * cf * 2..((i + 1) * cf * 2).min(pcm.len())].into();
        let peaks = LoadedTrackAudio::compute_waveform_peaks(&chunk, layout.sample_rate).0.into_boxed_slice();
        if layout.chunks[i].set(PreviewChunk { pcm: chunk, peaks }).is_ok() {
            self.filled.fetch_add(1, Ordering::AcqRel);
        }
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
    deck_fx: [DeckFxUnit; 2],
    pattern_noise: u32,
    preview: PreviewPlayer,
    generation: u64,
}

impl Renderer {
    /// Built on a control thread (it waits for the lock): starts from the published playheads and the current
    /// controls and tracks, so a new or reopened stream carries on where the last one left off.
    pub(crate) fn new(shared: Arc<Mutex<AudioSharedState>>, stats: Arc<AudioStats>, recorder: Arc<Mutex<Option<RecorderTap>>>) -> Self {
        let mut renderer = Self {
            shared,
            stats,
            recorder,
            state: AudioSharedState::default(),
            tracks: Default::default(),
            deck_fx: Default::default(),
            pattern_noise: 0x9e37_79b9,
            preview: PreviewPlayer::default(),
            generation: 0,
        };
        let shared = Arc::clone(&renderer.shared);
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        s.renderer_generation += 1;
        renderer.generation = s.renderer_generation;
        // `playhead_sec` already includes any seek still pending.
        s.pending_seek = [None; 4];
        renderer.state.playhead_sec = s.playhead_sec;
        renderer.state.pattern_phase_sec = s.pattern_phase_sec;
        renderer.state.scratch_active = s.scratch_active;
        renderer.state.scratch_rate = s.scratch_rate;
        renderer.state.scratch_target_rate = s.scratch_target_rate;
        renderer.state.scratch_tick_serial = s.scratch_tick_serial;
        renderer.state.scratch_ramping = s.scratch_ramping;
        renderer.state.scratch_ramp_target = s.scratch_ramp_target;
        renderer.state.jog_nudge_rate = s.jog_nudge_rate;
        s.jog_nudge_delta = [0.0; 4];
        renderer.take_from(&mut s);
        // The preview carries on from where the last stream left it; one that already ended stays silent.
        let p = s.preview_progress;
        if renderer.preview.pcm.is_some() && p.serial == renderer.preview.serial {
            if p.started {
                renderer.preview.frame = p.frame;
                renderer.preview.start = None;
            }
            renderer.preview.ended = p.ended;
        }
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
        let recorder = Arc::clone(&self.recorder);
        let mut slot = recorder.try_lock().ok();
        let mut tap = slot.as_deref_mut().and_then(Option::as_mut);
        if let Some(tap) = tap.as_deref_mut() {
            tap.begin_block(sr);
        }
        jog_physics(&mut self.state, (output.len() / channels) as f64 / (sr as f64).max(1.0));
        phase_lock(&mut self.state);
        self.mix(tap.as_deref_mut(), output, channels, sr, to_sample);
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
        r.sync_follow = s.sync_follow;
        for d in 0..4 {
            if std::mem::take(&mut s.sync_reset[d]) {
                r.sync_adjust[d] = 1.0;
                r.sync_user_offset[d] = 0.0;
            }
        }
        r.loop_active = s.loop_active;
        r.loop_start_sec = s.loop_start_sec;
        r.loop_len_sec = s.loop_len_sec;
        r.channel_gain = s.channel_gain;
        r.cue_mix = s.cue_mix;
        r.cue_volume = s.cue_volume;
        r.output_route = s.output_route;
        r.keylock = s.keylock;
        r.keylock_hq = s.keylock_hq;
        r.crossfader = s.crossfader;
        r.crossfader_curve = s.crossfader_curve;
        r.deck_fx_assign = s.deck_fx_assign;
        r.engine.maximizer.enabled = s.limiter_enabled;
        for (u, fx) in self.deck_fx.iter_mut().enumerate() {
            fx.enabled = s.fx_enabled[u];
            fx.kind = s.fx_kind[u];
            fx.dry_wet = s.fx_dry_wet[u];
            fx.params = s.fx_params[u];
        }
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

        // Transfer each new target once so a handoff can't restart the callback's idle decay.
        for d in 0..4 {
            let changed = std::mem::take(&mut s.scratch_changed[d]);
            if changed {
                r.scratch_active[d] = s.scratch_active[d];
                r.scratch_ramping[d] = s.scratch_ramping[d];
                r.scratch_ramp_target[d] = s.scratch_ramp_target[d];
                if s.scratch_active[d] {
                    r.scratch_rate[d] = s.scratch_rate[d];
                }
            }
            if changed || r.scratch_tick_serial[d] != s.scratch_tick_serial[d] {
                r.scratch_target_rate[d] = s.scratch_target_rate[d];
                r.scratch_tick_serial[d] = s.scratch_tick_serial[d];
                r.scratch_idle_sec[d] = s.scratch_idle_sec[d];
            }
            let delta = std::mem::take(&mut s.jog_nudge_delta[d]);
            r.jog_nudge_rate[d] = (r.jog_nudge_rate[d] + delta).clamp(-0.18, 0.18);
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
                    s.retired.extend(old.buffers().into_iter().flatten().map(|b| Arc::clone(b) as Retired));
                }
                self.tracks[d] = s.loaded_tracks[d].as_ref().map(DeckPcm::of);
                // Re-prime key lock on track swaps and late stem arrivals so stale overlap-add buffers aren't heard.
                r.keylock_engaged[d] = false;
            } else if let (Some(mine), Some(theirs)) = (&mut self.tracks[d], &s.loaded_tracks[d]) {
                mine.duration_sec = theirs.duration_sec;
                mine.sample_rate = theirs.sample_rate;
                mine.auto_gain = theirs.auto_gain;
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

        if s.preview_serial != self.preview.serial {
            // A newer start or stop: what was heard goes silent now, even if its buffer must wait to be retired.
            self.preview.ended = true;
            // shortcut: with no room to retire the old preview, the new one waits (silent) for the UI to free some.
            if self.preview.pcm.is_none() || s.retired.len() < s.retired.capacity() {
                if let Some(old) = self.preview.pcm.take() {
                    s.retired.push(old);
                }
                self.preview = match &s.preview {
                    Some((pcm, at)) => PreviewPlayer { pcm: Some(Arc::clone(pcm)), start: Some(*at), ..Default::default() },
                    None => PreviewPlayer::default(),
                };
                self.preview.serial = s.preview_serial;
            }
        }
        // A start or stop clears any seek, so one waiting here belongs to the preview the UI has now.
        if s.preview_serial == self.preview.serial {
            if let Some(at) = s.preview_seek.take() {
                self.preview.start = Some(at);
                self.preview.ended = false;
            }
        }
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
        for d in 0..4 {
            if !s.scratch_changed[d] && r.scratch_tick_serial[d] == s.scratch_tick_serial[d] {
                s.scratch_target_rate[d] = r.scratch_target_rate[d];
                s.scratch_idle_sec[d] = r.scratch_idle_sec[d];
            }
        }
        s.jog_nudge_rate = r.jog_nudge_rate;
        s.preview_progress = self.preview.progress();
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
            s.preview_progress = self.preview.progress();
        }
        for t in self.tracks.iter_mut().filter_map(Option::take) {
            s.retired.extend(t.buffers().into_iter().flatten().map(|b| Arc::clone(b) as Retired));
        }
        if let Some(pcm) = self.preview.pcm.take() {
            s.retired.push(pcm);
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

/// A synced deck's phase lock: who it follows and both beatgrids (see [`AudioSharedState::sync_follow`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SyncLock {
    pub leader: usize,
    pub grid: (f64, f64),
    pub leader_grid: (f64, f64),
}

/// How far (in beats, within ±period/2) a deck at `beats` must move forward to line up with the leader
/// every `period` beats: 1 for the beat, 4 for the bar.
pub(crate) fn beat_phase_diff(leader_beats: f64, beats: f64, period: f64) -> f64 {
    let diff = (leader_beats - beats).rem_euclid(period);
    if diff > period / 2.0 { diff - period } else { diff }
}

/// Once per block, from the callback's own playheads: the speed factor that keeps each synced deck on its
/// leader's beat. Mixxx's control loop (`BpmControl::calcSyncAdjustment`): errors under 1% of a beat are left
/// alone; above that a proportional pull, changing at most 2% per block and never past ±5%. A hand on the jog
/// (scratch or a pitch bend) is the DJ choosing an offset: it is adopted, not fought.
fn phase_lock(r: &mut AudioSharedState) {
    const DEADBAND: f64 = 0.01;
    const GAIN: f64 = 0.7;
    const STEP_CAP: f64 = 0.02;
    const CAP: f64 = 0.05;
    for d in 0..4 {
        let Some(lock) = r.sync_follow[d].filter(|l| r.is_playing[d] && r.is_playing[l.leader]) else {
            r.sync_adjust[d] = 1.0;
            continue;
        };
        let beats = |playhead: f64, (bpm, first): (f64, f64)| (playhead - first) * bpm / 60.0;
        let raw = beat_phase_diff(beats(r.playhead_sec[lock.leader], lock.leader_grid), beats(r.playhead_sec[d], lock.grid), 1.0);
        let touched = |i: usize| r.scratch_active[i] || r.scratch_ramping[i] || r.jog_nudge_rate[i] != 0.0;
        if touched(d) || touched(lock.leader) {
            r.sync_user_offset[d] = raw;
            r.sync_adjust[d] = 1.0;
            continue;
        }
        let error = beat_phase_diff(raw - r.sync_user_offset[d], 0.0, 1.0);
        let want = if error.abs() < DEADBAND { 0.0 } else { GAIN * error };
        let last = r.sync_adjust[d] - 1.0;
        r.sync_adjust[d] = 1.0 + (last + (want - last).clamp(-STEP_CAP, STEP_CAP)).clamp(-CAP, CAP);
    }
}

/// One block of jog physics in the callback's state: the scratch filter observes the ticks that arrived (none
/// is an observation too), and a released platter eases back to playing speed.
fn jog_physics(r: &mut AudioSharedState, block_dt: f64) {
    let scratch_slew = 1.0 - (-block_dt / 0.012).exp();
    let nudge_decay = (-block_dt / 0.09).exp();
    for d in 0..4 {
        r.jog_nudge_rate[d] *= nudge_decay;
        if !r.is_playing[d] || r.jog_nudge_rate[d].abs() < 1e-5 {
            r.jog_nudge_rate[d] = 0.0;
        }
        if r.scratch_active[d] {
            r.scratch_idle_sec[d] += block_dt;
            let decay_dt = (r.scratch_idle_sec[d] - 0.045).clamp(0.0, block_dt);
            r.scratch_target_rate[d] *= (-decay_dt / 0.035).exp();
            r.scratch_rate[d] += (r.scratch_target_rate[d] - r.scratch_rate[d]) * scratch_slew;
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
        &mut self,
        mut rec: Option<&mut RecorderTap>,
        output: &mut [T],
        channels: usize,
        sr: u32,
        to_sample: fn(f32) -> T,
    ) {
        let state = &mut self.state;
        let tracks = &self.tracks;
        let frames = output.len() / channels;
        let dt = 1.0 / (sr as f64).max(1.0);
        output.fill(to_sample(0.0));
        state.engine.routing.sample_rate = sr;
        // Once per block, against the real stream width: every bound below is checked here.
        let placement = state.output_route.place(channels);
        // With no headphone output the preview neither plays nor moves on; it never falls back to the main output.
        let preview_heard = placement.headphones_heard();

        for f in 0..frames {
            let mut deck_samples: [(f32, f32); 4] = [(0.0, 0.0); 4];

            for d in 0..4 {
                let scratching = state.scratch_active[d] || state.scratch_ramping[d];
                state.engine.strips[d].playing = state.is_playing[d] || scratching;
                if !state.engine.strips[d].playing {
                    state.jump_ease[d] = JumpEase::default(); // silent: no stale frame or request survives a stop
                    state.keylock_engaged[d] = false;
                    continue;
                }
                let mut advance_dt = 0.0_f64;
                let mut track_dur = 120.0_f64;
                let mut raw_l = 0.0_f32;
                let mut raw_r = 0.0_f32;
                let mut has_sample = false;
                let mut auto_gain = 1.0_f32;

                let effective_rate = if scratching {
                    state.scratch_rate[d]
                } else {
                    state.pitch_ratio[d] * state.sync_adjust[d] + state.jog_nudge_rate[d]
                };

                let mut true_stems = None;
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
                    auto_gain = track.auto_gain;
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
                    if state.scratch_active[d] || (state.scratch_ramping[d] && !state.is_playing[d]) {
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

                    // 2. Channel gain times the track's auto gain (the 3-band EQ runs in the mixer engine).
                    let gain = (state.channel_gain[d] * 2.0).clamp(0.0, 2.0) * auto_gain;
                    deck_samples[d] = (stem_l * gain, stem_r * gain);
                } else {
                    state.jump_ease[d] = JumpEase::default();
                }

                advance_playhead(state, d, advance_dt);

                if looping {
                    let loop_start = state.loop_start_sec[d];
                    let loop_len = state.loop_len_sec[d];
                    let loop_end = loop_start + loop_len;
                    if state.playhead_sec[d] >= loop_end || state.playhead_sec[d] < loop_start {
                        state.playhead_sec[d] = loop_start + (state.playhead_sec[d] - loop_start).rem_euclid(loop_len);
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
                    self.pattern_noise ^= self.pattern_noise << 13;
                    self.pattern_noise ^= self.pattern_noise >> 17;
                    self.pattern_noise ^= self.pattern_noise << 5;
                    let noise = (self.pattern_noise >> 8) as f32 / 8_388_608.0 - 1.0;
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
            let bpm = state.engine.clock.bpm as f32;
            let (mut ml, mut mr, cl, cr) = state.engine.process_frame_with_fx(deck_samples, |post_fader| {
                let (mut delta_l, mut delta_r) = (0.0, 0.0);
                let (mut both_l, mut both_r) = (0.0, 0.0);
                for (d, (l, r)) in post_fader.into_iter().enumerate() {
                    if state.deck_fx_assign[d][0] && state.deck_fx_assign[d][1] {
                        both_l += l;
                        both_r += r;
                    }
                }
                let mut mix = [0.0f32; 2];
                for (u, fx) in self.deck_fx.iter_mut().enumerate() {
                    let (mut in_l, mut in_r) = (0.0, 0.0);
                    for (d, (l, r)) in post_fader.into_iter().enumerate() {
                        if state.deck_fx_assign[d][u] {
                            in_l += l;
                            in_r += r;
                        }
                    }
                    let (out_l, out_r) = fx.process(in_l, in_r, sr as f32, bpm);
                    mix[u] = fx.active_mix();
                    delta_l += out_l - in_l;
                    delta_r += out_r - in_r;
                }
                // Decks assigned to both units had dry attenuated twice (-m0 - m1); restore m0*m1 so dry stays (1-m0)(1-m1) >= 0.
                let comp = mix[0] * mix[1];
                (delta_l + both_l * comp, delta_r + both_r * comp)
            });
            ml = (ml + pat_sample).clamp(-1.0, 1.0);
            mr = (mr + pat_sample).clamp(-1.0, 1.0);

            if let Some(tap) = rec.as_deref_mut() {
                tap.push(ml, mr);
            }

            // The library preview joins the cue bus alone: after the main mix, its meters and the recorder.
            let (cl, cr) = if preview_heard {
                let (pl, pr) = self.preview.next(sr);
                (cl + pl, cr + pr)
            } else {
                (cl, cr)
            };

            let out = &mut output[f * channels..(f + 1) * channels];
            let (hl, hr) = headphone_mix((cl, cr), (ml, mr), state.cue_mix, state.cue_volume);
            match placement {
                Placement::Mono => out[0] = to_sample((ml + mr) * 0.5),
                Placement::Split => {
                    out[0] = to_sample((hl + hr) * 0.5);
                    out[1] = to_sample((ml + mr) * 0.5);
                }
                Placement::Pairs { main, phones } => {
                    out[main..main + 2].copy_from_slice(&[ml, mr].map(to_sample));
                    if let Some(h) = phones {
                        out[h..h + 2].copy_from_slice(&[hl, hr].map(to_sample));
                    }
                }
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

/// What the preview worker decodes: the preview playing now first, then the selected track for its overview, then
/// one visible browser row for its waveform.
#[derive(Default)]
struct PreviewJobs {
    play: Option<Arc<PreviewPcm>>,
    browse: Option<Arc<PreviewPcm>>,
    row: Option<Arc<PreviewPcm>>,
    closed: bool,
}

/// Decodes library previews (PCM and overview peaks only: no analysis, no stems) on one worker thread, a second
/// at a time, so a preview plays as soon as its first second lands. A job that is replaced is dropped after at
/// most one more step, so rapid browsing or clicking never queues decodes; at most three files are open. A row's
/// waveform kept in the stripe cache is read from there instead of decoding (a preview shows it while its audio
/// is decoded), and every decode that reaches the end is kept there, after its last second has landed.
pub struct PreviewLoader {
    jobs: Arc<(Mutex<PreviewJobs>, std::sync::Condvar)>,
}

impl PreviewLoader {
    /// Starts the worker, using `stripes`; it ends when the loader is dropped.
    pub fn new(stripes: crate::overview::StripeCache) -> Self {
        let jobs: Arc<(Mutex<PreviewJobs>, std::sync::Condvar)> = Arc::default();
        let worker = Arc::clone(&jobs);
        std::thread::spawn(move || preview_worker(&worker, &stripes));
        Self { jobs }
    }

    /// Decodes `play` first, then `browse`, then `row`; anything else it was decoding is dropped.
    pub fn set(&self, play: Option<&Arc<PreviewPcm>>, browse: Option<&Arc<PreviewPcm>>, row: Option<&Arc<PreviewPcm>>) {
        let mut jobs = self.jobs.0.lock().unwrap_or_else(PoisonError::into_inner);
        jobs.play = play.cloned();
        jobs.browse = browse.cloned();
        jobs.row = row.cloned();
        self.jobs.1.notify_one();
    }

    /// A job wants a new spot ([`PreviewPcm::want`]): look again even if it seemed finished.
    pub fn nudge(&self) {
        self.jobs.1.notify_one();
    }
}

impl Drop for PreviewLoader {
    fn drop(&mut self) {
        self.jobs.0.lock().unwrap_or_else(PoisonError::into_inner).closed = true;
        self.jobs.1.notify_one();
    }
}

/// One preview the worker has open: its file and the decoded audio not yet cut into a chunk.
struct PreviewDecode {
    pcm: Arc<PreviewPcm>,
    file: AudioFile,
    /// Decoded frames contiguous from `pending_from`, waiting to fill a chunk.
    pending: Vec<f32>,
    pending_from: Option<u64>,
    scratch: Vec<f32>,
    /// The container can seek; when it can't, the worker reads on (or reopens the file) to get somewhere.
    seekable: bool,
    /// Where the decoder was last put (0 when just opened); `None` once it has read to the end.
    placed_at: Option<u64>,
    /// The stream had no audio from here up to `pending_from` (it started or resumed later): silence there.
    silent_from: Option<u64>,
}

fn preview_worker(jobs: &(Mutex<PreviewJobs>, std::sync::Condvar), stripes: &crate::overview::StripeCache) {
    let mut open: Vec<PreviewDecode> = Vec::new();
    loop {
        let next = {
            let mut j = jobs.0.lock().unwrap_or_else(PoisonError::into_inner);
            loop {
                if j.closed {
                    return;
                }
                let wanted = [j.play.clone(), j.browse.clone(), j.row.clone()];
                open.retain(|d| wanted.iter().flatten().any(|p| Arc::ptr_eq(p, &d.pcm)));
                if let Some(p) = wanted.into_iter().flatten().find(|p| !p.done()) {
                    break p;
                }
                j = jobs.1.wait(j).unwrap_or_else(PoisonError::into_inner);
            }
        };
        let i = match open.iter().position(|d| Arc::ptr_eq(&d.pcm, &next)) {
            Some(i) => i,
            None => match PreviewDecode::open(next, stripes) {
                Some(d) => {
                    open.push(d);
                    open.len() - 1
                }
                None => continue,
            },
        };
        open[i].step();
        let pcm = &open[i].pcm;
        if pcm.complete() && pcm.cached().is_none() {
            let (peaks, duration_sec) = (pcm.overview_peaks().unwrap_or_default(), pcm.duration_sec().unwrap_or(0.0));
            // What the cache keeps (another decode's, if it got there first), so the rows and decks agree.
            let overview = stripes.store(&pcm.path, pcm.identity(), &peaks, duration_sec).unwrap_or_else(|| crate::overview::from_peaks(&peaks));
            let _ = pcm.cached.set((overview, duration_sec));
        }
    }
}

impl PreviewDecode {
    /// Opens the file and lays the preview out; on failure the preview says why and `None` comes back. An overview
    /// kept in `stripes` for the file as it is now is published at once; a peaks-only job then needs no decode
    /// (`None` comes back), while a preview's audio is still decoded.
    fn open(pcm: Arc<PreviewPcm>, stripes: &crate::overview::StripeCache) -> Option<Self> {
        let identity = *pcm.identity.get_or_init(|| file_identity(&pcm.path));
        if let Some(kept) = stripes.load(&pcm.path, identity) {
            let _ = pcm.cached.set(kept);
            if pcm.peaks_only {
                return None;
            }
        }
        let mut file = match AudioFile::open(&pcm.path) {
            Ok(f) => f,
            Err(e) => {
                let _ = pcm.failed.set(e);
                return None;
            }
        };
        let _ = pcm.tags.set(file.tags());
        let max = file.max_frames();
        let chunk_frames = file.sample_rate.max(1) as usize;
        // Slots for the whole cap, whatever the header says: its length may be an estimate, and only the real
        // end (or the cap) ends the track.
        let _ = pcm.layout.set(PreviewLayout {
            sample_rate: file.sample_rate,
            header_frames: file.header_frames.map(|n| n.min(max)),
            chunk_frames,
            chunks: (0..max.div_ceil(chunk_frames as u64).max(1)).map(|_| Default::default()).collect(),
        });
        Some(Self { pcm, file, pending: Vec::new(), pending_from: None, scratch: Vec::new(), seekable: true, placed_at: Some(0), silent_from: None })
    }

    /// Decodes towards the first missing chunk at or after the wanted spot (wrapping round to the start), at
    /// most a few seconds of audio, then returns so the worker can look at its jobs again.
    fn step(&mut self) {
        let Some(layout) = self.pcm.layout() else { return };
        let (cf, slots) = (layout.chunk_frames as u64, layout.chunks.len());
        let last = self.pcm.chunks_to_end(layout).unwrap_or(slots);
        // From the frame before the one wanted: interpolation reads it too, and a spot right on a chunk's start
        // would otherwise wait for the whole rest of the file and the wrap back round. A spot past the real end
        // (the header overestimated) starts at the last chunk there is, so the real end is found from there.
        let wanted = self.pcm.frame_at(f64::from_bits(self.pcm.wanted.load(Ordering::Relaxed))).unwrap_or(0.0) as u64;
        let from = ((wanted.saturating_sub(1) / cf) as usize).min(last.saturating_sub(1));
        let Some(target) = (from..last).chain(0..from).find(|&c| layout.chunks[c].get().is_none()) else {
            // Every slot is full and the end was never read: the decode cap. (A deck's decode stops a packet
            // later, so this is no longer what it would play.)
            if self.pcm.end().is_none() {
                self.pcm.exact.store(false, Ordering::Release);
                self.pcm.set_exact_end(slots as u64 * cf);
            }
            return;
        };
        let start = target as u64 * cf;
        let have = (self.pending.len() / 2) as u64;
        let in_hole = self.silent_from.is_some_and(|s| s <= start);
        let reposition = match (self.pending_from, self.placed_at) {
            (Some(from), placed) => (start < from && !in_hole) || (self.seekable && start > from + have + cf && placed != Some(start)),
            (None, Some(placed)) => start < placed || (self.seekable && start > placed + cf),
            (None, None) => true,
        };
        if reposition {
            self.pending.clear();
            self.pending_from = None;
            self.silent_from = None;
            // A little early, so the decoder (an MP3's bit reservoir) is settled by the frame wanted.
            let preroll = layout.sample_rate as u64 / 10;
            let sought = start > 0 && self.seekable && self.file.seek(start.saturating_sub(preroll)).is_ok();
            self.seekable &= sought || start == 0;
            if sought {
                self.placed_at = Some(start);
                self.pcm.exact.store(false, Ordering::Release); // no longer one straight run from the start
            } else {
                // From the top, reading on to the chunk wanted.
                match AudioFile::open(&self.pcm.path) {
                    Ok(f) => {
                        self.file = f;
                        self.placed_at = Some(0);
                    }
                    Err(e) => {
                        let _ = self.pcm.failed.set(e);
                        return;
                    }
                }
            }
        }
        let max = self.file.max_frames().min(slots as u64 * cf);
        let mut decoded = 0u64;
        loop {
            // Frames before the chunk are only lead-in: drop them.
            if let Some(from) = self.pending_from.filter(|&f| f < start) {
                let drop = ((start - from) as usize).min(self.pending.len() / 2);
                self.pending.drain(..drop * 2);
                self.pending_from = Some(from + drop as u64);
            }
            // No audio between the chunk's start and the run (the stream starts or resumes later): silence,
            // a chunk at a time, never more than a chunk of it held.
            if let Some(from) = self.pending_from.filter(|&f| f > start && self.silent_from.is_some_and(|s| s <= start)) {
                if from - start >= cf {
                    self.emit_silence(target, cf as usize);
                    return;
                }
                self.pending.splice(0..0, std::iter::repeat(0.0).take(((from - start) * 2) as usize));
                self.pending_from = Some(start);
                self.silent_from = None;
            }
            let have = (self.pending.len() / 2) as u64;
            if self.pending_from == Some(start) && have >= cf {
                self.emit(target, cf as usize);
                return;
            }
            if decoded > 4 * cf {
                return; // more next step, after a look at the jobs
            }
            self.scratch.clear();
            let Some(at) = self.file.next_stereo(&mut self.scratch) else {
                self.placed_at = None;
                match self.pending_from {
                    // A run decoded up to here ends the track exactly.
                    Some(f) => self.pcm.set_exact_end((f + have).min(max)),
                    // Nothing after the place it was put: the track ends before this chunk. An upper bound only
                    // (except at frame 0, where the file is empty); the chunk before finds the exact end.
                    None if start == 0 => self.pcm.set_exact_end(0),
                    None => {
                        let _ = self.pcm.end.fetch_min(start, Ordering::AcqRel);
                    }
                }
                if self.pending_from == Some(start) && have > 0 {
                    self.emit(target, have as usize);
                }
                return;
            };
            let frames = (self.scratch.len() / 2) as u64;
            decoded += frames;
            let mut emitted_before_gap = false;
            match self.pending_from {
                None => {
                    if at > start {
                        self.silent_from = Some(start);
                        self.pcm.exact.store(false, Ordering::Release);
                    }
                    self.pending_from = Some(at);
                    self.pending.extend_from_slice(&self.scratch);
                }
                Some(f) => {
                    let expected = f + (self.pending.len() / 2) as u64;
                    if at != expected {
                        // A gap (silence keeps later frames near where they belong) or an overlap (dropped).
                        self.pcm.exact.store(false, Ordering::Release);
                    }
                    if at > expected {
                        if at - expected <= cf {
                            self.pending.resize(self.pending.len() + ((at - expected) * 2) as usize, 0.0);
                            self.pending.extend_from_slice(&self.scratch);
                        } else {
                            // Large gap (> 1 chunk): finish the current chunk if it already has audio before the gap,
                            // then anchor pending at `at` and let `silent_from` emit intermediate silent chunks.
                            if self.pending_from == Some(start) && expected > start {
                                self.pending.resize((cf * 2) as usize, 0.0);
                                self.emit(target, cf as usize);
                                self.silent_from = Some(start + cf);
                                emitted_before_gap = true;
                            } else {
                                self.silent_from = Some(self.silent_from.map_or(expected, |s| s.min(expected)));
                            }
                            self.pending.clear();
                            self.pending_from = Some(at);
                            self.pending.extend_from_slice(&self.scratch);
                        }
                    } else {
                        let skip = (expected.saturating_sub(at).min(frames) * 2) as usize;
                        self.pending.extend_from_slice(&self.scratch[skip..]);
                    }
                }
            }
            // Nothing past the decode cap.
            if let Some(f) = self.pending_from {
                if f + (self.pending.len() / 2) as u64 > max {
                    self.pending.truncate((max.saturating_sub(f) * 2) as usize);
                    self.pcm.exact.store(false, Ordering::Release);
                    self.pcm.set_exact_end(max);
                }
            }
            if emitted_before_gap {
                return;
            }
        }
    }

    /// Cuts `frames` from the front of `pending` into chunk `index`, with its overview peaks.
    fn emit(&mut self, index: usize, frames: usize) {
        let pcm: Box<[f32]> = self.pending[..frames * 2].into();
        self.pending.drain(..frames * 2);
        self.pending_from = self.pending_from.map(|f| f + frames as u64);
        self.land(index, pcm);
    }

    /// Chunk `index` as `frames` of silence: the stream has no audio there.
    fn emit_silence(&mut self, index: usize, frames: usize) {
        self.land(index, vec![0.0; frames * 2].into());
    }

    fn land(&self, index: usize, pcm: Box<[f32]>) {
        let Some(layout) = self.pcm.layout() else { return };
        let peaks = LoadedTrackAudio::compute_waveform_peaks(&pcm, layout.sample_rate).0.into_boxed_slice();
        let pcm = if self.pcm.peaks_only { Box::default() } else { pcm };
        if layout.chunks[index].set(PreviewChunk { pcm, peaks }).is_ok() {
            self.pcm.filled.fetch_add(1, Ordering::AcqRel);
        }
    }
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
            overview: Vec::new().into(),
            pcm_stereo,
            stems_ready: false,
            pcm_drums: None,
            pcm_bass: None,
            pcm_other: None,
            pcm_vocals: None,
            phrases: Vec::new(),
            auto_gain: 1.0,
            played_peaks: Vec::new().into(),
            played_stem_peaks: Vec::new().into(),
            played_overview: Vec::new().into(),
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
    /// buffers, and a fresh callback state as after a reopen. A headphone preview (split cue) runs out, is
    /// replaced and stopped along the way.
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
            st.output_route = OutputRoute::Split;
            let short = tone_track(600.0, 44_100, 0.05).pcm_stereo;
            st.start_preview(PreviewPcm::decoded(&short, 44_100, 1000, |_| true, true, true), 0.0); // ends within the first round
        }
        let (stats, recorder) = (Arc::new(AudioStats::default()), Arc::default());
        let rec = start_take(&recorder, "alloc");
        let mut streaming: Option<(Arc<Vec<f32>>, Arc<PreviewPcm>)> = None;
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
                if block % 30 == 20 {
                    // A preview replaces the last one, which the callback retires; only its first second is in.
                    let tone = tone_track(500.0 + block as f32, 96_000, 2.5).pcm_stereo;
                    let preview = PreviewPcm::decoded(&tone, 96_000, 96_000, |i| i == 0, false, true);
                    shared.lock().unwrap().start_preview(Arc::clone(&preview), 0.0);
                    streaming = Some((tone, preview));
                }
                if block % 30 == 24 {
                    shared.lock().unwrap().seek_preview(0.7); // a second not decoded yet: it waits
                }
                if let Some((tone, preview)) = streaming.as_ref().filter(|_| block % 30 == 27) {
                    for i in 1..3 {
                        preview.land(tone, i);
                    }
                    preview.end.store((tone.len() / 2) as u64, Ordering::Release);
                }
                if block == 115 {
                    shared.lock().unwrap().stop_preview();
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

    /// Decks A and B both at 120 BPM with grids at 0 s; B follows A, `behind` beats behind it.
    fn synced_pair(behind: f64) -> (Arc<Mutex<AudioSharedState>>, Renderer) {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut s = shared.lock().unwrap();
            s.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 30.0));
            s.loaded_tracks[1] = Some(tone_track(330.0, 48_000, 30.0));
            s.playhead_sec = [4.0, 4.0 - behind * 0.5, 0.0, 0.0];
            s.is_playing = [true, true, false, false];
            s.sync_follow[1] = Some(SyncLock { leader: 0, grid: (120.0, 0.0), leader_grid: (120.0, 0.0) });
        }
        let r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        (shared, r)
    }

    /// B's distance from A's beat, in beats, as the callback has it.
    fn phase_error(r: &Renderer) -> f64 {
        let p = r.state.playhead_sec;
        beat_phase_diff(p[0] * 2.0, p[1] * 2.0, 1.0)
    }

    /// The callback pulls a synced deck onto the beat by itself, no UI frames needed (a minimised or busy window
    /// used to freeze the correction), never faster than ±5%, and leaves it at exactly its tempo once there.
    #[test]
    fn phase_lock_converges_in_the_callback_alone() {
        for behind in [0.15, -0.3] {
            let (_shared, mut r) = synced_pair(behind);
            let mut fastest = 0.0f64;
            for _ in 0..(48_000 * 4 / 128) {
                block(&mut r);
                fastest = fastest.max((r.state.sync_adjust[1] - 1.0).abs());
            }
            assert!(phase_error(&r).abs() < 0.011, "{behind}: still {} beats off", phase_error(&r));
            assert!(fastest <= 0.05 + 1e-12, "{behind}: corrected at {fastest}");
            assert_eq!(r.state.sync_adjust[1], 1.0, "{behind}: in phase, the deck plays at its tempo");
        }
        // Inside the 1% deadband nothing moves.
        let (_shared, mut r) = synced_pair(0.005);
        block(&mut r);
        assert_eq!(r.state.sync_adjust[1], 1.0);
    }

    /// A pitch bend on the jog while synced is the DJ choosing an offset: the lock keeps it instead of undoing it.
    /// SYNC pressed again (a reset) drops it.
    #[test]
    fn phase_lock_keeps_a_hand_nudge() {
        let (shared, mut r) = synced_pair(0.0);
        for _ in 0..20 {
            block(&mut r);
        }
        // Bend B ahead for a while, then let the bend decay on its own.
        shared.lock().unwrap().jog_nudge(1, 400, 1.0);
        for _ in 0..(48_000 * 3 / 128) {
            block(&mut r);
        }
        let chosen = phase_error(&r);
        assert!(chosen.abs() > 0.02, "the bend moved B by {chosen} beats (past the 0.01 deadband)");
        for _ in 0..(48_000 * 2 / 128) {
            block(&mut r);
        }
        assert!((phase_error(&r) - chosen).abs() < 0.011, "kept {chosen}, now {}", phase_error(&r));
        shared.lock().unwrap().sync_reset[1] = true;
        for _ in 0..(48_000 * 4 / 128) {
            block(&mut r);
        }
        assert!(phase_error(&r).abs() < 0.011, "a fresh SYNC locks to the beat again");
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

    #[test]
    fn paused_scratch_sounds_and_holds_velocity_between_reports() {
        let (shared, _, mut r) = playing_deck();
        {
            let mut s = shared.lock().unwrap();
            s.is_playing[0] = false;
            s.seek_to(0, 1.0);
        }
        assert!(block(&mut r).iter().all(|&x| x == 0.0));
        {
            let mut s = shared.lock().unwrap();
            s.enable_scratch(0, 1024, 33.333, 0.125, 0.004);
            s.scratch_tick(0, 10);
            s.scratch_tick(0, 0);
        }
        let mut previous = 0.0;
        for _ in 0..6 {
            assert!(block(&mut r).iter().any(|&x| x.abs() > 1e-4));
            let rate = r.state.scratch_rate[0];
            assert!(rate > previous && rate < 1.0, "scratch velocity {rate} after {previous}");
            previous = rate;
        }
        for _ in 0..100 {
            block(&mut r);
        }
        assert!(r.state.scratch_rate[0].abs() < 0.005, "a resting platter kept moving");
        assert!(!shared.lock().unwrap().is_playing[0]);
    }

    #[test]
    fn jog_nudge_bends_playback_then_decays() {
        let (shared, _, mut r) = playing_deck();
        block(&mut r);
        let start = r.state.playhead_sec[0];
        shared.lock().unwrap().jog_nudge(0, 1024, 1.0);
        block(&mut r);
        assert!((1.0..1.18).contains(&((r.state.playhead_sec[0] - start) / PERIOD)));
        let first = r.state.jog_nudge_rate[0];
        {
            let _busy = shared.lock().unwrap();
            for _ in 0..34 {
                block(&mut r);
            }
        }
        block(&mut r);
        assert!((0.34..0.38).contains(&(r.state.jog_nudge_rate[0] / first)));
        for _ in 0..400 {
            block(&mut r);
        }
        assert_eq!(r.state.jog_nudge_rate[0], 0.0);

        {
            let mut s = shared.lock().unwrap();
            s.is_playing[0] = false;
            s.seek_to(0, 1.0);
            s.jog_nudge(0, 1024, 1.0);
        }
        block(&mut r);
        assert_eq!(r.state.playhead_sec[0], 1.08);
        {
            let mut s = shared.lock().unwrap();
            s.seek_to(0, 11.0);
            s.fast_scrub(0, 960, 1.0);
        }
        block(&mut r);
        assert_eq!(r.state.playhead_sec[0], 12.0);
    }

    #[test]
    fn deck_reverb_rings_after_the_channel_fader_closes() {
        for enabled in [false, true] {
            let (shared, _, mut r) = playing_deck();
            // Sample-exact silence checks: without the master limiter's 1.5 ms lookahead delay.
            shared.lock().unwrap().limiter_enabled = false;
            shared.lock().unwrap().fx_enabled[0] = enabled;
            for _ in 0..64 {
                block(&mut r);
            }
            shared.lock().unwrap().engine.strips[0].volume = 0.0;
            let tail = block(&mut r);
            assert_eq!(tail.iter().any(|&x| x.abs() > 1e-4), enabled);
            shared.lock().unwrap().engine.master_volume = 0.0;
            assert!(block(&mut r).iter().all(|&x| x == 0.0));
        }
    }

    #[test]
    fn master_limiter_can_be_bypassed() {
        let (shared, _, mut r) = playing_deck();
        shared.lock().unwrap().engine.master_volume = 100.0;
        let peak = |r: &mut Renderer| block(r).into_iter().map(f32::abs).fold(0.0, f32::max);
        assert!(peak(&mut r) < 1.0);
        shared.lock().unwrap().limiter_enabled = false;
        assert_eq!(peak(&mut r), 1.0);
    }

    #[test]
    fn pattern_clap_noise_does_not_repeat_each_buffer() {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut s = shared.lock().unwrap();
            s.pattern_enabled = true;
            s.pattern_steps = [[false; 16], [true; 16], [false; 16], [false; 16]];
        }
        let mut r = Renderer::new(shared, Arc::default(), Arc::default());
        let first = block(&mut r);
        r.state.pattern_phase_sec = 0.0; // Keep the envelope identical and compare the noise.
        let second = block(&mut r);
        assert!(first.iter().any(|&x| x.abs() > 0.01));
        assert_ne!(first, second);
    }

    /// A jog touch, 20 blocks scratching forward, then release; then 75 blocks (200 ms)
    /// with no new input, the UI holding the lock or not, and 75 more. The release ramp runs in the callback
    /// either way: the deck is at the same spot at the same speed, and once back at playing speed the UI is
    /// told the ramp is over.
    #[test]
    fn jog_release_eases_back_while_the_ui_holds_the_lock() {
        let run = |busy: bool| -> (f64, f64, f64, bool) {
            let (shared, _, mut r) = playing_deck();
            block(&mut r);
            shared.lock().unwrap().enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
            shared.lock().unwrap().scratch_tick(0, 24);
            for _ in 0..20 {
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
                s.limiter_enabled = false; // frame-exact: no lookahead delay
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
                s.retired.push(Arc::new(()));
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
        s.enable_scratch(0, 1024, 33.333, 0.125, 0.004);
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
        s.scratch_tick(0, -8);
        fallback_tick(&mut s, 0.016);
        assert!((1.98..2.0).contains(&s.playhead_sec[0]));
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
                s.limiter_enabled = false; // frame-exact: no lookahead delay
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

    /// Try the named sink or raw device first, preserving its width, then the default output.
    #[test]
    fn routes_try_native_first_and_keep_named_outputs() {
        use Route::*;
        let native = |channels| NativeNode { node: "usb.mx2".into(), channels };
        assert_eq!(routes("pw:usb.mx2", Some(4), false, true, 4), [native(Some(4)), NativeDefault, AlsaDefault]);
        assert_eq!(routes("pw:usb.mx2", Some(4), false, false, 4), [AlsaDefault]);
        assert_eq!(routes("pw:usb.mx2", Some(4), false, true, 2)[0], native(Some(2)), "split cue needs only stereo");
        assert_eq!(routes("pw:usb.mx2", Some(2), false, true, 4)[0], native(Some(2)));
        assert_eq!(routes("pw:usb.mx2", None, false, true, 4)[0], native(None), "no pactl: the node decides");
        // Wider and odd sinks: as many as the pairs need, or all the sink has. A mono sink gets one channel, never
        // a stereo stream it would downmix (with the split cue in it); a sink that states nothing gets stereo.
        for (listed, want, asked) in [(8, 6, 6), (8, 4, 4), (4, 6, 4), (3, 4, 3), (5, 6, 5), (1, 4, 1), (1, 2, 1), (0, 4, 2)] {
            assert_eq!(routes("pw:usb.mx2", Some(listed), false, true, want)[0], native(Some(asked)), "{listed} ch, {want} wanted");
        }
        assert_eq!(routes("pw:qa.gone", None, true, true, 4), [NativeDefault, AlsaDefault], "a sink the server knows is gone");
        assert_eq!(routes("alsa:hw:CARD=MX2,DEV=0", None, false, true, 4), [Alsa { id: "alsa:hw:CARD=MX2,DEV=0".into() }, NativeDefault, AlsaDefault]);
        assert_eq!(routes("", None, false, true, 4), [NativeDefault, AlsaDefault]);
        assert_eq!(routes("", None, false, false, 4), [AlsaDefault]);
    }

    /// Raw ALSA and plugin layouts: the wanted width at the asked rate, else at another rate, else the narrowest
    /// usable wider layout (a fixed 4- or 8-channel controller, even at another rate, before stereo), else stereo.
    /// Formats the callback can't write are skipped; a plugin capped at its sink's width never opens wider.
    #[test]
    fn raw_layouts_pick_the_narrowest_that_reaches_the_pairs() {
        use cpal::SampleFormat::{F32, I16, I32, U16, U8};
        let layout = |channels, min, max, format| cpal::SupportedStreamConfigRange::new(channels, min, max, cpal::SupportedBufferSize::Unknown, format);
        let pick = |ranges: &[cpal::SupportedStreamConfigRange], want, most| pick_config(ranges, want, most, 48_000).map(|r| (r.channels(), r.min_sample_rate()));
        let any = u16::MAX;
        for format in [I16, U16, I32] {
            let four_only = [layout(4, 44_100, 96_000, format)];
            assert_eq!(pick(&four_only, 2, any), Some((4, 44_100)), "a fixed four-channel layout");
        }
        let both = [layout(4, 48_000, 48_000, F32), layout(2, 48_000, 48_000, F32)];
        assert_eq!(pick(&both, 2, any), Some((2, 48_000)));
        let stereo_or_wide_elsewhere = [layout(2, 48_000, 48_000, F32), layout(8, 44_100, 44_100, I16)];
        assert_eq!(pick(&stereo_or_wide_elsewhere, 6, any), Some((8, 44_100)), "eight channels at 44.1 kHz reach 5–6; stereo doesn't");
        let exact_elsewhere = [layout(8, 48_000, 48_000, F32), layout(6, 44_100, 44_100, F32), layout(2, 48_000, 48_000, F32)];
        assert_eq!(pick(&exact_elsewhere, 6, any), Some((6, 44_100)));
        let unusable_wide = [layout(8, 48_000, 48_000, U8), layout(2, 48_000, 48_000, I16)];
        assert_eq!(pick(&unusable_wide, 6, any), Some((2, 48_000)));
        assert_eq!(pick(&[layout(1, 48_000, 48_000, F32)], 1, any), Some((1, 48_000)), "a mono device");
        // Through a plugin, capped at what the sink lists.
        let plugin = [layout(2, 8_000, 192_000, F32), layout(8, 8_000, 192_000, F32)];
        assert_eq!(pick(&plugin, 4, 4), Some((2, 8_000)), "a plugin never fakes outputs 5–8");
        assert_eq!(pick(&plugin, 1, 1), None, "no stereo stream for a mono sink");
    }

    /// The one resolver the callback, the stream width and the preview check share: a pair the stream lacks
    /// falls back to 1–2 for the main mix and to nothing for the headphones, as does a headphone pair on the
    /// main one; a system-default output plays the main mix alone, so a controller's cue never reaches laptop
    /// speakers. Out-of-range pairs neither overflow nor index past the stream.
    #[test]
    fn output_route_falls_back_safely() {
        let k2 = OutputRoute::Pairs { main: 1, headphones: Some(0) };
        let alone = OutputRoute::Pairs { main: 0, headphones: None };
        assert_eq!(k2.place(4), Placement::Pairs { main: 2, phones: Some(0) });
        assert_eq!(k2.place(3), Placement::Pairs { main: 0, phones: None });
        assert_eq!(k2.place(2), Placement::Pairs { main: 0, phones: None });
        assert_eq!((k2.place(1), k2.place(0), OutputRoute::Split.place(1)), (Placement::Mono, Placement::Mono, Placement::Mono));
        assert_eq!(OutputRoute::Split.place(4), Placement::Split);
        let far = OutputRoute::Pairs { main: u8::MAX, headphones: Some(u8::MAX - 1) };
        assert_eq!((far.width(), far.place(8)), (512, Placement::Pairs { main: 0, phones: None }));
        assert_eq!(far.place(512), Placement::Pairs { main: 510, phones: Some(508) });
        assert_eq!([OutputRoute::default(), k2, alone, OutputRoute::Split].map(OutputRoute::width), [4, 4, 2, 2]);

        let mut audio = AudioSystem::new();
        audio.open("", 48_000, 256, k2);
        assert_eq!(audio.shared.lock().unwrap().output_route, k2);
        audio.on_system_default = true; // what `open` sets on falling back to the default output
        audio.reroute(k2);
        assert_eq!((audio.output_route, audio.shared.lock().unwrap().output_route), (alone, alone));
        assert!(!audio.output_route.place(8).headphones_heard());
        audio.reroute(OutputRoute::Split);
        assert_eq!(audio.shared.lock().unwrap().output_route, OutputRoute::Split, "split cue is the user's own stereo choice");
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

        let p = render_peaks(|st| { cue_only(st); st.output_route = OutputRoute::Split; }, 2);
        assert!(p[0] > 0.1 && p[1] < 0.01, "split: left = cue, right = main: {p:?}");

        // A Xone:K2's wiring, main on the rear 3–4 and headphones in the front 1–2, and wider cards: each bus on
        // its own pair, nothing anywhere else. Fader up, CUE off, cue/main knob at cue: only the main pair sounds.
        let k2 = OutputRoute::Pairs { main: 1, headphones: Some(0) };
        for (route, channels, main, phones) in [(k2, 4, 2, 0), (OutputRoute::Pairs { main: 2, headphones: Some(3) }, 8, 4, 6), (k2, 5, 2, 0)] {
            let only_on = |p: &[f32], first: usize| p.iter().enumerate().all(|(c, &x)| if (first..first + 2).contains(&c) { x > 0.1 } else { x < 0.01 });
            let p = render_peaks(|st| { cue_only(st); st.output_route = route; }, channels);
            assert!(only_on(&p, phones), "{route:?} on {channels} ch: the cue belongs on {phones} only: {p:?}");
            let p = render_peaks(|st| st.output_route = route, channels);
            assert!(only_on(&p, main), "{route:?} on {channels} ch: the main mix belongs on {main} only: {p:?}");
        }
        // The K2 routing on a stereo fallback: the main mix on 1–2, the cue nowhere.
        assert!(render_peaks(|st| { cue_only(st); st.output_route = k2; }, 2).iter().all(|&x| x < 0.01), "the cue reached a stereo output");
        assert!(render_peaks(|st| st.output_route = k2, 2).iter().all(|&x| x > 0.1), "the main mix left a stereo output");

        // CUE off: nothing in the headphones at the CUE end of the blend.
        let p = render_peaks(|st| st.engine.strips[0].volume = 0.0, 4);
        assert!(p[2] < 0.01, "no cue pressed, headphones silent: {p:?}");
    }

    /// Output channel `c` of interleaved `out`.
    fn channel(out: &[f32], channels: usize, c: usize) -> Vec<f32> {
        out.iter().skip(c).step_by(channels).copied().collect()
    }

    /// Two decks playing (A also on CUE) with REC on, 40 blocks of 256 frames at 48 kHz into `channels`
    /// outputs, after `setup`; with `preview`, a 44.1 kHz one starts first. Returns the output, the take, the
    /// meters (master L/R, then the four channels) and the preview's published progress.
    fn two_decks(
        setup: &dyn Fn(&mut AudioSharedState),
        preview: Option<&Arc<PreviewPcm>>,
        channels: usize,
        name: &str,
    ) -> (Vec<f32>, Vec<i16>, [f32; 6], PreviewProgress) {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        {
            let mut st = shared.lock().unwrap();
            st.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 4.0));
            st.loaded_tracks[1] = Some(tone_track(330.0, 44_100, 4.0));
            st.is_playing = [true, true, false, false];
            st.engine.strips[0].cue_enabled = true;
            setup(&mut st);
            if let Some(pcm) = preview {
                st.start_preview(Arc::clone(pcm), 0.0);
            }
        }
        let recorder = Arc::default();
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::clone(&recorder));
        let rec = start_take(&recorder, name);
        let (mut out, mut buf) = (Vec::new(), vec![0.0f32; 256 * channels]);
        for _ in 0..40 {
            r.render(&mut buf, channels, 48_000, |x| x);
            out.extend_from_slice(&buf);
        }
        let (_, take) = finish_take(&recorder, rec);
        let s = shared.lock().unwrap();
        let meters = [s.master_peak_l, s.master_peak_r, s.channel_peaks[0], s.channel_peaks[1], s.channel_peaks[2], s.channel_peaks[3]];
        (out, take, meters, s.preview_progress)
    }

    /// With both decks playing and deck A's CUE on, a preview changes only the headphones: the main output,
    /// the recording and every meter match an identical run without it, sample for sample. On 3–4, on 1–2
    /// with the main mix on 3–4 (a Xone:K2), on a pair of a wider odd card and on split cue, the headphones
    /// carry deck A's cue plus the preview, read at 44.1 kHz on a 48 kHz card and scaled by the headphone
    /// level; spare channels stay silent, and the main mix and the take are the same whichever pair is main.
    /// Toward main on the cue/main knob it fades out with the cue; level 0 is silent. Without a headphone
    /// route (3–4 on a stereo card, the K2 routing on a stereo card, both on one pair, none, split on mono)
    /// the main mix stays on 1–2, nothing changes anywhere and the preview doesn't move on.
    #[test]
    fn preview_reaches_only_the_headphones() {
        let tone = tone_track(700.0, 44_100, 3.0).pcm_stereo;
        let preview = PreviewPcm::decoded(&tone, 44_100, 4096, |_| true, true, true);
        let expected = |frame: usize| sample_stereo_hermite(&tone, frame as f64 * 44_100.0 / 48_000.0);
        let k2 = OutputRoute::Pairs { main: 1, headphones: Some(0) };
        let wide = OutputRoute::Pairs { main: 2, headphones: Some(1) };
        let mut first: Option<(Vec<f32>, Vec<i16>)> = None;
        for (route, channels, main, phones) in [
            (OutputRoute::default(), 4, vec![0, 1], vec![2, 3]),
            (k2, 4, vec![2, 3], vec![0, 1]),
            (wide, 7, vec![4, 5], vec![2, 3]),
            (OutputRoute::Split, 2, vec![1], vec![0]),
        ] {
            let setup = move |st: &mut AudioSharedState| st.output_route = route;
            let name = format!("{route:?} on {channels} ch");
            let (base, base_take, base_meters, _) = two_decks(&setup, None, channels, &format!("base-{name}"));
            let (with, take, meters, progress) = two_decks(&setup, Some(&preview), channels, &format!("prev-{name}"));
            for &c in &main {
                assert!(channel(&with, channels, c) == channel(&base, channels, c), "{name}: main channel {c} changed");
            }
            assert!(channel(&base, channels, main[0]).iter().any(|x| x.abs() > 0.05), "{name}: the decks are silent");
            assert!(take == base_take && take.iter().any(|&s| s.abs() > 1000), "{name}: the recording changed or is silent");
            assert_eq!(meters, base_meters, "{name}: a meter saw the preview");
            assert!((progress.frame - 40.0 * 256.0 * 44_100.0 / 48_000.0).abs() < 1e-6 && progress.started && !progress.ended, "{name}: {progress:?}");
            for c in (0..channels).filter(|c| !main.contains(c) && !phones.contains(c)) {
                assert!(channel(&with, channels, c).iter().all(|&x| x == 0.0), "{name}: spare channel {c} isn't silent");
            }
            // Whichever pair is main, it carries the same program, and the take is the same.
            let program = channel(&with, channels, main[0]);
            match (&first, route) {
                (None, _) => first = Some((program, take.clone())),
                (Some((p, t)), OutputRoute::Pairs { .. }) => assert!(*p == program && *t == take, "{name}: the program differs from 1–2's"),
                (Some((_, t)), _) => assert!(*t == take, "{name}: the take differs"),
            }

            // Headphones: deck A's cue as before, plus the preview at the default level 0.8.
            let mut checked = 0;
            for (side, &c) in phones.iter().enumerate() {
                let (b, w) = (channel(&base, channels, c), channel(&with, channels, c));
                assert!(b.iter().any(|x| x.abs() > 0.05), "{name}: deck A's cue is gone from channel {c}");
                for (f, (b, w)) in b.iter().zip(&w).enumerate() {
                    let (pl, pr) = expected(f);
                    let p = match (route, side) {
                        (OutputRoute::Split, _) => (pl + pr) * 0.5,
                        (_, 0) => pl,
                        _ => pr,
                    };
                    if w.abs() < 0.99 && b.abs() < 0.99 {
                        assert!((w - b - p * 0.8).abs() < 1e-4, "{name}: channel {c} frame {f}: {w} vs {b} + 0.8 * {p}");
                        checked += 1;
                    }
                }
            }
            assert!(checked > 5000, "{name}: only {checked} unclipped frames compared");
        }

        // The cue/main knob at main: the preview fades with the cue. Headphone level 0: silence.
        for (knob, level) in [(1.0, 0.8), (0.0, 0.0)] {
            let setup = move |st: &mut AudioSharedState| {
                st.cue_mix = knob;
                st.cue_volume = level;
            };
            let (base, _, _, _) = two_decks(&setup, None, 4, "base-knob");
            let (with, _, _, _) = two_decks(&setup, Some(&preview), 4, "prev-knob");
            assert!(with == base, "cue/main {knob}, level {level}: the preview is still heard");
        }

        // No headphone route: the main mix on 1–2, the output is the baseline exactly, and the preview waits at its start.
        let none = OutputRoute::Pairs { main: 0, headphones: None };
        let same = OutputRoute::Pairs { main: 1, headphones: Some(1) };
        for (route, channels) in [(OutputRoute::default(), 2), (k2, 2), (k2, 3), (none, 4), (same, 4), (OutputRoute::Split, 1)] {
            let setup = move |st: &mut AudioSharedState| st.output_route = route;
            let (base, base_take, _, _) = two_decks(&setup, None, channels, "base-none");
            let (with, take, _, progress) = two_decks(&setup, Some(&preview), channels, "prev-none");
            assert!(with == base && take == base_take, "{route:?} on {channels} ch: the preview reached an output");
            assert_eq!(progress.frame, 0.0, "{route:?} on {channels} ch: the preview played into nothing");
            let main = if route == same { 2 } else { 0 };
            assert!(channel(&with, channels, main).iter().any(|x| x.abs() > 0.05), "{route:?} on {channels} ch: the main mix is lost");
        }
    }

    /// One callback on 3–4 at headphone level 1, nothing else playing: the headphones carry `pcm` read at its
    /// own rate. Returns (left headphone samples, progress) after blocks of each of `sizes` frames at `sr`.
    fn preview_run(r: &mut Renderer, shared: &Mutex<AudioSharedState>, sizes: &[usize], sr: u32) -> (Vec<f32>, PreviewProgress) {
        let mut left = Vec::new();
        for &frames in sizes {
            let mut buf = vec![0.0f32; frames * 4];
            r.render(&mut buf, 4, sr, |x| x);
            left.extend(channel(&buf, 4, 2));
            assert!(channel(&buf, 4, 0).iter().all(|&x| x == 0.0), "the preview reached the main output");
        }
        (left, shared.lock().unwrap().preview_progress)
    }

    fn preview_only(pcm: &Arc<PreviewPcm>) -> (Arc<Mutex<AudioSharedState>>, Renderer, u64) {
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        let serial = {
            let mut st = shared.lock().unwrap();
            st.cue_volume = 1.0;
            st.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 12.0));
            st.playhead_sec[0] = 1.0;
            st.engine.strips[0].volume = 0.0; // deck A plays on, unheard, to show its playhead keeps time
            st.is_playing[0] = true;
            st.start_preview(Arc::clone(pcm), 0.0)
        };
        let r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        (shared, r, serial)
    }

    /// The preview keeps its own cursor at its own rate whatever the card's rate and block sizes: a 96 kHz
    /// preview on 44.1 kHz and 22.05 kHz cards, and a 22.05 kHz one on 48 kHz, in blocks of 37, 128, 1000
    /// and 5 frames, is exactly the source read at source/card rate, seamless across block edges and across
    /// its decoded chunks (300 frames each here, so reads straddle them all the time). It ends
    /// once, then stays silent; deck A keeps time throughout. Starting the same audio again plays it from
    /// the top.
    #[test]
    fn preview_keeps_its_own_rate_ends_once_and_restarts() {
        for (rate, sr) in [(96_000u32, 44_100u32), (96_000, 22_050), (22_050, 48_000)] {
            // 0.0503 s: the last frame falls well inside a card frame at all three rate pairs.
            let pcm = tone_track(900.0, rate, 0.0503).pcm_stereo;
            let total = (pcm.len() / 2) as f64;
            let step = rate as f64 / sr as f64;
            let preview = PreviewPcm::decoded(&pcm, rate, 300, |_| true, true, true);
            let (shared, mut r, serial) = preview_only(&preview);
            let sizes = [37, 128, 1000, 5].repeat(4);
            let (left, progress) = preview_run(&mut r, &shared, &sizes, sr);
            let played = (total / step).ceil() as usize;
            for (f, x) in left.iter().enumerate() {
                let want = if f < played { sample_stereo_hermite(&pcm, f as f64 * step).0 } else { 0.0 };
                assert!((x - want).abs() < 1e-6, "{rate} Hz on {sr} Hz: frame {f} is {x}, expected {want}");
            }
            assert!(left.len() > played && progress.ended && progress.serial == serial, "{rate} on {sr}: {progress:?}");
            let frames: usize = sizes.iter().sum();
            let deck = shared.lock().unwrap().playhead_sec[0];
            assert!((deck - (1.0 + frames as f64 / sr as f64)).abs() < 1e-9, "deck A drifted to {deck}");

            // Silent from here on, then the same audio again plays from its start.
            let (after, _) = preview_run(&mut r, &shared, &[256], sr);
            assert!(after.iter().all(|&x| x == 0.0), "the ended preview came back");
            let again = shared.lock().unwrap().start_preview(Arc::clone(&preview), 0.0);
            let (left, progress) = preview_run(&mut r, &shared, &[64], sr);
            assert_eq!((progress.serial, progress.ended), (again, false));
            assert!((left[1] - sample_stereo_hermite(&pcm, step).0).abs() < 1e-6, "the restart didn't start at the top");
        }
    }

    /// The preview plays on while the UI holds the lock. A stop silences it from the next block. A preview
    /// replaced while there is no room to retire the old buffer goes silent at once and the new one waits,
    /// silent, until the UI frees some; it then starts at its top. A reopen carries on where the preview was,
    /// and one that ended stays ended; a stream closing late writes nothing.
    #[test]
    fn preview_stop_replace_and_reopen_never_bring_back_stale_audio() {
        let first = tone_track(500.0, 48_000, 2.0).pcm_stereo;
        let first_preview = PreviewPcm::decoded(&first, 48_000, 4096, |_| true, true, true);
        let (shared, mut r, serial) = preview_only(&first_preview);
        preview_run(&mut r, &shared, &[128], 48_000);
        let held = {
            let _ui = shared.lock().unwrap();
            preview_run_unlocked(&mut r, 50)
        };
        assert!(held.iter().all(|&p| p > 0.05), "the preview stopped while the UI held the lock");
        let (_, progress) = preview_run(&mut r, &shared, &[128], 48_000);
        assert_eq!(progress, PreviewProgress { serial, frame: 52.0 * 128.0, started: true, ended: false }, "a busy block lost its place");

        // Reopen mid-preview: the next stream picks up at the same frame.
        drop(r);
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        let (left, progress) = preview_run(&mut r, &shared, &[128], 48_000);
        assert_eq!(progress.frame, 53.0 * 128.0, "the reopen restarted the preview");
        assert!((left[0] - sample_stereo_hermite(&first, 52.0 * 128.0).0).abs() < 1e-6);

        // Stop: silent from the next block, and the buffer comes back to the UI.
        drop(shared.lock().unwrap().take_retired()); // what the closed stream handed back
        shared.lock().unwrap().stop_preview();
        let (left, _) = preview_run(&mut r, &shared, &[128, 128], 48_000);
        assert!(left.iter().all(|&x| x == 0.0), "a stopped preview was heard");
        let retired = shared.lock().unwrap().take_retired();
        assert!(retired.into_iter().any(|a| a.downcast::<PreviewPcm>().is_ok_and(|p| Arc::ptr_eq(&p, &first_preview))), "the stopped preview wasn't retired");

        // Replace with the retire list full: the old one is silent at once, the new one waits for room.
        shared.lock().unwrap().start_preview(Arc::clone(&first_preview), 0.0);
        preview_run(&mut r, &shared, &[128], 48_000);
        let second = tone_track(750.0, 44_100, 2.0).pcm_stereo;
        let second_serial = {
            let mut st = shared.lock().unwrap();
            while st.retired.len() < st.retired.capacity() {
                st.retired.push(Arc::new(()));
            }
            st.start_preview(PreviewPcm::decoded(&second, 44_100, 4096, |_| true, true, true), 0.0)
        };
        let (left, progress) = preview_run(&mut r, &shared, &[128, 128], 48_000);
        assert!(left.iter().all(|&x| x == 0.0) && progress.serial != second_serial, "stale or early audio while waiting for room");
        drop(shared.lock().unwrap().take_retired());
        let (left, progress) = preview_run(&mut r, &shared, &[128], 48_000);
        assert_eq!((progress.serial, progress.ended), (second_serial, false));
        assert!((left[1] - sample_stereo_hermite(&second, 44_100.0 / 48_000.0).0).abs() < 1e-6, "the new preview didn't start at its top");

        // Ended, then reopened: still silent. An older stream closing late changes nothing.
        let short = tone_track(500.0, 48_000, 0.01).pcm_stereo;
        shared.lock().unwrap().start_preview(PreviewPcm::decoded(&short, 48_000, 4096, |_| true, true, true), 0.0);
        let (_, progress) = preview_run(&mut r, &shared, &[1024], 48_000);
        assert!(progress.ended);
        drop(r);
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        let newer = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        let (left, _) = preview_run(&mut r, &shared, &[256], 48_000);
        assert!(left.iter().all(|&x| x == 0.0), "a reopen brought back an ended preview");
        let published = shared.lock().unwrap().preview_progress;
        drop(r); // closes after `newer` took over
        assert_eq!(shared.lock().unwrap().preview_progress, published, "a late close wrote the preview's progress");
        drop(newer);
    }

    /// A preview plays what has been decoded and waits, silent and without moving on, where a second hasn't
    /// landed; it carries on seamlessly when it does. A seek to an undecoded spot waits there, then plays that
    /// exact spot. A start before the track's length is known waits for it, then lands exactly. A seek the
    /// callback hasn't taken yet is dropped by a newer start.
    #[test]
    fn preview_waits_for_undecoded_audio_and_seeks_exactly() {
        let tone = tone_track(440.0, 48_000, 3.0).pcm_stereo; // 144 000 frames
        let cf = 4800;
        let at = |frame: f64| sample_stereo_hermite(&tone, frame).0;
        let exact = |left: &[f32], from: f64, what: &str| {
            for (f, x) in left.iter().enumerate() {
                assert!((x - at(from + f as f64)).abs() < 1e-6, "{what}: frame {f} is {x}, expected {}", at(from + f as f64));
            }
        };
        let preview = PreviewPcm::decoded(&tone, 48_000, cf, |i| i == 0, false, true);
        let (shared, mut r, _) = preview_only(&preview);
        let (left, _) = preview_run(&mut r, &shared, &[4096], 48_000);
        exact(&left, 0.0, "the first second");

        // The end of what is decoded (interpolation reads two frames ahead): it waits there.
        let (left, waiting) = preview_run(&mut r, &shared, &[1024, 1024], 48_000);
        let (_, still) = preview_run(&mut r, &shared, &[512], 48_000);
        assert!(waiting.frame == still.frame && waiting.frame == (cf - 2) as f64, "it moved through audio it doesn't have: {waiting:?} then {still:?}");
        assert!(left[cf - 2 - 4096..].iter().all(|&x| x == 0.0), "it played something while waiting");
        preview.land(&tone, 1);
        let (left, on) = preview_run(&mut r, &shared, &[256], 48_000);
        exact(&left, still.frame, "carrying on");
        assert_eq!(on.frame, still.frame + 256.0);

        // A seek to the middle, not decoded yet: silent until it lands, then exactly there.
        shared.lock().unwrap().seek_preview(0.5);
        let (left, p) = preview_run(&mut r, &shared, &[256], 48_000);
        assert!(left.iter().all(|&x| x == 0.0) && p.frame == 72_000.0 && p.started, "{p:?}");
        for i in 14..17 {
            preview.land(&tone, i);
        }
        let (left, _) = preview_run(&mut r, &shared, &[256], 48_000);
        exact(&left, 72_000.0, "after the seek");

        // Length unknown (no header, end not reached): a start a quarter in waits for it.
        let unknown = PreviewPcm::decoded(&tone, 48_000, cf, |_| true, false, false);
        shared.lock().unwrap().start_preview(Arc::clone(&unknown), 0.25);
        let (left, p) = preview_run(&mut r, &shared, &[256], 48_000);
        assert!(left.iter().all(|&x| x == 0.0) && !p.started, "it started before it knew where: {p:?}");
        unknown.end.store(144_000, Ordering::Release);
        let (left, p) = preview_run(&mut r, &shared, &[256], 48_000);
        assert!(p.started);
        exact(&left, 36_000.0, "a quarter in, once the length was known");

        // A seek still waiting for the callback, then a new start: the start wins.
        {
            let mut st = shared.lock().unwrap();
            st.seek_preview(0.9);
            st.start_preview(Arc::clone(&preview), 0.0);
        }
        let (left, _) = preview_run(&mut r, &shared, &[64], 48_000);
        exact(&left, 0.0, "a stale seek moved the new preview");
    }

    /// The worker on a real WAV (two seconds at 200 Hz, then two at 2 kHz): asked for three quarters in (exactly the
    /// start of its last second), the first two seconds it lands are the ones around that spot, so the preview can
    /// play there at once; the finished preview is the file sample for sample, with its exact
    /// length and a full overview. A job replaced by another stops being decoded. A row's waveform waits behind the
    /// preview and keeps only peaks. A file that isn't audio fails with the reason.
    #[test]
    fn preview_worker_decodes_the_wanted_spot_first_and_the_file_exactly() {
        let dir = std::env::temp_dir().join(format!("retraktor-preview-worker-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav = |name: &str, secs: usize, hz: &dyn Fn(usize) -> f32| {
            let samples: Vec<i16> = (0..secs * 44_100)
                .flat_map(|i| {
                    let v = ((i as f32 * hz(i) * std::f32::consts::TAU / 44_100.0).sin() * 12_000.0) as i16;
                    [v, v]
                })
                .collect();
            AudioSystem::write_stereo_wav_file(&dir.join(name), &samples, 44_100).unwrap();
            dir.join(name)
        };
        let two_parts = wav("two-parts.wav", 4, &|i| if i < 2 * 44_100 { 200.0 } else { 2000.0 });
        let long = wav("long.wav", 60, &|_| 330.0);
        let bad = dir.join("bad.wav");
        std::fs::write(&bad, b"not audio").unwrap();
        let wait = |what: &str, done: &dyn Fn() -> bool| {
            let t0 = std::time::Instant::now();
            while !done() {
                assert!(t0.elapsed() < std::time::Duration::from_secs(120), "{what}");
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        let loader = PreviewLoader::new(crate::overview::StripeCache::default());

        let preview = PreviewPcm::new(two_parts.clone(), 0.75);
        loader.set(Some(&preview), None, None);
        wait("nothing landed", &|| preview.filled() >= 2);
        let layout = preview.layout().unwrap();
        assert!(layout.chunks[2].get().is_some() && layout.chunks[3].get().is_some(), "the seconds around the spot weren't decoded first");
        assert!(matches!(preview.sample(132_300.0), PreviewSample::Ready(_)), "the spot wanted isn't playable yet");
        wait("never completed", &|| preview.complete());
        let (expected, rate) = LoadedTrackAudio::decode_file_to_pcm(&two_parts).unwrap();
        assert_eq!((rate, preview.playable_frames()), (44_100, Some(expected.len() as u64 / 2)));
        assert!(preview.assembled().unwrap() == expected, "the preview isn't the file");
        assert!(!preview.exact() && preview.to_vec().is_none(), "a seeked decode was offered to a deck");
        let straight = PreviewPcm::new(two_parts.clone(), 0.0);
        loader.set(None, Some(&straight), None);
        wait("the straight decode never completed", &|| straight.complete());
        assert!(straight.exact() && straight.to_vec().unwrap() == expected, "a straight decode isn't the file");
        let peaks = preview.overview_peaks().unwrap();
        assert!(peaks.len() >= 4 * PEAKS_PER_SEC - 4 && peaks.iter().all(|p| p[0].max(p[1]).max(p[2]) > 0.0), "{} peaks", peaks.len());

        // A long file, replaced after its first second: it stops there (give or take the step under way).
        let first = PreviewPcm::new(long.clone(), 0.0);
        loader.set(Some(&first), None, None);
        wait("the long file never started", &|| first.filled() > 0);
        let second = PreviewPcm::new(two_parts.clone(), 0.0);
        loader.set(Some(&second), None, None);
        wait("the replacement never completed", &|| second.complete());
        let stopped_at = first.filled();
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(first.filled() == stopped_at && !first.complete(), "a replaced job kept decoding");

        // A row's waveform is decoded only once the preview playing is, with a full decode's peaks and none of its audio.
        let playing = PreviewPcm::new(long, 0.0);
        let row = PreviewPcm::peaks_only(two_parts.clone());
        loader.set(Some(&playing), None, Some(&row));
        wait("the row's waveform never completed", &|| row.complete());
        assert!(playing.complete(), "the row's waveform went ahead of the preview");
        assert!(row.overview_peaks() == straight.overview_peaks(), "a row's peaks aren't a full decode's");
        assert!(row.identity().is_some() && row.identity() == straight.identity(), "the worker didn't note which file the row's waveform is of");
        let chunks = &row.layout().unwrap().chunks;
        assert!(chunks.iter().filter_map(|c| c.get()).all(|c| c.pcm.is_empty()) && row.to_vec().is_none(), "a row's waveform kept audio");

        let failed = PreviewPcm::new(bad, 0.0);
        loader.set(None, Some(&failed), None);
        wait("the bad file never failed", &|| failed.failed().is_some());
        assert!(failed.failed().unwrap().contains("Unsupported audio format"), "{:?}", failed.failed());
        drop(loader);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A scripted stream at 8 kHz (so a chunk is 8000 frames): `frames` frames of a ramp no two frames share,
    /// in packets of 1152 starting at `first`; and the samples it carries.
    fn ramp_stream(frames: usize, first: u64, header: Option<u64>, seekable: bool) -> (TestStream, Vec<f32>) {
        let pcm: Vec<f32> = (0..frames).flat_map(|i| [(i % 7919) as f32 / 7919.0, -((i % 7919) as f32) / 7919.0]).collect();
        let packets = pcm.chunks(1152 * 2).enumerate().map(|(n, p)| (first + n as u64 * 1152, p.to_vec())).collect();
        (TestStream { rate: 8000, header_frames: header, packets, seekable }, pcm)
    }

    /// Runs the real worker's steps on a scripted stream registered as `name`, from `at`, until it has nothing
    /// left to do, as the worker thread would; it must get there in `max_steps`.
    fn decode_script(name: &str, stream: TestStream, at: f64, max_steps: usize) -> Arc<PreviewPcm> {
        let path = std::path::PathBuf::from(format!("/test-stream/{name}"));
        register_test_stream(&path, stream);
        let pcm = PreviewPcm::new(path, at);
        let mut decode = PreviewDecode::open(Arc::clone(&pcm), &Default::default()).unwrap();
        for _ in 0..max_steps {
            if pcm.done() {
                return pcm;
            }
            decode.step();
        }
        panic!("{name}: still decoding after {max_steps} steps");
    }

    /// Lengths the header gets wrong never cut or stretch a track: 10 s stated as 5 s is decoded to its real end
    /// in one exact run; 10 s stated as 20 s, with a cold click at 90 % (past the real end), finds the real end
    /// in bounded steps, and the preview that was waiting there plays from 90 % of the real track.
    #[test]
    fn preview_worker_finds_the_real_end_whatever_the_header_says() {
        let (stream, pcm) = ramp_stream(80_000, 0, Some(40_000), true);
        let short = decode_script("short-header", stream, 0.0, 60);
        assert_eq!(short.playable_frames(), Some(80_000), "cut at the header's estimate");
        assert!(short.exact() && short.assembled().unwrap() == pcm, "a straight run isn't the stream");

        let (stream, pcm) = ramp_stream(80_000, 0, Some(160_000), true);
        let path = std::path::PathBuf::from("/test-stream/long-header");
        register_test_stream(&path, stream);
        let long = PreviewPcm::new(path, 0.9);
        let mut decode = PreviewDecode::open(Arc::clone(&long), &Default::default()).unwrap();
        let (shared, mut r, _) = {
            let shared = Arc::new(Mutex::new(AudioSharedState::default()));
            let serial = { let mut st = shared.lock().unwrap(); st.cue_volume = 1.0; st.start_preview(Arc::clone(&long), 0.9) };
            let r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
            (shared, r, serial)
        };
        let (left, p) = preview_run(&mut r, &shared, &[64], 48_000);
        assert!(left.iter().all(|&x| x == 0.0) && p.frame == 144_000.0, "placed by the header first: {p:?}");
        let mut steps = 0;
        while !long.done() {
            decode.step();
            steps += 1;
            assert!(steps < 100, "the search for the real end never settled");
        }
        assert_eq!(long.playable_frames(), Some(80_000));
        assert!(long.assembled().unwrap() == pcm && !long.exact(), "a seeked run counted as exact");
        let (left, _) = preview_run(&mut r, &shared, &[64, 64], 48_000);
        let heard = left.iter().position(|&x| x != 0.0).expect("the preview never found its place");
        assert!((left[heard] - pcm[72_000 * 2]).abs() < 1e-6, "not 90 % of the real track: {}", left[heard]);
    }

    /// A stream with no stated length plays from its start at once, and a click halfway before its length is known
    /// lands halfway once it is. A stream whose first packet comes late (5000 frames, then 30 000: more than a
    /// chunk) has silence before it and every frame after in its place, in bounded steps. A stream that can't
    /// seek, asked for three quarters in, reads its way there and back without reopening for ever, exactly.
    #[test]
    fn preview_worker_streams_unknown_lengths_late_starts_and_unseekable_files() {
        let (stream, pcm) = ramp_stream(40_000, 0, None, true);
        let path = std::path::PathBuf::from("/test-stream/no-length");
        register_test_stream(&path, stream);
        let unknown = PreviewPcm::new(path, 0.0);
        let mut decode = PreviewDecode::open(Arc::clone(&unknown), &Default::default()).unwrap();
        let (shared, mut r, _) = {
            let shared = Arc::new(Mutex::new(AudioSharedState::default()));
            let serial = { let mut st = shared.lock().unwrap(); st.cue_volume = 1.0; st.start_preview(Arc::clone(&unknown), 0.0) };
            let r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
            (shared, r, serial)
        };
        decode.step();
        assert!(unknown.playable_frames().is_none());
        let (left, _) = preview_run(&mut r, &shared, &[64], 8_000);
        assert!((left[1] - pcm[2]).abs() < 1e-6, "the start waited for the length");
        shared.lock().unwrap().seek_preview(0.5);
        let (left, p) = preview_run(&mut r, &shared, &[64], 8_000);
        assert!(left.iter().all(|&x| x == 0.0) && !p.started, "a click before the length was known landed somewhere");
        for _ in 0..60 {
            decode.step();
        }
        assert!(unknown.done(), "the unknown length was never found");
        let (left, _) = preview_run(&mut r, &shared, &[64], 8_000);
        assert!((left[0] - pcm[20_000 * 2]).abs() < 1e-6, "the early click didn't land halfway");

        for first in [5_000u64, 30_000] {
            let (stream, pcm) = ramp_stream(20_000, first, Some(20_000), true);
            let late = decode_script(&format!("late-{first}"), stream, 0.0, 60);
            let all = late.assembled().unwrap();
            assert_eq!(late.playable_frames(), Some(first + 20_000));
            assert!(all[..first as usize * 2].iter().all(|&x| x == 0.0) && all[first as usize * 2..] == pcm[..], "late start {first}: misplaced");
            assert!(!late.exact(), "padded with silence yet counted as exact");
        }

        let (stream, pcm) = ramp_stream(80_000, 0, Some(80_000), false);
        let fixed = decode_script("unseekable", stream, 0.75, 120);
        assert!(fixed.assembled().unwrap() == pcm && fixed.exact(), "reading on (no seek) isn't the stream");
    }

    /// Left headphone peaks of `blocks` 128-frame 48 kHz blocks, the lock left as the caller has it.
    fn preview_run_unlocked(r: &mut Renderer, blocks: usize) -> Vec<f32> {
        (0..blocks)
            .map(|_| {
                let mut buf = vec![0.0f32; 128 * 4];
                r.render(&mut buf, 4, 48_000, |x| x);
                channel(&buf, 4, 2).iter().fold(0.0f32, |p, x| p.max(x.abs()))
            })
            .collect()
    }

    /// Two tones 12 dB apart: with auto gain they play at the same level on the main and in the cue, and GAIN still trims around it.
    #[test]
    fn auto_gain_matches_tracks_in_main_and_cue_beside_manual_gain() {
        let play = |amp: f32, matched: bool, setup: &dyn Fn(&mut AudioSharedState)| {
            render_peaks(|st| {
                let t = st.loaded_tracks[0].as_mut().unwrap();
                Arc::make_mut(&mut t.pcm_stereo).iter_mut().for_each(|s| *s *= amp / 0.5);
                if matched {
                    t.auto_gain = retraktor_library::autogain_multiplier(&t.pcm_stereo, t.sample_rate);
                }
                setup(st);
            }, 4)
        };
        let cue_only = |st: &mut AudioSharedState| {
            st.engine.strips[0].volume = 0.0;
            st.engine.strips[0].cue_enabled = true;
        };
        let ratio = |matched: bool, setup: &dyn Fn(&mut AudioSharedState), ch: usize| {
            play(0.4, matched, setup)[ch] / play(0.1, matched, setup)[ch]
        };
        assert!((ratio(false, &|_| {}, 0) / 4.0 - 1.0).abs() < 0.02, "off keeps the 12 dB difference");
        assert!((ratio(true, &|_| {}, 0) - 1.0).abs() < 0.02, "main not matched");
        assert!((ratio(true, &cue_only, 2) - 1.0).abs() < 0.02, "cue not matched");

        let centre = play(0.1, true, &|_| {})[0];
        let trimmed = play(0.1, true, &|st| st.channel_gain[0] = 0.25)[0];
        assert!((trimmed / centre - 0.5).abs() < 0.01, "GAIN at -6 dB: {trimmed} vs {centre}");
        assert!(centre < 0.99, "matched level clipped: {centre}");
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

    #[test]
    fn stem_loading_matches_track_sample_rate_and_reads_both_channels() {
        let dir = std::env::temp_dir().join(format!("retraktor-resampled-stems-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pcm: Vec<i16> = (0..4_410)
            .flat_map(|i| [0, if i < 2_205 { 0 } else { 16_384 }])
            .collect();
        for stem in ["drums", "bass", "other", "vocals"] {
            AudioSystem::write_stereo_wav_file(&dir.join(format!("{stem}.wav")), &pcm, 44_100).unwrap();
        }
        for sample_rate in [48_000, 96_000] {
            let frames = sample_rate as usize / 10;
            let mut track = LoadedTrackAudio::from_pcm_with_autogain(
                vec![0.0; frames * 2], sample_rate, "", "", 128.0, "", false,
            );
            assert!(track.load_stems_from_dir(&dir));
            let drums = track.pcm_drums.as_ref().unwrap();
            assert_eq!(drums.len(), frames * 2);
            assert_eq!(drums[frames..frames + 2], [0.0, 0.5]);
            assert_eq!(track.stem_peaks.first().unwrap(), &[0.04; 4]);
            assert_eq!(track.stem_peaks.last().unwrap(), &[0.5; 4]);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn autogain_reload_uses_the_current_file_version() {
        let path = std::env::temp_dir().join(format!("retraktor-autogain-reload-{}.wav", std::process::id()));
        let write_tone = |frames: usize, amplitude: f32| {
            let pcm: Vec<i16> = (0..frames).flat_map(|i| {
                let sample = (amplitude * (i as f32 * 2.0 * PI / 48.0).sin()) as i16;
                [sample; 2]
            }).collect();
            AudioSystem::write_stereo_wav_file(&path, &pcm, 48_000).unwrap();
        };
        let load = || LoadedTrackAudio::from_file(&path, "", "", 128.0, "").unwrap().auto_gain;
        write_tone(48_000, 12_000.0);
        let first = load();
        assert_eq!(load(), first);
        write_tone(96_000, 3_000.0);
        assert!(load() > first * 3.0);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn unity_autogain_reuses_peak_arrays() {
        let track = LoadedTrackAudio::from_pcm(vec![0.0; 48_000 * 2], 48_000, "", "", 128.0, "");
        assert_eq!(track.auto_gain, 1.0);
        assert!(Arc::ptr_eq(&track.waveform_peaks, &track.played_peaks));
        assert!(Arc::ptr_eq(&track.stem_peaks, &track.played_stem_peaks));
        assert!(Arc::ptr_eq(&track.overview, &track.played_overview));
    }

    /// Two copies of one recording 18 dB apart, each loaded with its own measured auto gain, play the
    /// same, so their waveforms and stem lanes (estimated, then separated stems arriving late) must look
    /// the same. The silent break stays dark and the quieter section stays quieter and visible.
    #[test]
    fn auto_gain_waveforms_match_level_shifted_copies() {
        let dir = std::env::temp_dir().join(format!("retraktor-played-peaks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let sr = 44_100;
        let tau = std::f32::consts::TAU;
        // Bass, chords and hats: 3 s, then 2 s of digital silence, then 3 s of the same 10 dB down.
        let sample = |i: usize, amp: f32| {
            let t = i as f32 / sr as f32;
            let section = [1.0, 1.0, 1.0, 0.0, 0.0, 0.316, 0.316, 0.316][i / sr as usize];
            amp * section * (0.5 * (tau * 60.0 * t).sin() + 0.3 * (tau * 1000.0 * t).sin() + 0.2 * (tau * 6000.0 * t).sin())
        };
        let load = |name: &str, amp: f32| {
            let pcm: Vec<i16> = (0..8 * sr as usize).flat_map(|i| {
                let v = (sample(i, amp) * 32767.0) as i16;
                [v, v]
            }).collect();
            let path = dir.join(format!("{name}.wav"));
            AudioSystem::write_stereo_wav_file(&path, &pcm, sr).unwrap();
            let mut track = LoadedTrackAudio::from_file(&path, name, "", 128.0, "1d").unwrap();
            let estimated_stems = track.played_stem_peaks.clone();
            let stem_dir = dir.join(name);
            std::fs::create_dir_all(&stem_dir).unwrap();
            for stem in ["drums", "bass", "other", "vocals"] {
                std::fs::copy(&path, stem_dir.join(format!("{stem}.wav"))).unwrap();
            }
            assert!(track.load_stems_from_dir(&stem_dir));
            (track, estimated_stems)
        };
        let (loud, loud_estimated) = load("loud", 0.5);
        let (quiet, quiet_estimated) = load("quiet", 0.5 / 7.943);

        let db = |g: f32| 20.0 * g.log10();
        assert!((db(quiet.auto_gain) - db(loud.auto_gain) - 18.0).abs() < 0.1, "{} vs {}", quiet.auto_gain, loud.auto_gain);
        // Half a pixel of a 36 px Mix-deck half height.
        fn same_picture<const N: usize>(a: &[[f32; N]], b: &[[f32; N]], what: &str) {
            assert_eq!(a.len(), b.len());
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                assert!(x.iter().zip(y).all(|(x, y)| (x - y).abs() < 0.5 / 36.0), "{what} bucket {i}: {x:?} vs {y:?}");
            }
        }
        same_picture(&loud.played_peaks, &quiet.played_peaks, "3-band");
        same_picture(&loud_estimated, &quiet_estimated, "estimated stems");
        same_picture(&loud.played_stem_peaks, &quiet.played_stem_peaks, "separated stems");

        // Buckets well inside each section, at 60 a second.
        let (loud_part, silent_part, quieter_part) = (30..150, 210..290, 330..450);
        let loudest = |p: &[f32]| p.iter().copied().fold(0.0, f32::max);
        for track in [&loud, &quiet] {
            let bands = &track.played_peaks;
            assert!(silent_part.clone().all(|i| crate::widgets::quantize_dot_rows(loudest(&bands[i]), 7) == 0), "silent break lit");
            assert!(silent_part.clone().all(|i| crate::widgets::quantize_dot_rows(loudest(&track.played_stem_peaks[i]), 3) == 0), "silent stems lit");
            let level = |part: std::ops::Range<usize>| part.clone().map(|i| loudest(&bands[i])).sum::<f32>() / part.len() as f32;
            assert!(level(quieter_part.clone()) < 0.8 * level(loud_part.clone()), "section change flattened");
            assert!(crate::widgets::quantize_dot_rows(level(quieter_part.clone()), 7) >= 1, "quieter section went dark");
        }
        // Unity keeps the analysed picture, which phrases read: the copies differ there.
        assert_eq!(&*quiet.waveform_peaks, &LoadedTrackAudio::compute_waveform_peaks(&quiet.pcm_stereo, sr).0[..]);
        assert!(loud.waveform_peaks[90][0] - quiet.waveform_peaks[90][0] > 0.2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn preview_interleaved_overestimated_header_and_large_gap_and_audio_fixes() {
        // 1. Overestimated header (20 s vs real 5 s) with audio callbacks interleaved between every decode step.
        let (stream, pcm) = ramp_stream(40_000, 0, Some(160_000), true);
        let path = std::env::temp_dir().join(format!("retraktor-interleaved-{}.script", std::process::id()));
        register_test_stream(&path, stream);
        let p = PreviewPcm::new(path, 0.9);
        let mut decode = PreviewDecode::open(Arc::clone(&p), &Default::default()).unwrap();
        let shared = Arc::new(Mutex::new(AudioSharedState::default()));
        shared.lock().unwrap().start_preview(Arc::clone(&p), 0.9);
        let mut r = Renderer::new(Arc::clone(&shared), Arc::default(), Arc::default());
        let mut heard = 0.0f32;
        for _ in 0..80 {
            decode.step();
            let (left, _) = preview_run(&mut r, &shared, &[64], 8_000);
            if heard == 0.0 {
                heard = left.iter().copied().find(|&x| x != 0.0).unwrap_or(0.0);
            }
            if p.done() && heard != 0.0 {
                break;
            }
        }
        assert!(heard != 0.0, "preview died permanently during overestimated-header stepping");
        // `preview_run` routes through `headphone_mix` at default `cue_volume = 0.8`.
        assert!((heard - pcm[36_000 * 2] * 0.8).abs() < 1e-4, "expected 90% of real 40k frames, got {heard}");

        // 2. Packet gap > 1 s (3 s jump) preserves timestamps without injecting extra 1 s silences between later packets.
        let sr = 8_000u32;
        let pkt = |v: f32, n: usize| vec![v; n * 2];
        let gap_stream = TestStream {
            rate: sr,
            header_frames: Some(30_000),
            packets: vec![(0, pkt(0.2, 1_000)), (25_000, pkt(0.5, 1_000)), (26_000, pkt(0.7, 1_000))],
            seekable: true,
        };
        let gap_pcm = decode_script("large-gap", gap_stream, 0.0, 40);
        let all = gap_pcm.assembled().unwrap();
        assert_eq!(all.len(), 27_000 * 2, "contiguous packet after >1s gap must not trigger extra silence");
        assert!((all[25_000 * 2] - 0.5).abs() < 1e-6 && (all[26_000 * 2] - 0.7).abs() < 1e-6);

        // 3. seek_by clamps playhead_sec and graph_sinks ignores 0-channel format entries.
        let mut st = AudioSharedState::default();
        st.loaded_tracks[0] = Some(tone_track(440.0, 48_000, 10.0));
        st.seek_to(0, 1.0);
        st.seek_by(0, -5.0);
        assert_eq!(st.playhead_sec[0], 0.0);
        let dump = serde_json::json!([{
            "type": "PipeWire:Interface:Node",
            "info": {
                "props": { "media.class": "Audio/Sink", "node.name": "k2", "audio.channels": 4 },
                "params": { "EnumFormat": [{ "channels": 0 }] }
            }
        }]);
        let sinks = graph_sinks(&dump);
        assert_eq!(sinks.len(), 1);
        assert_eq!(sinks[0].channels, 4);
    }
}
