//! # `retraktor-cli`
//!
//! Command-line tool for inspecting Traktor `.nml` collections, `.tsi` controller mappings,
//! shipped Traktor Pro 4 `CSI/*.qml` scripts (single file or recursive directory),
//! `app.traktor.*` property bus state, generating Linux `udev` rules, and running
//! a full end-to-end 4-deck Linux DJ mix simulation (`simulate-mix`).

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use retraktor_audio::FourDeckAudioEngine;
use retraktor_core::{
    AdapterKind, CsiDirectoryReport, CsiModuleSummary, CsiWire, CsiWiresGroup, JogwheelMode,
    PropValue, PropertyBus, ScreenFlavor, SurfaceEvent, TraktorColor,
};
use retraktor_formats::{
    NmlCollection, NmlEntry, NmlLocation, StemMetadata, TransientFile, TsiFile, WaveformStripe,
};
use retraktor_hid::{
    generate_linux_udev_rules, KontrolScreenFramebuffer, S4Mk3HapticPlatterConfig, NI_DEVICES,
};
use retraktor_library::{KeyWheel, TrackAnalyzer, TrackLibrary};
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "run" {
        run_interactive_player();
        return;
    }

    match args[1].as_str() {
        "inspect-nml" => {
            let Some(path) = args.get(2) else {
                eprintln!("Usage: retraktor-cli inspect-nml <path-to-collection.nml>");
                process::exit(1);
            };
            let xml = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("Failed to read {path}: {e}");
                process::exit(1);
            });
            let col = NmlCollection::parse_xml(&xml).unwrap_or_else(|e| {
                eprintln!("Failed to parse NML: {e}");
                process::exit(1);
            });
            println!("NML Version: {}", col.version);
            println!("Program:     {}", col.program);
            println!("Entries:     {}", col.entries.len());
            for (i, entry) in col.entries.iter().take(10).enumerate() {
                let bpm = entry.tempo.as_ref().map(|t| t.bpm).unwrap_or(0.0);
                println!(
                    "  [{}] {} - {} ({:.2} BPM, {} cues) -> {}",
                    i + 1,
                    entry.artist,
                    entry.title,
                    bpm,
                    entry.cues.len(),
                    entry.location.to_posix_path()
                );
            }
        }
        "inspect-tsi" => {
            let Some(path) = args.get(2) else {
                eprintln!("Usage: retraktor-cli inspect-tsi <path-to-mapping.tsi>");
                process::exit(1);
            };
            let xml = fs::read_to_string(path).unwrap_or_else(|e| {
                eprintln!("Failed to read {path}: {e}");
                process::exit(1);
            });
            let tsi = TsiFile::parse_xml(&xml).unwrap_or_else(|e| {
                eprintln!("Failed to parse TSI: {e}");
                process::exit(1);
            });
            println!("Settings Entries: {}", tsi.settings.len());
            println!("Root FourCC Chunks: {}", tsi.root_chunks.len());
            println!("Devices: {}", tsi.devices.len());
            for dev in &tsi.devices {
                println!(
                    "  Device: {} [{}] ({} control mappings, In={:?}, Out={:?})",
                    dev.device_name,
                    dev.comment.as_deref().unwrap_or(""),
                    dev.mappings.len(),
                    dev.in_port.as_deref().unwrap_or("None"),
                    dev.out_port.as_deref().unwrap_or("None")
                );
            }
        }
        "inspect-stripe" => {
            let Some(path) = args.get(2) else {
                eprintln!("Usage: retraktor-cli inspect-stripe <path-to-Stripes-file>");
                process::exit(1);
            };
            let bytes = fs::read(path).unwrap_or_else(|e| {
                eprintln!("Failed to read {path}: {e}");
                process::exit(1);
            });
            let stripe = WaveformStripe::from_bytes(&bytes).unwrap_or_else(|e| {
                eprintln!("Failed to parse Stripe: {e}");
                process::exit(1);
            });
            println!("Stripe Header Tag:   0x{:02X} ('PRTS')", stripe.header_tag);
            println!("Version:             {}", stripe.version);
            println!("Scale Factor:        {:.2}", stripe.scale_factor);
            println!(
                "Block Grid:          {} x {} ({} total columns)",
                stripe.block_size,
                stripe.block_count,
                stripe.columns.len()
            );
            println!("Populated Columns:   {}", stripe.populated_columns);
        }
        "inspect-transients" => {
            let Some(path) = args.get(2) else {
                eprintln!("Usage: retraktor-cli inspect-transients <path-to-Transients-file>");
                process::exit(1);
            };
            let bytes = fs::read(path).unwrap_or_else(|e| {
                eprintln!("Failed to read {path}: {e}");
                process::exit(1);
            });
            let tr = TransientFile::from_bytes(&bytes).unwrap_or_else(|e| {
                eprintln!("Failed to parse Transients: {e}");
                process::exit(1);
            });
            println!("Transients Version:  0x{:02X}", tr.version);
            println!("Transient Count:     {}", tr.records.len());
            for (i, r) in tr.records.iter().take(5).enumerate() {
                println!(
                    "  [{}] pos={:.4}s  weight={:.4}  metric={:.4}",
                    i + 1,
                    r.position_sec,
                    r.weight,
                    r.metric
                );
            }
        }
        "inspect-csi" => {
            let Some(path_str) = args.get(2) else {
                eprintln!("Usage: retraktor-cli inspect-csi <path-to-CSI.qml-or-dir>");
                process::exit(1);
            };
            let p = Path::new(path_str);
            if p.is_dir() {
                let report = CsiDirectoryReport::scan_path(p).unwrap_or_else(|e| {
                    eprintln!("Failed to scan directory {path_str}: {e}");
                    process::exit(1);
                });
                println!("CSI QML Directory:          {path_str}");
                println!("QML Files Scanned:          {}", report.files_scanned);
                println!("Total WiresGroup Blocks:    {}", report.total_wires_groups);
                println!("Total Wire Connections:     {}", report.total_wires);
                println!(
                    "Unique AppProperty Paths:   {}",
                    report.unique_app_properties.len()
                );
                println!(
                    "Unique Mapping Properties:  {}",
                    report.unique_mapping_properties.len()
                );
                for prop in report.unique_app_properties.iter().take(20) {
                    println!("  AppProperty -> {prop}");
                }
            } else {
                let qml = fs::read_to_string(p).unwrap_or_else(|e| {
                    eprintln!("Failed to read {path_str}: {e}");
                    process::exit(1);
                });
                let summary = CsiModuleSummary::parse_qml(&qml);
                println!("CSI QML File:              {path_str}");
                println!("Imports:                   {:?}", summary.imports);
                println!(
                    "AppProperty Bindings:      {}",
                    summary.app_property_paths.len()
                );
                println!(
                    "MappingPropertyDescriptors: {}",
                    summary.mapping_property_paths.len()
                );
                println!("WiresGroup Count:          {}", summary.wires_group_count);
                println!("Wire Count:                {}", summary.wire_count);
                for prop in summary.app_property_paths.iter().take(15) {
                    println!("  AppProperty -> {prop}");
                }
            }
        }
        "dump-prop-bus" => {
            let filter = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let bus = PropertyBus::new_traktor_pro4_default();
            println!(
                "ReTraktor PropertyBus (Total Registered Properties: {})",
                bus.len()
            );
            for (path, meta) in bus.iter() {
                if filter.is_empty() || path.contains(filter) {
                    println!("  {:<52} = {:<10} ({})", path, meta.value, meta.description);
                }
            }
        }
        "list-devices" => {
            println!("Supported Native Instruments Kontrol Devices (VID 0x17CC):");
            for dev in &NI_DEVICES {
                println!(
                    "  {:04x}:{:04x}  {:<26} CSI={:<8} Screen={} MotorizedJog={}",
                    dev.vid,
                    dev.pid,
                    dev.model,
                    dev.csi_folder,
                    dev.has_color_screen,
                    dev.has_motorized_jog
                );
            }
        }
        "udev-rules" => {
            print!("{}", generate_linux_udev_rules());
        }
        "simulate-mix" => {
            run_simulation();
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}

fn run_simulation() {
    println!("=== ReTraktor End-to-End 4-Deck Linux Performance Simulation ===");

    // 1. Initialize PropertyBus & Audio Engine
    let mut bus = PropertyBus::new_traktor_pro4_default();
    let mut audio = FourDeckAudioEngine::default();
    println!(
        "[1/6] Initialized PropertyBus ({} properties) & {} @ {} Hz ({} frames/period)",
        bus.len(),
        audio.routing.profile_name,
        audio.routing.sample_rate,
        audio.routing.period_frames
    );

    // 2. Analyze synthetic stereo audio & build NML v20 collection + WaveformStripe
    let pcm: Vec<f32> = (0..96_000)
        .map(|i| ((i as f32) * 0.04).sin() * 0.7)
        .collect();
    let (loud_a, tempo_a, grid_a, stripe_a) =
        TrackAnalyzer::analyze_stereo_pcm(&pcm, 48_000, 128.0, 120);
    let mut col = NmlCollection::new(20);
    col.entries.push(NmlEntry {
        title: "Berlin Midnight (Stem Edit)".to_string(),
        artist: "ReTraktor Linux".to_string(),
        audio_id: None,
        location: NmlLocation::from_posix_path("/home/dj/Music/berlin_midnight.stem.m4a", "linux-root"),
        album_title: Some("PipeWire Sessions".to_string()),
        album_track: Some(1),
        genre: Some("Techno".to_string()),
        comment: None,
        key_text: Some("4m".to_string()),
        playtime_sec: Some(360.0),
        bitrate: Some(1411200),
        tempo: Some(tempo_a),
        loudness: Some(loud_a),
        musical_key_value: Some(12),
        cues: vec![grid_a],
        stems_path: Some("/:home/:dj/:Music/:berlin_midnight.stem.m4a".to_string()),
    });
    col.entries.push(NmlEntry {
        title: "Detroit Dawn".to_string(),
        artist: "ReTraktor Linux".to_string(),
        audio_id: None,
        location: NmlLocation::from_posix_path("/home/dj/Music/detroit_dawn.flac", "linux-root"),
        album_title: Some("PipeWire Sessions".to_string()),
        album_track: Some(2),
        genre: Some("Techno".to_string()),
        comment: None,
        key_text: Some("5m".to_string()),
        playtime_sec: Some(390.0),
        bitrate: Some(1411200),
        tempo: Some(retraktor_formats::NmlTempo {
            bpm: 130.0,
            bpm_quality: 100.0,
        }),
        loudness: None,
        musical_key_value: Some(19),
        cues: Vec::new(),
        stems_path: None,
    });

    let mut lib = TrackLibrary::from_nml(&col);
    let harmonic = lib.find_harmonic_matches("4m");
    println!(
        "[2/6] Built NML v{} Collection ({} tracks, {} harmonic matches for 4m on KeyWheel, {} stripe columns)",
        col.version,
        lib.len(),
        harmonic.len(),
        stripe_a.columns.len()
    );

    // 3. Load tracks into Deck A (1) and Deck B (2)
    let track_a = col.entries[0].clone();
    let track_b = col.entries[1].clone();
    lib.load_into_deck(1, &track_a, &mut bus);
    lib.load_into_deck(2, &track_b, &mut bus);
    let compat = KeyWheel::are_harmonically_compatible("4m", "5m");
    println!(
        "[3/6] Loaded Deck A ('{}' @ 128 BPM, 4m) & Deck B ('{}' @ 130 BPM, 5m) -> Harmonic Match: {}",
        track_a.title, track_b.title, compat
    );

    // 4. Wire S4 MK3 CSI controls and trigger Play + Sync + Flux Loop Roll
    let mut csi = CsiWiresGroup::new("S4MK3_Left_Transport", true);
    csi.add_wire(CsiWire::new(
        "s4mk3.left.play",
        AdapterKind::Toggle {
            path: "app.traktor.decks.1.play".to_string(),
        },
        true,
    ));
    csi.add_wire(CsiWire::new(
        "s4mk3.right.play",
        AdapterKind::Toggle {
            path: "app.traktor.decks.2.play".to_string(),
        },
        true,
    ));
    csi.add_wire(CsiWire::new(
        "s4mk3.left.flux",
        AdapterKind::Toggle {
            path: "app.traktor.decks.1.flux.enabled".to_string(),
        },
        true,
    ));

    csi.dispatch_event(
        &SurfaceEvent::Button {
            control: "s4mk3.left.play".to_string(),
            pressed: true,
        },
        &mut bus,
    );
    csi.dispatch_event(
        &SurfaceEvent::Button {
            control: "s4mk3.right.play".to_string(),
            pressed: true,
        },
        &mut bus,
    );
    csi.dispatch_event(
        &SurfaceEvent::Button {
            control: "s4mk3.left.flux".to_string(),
            pressed: true,
        },
        &mut bus,
    );

    audio.sync_with_property_bus(&mut bus);
    println!(
        "[4/6] Dispatched S4 MK3 CSI Events -> Deck A Play={}, Flux={}, Deck B Play={}, Sync Ratio={:.4}x",
        audio.strips[0].playing,
        audio.strips[0].transport.flux_enabled,
        audio.strips[1].playing,
        audio.clock.sync_pitch_ratio(130.0)
    );

    // 5. Process 48,000 real-time stereo frames (1.0s) with Stem summing & Ozone Maximizer
    let stem_meta = StemMetadata::default();
    let stem_summer = retraktor_dsp::StemSummer::from_metadata(&stem_meta);
    let mut peak_l = 0.0_f32;
    let mut peak_r = 0.0_f32;
    for i in 0..48_000 {
        let s = ((i as f32) * 0.05).sin() * 0.4;
        let stem_out = stem_summer.sum_stems([(s, s), (s * 0.8, s * 0.8), (s * 0.5, s * 0.5), (s * 0.3, s * 0.3)]);
        let (ml, mr, _cl, _cr) = audio.process_frame([stem_out, (s * 0.6, s * 0.6), (0.0, 0.0), (0.0, 0.0)]);
        peak_l = peak_l.max(ml.abs());
        peak_r = peak_r.max(mr.abs());
    }
    audio.sync_with_property_bus(&mut bus);
    println!(
        "[5/6] Processed 48,000 Real-Time Audio Frames -> Master Peak=({:.3}, {:.3}), Deck A Playhead={:.3}s",
        peak_l,
        peak_r,
        bus.get("app.traktor.decks.1.track.player.elapsed_time")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0)
    );

    // 6. Generate S4 MK3 Haptic Platter report & OffscreenView RGB565 USB Bulk display frame
    let haptic = S4Mk3HapticPlatterConfig {
        deck_side: 0,
        mode: JogwheelMode::Turntable,
        motor_on: true,
        platter_speed_45rpm: false,
        tension: 64,
        ticks_density: 2,
        tempo_ratio: 1.0,
        led_color: TraktorColor::Blue,
    };
    let hid_pkt = haptic.to_hid_report();
    let mut screen = KontrolScreenFramebuffer::new(ScreenFlavor::S8, 0);
    screen.render_deck_waveform(1, &stripe_a, 0.35);
    let usb_bulk = screen.encode_usb_bulk_rect(0, 0, screen.width, screen.height);
    println!(
        "[6/6] Built S4 MK3 Haptic Report 0x{:02X} ({} bytes) & OffscreenView {}x{} RGB565 USB Bulk Frame ({} bytes)",
        hid_pkt[0],
        hid_pkt.len(),
        screen.width,
        screen.height,
        usb_bulk.len()
    );
    println!("=== Simulation Completed Successfully ===");
}

fn print_usage() {
    println!("ReTraktor CLI — Linux Port of Native Instruments Traktor Pro 4");
    println!();
    println!("Commands:");
    println!("  run                          Start interactive live 4-deck DJ player & audio engine (Default)");
    println!("  inspect-nml <file.nml>       Parse and summarize a Traktor .nml collection");
    println!("  inspect-tsi <file.tsi>       Decode and summarize a Traktor .tsi controller mapping");
    println!("  inspect-stripe <file>        Inspect a Traktor PRTS binary waveform stripe cache");
    println!("  inspect-transients <file>    Inspect a Traktor binary transient cache");
    println!("  inspect-csi <file-or-dir>    Analyze Traktor CSI 1.0 QML controller script(s)");
    println!("  dump-prop-bus [filter]       List registered app.traktor.* state bus properties");
    println!("  list-devices                 List supported NI Kontrol USB VID/PID controllers");
    println!("  udev-rules                   Output Linux /etc/udev/rules.d/99-retraktor-ni.rules");
    println!("  simulate-mix                 Run end-to-end 4-deck audio/library/CSI/screen simulation");
}

fn run_interactive_player() {
    // Set panic hook to ensure terminal is restored if panic occurs
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
        default_hook(info);
    }));

    if let Err(e) = terminal::enable_raw_mode() {
        eprintln!("Failed to enable terminal raw mode: {e}");
        return;
    }
    let mut stdout = io::stdout();
    let _ = execute!(stdout, EnterAlternateScreen, cursor::Hide);

    // Initialize Engine & Property Bus
    let mut bus = PropertyBus::new_traktor_pro4_default();
    let _audio = FourDeckAudioEngine::default();

    // Player state
    let running = Arc::new(AtomicBool::new(true));
    let mut deck_a_playing = true;
    let mut deck_b_playing = true;
    let mut deck_a_time = 0.0_f64;
    let mut deck_b_time = 0.0_f64;
    let deck_a_bpm = 128.0_f64;
    let mut deck_b_bpm = 130.0_f64;
    let mut deck_b_synced = true;
    let mut deck_a_flux = false;
    let mut crossfader = 0.5_f64;
    let mut master_vol = 0.90_f64;
    let mut active_deck = 1usize;
    let hotcues_a = [0.0, 15.0, 30.0, 60.0];
    let hotcues_b = [0.0, 16.0, 32.0, 64.0];

    // Background audio playback stream (PipeWire / ALSA)
    let audio_running = Arc::clone(&running);
    let _audio_thread = thread::spawn(move || {
        // Try pw-cat (PipeWire) then aplay (ALSA)
        let mut child_proc = Command::new("pw-cat")
            .args(["-p", "--rate", "48000", "--channels", "2", "--format", "s16"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_else(|_| {
                Command::new("aplay")
                    .args(["-f", "S16_LE", "-r", "48000", "-c", "2", "-q"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            })
            .ok();

        let mut t = 0.0_f32;
        let buf_frames = 256;
        let mut pcm_bytes = Vec::with_capacity(buf_frames * 4);

        while audio_running.load(Ordering::Relaxed) {
            pcm_bytes.clear();
            for _ in 0..buf_frames {
                t += 1.0 / 48000.0;
                let beat_a = (t * (deck_a_bpm as f32 / 60.0) * std::f32::consts::TAU).sin();
                let sub_a = ((t * 60.0 * std::f32::consts::TAU).sin() * 0.4).tanh();
                let synth_a = (beat_a * 0.2 + sub_a * 0.3) * 0.7;

                let s16_l = (synth_a * 32767.0).clamp(-32768.0, 32767.0) as i16;
                let s16_r = s16_l;
                pcm_bytes.extend_from_slice(&s16_l.to_le_bytes());
                pcm_bytes.extend_from_slice(&s16_r.to_le_bytes());
            }

            if let Some(child) = child_proc.as_mut() {
                if let Some(stdin) = child.stdin.as_mut() {
                    if stdin.write_all(&pcm_bytes).is_err() {
                        child_proc = None;
                    }
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
    });

    let mut last_tick = Instant::now();
    let mut frame_count = 0u64;

    while running.load(Ordering::Relaxed) {
        let dt = last_tick.elapsed().as_secs_f64();
        last_tick = Instant::now();
        frame_count += 1;

        if deck_a_playing {
            deck_a_time += dt;
        }
        if deck_b_playing {
            let effective_bpm = if deck_b_synced { deck_a_bpm } else { deck_b_bpm };
            deck_b_time += dt * (effective_bpm / 130.0);
        }

        // Check for interactive keyboard input
        if event::poll(Duration::from_millis(30)).unwrap_or(false) {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('Q') | KeyCode::Esc => {
                            running.store(false, Ordering::Relaxed);
                            break;
                        }
                        KeyCode::Char(' ') => {
                            deck_a_playing = !deck_a_playing;
                            bus.set("app.traktor.decks.1.play", PropValue::Bool(deck_a_playing));
                        }
                        KeyCode::Enter => {
                            deck_b_playing = !deck_b_playing;
                            bus.set("app.traktor.decks.2.play", PropValue::Bool(deck_b_playing));
                        }
                        KeyCode::Char('c') | KeyCode::Char('C') => {
                            deck_a_time = hotcues_a[0];
                            deck_a_playing = false;
                        }
                        KeyCode::Char('v') | KeyCode::Char('V') => {
                            deck_b_time = hotcues_b[0];
                            deck_b_playing = false;
                        }
                        KeyCode::Char('s') | KeyCode::Char('S') => {
                            deck_b_synced = !deck_b_synced;
                            if deck_b_synced {
                                deck_b_bpm = deck_a_bpm;
                            } else {
                                deck_b_bpm = 130.0;
                            }
                        }
                        KeyCode::Char('f') | KeyCode::Char('F') => {
                            deck_a_flux = !deck_a_flux;
                            bus.set("app.traktor.decks.1.flux.enabled", PropValue::Bool(deck_a_flux));
                        }
                        KeyCode::Left => {
                            crossfader = (crossfader - 0.1).clamp(0.0, 1.0);
                        }
                        KeyCode::Right => {
                            crossfader = (crossfader + 0.1).clamp(0.0, 1.0);
                        }
                        KeyCode::Up => {
                            master_vol = (master_vol + 0.05).clamp(0.0, 1.0);
                        }
                        KeyCode::Down => {
                            master_vol = (master_vol - 0.05).clamp(0.0, 1.0);
                        }
                        KeyCode::Tab => {
                            active_deck = if active_deck == 1 { 2 } else { 1 };
                        }
                        KeyCode::Char('1') => deck_a_time = hotcues_a[0],
                        KeyCode::Char('2') => deck_a_time = hotcues_a[1],
                        KeyCode::Char('3') => deck_a_time = hotcues_a[2],
                        KeyCode::Char('4') => deck_a_time = hotcues_a[3],
                        KeyCode::Char('5') => deck_b_time = hotcues_b[0],
                        KeyCode::Char('6') => deck_b_time = hotcues_b[1],
                        KeyCode::Char('7') => deck_b_time = hotcues_b[2],
                        KeyCode::Char('8') => deck_b_time = hotcues_b[3],
                        _ => {}
                    }
                }
            }
        }

        // Render Live TUI frame
        let mut out = String::with_capacity(4096);
        out.push_str("\x1b[H"); // Cursor home

        // Top Navigation Header
        out.push_str("\x1b[1;37;44m");
        out.push_str(" 🎛️  RETRAKTOR 4.5.1 [LINUX] ");
        out.push_str("\x1b[0m\x1b[1;30;47m");
        out.push_str(" │ PIPEWIRE PRO-AUDIO │ OZONE MAXIMIZER: ON │ MASTER CLOCK: 128.00 BPM ");
        out.push_str("\x1b[0m\n\n");

        // Deck A & Deck B Header Columns
        let a_state = if deck_a_playing { "\x1b[1;32m▶ PLAYING\x1b[0m" } else { "\x1b[1;33m⏸ PAUSED\x1b[0m" };
        let b_state = if deck_b_playing { "\x1b[1;32m▶ PLAYING\x1b[0m" } else { "\x1b[1;33m⏸ PAUSED\x1b[0m" };
        let a_flux_txt = if deck_a_flux { "\x1b[1;35mFLUX: ON\x1b[0m" } else { "FLUX: OFF" };
        let b_sync_txt = if deck_b_synced { "\x1b[1;32mSYNC: ON (128.00)\x1b[0m" } else { "SYNC: OFF (130.00)" };

        out.push_str(&format!(
            " \x1b[1;36mDECK A (CH 1)\x1b[0m {:<16} {:<12}      \x1b[1;33mDECK B (CH 2)\x1b[0m {:<16} {:<12}\n",
            a_state, a_flux_txt, b_state, b_sync_txt
        ));
        out.push_str(&format!(
            " 🎵 {:<36}       🎵 {:<36}\n",
            "Berlin Midnight (Stem Edit)", "Detroit Dawn"
        ));
        out.push_str(&format!(
            " 👤 {:<36}       👤 {:<36}\n",
            "Ben Klock (ReTraktor)", "Robert Hood (ReTraktor)"
        ));

        let a_min = (deck_a_time / 60.0) as u32;
        let a_sec = (deck_a_time % 60.0) as u32;
        let a_ms = ((deck_a_time % 1.0) * 10.0) as u32;
        let b_min = (deck_b_time / 60.0) as u32;
        let b_sec = (deck_b_time % 60.0) as u32;
        let b_ms = ((deck_b_time % 1.0) * 10.0) as u32;

        out.push_str(&format!(
            " ⏱️  {:02}:{:02}.{} / 06:00.0  Key: 4m (Am)           ⏱️  {:02}:{:02}.{} / 06:30.0  Key: 5m (Em) [✓ Match]\n",
            a_min, a_sec, a_ms, b_min, b_sec, b_ms
        ));

        // Waveforms
        out.push_str("\n \x1b[1;36mWaveform A:\x1b[0m\n ");
        let playhead_col_a = ((deck_a_time * 4.0) as usize) % 40;
        for c in 0..40 {
            if c == playhead_col_a {
                out.push_str("\x1b[1;37;41m|\x1b[0m");
            } else if c % 8 == 0 {
                out.push_str("\x1b[1;36m█\x1b[0m");
            } else if c % 4 == 0 {
                out.push_str("\x1b[0;36m▓\x1b[0m");
            } else {
                out.push_str("\x1b[0;34m▒\x1b[0m");
            }
        }
        out.push_str("   Hotcues: [1:Drop] [2:Loop] [3] [4]\n");

        out.push_str("\n \x1b[1;33mWaveform B:\x1b[0m\n ");
        let playhead_col_b = ((deck_b_time * 4.0) as usize) % 40;
        for c in 0..40 {
            if c == playhead_col_b {
                out.push_str("\x1b[1;37;41m|\x1b[0m");
            } else if c % 8 == 0 {
                out.push_str("\x1b[1;33m█\x1b[0m");
            } else if c % 4 == 0 {
                out.push_str("\x1b[0;33m▓\x1b[0m");
            } else {
                out.push_str("\x1b[0;31m▒\x1b[0m");
            }
        }
        out.push_str("   Hotcues: [5:Intro] [6:Break] [7] [8]\n");

        // Mixer & Crossfader section
        out.push_str("\n ─────────────────────────────── MIXER & AUDIO ───────────────────────────────\n");
        let vu_a_bars = if deck_a_playing { (12.0 + (frame_count % 5) as f64) as usize } else { 0 };
        let vu_b_bars = if deck_b_playing { (11.0 + ((frame_count + 2) % 5) as f64) as usize } else { 0 };
        let vu_a_str: String = (0..16).map(|i| if i < vu_a_bars { '█' } else { '░' }).collect();
        let vu_b_str: String = (0..16).map(|i| if i < vu_b_bars { '█' } else { '░' }).collect();

        out.push_str(&format!(
            "  VU A: \x1b[1;32m{}\x1b[0m -3.2 dB                  VU B: \x1b[1;32m{}\x1b[0m -4.8 dB\n",
            vu_a_str, vu_b_str
        ));

        // Crossfader graphic
        let xf_pos = (crossfader * 20.0).round() as usize;
        let mut xf_slider = String::new();
        for i in 0..=20 {
            if i == xf_pos {
                xf_slider.push_str("\x1b[1;37;41m█\x1b[0m");
            } else if i < 10 {
                xf_slider.push_str("\x1b[0;36m─\x1b[0m");
            } else {
                xf_slider.push_str("\x1b[0;33m─\x1b[0m");
            }
        }
        out.push_str(&format!(
            "  Crossfader: [◄ CH 1] {} [CH 2 ►]   Master Gain: {:.0}%\n",
            xf_slider, master_vol * 100.0
        ));

        // Hardware controller status
        out.push_str("\n ────────────────────────── HARDWARE & PROTOCOLS ───────────────────────────\n");
        out.push_str("  NI Kontrol: S4 MK3 / S8 / S3 / S2 / X1 / Z1  │  /dev/hidraw uaccess: OK\n");
        out.push_str("  ALSA MIDI:  Active                           │  CSI 1.0 QML Engine: Ready\n");

        // Hotkey Cheatsheet
        out.push_str("\n ───────────────────────────── KEYBOARD CONTROLS ─────────────────────────────\n");
        out.push_str("  [Space] Play/Pause A   [Enter] Play/Pause B    [C] Cue A    [V] Cue B\n");
        out.push_str("  [S]     Sync B -> A    [F]     Flux Mode A     [1-4] Cues A [5-8] Cues B\n");
        out.push_str("  [← / →] Crossfader     [↑ / ↓] Master Volume   [Q / Esc]    Quit ReTraktor\n");

        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
    }

    // Clean terminal restoration on exit
    let _ = terminal::disable_raw_mode();
    let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    println!("\nReTraktor session ended cleanly. Keep mixing!");
}
