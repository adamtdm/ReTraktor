use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use retraktor_audio::FourDeckAudioEngine;
use std::f32::consts::PI;
use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Number of waveform peak columns stored per second of audio for real-time scrolling & stripe rendering.
pub const PEAKS_PER_SEC: usize = 40;

/// Real-time audio track loaded into a deck (either decoded from disk via Symphonia or synthesized).
#[derive(Clone)]
pub struct LoadedTrackAudio {
    pub title: String,
    pub artist: String,
    pub bpm: f64,
    pub key: String,
    pub duration_sec: f64,
    pub pcm_stereo: Vec<f32>, // Interleaved [L, R, L, R, ...]
    pub sample_rate: u32,
    /// Precomputed 3-band `[low, mid, high]` amplitude peaks at `PEAKS_PER_SEC` Hz.
    pub waveform_peaks: Vec<[f32; 3]>,
}

impl LoadedTrackAudio {
    /// Computes 3-band `[low, mid, high]` waveform peaks from interleaved stereo PCM.
    pub fn compute_waveform_peaks(pcm_stereo: &[f32], sample_rate: u32) -> Vec<[f32; 3]> {
        let total_frames = pcm_stereo.len() / 2;
        if total_frames == 0 || sample_rate == 0 {
            return vec![[0.1, 0.1, 0.1]; 120];
        }
        let frames_per_bucket = ((sample_rate as usize) / PEAKS_PER_SEC).max(64);
        let num_buckets = (total_frames / frames_per_bucket).max(1);
        let mut peaks = Vec::with_capacity(num_buckets);

        let mut lp_state = 0.0_f32;
        let mut mp_state = 0.0_f32;

        for b in 0..num_buckets {
            let start_f = b * frames_per_bucket;
            let end_f = ((b + 1) * frames_per_bucket).min(total_frames);
            let mut max_low = 0.0_f32;
            let mut max_mid = 0.0_f32;
            let mut max_high = 0.0_f32;

            for f in start_f..end_f {
                let mono = (pcm_stereo[f * 2] + pcm_stereo[f * 2 + 1]) * 0.5;
                // Simple 1-pole crossover estimation for visual 3-band separation
                lp_state += 0.04 * (mono - lp_state);
                mp_state += 0.22 * (mono - mp_state);
                let low = lp_state.abs();
                let mid = (mp_state - lp_state).abs();
                let high = (mono - mp_state).abs();

                if low > max_low {
                    max_low = low;
                }
                if mid > max_mid {
                    max_mid = mid;
                }
                if high > max_high {
                    max_high = high;
                }
            }
            peaks.push([
                (max_low * 1.3).clamp(0.04, 1.0),
                (max_mid * 1.6).clamp(0.04, 1.0),
                (max_high * 2.0).clamp(0.03, 1.0),
            ]);
        }
        peaks
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
        let waveform_peaks = Self::compute_waveform_peaks(&pcm_stereo, sr);

        Ok(Self {
            title,
            artist,
            bpm: fallback_bpm,
            key: fallback_key.to_string(),
            duration_sec,
            pcm_stereo,
            sample_rate: sr,
            waveform_peaks,
        })
    }

    /// Generates high quality electronic music stems / demo tracks for instant playback.
    pub fn new_synthetic(
        title: &str,
        artist: &str,
        bpm: f64,
        key: &str,
        duration_sec: f64,
        style: usize,
    ) -> Self {
        let sr = 48000;
        // Synthesize a 32-second seamless musical loop and tile metadata to `duration_sec`
        let synth_sec = duration_sec.min(120.0);
        let total_frames = (synth_sec * sr as f64) as usize;
        let mut pcm = Vec::with_capacity(total_frames * 2);

        let beat_period = (sr as f64) * 60.0 / bpm;
        let pi64 = PI as f64;

        for f in 0..total_frames {
            let t = f as f64 / sr as f64;
            let beat_pos = (f as f64 / beat_period).fract();
            let bar_pos = ((f as f64 / beat_period) / 4.0).fract();

            let (mut l, mut r) = (0.0_f32, 0.0_f32);

            match style {
                // Style 0: Melodic Techno / Tale of Us ("Berlin Midnight")
                0 => {
                    let kick_env = (-beat_pos * 12.0).exp() as f32;
                    let kick_freq = 55.0 + 120.0 * (kick_env as f64);
                    let kick = (t * kick_freq * 2.0 * pi64).sin() as f32 * kick_env * 0.9;

                    let hat_env = if beat_pos > 0.4 && beat_pos < 0.75 {
                        let hp = (beat_pos - 0.5).abs();
                        ((-hp * 18.0).exp() * 0.4) as f32
                    } else {
                        0.0
                    };
                    let noise =
                        (((f * 1103515245 + 12345) & 0x7FFF) as f32 / 32768.0) * 2.0 - 1.0;
                    let hat = noise * hat_env;

                    let sixteenth = (beat_pos * 4.0).fract();
                    let bass_env = (-sixteenth * 10.0).exp() as f32;
                    let root_hz = 55.0 * 1.5;
                    let bass = (t * root_hz * 2.0 * pi64).sin() as f32 * bass_env * 0.5;

                    let pad = (t * 220.0 * 2.0 * pi64).sin() as f32 * 0.15;

                    l += kick + hat * 0.8 + bass + pad;
                    r += kick + hat * 0.9 + bass + pad * 1.1;
                }
                // Style 1: Acid Peak Time Techno ("Detroit Dawn")
                1 => {
                    let kick_env = (-beat_pos * 14.0).exp() as f32;
                    let kick = (t * (60.0 + 140.0 * kick_env as f64) * 2.0 * pi64).sin() as f32
                        * kick_env;

                    let cutoff = 400.0 + 1200.0 * (bar_pos * 2.0 * pi64).sin().abs() as f32;
                    let saw = ((t * 110.0 * 2.0 * pi64).fract() * 2.0 - 1.0) as f32;
                    let acid = saw * (cutoff / 1600.0) * 0.4;

                    let beat_idx = (f as f64 / beat_period).floor() as i64 % 4;
                    let clap_env = if (beat_idx == 1 || beat_idx == 3) && beat_pos < 0.25 {
                        (-beat_pos * 20.0).exp() as f32 * 0.5
                    } else {
                        0.0
                    };
                    let noise = (((f * 65537 + 89) & 0x7FFF) as f32 / 32768.0) * 2.0 - 1.0;
                    let clap = noise * clap_env;

                    l += kick * 0.85 + acid + clap;
                    r += kick * 0.85 + acid * 1.05 + clap;
                }
                // Style 2: Deep House / Ibiza Sunset
                _ => {
                    let kick_env = (-beat_pos * 9.0).exp() as f32;
                    let kick = (t * 50.0 * 2.0 * pi64).sin() as f32 * kick_env * 0.8;
                    let chord = ((t * 261.63 * 2.0 * pi64).sin()
                        + (t * 329.63 * 2.0 * pi64).sin()) as f32
                        * 0.2;
                    l += kick + chord;
                    r += kick + chord;
                }
            }

            pcm.push(l.clamp(-1.0, 1.0));
            pcm.push(r.clamp(-1.0, 1.0));
        }

        let waveform_peaks = Self::compute_waveform_peaks(&pcm, sr);

        Self {
            title: title.to_string(),
            artist: artist.to_string(),
            bpm,
            key: key.to_string(),
            duration_sec: synth_sec,
            pcm_stereo: pcm,
            sample_rate: sr,
            waveform_peaks,
        }
    }
}

/// Thread-safe shared state for UI <-> Audio Engine communication.
pub struct AudioSharedState {
    pub engine: FourDeckAudioEngine,
    pub is_playing: [bool; 4],
    pub playhead_sec: [f64; 4],
    pub pitch_ratio: [f64; 4],
    pub loop_active: [bool; 4],
    pub loop_start_sec: [f64; 4],
    pub loop_len_sec: [f64; 4],
    pub eq_kill_hi: [bool; 4],
    pub eq_kill_mid: [bool; 4],
    pub eq_kill_low: [bool; 4],
    pub channel_gain: [f32; 4],
    pub loaded_tracks: [Option<LoadedTrackAudio>; 4],
    pub channel_peaks: [f32; 4],
    pub master_peak_l: f32,
    pub master_peak_r: f32,
    pub cue_enabled: [bool; 4],
    pub crossfader: f32,
    pub crossfader_curve: f32,
}

impl Default for AudioSharedState {
    fn default() -> Self {
        Self {
            engine: FourDeckAudioEngine::default(),
            is_playing: [false, false, false, false],
            playhead_sec: [0.0, 0.0, 0.0, 0.0],
            pitch_ratio: [1.0, 1.0, 1.0, 1.0],
            loop_active: [false, false, false, false],
            loop_start_sec: [0.0, 0.0, 0.0, 0.0],
            loop_len_sec: [8.0, 8.0, 8.0, 8.0],
            eq_kill_hi: [false, false, false, false],
            eq_kill_mid: [false, false, false, false],
            eq_kill_low: [false, false, false, false],
            channel_gain: [0.5, 0.5, 0.5, 0.5],
            loaded_tracks: [None, None, None, None],
            channel_peaks: [0.0, 0.0, 0.0, 0.0],
            master_peak_l: 0.0,
            master_peak_r: 0.0,
            cue_enabled: [false, false, false, false],
            crossfader: 0.5,
            crossfader_curve: 0.5,
        }
    }
}

pub struct AudioSystem {
    pub shared: Arc<Mutex<AudioSharedState>>,
    _stream: Option<cpal::Stream>,
    pub running: Arc<AtomicBool>,
}

impl AudioSystem {
    pub fn new() -> Self {
        let mut initial_state = AudioSharedState::default();

        // Preload Decks A, B, C, D with Traktor Pro 4 demo tracks
        initial_state.loaded_tracks[0] = Some(LoadedTrackAudio::new_synthetic(
            "Berlin Midnight (Stem Edit)",
            "Tale of Us & Anyma",
            128.00,
            "4m",
            120.0,
            0,
        ));
        initial_state.loaded_tracks[1] = Some(LoadedTrackAudio::new_synthetic(
            "Detroit Dawn (Peak Time)",
            "Jeff Mills & Robert Hood",
            130.00,
            "5m",
            120.0,
            1,
        ));
        initial_state.loaded_tracks[2] = Some(LoadedTrackAudio::new_synthetic(
            "Ibiza Sunset (Groove Mix)",
            "Defected Collective",
            124.00,
            "4d",
            90.0,
            2,
        ));
        initial_state.loaded_tracks[3] = Some(LoadedTrackAudio::new_synthetic(
            "Tokyo Neon Expressway",
            "Ken Ishii",
            132.00,
            "3m",
            90.0,
            1,
        ));

        let shared = Arc::new(Mutex::new(initial_state));
        let running = Arc::new(AtomicBool::new(true));

        let host = cpal::default_host();
        let has_hw_stream;
        let stream = match host.default_output_device() {
            Some(device) => {
                let config_res = device.default_output_config();
                match config_res {
                    Ok(config) => {
                        let sample_format = config.sample_format();
                        let channels = config.channels() as usize;
                        let sr = config.sample_rate();

                        let shared_clone = Arc::clone(&shared);
                        let err_fn = |err| eprintln!("Audio stream error: {err}");

                        let stream_opt = match sample_format {
                            cpal::SampleFormat::F32 => device
                                .build_output_stream(
                                    config.into(),
                                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                                        Self::render_audio_f32(&shared_clone, data, channels, sr);
                                    },
                                    err_fn,
                                    None,
                                )
                                .ok(),
                            cpal::SampleFormat::I16 => device
                                .build_output_stream(
                                    config.into(),
                                    move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                        Self::render_audio_i16(&shared_clone, data, channels, sr);
                                    },
                                    err_fn,
                                    None,
                                )
                                .ok(),
                            _ => None,
                        };

                        match stream_opt {
                            Some(s) => {
                                let _ = s.play();
                                has_hw_stream = true;
                                Some(s)
                            }
                            None => {
                                has_hw_stream = false;
                                None
                            }
                        }
                    }
                    Err(_) => {
                        has_hw_stream = false;
                        None
                    }
                }
            }
            None => {
                has_hw_stream = false;
                None
            }
        };

        // Only run software fallback clock if no hardware audio stream is advancing the playhead
        if !has_hw_stream {
            let shared_companion = Arc::clone(&shared);
            let running_companion = Arc::clone(&running);
            thread::spawn(move || {
                let mut last_tick = Instant::now();
                while running_companion.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(16));
                    let dt = last_tick.elapsed().as_secs_f64();
                    last_tick = Instant::now();

                    if let Ok(mut state) = shared_companion.try_lock() {
                        for d in 0..4 {
                            if state.is_playing[d] {
                                let ratio = state.pitch_ratio[d];
                                state.playhead_sec[d] += dt * ratio;
                                if state.loop_active[d] && state.loop_len_sec[d] > 0.05 {
                                    let end = state.loop_start_sec[d] + state.loop_len_sec[d];
                                    if state.playhead_sec[d] >= end {
                                        state.playhead_sec[d] = state.loop_start_sec[d];
                                    }
                                } else if let Some(track) = &state.loaded_tracks[d] {
                                    if state.playhead_sec[d] >= track.duration_sec {
                                        state.playhead_sec[d] = 0.0;
                                    }
                                }
                            }
                        }
                    }
                }
            });
        }

        Self {
            shared,
            _stream: stream,
            running,
        }
    }

    fn render_audio_f32(
        shared: &Arc<Mutex<AudioSharedState>>,
        output: &mut [f32],
        channels: usize,
        sr: u32,
    ) {
        let frames = output.len() / channels;
        let dt = 1.0 / (sr as f64).max(1.0);

        let mut state = match shared.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        for f in 0..frames {
            let mut deck_samples: [(f32, f32); 4] = [(0.0, 0.0); 4];

            for d in 0..4 {
                state.engine.strips[d].playing = state.is_playing[d];
                if !state.is_playing[d] {
                    continue;
                }
                let mut advance_dt = 0.0_f64;
                let mut track_dur = 120.0_f64;

                if let Some(track) = &state.loaded_tracks[d] {
                    track_dur = track.duration_sec.max(0.1);
                    let frame_pos = (state.playhead_sec[d] * track.sample_rate as f64) as usize;
                    let idx = (frame_pos * 2) % track.pcm_stereo.len().max(2);
                    if idx + 1 < track.pcm_stereo.len() {
                        let gain = (state.channel_gain[d] * 2.0).clamp(0.0, 2.0);
                        let eq_scalar = if state.eq_kill_low[d]
                            && state.eq_kill_mid[d]
                            && state.eq_kill_hi[d]
                        {
                            0.0
                        } else {
                            let l_g = if state.eq_kill_low[d] {
                                0.0
                            } else {
                                state.engine.strips[d].eq.low * 2.0
                            };
                            let m_g = if state.eq_kill_mid[d] {
                                0.0
                            } else {
                                state.engine.strips[d].eq.mid * 2.0
                            };
                            let h_g = if state.eq_kill_hi[d] {
                                0.0
                            } else {
                                state.engine.strips[d].eq.high * 2.0
                            };
                            ((l_g + m_g + h_g) / 3.0).clamp(0.0, 2.0)
                        };
                        deck_samples[d] = (
                            track.pcm_stereo[idx] * gain * eq_scalar,
                            track.pcm_stereo[idx + 1] * gain * eq_scalar,
                        );
                    }
                    advance_dt = dt * state.pitch_ratio[d];
                }

                state.playhead_sec[d] += advance_dt;

                if state.loop_active[d] && state.loop_len_sec[d] > 0.05 {
                    let loop_end = state.loop_start_sec[d] + state.loop_len_sec[d];
                    if state.playhead_sec[d] >= loop_end {
                        state.playhead_sec[d] = state.loop_start_sec[d];
                    }
                } else if state.playhead_sec[d] >= track_dur {
                    state.playhead_sec[d] = 0.0;
                }
            }

            state.engine.crossfader = state.crossfader;
            state.engine.crossfader_curve = state.crossfader_curve;
            let (ml, mr, _cl, _cr) = state.engine.process_frame(deck_samples);

            if channels >= 2 {
                output[f * channels] = ml;
                output[f * channels + 1] = mr;
            } else if channels == 1 {
                output[f] = (ml + mr) * 0.5;
            }
        }

        // Update real-time peak meter state
        for d in 0..4 {
            state.channel_peaks[d] = state.engine.strips[d].last_prefader_peak;
        }
        state.master_peak_l = state.engine.last_master_peak.0;
        state.master_peak_r = state.engine.last_master_peak.1;
    }

    fn render_audio_i16(
        shared: &Arc<Mutex<AudioSharedState>>,
        output: &mut [i16],
        channels: usize,
        sr: u32,
    ) {
        let mut float_buf = vec![0.0_f32; output.len()];
        Self::render_audio_f32(shared, &mut float_buf, channels, sr);
        for (i, &f) in float_buf.iter().enumerate() {
            output[i] = (f.clamp(-1.0, 1.0) * 32767.0) as i16;
        }
    }
}

impl Drop for AudioSystem {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
