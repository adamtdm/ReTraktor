//! # `retraktor-cli`
//!
//! Command-line tool for inspecting Traktor `.nml` collections, `.tsi` controller mappings,
//! shipped Traktor Pro 4 `CSI/*.qml` scripts (single file or recursive directory),
//! `app.traktor.*` property bus state, generating Linux `udev` rules, and running
//! a full end-to-end 4-deck Linux DJ mix simulation (`simulate-mix`).

use retraktor_audio::FourDeckAudioEngine;
use retraktor_core::{
    AdapterKind, CsiDirectoryReport, CsiModuleSummary, CsiWire, CsiWiresGroup, JogwheelMode,
    PropertyBus, ScreenFlavor, SurfaceEvent, TraktorColor,
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
use std::path::Path;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_usage();
        process::exit(0);
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
