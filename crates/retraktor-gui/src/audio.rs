use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use retraktor_audio::FourDeckAudioEngine;
use std::f32::consts::PI;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

/// Real-time audio track loaded into a deck
#[derive(Clone)]
pub struct LoadedTrackAudio {
    pub title: String,
    pub artist: String,
    pub bpm: f64,
    pub key: String,
    pub duration_sec: f64,
    pub pcm_stereo: Vec<f32>, // Interleaved [L, R, L, R, ...] at 48000 Hz
    pub sample_rate: u32,
}

impl LoadedTrackAudio {
    /// Generates high quality electronic music stems / demo tracks for instant playback
    pub fn new_synthetic(title: &str, artist: &str, bpm: f64, key: &str, duration_sec: f64, style: usize) -> Self {
        let sr = 48000;
        let total_frames = (duration_sec * sr as f64) as usize;
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
                    // Kick drum (punchy sub 55Hz dropping)
                    let kick_env = (-beat_pos * 12.0).exp() as f32;
                    let kick_freq = 55.0 + 120.0 * (kick_env as f64);
                    let kick = (t * kick_freq * 2.0 * pi64).sin() as f32 * kick_env * 0.9;

                    // Offbeat open hi-hat
                    let hat_env = if beat_pos > 0.4 && beat_pos < 0.75 {
                        let hp = (beat_pos - 0.5).abs();
                        ((-hp * 18.0).exp() * 0.4) as f32
                    } else {
                        0.0
                    };
                    let noise = (((f * 1103515245 + 12345) & 0x7FFF) as f32 / 32768.0) * 2.0 - 1.0;
                    let hat = noise * hat_env;

                    // Melodic rolling bassline (16th notes)
                    let sixteenth = (beat_pos * 4.0).fract();
                    let bass_env = (-sixteenth * 10.0).exp() as f32;
                    let root_hz = 55.0 * 1.5; // A minor
                    let bass = (t * root_hz * 2.0 * pi64).sin() as f32 * bass_env * 0.5;

                    // Atmospheric synth pad
                    let pad = (t * 220.0 * 2.0 * pi64).sin() as f32 * 0.15;

                    l += kick + hat * 0.8 + bass + pad;
                    r += kick + hat * 0.9 + bass + pad * 1.1;
                }
                // Style 1: Acid Peak Time Techno ("Detroit Dawn")
                1 => {
                    // Fast punchy 909 kick
                    let kick_env = (-beat_pos * 14.0).exp() as f32;
                    let kick = (t * (60.0 + 140.0 * kick_env as f64) * 2.0 * pi64).sin() as f32 * kick_env;

                    // Resonant 303 acid saw sweep
                    let cutoff = 400.0 + 1200.0 * (bar_pos * 2.0 * pi64).sin().abs() as f32;
                    let saw = ((t * 110.0 * 2.0 * pi64).fract() * 2.0 - 1.0) as f32;
                    let acid = saw * (cutoff / 1600.0) * 0.4;

                    // Claps on beats 2 and 4
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
                    let chord = ((t * 261.63 * 2.0 * pi64).sin() + (t * 329.63 * 2.0 * pi64).sin()) as f32 * 0.2;
                    l += kick + chord;
                    r += kick + chord;
                }
            }

            pcm.push(l.clamp(-1.0, 1.0));
            pcm.push(r.clamp(-1.0, 1.0));
        }

        Self {
            title: title.to_string(),
            artist: artist.to_string(),
            bpm,
            key: key.to_string(),
            duration_sec,
            pcm_stereo: pcm,
            sample_rate: sr,
        }
    }
}

/// Thread-safe shared state for UI <-> Audio Engine communication
pub struct AudioSharedState {
    pub engine: FourDeckAudioEngine,
    pub is_playing: [bool; 4],
    pub playhead_sec: [f64; 4],
    pub pitch_ratio: [f64; 4],
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

        // Preload Deck A and Deck B with Traktor Pro 4 signature demo tracks
        initial_state.loaded_tracks[0] = Some(LoadedTrackAudio::new_synthetic(
            "Berlin Midnight (Stem Edit)",
            "Tale of Us & Anyma",
            128.00,
            "4m",
            380.0,
            0,
        ));
        initial_state.loaded_tracks[1] = Some(LoadedTrackAudio::new_synthetic(
            "Detroit Dawn (Peak Time)",
            "Jeff Mills & Robert Hood",
            130.00,
            "5m",
            410.0,
            1,
        ));

        let shared = Arc::new(Mutex::new(initial_state));
        let running = Arc::new(AtomicBool::new(true));

        let host = cpal::default_host();
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
                            cpal::SampleFormat::F32 => device.build_output_stream(
                                config.into(),
                                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                                    Self::render_audio_f32(&shared_clone, data, channels, sr);
                                },
                                err_fn,
                                None,
                            ).ok(),
                            cpal::SampleFormat::I16 => device.build_output_stream(
                                config.into(),
                                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                                    Self::render_audio_i16(&shared_clone, data, channels, sr);
                                },
                                err_fn,
                                None,
                            ).ok(),
                            _ => None,
                        };

                        match stream_opt {
                            Some(s) => {
                                let _ = s.play();
                                Some(s)
                            }
                            None => {
                                eprintln!("[INFO] CPAL hardware stream build unavailable; using internal realtime timeline clock");
                                None
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("[INFO] CPAL default config error: {e}");
                        None
                    }
                }
            }
            None => {
                eprintln!("[INFO] No default audio output device found; using internal realtime timeline clock");
                None
            }
        };

        // Companion high-resolution timeline pump to ensure UI playhead advances even if no hardware sound device
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
                            if let Some(track) = &state.loaded_tracks[d] {
                                if state.playhead_sec[d] >= track.duration_sec {
                                    state.playhead_sec[d] = 0.0;
                                }
                            }
                        }
                    }
                }
            }
        });

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
                if !state.is_playing[d] {
                    continue;
                }
                let mut advance_dt = 0.0_f64;
                let mut reset_playhead = false;
                if let Some(track) = &state.loaded_tracks[d] {
                    let frame_pos = (state.playhead_sec[d] * track.sample_rate as f64) as usize;
                    let idx = frame_pos * 2;
                    if idx + 1 < track.pcm_stereo.len() {
                        deck_samples[d] = (track.pcm_stereo[idx], track.pcm_stereo[idx + 1]);
                    }
                    advance_dt = dt * state.pitch_ratio[d];
                    if state.playhead_sec[d] + advance_dt >= track.duration_sec {
                        reset_playhead = true;
                    }
                }
                if reset_playhead {
                    state.playhead_sec[d] = 0.0;
                } else {
                    state.playhead_sec[d] += advance_dt;
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
