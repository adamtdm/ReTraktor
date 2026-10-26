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
    /// Precomputed 4-stem `[drums, bass, instruments, vocals]` amplitude peaks at `PEAKS_PER_SEC` Hz.
    pub stem_peaks: Vec<[f32; 4]>,
    pub stems_ready: bool,
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

            for f in start_f..end_f {
                let l = pcm_stereo[f * 2];
                let r = pcm_stereo[f * 2 + 1];
                let mid_ch = (l + r) * 0.5;
                let side_ch = (l - r) * 0.5;

                // 1-pole crossover estimation for 3-band & 4-stem decomposition
                lp_state += 0.035 * (mid_ch - lp_state);
                mp_state += 0.22 * (mid_ch - mp_state);
                let low = lp_state.abs();
                let mid = (mp_state - lp_state).abs();
                let high = (mid_ch - mp_state).abs();

                let abs_in = mid_ch.abs();
                fast_env = if abs_in > fast_env {
                    fast_env + 0.35 * (abs_in - fast_env)
                } else {
                    fast_env * 0.992
                };
                slow_env += 0.008 * (abs_in - slow_env);
                let transient = (fast_env - slow_env).max(0.0);

                let drums_val = (transient * 2.2 + high * 0.85).min(1.0);
                let bass_val = (low * 1.35).min(1.0);
                let inst_val = (side_ch.abs() * 1.9 + high * 0.55).min(1.0);
                let vocal_val = ((mid - side_ch.abs() * 0.6).max(0.0) * 1.75).min(1.0);

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
            peaks.push([
                (max_low * 1.3).clamp(0.04, 1.0),
                (max_mid * 1.6).clamp(0.04, 1.0),
                (max_high * 2.0).clamp(0.03, 1.0),
            ]);
            stems.push([
                max_drums.clamp(0.04, 1.0),
                max_bass.clamp(0.04, 1.0),
                max_inst.clamp(0.04, 1.0),
                max_vocals.clamp(0.04, 1.0),
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

        Ok(Self {
            title,
            artist,
            bpm: fallback_bpm,
            key: fallback_key.to_string(),
            duration_sec,
            pcm_stereo,
            sample_rate: sr,
            waveform_peaks,
            stem_peaks,
            stems_ready: true,
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

        let (waveform_peaks, stem_peaks) = Self::compute_waveform_peaks(&pcm, sr);

        Self {
            title: title.to_string(),
            artist: artist.to_string(),
            bpm,
            key: key.to_string(),
            duration_sec: synth_sec,
            pcm_stereo: pcm,
            sample_rate: sr,
            waveform_peaks,
            stem_peaks,
            stems_ready: true,
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

    // Master Mix Recorder (.WAV)
    pub is_recording: bool,
    pub recorded_samples: Vec<i16>,
    pub recording_sample_rate: u32,
}

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
            is_recording: false,
            recorded_samples: Vec::new(),
            recording_sample_rate: 48_000,
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
                        if state.pattern_enabled {
                            let bpm = state.engine.clock.bpm.max(40.0);
                            let step_dur = (60.0 / bpm) / 4.0;
                            state.pattern_phase_sec += dt;
                            state.pattern_current_step =
                                ((state.pattern_phase_sec / step_dur).floor() as usize) % 16;
                        }
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

    /// Writes a slice of interleaved stereo `i16` PCM samples to a standard RIFF `.wav` file on disk.
    pub fn write_stereo_wav_file(
        path: &Path,
        samples: &[i16],
        sample_rate: u32,
    ) -> Result<f64, String> {
        use std::io::Write;
        let mut f = File::create(path)
            .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;
        let data_bytes = (samples.len() * 2) as u32;
        let riff_size = 36u32.saturating_add(data_bytes);
        let channels = 2u16;
        let bits_per_sample = 16u16;
        let byte_rate = sample_rate * (channels as u32) * (bits_per_sample as u32 / 8);
        let block_align = channels * (bits_per_sample / 8);

        f.write_all(b"RIFF").map_err(|e| e.to_string())?;
        f.write_all(&riff_size.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(b"WAVEfmt ").map_err(|e| e.to_string())?;
        f.write_all(&16u32.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&1u16.to_le_bytes()).map_err(|e| e.to_string())?; // PCM
        f.write_all(&channels.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&sample_rate.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&byte_rate.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&block_align.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(&bits_per_sample.to_le_bytes()).map_err(|e| e.to_string())?;
        f.write_all(b"data").map_err(|e| e.to_string())?;
        f.write_all(&data_bytes.to_le_bytes()).map_err(|e| e.to_string())?;

        let mut byte_buf = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            byte_buf.extend_from_slice(&s.to_le_bytes());
        }
        f.write_all(&byte_buf).map_err(|e| e.to_string())?;

        let dur_sec = (samples.len() / 2) as f64 / (sample_rate.max(1) as f64);
        Ok(dur_sec)
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
        state.recording_sample_rate = sr;

        for f in 0..frames {
            let mut deck_samples: [(f32, f32); 4] = [(0.0, 0.0); 4];

            for d in 0..4 {
                state.engine.strips[d].playing = state.is_playing[d];
                if !state.is_playing[d] {
                    continue;
                }
                let mut advance_dt = 0.0_f64;
                let mut track_dur = 120.0_f64;
                let mut raw_l = 0.0_f32;
                let mut raw_r = 0.0_f32;
                let mut has_sample = false;

                if let Some(track) = &state.loaded_tracks[d] {
                    track_dur = track.duration_sec.max(0.1);
                    let frame_pos = (state.playhead_sec[d] * track.sample_rate as f64) as usize;
                    let idx = (frame_pos * 2) % track.pcm_stereo.len().max(2);
                    if idx + 1 < track.pcm_stereo.len() {
                        raw_l = track.pcm_stereo[idx];
                        raw_r = track.pcm_stereo[idx + 1];
                        has_sample = true;
                    }
                    advance_dt = dt * state.pitch_ratio[d];
                }

                if has_sample {
                    // 1. Real-Time 4-Stem Decomposition ([Drums, Bass, Instruments, Vocals])
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

                    let stem_l =
                        drums_l * g_drums + bass_l * g_bass + inst_l * g_inst + vocal_l * g_voc;
                    let stem_r =
                        drums_r * g_drums + bass_r * g_bass + inst_r * g_inst + vocal_r * g_voc;

                    // 2. Channel Gain & 3-Band EQ
                    let gain = (state.channel_gain[d] * 2.0).clamp(0.0, 2.0);
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
                    let eq_scalar = ((l_g + m_g + h_g) / 3.0).clamp(0.0, 2.0);

                    deck_samples[d] = (stem_l * gain * eq_scalar, stem_r * gain * eq_scalar);
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
            let (mut ml, mut mr, _cl, _cr) = state.engine.process_frame(deck_samples);
            ml = (ml + pat_sample).clamp(-1.0, 1.0);
            mr = (mr + pat_sample).clamp(-1.0, 1.0);

            if state.is_recording && state.recorded_samples.len() < (sr as usize) * 2 * 3600 {
                state
                    .recorded_samples
                    .push((ml.clamp(-1.0, 1.0) * 32767.0) as i16);
                state
                    .recorded_samples
                    .push((mr.clamp(-1.0, 1.0) * 32767.0) as i16);
            }

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
