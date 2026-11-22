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
    to_rekordbox_xml, NmlCollection, NmlEntry, NmlLocation, StemMetadata, TransientFile, TsiFile,
    WaveformStripe,
};
use retraktor_hid::{
    controller_rules, generate_linux_udev_rules, mapping_import, KontrolScreenFramebuffer, S4Mk3HapticPlatterConfig, NI_DEVICES,
};
use retraktor_library::{analyze_mono, KeyWheel, TrackAnalyzer, TrackLibrary};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{self, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

fn launch_gui() -> Result<(), String> {
    // Attempt to launch the native ReTraktor egui desktop binary
    let current_exe = env::current_exe().unwrap_or_default();
    let sibling_gui = current_exe.with_file_name("retraktor-gui");
    let target = if sibling_gui.is_file() {
        sibling_gui
    } else {
        std::path::PathBuf::from("retraktor-gui")
    };

    match Command::new(&target).status() {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Exited with code {status}")),
        Err(e) => Err(format!("Could not spawn {}: {e}", target.display())),
    }
}

fn log_trace(msg: &str) {
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open("/tmp/retraktor.log") {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{now}] {msg}");
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 || args[1] == "run" {
        log_trace("Launching ReTraktor default application");
        // Try native desktop GUI window first
        if let Err(e) = launch_gui() {
            log_trace(&format!(
                "GUI window unavailable ({e}), falling back to interactive terminal player"
            ));
            run_interactive_player();
        }
        return;
    }

    match args[1].as_str() {
        "-h" | "--help" => print_usage(),
        "gui" => {
            if let Err(e) = launch_gui() {
                eprintln!("Failed to launch GUI window: {e}");
                process::exit(1);
            }
        }
        "tui" => {
            run_interactive_player();
        }
        "doctor" | "--doctor" | "diagnose" => {
            run_doctor();
        }
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
        "export-rekordbox" => {
            let Some(in_path) = args.get(2) else {
                eprintln!("Usage: retraktor-cli export-rekordbox <in.nml> <out.xml>");
                process::exit(1);
            };
            let Some(out_path) = args.get(3) else {
                eprintln!("Usage: retraktor-cli export-rekordbox <in.nml> <out.xml>");
                process::exit(1);
            };
            let xml = fs::read_to_string(in_path).unwrap_or_else(|e| {
                eprintln!("Failed to read {in_path}: {e}");
                process::exit(1);
            });
            let col = NmlCollection::parse_xml(&xml).unwrap_or_else(|e| {
                eprintln!("Failed to parse NML: {e}");
                process::exit(1);
            });
            let out_xml = to_rekordbox_xml(&col);
            if let Some(parent) = Path::new(out_path).parent() {
                if !parent.as_os_str().is_empty() {
                    let _ = fs::create_dir_all(parent);
                }
            }
            fs::write(out_path, out_xml).unwrap_or_else(|e| {
                eprintln!("Failed to write {out_path}: {e}");
                process::exit(1);
            });
            println!(
                "Exported {} tracks to rekordbox XML at {}",
                col.entries.len(),
                out_path
            );
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
            println!("Supported Traktor Kontrol & MX2 Devices (VID 0x17CC):");
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
        "mapping-reference" => {
            if args.get(2).map(String::as_str) == Some("--example") {
                print!("{}", controller_rules::EXAMPLE_RULES);
            } else {
                print!("{}", controller_rules::reference_text());
            }
        }
        "validate-mapping" => {
            let path = args.get(2).map(PathBuf::from).unwrap_or_else(controller_rules::default_rules_path);
            process::exit(validate_mapping(&path));
        }
        "import-mapping" => {
            process::exit(import_mapping(&args[2..], &mut io::stdout(), &mut io::stderr()));
        }
        "apply-mapping" => {
            process::exit(apply_mapping(&args[2..], &controller_rules::default_rules_path(), &mut io::stdout(), &mut io::stderr()));
        }
        "simulate-mix" => {
            run_simulation();
        }
        "analyze" => {
            if args.len() < 3 {
                eprintln!("Usage: retraktor-cli analyze <audio-file>...");
                process::exit(1);
            }
            for f in &args[2..] {
                match decode_mono_pcm(Path::new(f)) {
                    Ok((pcm, sr)) => {
                        let a = retraktor_library::analyze_mono(&pcm, sr);
                        println!("{:7.2} BPM  {:>3}  energy {:2}  {:6.1} dB  {f}", a.bpm, a.key_open, a.energy, a.loudness_db);
                    }
                    Err(e) => eprintln!("{f}: {e}"),
                }
            }
        }
        "analyze-bench" => {
            let Some(nml_path) = args.get(2) else {
                eprintln!(
                    "Usage: retraktor-cli analyze-bench <collection.nml> <index-file> [--limit N]"
                );
                process::exit(1);
            };
            let Some(index_path) = args.get(3) else {
                eprintln!(
                    "Usage: retraktor-cli analyze-bench <collection.nml> <index-file> [--limit N]"
                );
                process::exit(1);
            };
            let mut limit = 300usize;
            let mut i = 4;
            while i < args.len() {
                if args[i] == "--limit" {
                    if let Some(val) = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                        limit = val;
                    }
                    i += 2;
                } else if let Some(val) = args[i]
                    .strip_prefix("--limit=")
                    .and_then(|s| s.parse::<usize>().ok())
                {
                    limit = val;
                    i += 1;
                } else {
                    i += 1;
                }
            }
            run_analyze_bench(nml_path, index_path, limit);
        }
        _ => {
            print_usage();
            process::exit(1);
        }
    }
}

/// Checks a controller rules file without hardware. Returns the exit code: 0 valid, 1 not.
fn validate_mapping(path: &Path) -> i32 {
    let text = match controller_rules::read_rules_file(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{}: can't read: {e}", path.display());
            return 1;
        }
    };
    match controller_rules::parse_rules(&text) {
        Ok(set) => {
            println!("{}: OK, {} rules and {} pad lights. Press Reload in Preferences > Controllers to use it.", path.display(), set.rule_count(), set.led_count());
            0
        }
        Err(problems) => {
            for problem in &problems {
                eprintln!("{}: {problem}", path.display());
            }
            eprintln!("{} problem(s). `retraktor-cli mapping-reference` lists the actions and control IDs.", problems.len());
            1
        }
    }
}

const IMPORT_USAGE: &str = "\
Usage: retraktor-cli import-mapping <file> --list
       retraktor-cli import-mapping <file> [--device N] --port PORT [--out DRAFT.json | --out -] [--report REPORT.txt]

Converts a Traktor .tsi (Generic MIDI devices), Mixxx .midi.xml or rekordbox .midi.csv mapping into a draft
controller rules file. Nothing is written unless you name a file, an existing file is never overwritten, and the
active rules file is never changed: review the draft, then add it with `retraktor-cli apply-mapping DRAFT.json`
(several drafts at once, all or nothing) or with Preferences > Controllers > Import.

  --list          List the devices in the file (a Traktor settings file can hold several).
  --device N      Which device to convert (1-based); needed when the file holds more than one.
  --port PORT     The MIDI port the rules are for, as ReTraktor names it: touch a control and read
                  Last control in Preferences > Controllers (midi.<port>.ch1.note36), or use the ALSA
                  card id from /proc/asound/cards in lowercase (XONEK2 is xonek2).
  --out FILE      Write the draft to FILE (must not exist). With --out -, the draft goes to stdout and the
                  report to stderr.
  --report FILE   Also write the coverage report to FILE (must not exist).

The coverage report lists every source row as translated, approximated (what differs), merged into another
row's rule, or unsupported (why). Exit status: 0 a draft was made, 2 nothing could be converted (the report says
why), 1 an error (unreadable file, bad option, or an output file that already exists).
";

/// `retraktor-cli import-mapping`: returns the exit status (0 draft made, 2 nothing convertible, 1 error).
fn import_mapping(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let mut file = None;
    let (mut list, mut device, mut port, mut draft_to, mut report_to) = (false, None, None, None, None);
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--list" => list = true,
            "--device" | "--port" | "--out" | "--report" if value.is_none() => {
                let _ = writeln!(err, "{} needs a value\n\n{IMPORT_USAGE}", args[i]);
                return 1;
            }
            "--device" => {
                device = match value.as_deref().unwrap_or("").parse::<usize>() {
                    Ok(n) if n >= 1 => Some(n),
                    _ => {
                        let _ = writeln!(err, "--device takes a device number from --list (1, 2, ...)");
                        return 1;
                    }
                };
                i += 1;
            }
            "--port" => (port, i) = (value, i + 1),
            "--out" => (draft_to, i) = (value, i + 1),
            "--report" => (report_to, i) = (value, i + 1),
            "-h" | "--help" => {
                let _ = write!(out, "{IMPORT_USAGE}");
                return 0;
            }
            other if other.starts_with('-') => {
                let _ = writeln!(err, "unknown option {other}\n\n{IMPORT_USAGE}");
                return 1;
            }
            other if file.is_none() => file = Some(PathBuf::from(other)),
            other => {
                let _ = writeln!(err, "only one mapping file at a time ({other} is a second one)");
                return 1;
            }
        }
        i += 1;
    }
    let Some(file) = file else {
        let _ = write!(err, "{IMPORT_USAGE}");
        return 1;
    };
    let source = match mapping_import::read_source(&file) {
        Ok(s) => s,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    let devices: String = source.devices.iter().enumerate().map(|(n, d)| format!("  {}  {}  ({})\n", n + 1, d.name, d.detail)).collect();
    if list {
        let _ = write!(out, "{} \"{}\": {} device(s)\n{devices}", source.format, source.title, source.devices.len());
        return 0;
    }
    let device = match device {
        Some(n) => n,
        None if source.devices.len() == 1 => 1,
        None => {
            let _ = write!(err, "{} holds {} devices; choose one with --device N:\n{devices}", file.display(), source.devices.len());
            return 1;
        }
    };
    let Some(port) = port else {
        let _ = writeln!(err, "--port is required: the MIDI port as ReTraktor names it (Last control in Preferences > Controllers shows midi.<port>.…, or the lowercase ALSA card id from /proc/asound/cards). Devices:\n{devices}");
        return 1;
    };
    let conversion = match mapping_import::convert(&source, device, &port) {
        Ok(c) => c,
        Err(e) => {
            let _ = writeln!(err, "{e}");
            return 1;
        }
    };
    let report = conversion.report();
    // Check both output paths before writing either, so a refusal leaves nothing half-written.
    if let (Some(draft), Some(report)) = (draft_to.as_deref().filter(|p| *p != "-"), report_to.as_deref()) {
        if same_target(draft, report) {
            let _ = writeln!(err, "--out and --report name the same file ({draft}); nothing was written");
            return 1;
        }
    }
    for path in [draft_to.as_deref().filter(|p| *p != "-"), report_to.as_deref()].into_iter().flatten() {
        if Path::new(path).exists() {
            let _ = writeln!(err, "{path} already exists; nothing was written (choose a new file name)");
            return 1;
        }
    }
    if let Some(path) = &report_to {
        if let Err(e) = mapping_import::write_new_file(Path::new(path), &report) {
            let _ = writeln!(err, "can't write {path}: {e}");
            return 1;
        }
    }
    let Some(draft) = &conversion.draft else {
        let _ = write!(out, "{report}");
        let _ = writeln!(err, "No draft: {}", conversion.empty_reason.as_deref().unwrap_or("nothing could be converted"));
        return 2;
    };
    match draft_to.as_deref() {
        Some("-") => {
            let _ = write!(out, "{draft}");
            let _ = write!(err, "{report}");
        }
        Some(path) => {
            if let Err(e) = mapping_import::write_new_file(Path::new(path), draft) {
                // The report was created a moment ago by this run; don't leave it behind on its own.
                if let Some(report) = &report_to {
                    let _ = fs::remove_file(report);
                }
                let _ = writeln!(err, "can't write {path}: {e}; nothing was written");
                return 1;
            }
            let _ = write!(out, "{report}\nDraft written to {path}. It is not active until you add it: `retraktor-cli apply-mapping --dry-run {path}` checks it against the rules file, and without --dry-run adds it.\n");
        }
        None => {
            let _ = write!(out, "{report}\nPreview only: add --out FILE to write the draft.\n");
        }
    }
    0
}

/// Whether two output paths (which may not exist yet) name the same file, e.g. `out.txt` and `./x/../out.txt`.
fn same_target(a: &str, b: &str) -> bool {
    let place = |p: &str| {
        let p = Path::new(p);
        let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        (fs::canonicalize(dir).ok(), p.file_name().map(|n| n.to_os_string()))
    };
    a == b || matches!((place(a), place(b)), ((Some(x), n), (Some(y), m)) if x == y && n == m)
}

const APPLY_USAGE: &str = "\
Usage: retraktor-cli apply-mapping [--dry-run] [--json] DRAFT.json [DRAFT.json ...]

Adds draft rules files (from import-mapping, Save draft…, or written by hand) to the rules file the app reads,
~/.config/retraktor/controller-rules.json, like Preferences > Controllers > Import > Add to rules file. The
drafts' rules and pad lights go after the file's own, in the order given. All or nothing: if a draft can't be
read or has problems, or collides with the rules file or an earlier draft (a control that already has rules, a
modifier name already used on the same device, a pad that already has a colour, or a CC read another way),
nothing is written. Nothing is written either if the file changes on disk while this runs. The file it replaces
is kept as controller-rules.json.before-import-<time>. The app reads the rules file at startup and on Reload in
Preferences > Controllers.

  --dry-run   Check and count everything; write nothing.
  --json      Print one JSON object on stdout with outcome (applied, preview, refused or usage), dry_run,
              path, drafts, counts (rules_before, leds_before, rules_added, leds_added, rules_after,
              leds_after; null unless applied or previewed), backup (path or null) and diagnostics (a list of
              {draft, message}; draft is null for the rules file itself).

Exit status: 0 applied (with --dry-run: would apply), 1 refused or failed (the messages say why), 2 bad usage.
";

/// `retraktor-cli apply-mapping` against `rules_path`: merges every draft in order in memory, then replaces the
/// rules file once, only if it still holds what was read at the start. Returns the exit status (see APPLY_USAGE).
fn apply_mapping(args: &[String], rules_path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        let _ = write!(out, "{APPLY_USAGE}");
        return 0;
    }
    let json = args.iter().any(|a| a == "--json");
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let drafts: Vec<&str> = args.iter().map(String::as_str).filter(|a| !matches!(*a, "--json" | "--dry-run")).collect();
    let finish = |out: &mut dyn Write, err: &mut dyn Write, outcome: &str, counts: Option<[usize; 4]>, backup: Option<PathBuf>, diagnostics: Vec<(Option<&str>, String)>| {
        if json {
            let counts = counts.map(|[rules_before, leds_before, rules_after, leds_after]| {
                serde_json::json!({"rules_before": rules_before, "leds_before": leds_before, "rules_added": rules_after - rules_before,
                    "leds_added": leds_after - leds_before, "rules_after": rules_after, "leds_after": leds_after})
            });
            let diagnostics: Vec<_> = diagnostics.iter().map(|(draft, message)| serde_json::json!({"draft": draft, "message": message})).collect();
            let _ = writeln!(out, "{}", serde_json::json!({"outcome": outcome, "dry_run": dry_run, "path": rules_path.display().to_string(), "drafts": drafts,
                "counts": counts, "backup": backup.map(|b| b.display().to_string()), "diagnostics": diagnostics}));
        } else {
            for (draft, message) in &diagnostics {
                let _ = writeln!(err, "{}{message}", draft.map(|d| format!("{d}: ")).unwrap_or_default());
            }
            if outcome == "usage" {
                let _ = write!(err, "\n{APPLY_USAGE}");
            }
            if let Some([rules_before, leds_before, rules_after, leds_after]) = counts {
                let path = rules_path.display();
                let added = format!("{} rules and {} pad lights from {} draft(s)", rules_after - rules_before, leds_after - leds_before, drafts.len());
                let _ = match outcome {
                    "preview" => writeln!(out, "Dry run: would add {added} to {path}, giving {rules_after} rules and {leds_after} pad lights. Nothing was written."),
                    _ => writeln!(out, "Added {added} to {path}, now {rules_after} rules and {leds_after} pad lights.{} Press Reload in Preferences > Controllers (or restart ReTraktor) to use them.",
                        backup.map(|b| format!(" The previous file is kept as {}.", b.display())).unwrap_or_default()),
                };
            }
        }
        match outcome {
            "applied" | "preview" => 0,
            "usage" => 2,
            _ => 1,
        }
    };
    if let Some(option) = drafts.iter().find(|a| a.starts_with('-')) {
        return finish(out, err, "usage", None, None, vec![(None, format!("unknown option {option}"))]);
    }
    if drafts.is_empty() {
        return finish(out, err, "usage", None, None, vec![(None, "name at least one draft file".into())]);
    }
    let snapshot = match controller_rules::read_rules_file(rules_path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return finish(out, err, "refused", None, None, vec![(None, format!("can't read {}: {e}; nothing was written", rules_path.display()))]),
    };
    let count = |set: &controller_rules::RuleSet| (set.rule_count(), set.led_count());
    let (rules_before, leds_before) = match snapshot.as_deref().map(controller_rules::parse_rules) {
        None => (0, 0),
        Some(Ok(set)) => count(&set),
        Some(Err(problems)) => {
            let path = rules_path.display();
            let mut diagnostics: Vec<_> = problems.iter().map(|p| (None, format!("{path}: {p}"))).collect();
            diagnostics.push((None, format!("{path} has problems; nothing was written. Fix them (retraktor-cli validate-mapping lists them), then run again.")));
            return finish(out, err, "refused", None, None, diagnostics);
        }
    };
    let mut merged = snapshot.clone();
    for (n, draft) in drafts.iter().enumerate() {
        let text = match controller_rules::read_rules_file(Path::new(draft)) {
            Ok(text) => text,
            Err(e) => return finish(out, err, "refused", None, None, vec![(Some(draft), format!("can't read: {e}")), (None, format!("nothing was written to {}", rules_path.display()))]),
        };
        match mapping_import::merge_draft(merged.as_deref(), &text) {
            Ok(text) => merged = Some(text),
            Err(problems) => {
                let mut diagnostics: Vec<_> = problems.into_iter().map(|p| (Some(*draft), p)).collect();
                let earlier = if n > 0 { " (here \"the rules file\" includes the drafts named before this one)" } else { "" };
                diagnostics.push((None, format!("nothing was written to {}{earlier}. A draft that is already in the rules file is refused like this, and then there is nothing to do; otherwise fix the draft or remove the colliding entries, then run again.", rules_path.display())));
                return finish(out, err, "refused", None, None, diagnostics);
            }
        }
    }
    let merged = merged.expect("at least one draft was merged");
    let (rules_after, leds_after) = controller_rules::parse_rules(&merged).map_or((0, 0), |set| count(&set));
    let counts = Some([rules_before, leds_before, rules_after, leds_after]);
    if dry_run {
        return finish(out, err, "preview", counts, None, Vec::new());
    }
    match mapping_import::replace_rules_file(rules_path, snapshot.as_deref(), &merged) {
        Ok(backup) => finish(out, err, "applied", counts, backup, Vec::new()),
        Err(e) => finish(out, err, "refused", None, None, vec![(None, e)]),
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
        ..Default::default()
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
        ..Default::default()
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
    println!("ReTraktor CLI — Linux Port of Traktor Pro 4");
    println!();
    println!("Commands:");
    println!("  run                          Start interactive live 4-deck DJ player & audio engine (Default)");
    println!("  doctor                       Run system diagnostics, check audio, udev & permissions");
    println!("  inspect-nml <file.nml>       Parse and summarize a Traktor .nml collection");
    println!("  export-rekordbox <in> <out>  Export Traktor .nml collection to rekordbox DJ_PLAYLISTS XML");
    println!("  inspect-tsi <file.tsi>       Decode and summarize a Traktor .tsi controller mapping");
    println!("  inspect-stripe <file>        Inspect a Traktor PRTS binary waveform stripe cache");
    println!("  inspect-transients <file>    Inspect a Traktor binary transient cache");
    println!("  inspect-csi <file-or-dir>    Analyze Traktor CSI 1.0 QML controller script(s)");
    println!("  dump-prop-bus [filter]       List registered app.traktor.* state bus properties");
    println!("  list-devices                 List supported NI Kontrol USB VID/PID controllers");
    println!("  udev-rules                   Output Linux /etc/udev/rules.d/99-retraktor-ni.rules");
    println!("  mapping-reference [--example] Controller rules grammar, actions and control IDs (or just the example file)");
    println!("  validate-mapping [file]      Check a controller rules file (default ~/.config/retraktor/controller-rules.json)");
    println!("  import-mapping <file> ...    Convert a Traktor .tsi, Mixxx .midi.xml or rekordbox .midi.csv mapping into a");
    println!("                               draft rules file with a coverage report (run it without options for help)");
    println!("  apply-mapping [--dry-run] [--json] <draft.json>...");
    println!("                               Add draft rules files to the rules file the app reads, all or nothing (--help)");
    println!("  simulate-mix                 Run end-to-end 4-deck audio/library/CSI/screen simulation");
    println!("  analyze-bench <nml> <index>  Benchmark offline BPM/Key analysis against Traktor NML");
}

fn run_interactive_player() {
    log_trace("Starting ReTraktor live interactive player");

    // Set panic hook to ensure terminal is restored if panic occurs
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, cursor::Show);
        default_hook(info);
    }));

    let is_raw = match terminal::enable_raw_mode() {
        Ok(()) => true,
        Err(e) => {
            log_trace(&format!("Note: Raw terminal mode disabled ({e}), continuing"));
            false
        }
    };
    let mut stdout = io::stdout();
    if is_raw {
        let _ = execute!(stdout, EnterAlternateScreen, cursor::Hide);
    }

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

        // Top Navigation Header (Fits 76 columns)
        out.push_str("\x1b[1;37;44m");
        out.push_str(" 🎛️  RETRAKTOR 4.5.1 [LINUX] ");
        out.push_str("\x1b[0m\x1b[1;30;47m");
        out.push_str(" │ PIPEWIRE 48kHz (128q) │ OZONE: ON │ CLOCK: 128 BPM ");
        out.push_str("\x1b[0m\n\n");

        let a_state = if deck_a_playing { "\x1b[1;32m▶ PLAY\x1b[0m" } else { "\x1b[1;33m⏸ PAUSE\x1b[0m" };
        let b_state = if deck_b_playing { "\x1b[1;32m▶ PLAY\x1b[0m" } else { "\x1b[1;33m⏸ PAUSE\x1b[0m" };
        let a_flux = if deck_a_flux { "\x1b[1;35mFLUX\x1b[0m" } else { "    " };
        let b_sync = if deck_b_synced { "\x1b[1;32mSYNC\x1b[0m" } else { "    " };

        let a_min = (deck_a_time / 60.0) as u32;
        let a_sec = (deck_a_time % 60.0) as u32;
        let b_min = (deck_b_time / 60.0) as u32;
        let b_sec = (deck_b_time % 60.0) as u32;

        out.push_str(&format!(
            " \x1b[1;36mDECK A\x1b[0m {} {} {:02}:{:02} 128BPM 4m │ \x1b[1;33mDECK B\x1b[0m {} {} {:02}:{:02} 130BPM 5m [✓]\n",
            a_state, a_flux, a_min, a_sec, b_state, b_sync, b_min, b_sec
        ));
        out.push_str(" 🎵 Berlin Midnight (Stem Edit)      │ 🎵 Detroit Dawn                    \n");
        out.push_str(" 👤 Ben Klock (ReTraktor)            │ 👤 Robert Hood (ReTraktor)         \n");

        // Waveforms
        out.push_str("\n \x1b[1;36mWaveform A:\x1b[0m\n ");
        let playhead_col_a = ((deck_a_time * 4.0) as usize) % 36;
        for c in 0..36 {
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
        out.push_str("  Cues: [1:Drop] [2:Loop] [3] [4]\n");

        out.push_str("\n \x1b[1;33mWaveform B:\x1b[0m\n ");
        let playhead_col_b = ((deck_b_time * 4.0) as usize) % 36;
        for c in 0..36 {
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
        out.push_str("  Cues: [5:Intro] [6:Break] [7] [8]\n");

        // Mixer & Crossfader section (Within 76 columns)
        out.push_str("\n ────────────────────────── MIXER & HARDWARE ───────────────────────────\n");
        let vu_a_bars = if deck_a_playing { (8.0 + (frame_count % 3) as f64) as usize } else { 0 };
        let vu_b_bars = if deck_b_playing { (7.0 + ((frame_count + 1) % 3) as f64) as usize } else { 0 };
        let vu_a_str: String = (0..10).map(|i| if i < vu_a_bars { '█' } else { '░' }).collect();
        let vu_b_str: String = (0..10).map(|i| if i < vu_b_bars { '█' } else { '░' }).collect();

        // Crossfader graphic
        let xf_pos = (crossfader * 16.0).round() as usize;
        let mut xf_slider = String::new();
        for i in 0..=16 {
            if i == xf_pos {
                xf_slider.push_str("\x1b[1;37;41m█\x1b[0m");
            } else if i < 8 {
                xf_slider.push_str("\x1b[0;36m─\x1b[0m");
            } else {
                xf_slider.push_str("\x1b[0;33m─\x1b[0m");
            }
        }

        out.push_str(&format!(
            " VU A: \x1b[1;32m{}\x1b[0m -3dB │ XF: [◄] {} [►] │ VU B: \x1b[1;32m{}\x1b[0m -5dB\n",
            vu_a_str, xf_slider, vu_b_str
        ));
        out.push_str(" NI Kontrol: S4 MK3 / S8 / S3 / S2 / X1 / Z1 │ ALSA MIDI: Ready         \n");

        // Keyboard Controls
        out.push_str("\n ────────────────────────── KEYBOARD CONTROLS ──────────────────────────\n");
        out.push_str(" [Space] Play/Pause A   [Enter] Play/Pause B    [C/V] Cue A/B  [S] Sync \n");
        out.push_str(" [F]     Flux Mode A    [1-4]   Cues A (1..4)   [5-8] Cues B   [Q] Quit \n");
        out.push_str(" [←/→]   Crossfader     [↑/↓]   Master Volume   [Tab] Switch Active Deck\n");

        let _ = stdout.write_all(out.as_bytes());
        let _ = stdout.flush();
    }

    // Clean terminal restoration on exit
    if is_raw {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(stdout, LeaveAlternateScreen, cursor::Show);
    }
    log_trace("ReTraktor interactive session ended");
    println!("\nReTraktor session ended cleanly. Keep mixing!");
}

fn run_doctor() {
    println!("=== ReTraktor System Diagnostics & Troubleshooting (Doctor) ===\n");
    // 1. OS & Kernel
    println!("1. System Information:");
    println!("   OS:           {}", std::env::consts::OS);
    println!("   Architecture: {}", std::env::consts::ARCH);
    if let Ok(os_rel) = fs::read_to_string("/etc/os-release") {
        for line in os_rel.lines() {
            if line.starts_with("PRETTY_NAME=") {
                println!(
                    "   Distribution: {}",
                    line.trim_start_matches("PRETTY_NAME=").trim_matches('"')
                );
            }
        }
    }
    println!(
        "   Display:      {}",
        env::var("WAYLAND_DISPLAY")
            .or_else(|_| env::var("DISPLAY"))
            .unwrap_or_else(|_| "None".to_string())
    );
    println!(
        "   Terminal:     {}",
        env::var("TERM").unwrap_or_else(|_| "None".to_string())
    );

    // 2. Audio Subsystems
    println!("\n2. Audio Subsystems:");
    let has_pw = Command::new("pw-cat").arg("--version").output().is_ok();
    let has_aplay = Command::new("aplay").arg("--version").output().is_ok();
    println!(
        "   PipeWire (pw-cat): {}",
        if has_pw { "Found (Available)" } else { "Not installed (Optional)" }
    );
    println!(
        "   ALSA (aplay):     {}",
        if has_aplay { "Found (Available)" } else { "Not installed (Optional)" }
    );
    if !has_pw && !has_aplay {
        println!("   [!] Note: Neither pw-cat nor aplay was found. Audio stream will run internally.");
    }

    // 3. Permissions & Groups
    println!("\n3. User Permissions & Groups:");
    if let Ok(out) = Command::new("groups").output() {
        let grps = String::from_utf8_lossy(&out.stdout);
        let in_audio = grps.contains("audio");
        let in_plugdev = grps.contains("plugdev");
        println!("   User groups:     {}", grps.trim());
        println!(
            "   'audio' group:   {}",
            if in_audio { "OK (Member)" } else { "MISSING (Run: sudo usermod -aG audio $USER)" }
        );
        println!(
            "   'plugdev' group: {}",
            if in_plugdev { "OK (Member)" } else { "MISSING (Run: sudo usermod -aG plugdev $USER)" }
        );
    }

    // 4. Traktor Hardware & udev
    println!("\n4. Traktor Hardware & udev:");
    let udev_path = Path::new("/etc/udev/rules.d/99-retraktor-ni.rules");
    println!(
        "   udev rules:      {}",
        if udev_path.exists() {
            "Installed (/etc/udev/rules.d/99-retraktor-ni.rules)"
        } else {
            "MISSING (Run: retraktor-cli udev-rules | sudo tee /etc/udev/rules.d/99-retraktor-ni.rules)"
        }
    );
    let mut detected_controllers = 0;
    if let Ok(entries) = fs::read_dir("/sys/bus/usb/devices") {
        for entry in entries.flatten() {
            let id_vendor = fs::read_to_string(entry.path().join("idVendor")).unwrap_or_default();
            let id_product = fs::read_to_string(entry.path().join("idProduct")).unwrap_or_default();
            for dev in &NI_DEVICES {
                if id_vendor.trim() == format!("{:04x}", dev.vid)
                    && id_product.trim() == format!("{:04x}", dev.pid)
                {
                    println!("   [CONNECTED] {} ({:04x}:{:04x})", dev.model, dev.vid, dev.pid);
                    detected_controllers += 1;
                }
            }
        }
    }
    if detected_controllers == 0 {
        println!("   No NI Kontrol hardware currently connected via USB.");
    }

    // 5. Recent Logs
    println!("\n5. Log file (/tmp/retraktor.log):");
    if let Ok(log) = fs::read_to_string("/tmp/retraktor.log") {
        let lines: Vec<&str> = log.lines().collect();
        for line in lines.iter().rev().take(10).rev() {
            println!("   {}", line);
        }
    } else {
        println!("   No logs at /tmp/retraktor.log yet.");
    }
    println!("\n=== Diagnostics complete ===");
}

fn strip_sync_conflict_and_dupes(name: &str) -> String {
    let mut s = name.to_string();
    while let Some(pos) = s.find(".sync-conflict-") {
        let rest = &s[pos + ".sync-conflict-".len()..];
        if let Some(dot) = rest.find('.') {
            s = format!("{}{}", &s[..pos], &rest[dot..]);
        } else {
            s.truncate(pos);
            break;
        }
    }
    let (stem, ext) = match s.rfind('.') {
        Some(idx) => (&s[..idx], &s[idx..]),
        None => (s.as_str(), ""),
    };
    let mut clean_stem = stem.trim().to_string();
    for suffix in [" - {genre}", " - {Genre}", "_1", "(1)", "(2)", "(3)"] {
        if let Some(stripped) = clean_stem.strip_suffix(suffix) {
            clean_stem = stripped.trim().to_string();
        }
    }
    format!("{}{}", clean_stem.to_ascii_lowercase(), ext.to_ascii_lowercase())
}

fn score_candidate(entry: &NmlEntry, path: &Path) -> i32 {
    let p_low = path.to_string_lossy().to_ascii_lowercase();
    let mut score = 0;
    let artist = entry.artist.to_ascii_lowercase();
    if artist.len() >= 3 && p_low.contains(&artist) {
        score += 10;
    }
    let title = entry.title.to_ascii_lowercase();
    if title.len() >= 3 && p_low.contains(&title) {
        score += 10;
    }
    if let Some(alb) = &entry.album_title {
        let a_low = alb.to_ascii_lowercase();
        if a_low.len() >= 3 && p_low.contains(&a_low) {
            score += 6;
        }
    }
    for seg in entry.location.dir.split("/:") {
        let s = seg.trim().to_ascii_lowercase();
        if s.len() >= 4
            && s != "users"
            && s != "meekp"
            && s != "music"
            && s != "music-dj"
            && s != "audio"
            && s != "audio tracks"
            && p_low.contains(&s)
        {
            score += 4;
        }
    }
    // Slight preference against /dupes/ if a primary folder copy exists
    if p_low.contains("/dupes/") {
        score -= 1;
    }
    score
}

fn decode_mono_pcm(path: &Path) -> Result<(Vec<f32>, u32), String> {
    let file = File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?;
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
        .map_err(|e| format!("probe {}: {e}", path.display()))?;

    let track = probed
        .format
        .default_track()
        .ok_or_else(|| "no default audio track".to_string())?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let sr = codec_params.sample_rate.unwrap_or(44_100);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("codec {}: {e}", path.display()))?;

    let mut mono = Vec::new();
    // Cap decoded length at 10 minutes per track
    let max_frames = (sr as usize) * 600;

    while let Ok(packet) = probed.format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        if let Ok(decoded) = decoder.decode(&packet) {
            let spec = *decoded.spec();
            let ch = spec.channels.count().max(1);
            let mut buf = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
            buf.copy_interleaved_ref(decoded);
            let samples = buf.samples();
            if ch == 1 {
                mono.extend_from_slice(samples);
            } else {
                for frame in samples.chunks(ch) {
                    mono.push(0.5 * (frame[0] + frame[1]));
                }
            }
            if mono.len() >= max_frames {
                break;
            }
        }
    }

    if mono.is_empty() {
        return Err("decoded 0 samples".to_string());
    }
    Ok((mono, sr))
}

fn run_analyze_bench(nml_path: &str, index_path: &str, limit: usize) {
    let xml = fs::read_to_string(nml_path).unwrap_or_else(|e| {
        eprintln!("Failed to read NML {nml_path}: {e}");
        process::exit(1);
    });
    let col = NmlCollection::parse_xml(&xml).unwrap_or_else(|e| {
        eprintln!("Failed to parse NML {nml_path}: {e}");
        process::exit(1);
    });

    let index_text = fs::read_to_string(index_path).unwrap_or_else(|e| {
        eprintln!("Failed to read index {index_path}: {e}");
        process::exit(1);
    });

    let music_root = Path::new("/DATA/Media/Music");
    let mut by_exact: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut by_clean: HashMap<String, Vec<PathBuf>> = HashMap::new();

    for line in index_text.lines() {
        let rel = line.trim();
        if rel.is_empty() {
            continue;
        }
        let rel_stripped = rel.strip_prefix("./").unwrap_or(rel);
        let full = if Path::new(rel_stripped).is_absolute() {
            PathBuf::from(rel_stripped)
        } else {
            music_root.join(rel_stripped)
        };
        if let Some(fname) = full.file_name().and_then(|n| n.to_str()) {
            by_exact
                .entry(fname.to_string())
                .or_default()
                .push(full.clone());
            let clean = strip_sync_conflict_and_dupes(fname);
            by_clean.entry(clean).or_default().push(full);
        }
    }

    let resolve_entry = |entry: &NmlEntry| -> Option<PathBuf> {
        // Skip entries where NML filename clearly disagrees with entry title (corrupted duplicate NML rows)
        let f = &entry.location.file;
        let f_low = f.to_ascii_lowercase();
        let t_low = entry.title.to_ascii_lowercase();
        let first_word = t_low
            .split(|c: char| !c.is_alphanumeric())
            .find(|w| w.len() >= 4);
        if let Some(w) = first_word {
            if !f_low.contains(w) {
                return None;
            }
        }
        if let Some(cands) = by_exact.get(f) {
            return cands
                .iter()
                .max_by_key(|p| score_candidate(entry, p))
                .cloned();
        }
        let clean = strip_sync_conflict_and_dupes(f);
        by_clean.get(&clean).and_then(|cands| {
            cands
                .iter()
                .max_by_key(|p| score_candidate(entry, p))
                .cloned()
        })
    };

    // Collect entries that have ground-truth BPM and/or MUSICAL_KEY and resolve to a real file.
    // Prioritize entries that have BOTH BPM and Key so Key metrics are evaluated across up to 300 tracks,
    // then fill any remaining slots from BPM-only entries.
    let mut both_pool: Vec<(&NmlEntry, PathBuf)> = Vec::new();
    let mut single_pool: Vec<(&NmlEntry, PathBuf)> = Vec::new();
    for entry in &col.entries {
        let has_bpm = entry.tempo.as_ref().map(|t| t.bpm > 0.0).unwrap_or(false);
        let has_key = entry.musical_key_value.is_some();
        if !has_bpm && !has_key {
            continue;
        }
        let f = &entry.location.file;
        let resolved = if let Some(cands) = by_exact.get(f) {
            cands
                .iter()
                .max_by_key(|p| score_candidate(entry, p))
                .cloned()
        } else {
            resolve_entry(entry)
        };
        if let Some(path) = resolved {
            if has_bpm && has_key {
                both_pool.push((entry, path));
            } else {
                single_pool.push((entry, path));
            }
        }
    }

    let total_avail = both_pool.len() + single_pool.len();
    let target_count = limit.min(total_avail).max(1);
    let mut selected = Vec::with_capacity(target_count);
    let take_both = target_count.min(both_pool.len());
    for i in 0..take_both {
        let idx = (i * both_pool.len()) / take_both;
        selected.push(both_pool[idx].clone());
    }
    let rem = target_count.saturating_sub(take_both);
    if rem > 0 && !single_pool.is_empty() {
        for i in 0..rem {
            let idx = (i * single_pool.len()) / rem;
            selected.push(single_pool[idx].clone());
        }
    }

    let mut bpm_total = 0usize;
    let mut bpm_exact = 0usize;
    let mut bpm_octave = 0usize;

    let mut key_total = 0usize;
    let mut key_exact = 0usize;
    let mut key_compat = 0usize;
    let mut key_rel_err = 0usize;
    let mut key_fifth_err = 0usize;

    let mut total_analysis_sec = 0.0_f64;
    let mut analyzed_tracks = 0usize;

    let verbose = env::var("RETRAKTOR_BENCH_VERBOSE").is_ok();

    for (entry, path) in &selected {
        let Ok((mono, sr)) = decode_mono_pcm(path) else {
            continue;
        };
        let t0 = Instant::now();
        let res = analyze_mono(&mono, sr);
        total_analysis_sec += t0.elapsed().as_secs_f64();
        analyzed_tracks += 1;

        if let Some(gt_bpm) = entry.tempo.as_ref().map(|t| t.bpm).filter(|&b| b > 0.0) {
            bpm_total += 1;
            let diff_exact = (res.bpm - gt_bpm).abs();
            let is_exact = diff_exact <= 0.5;
            let is_oct = is_exact
                || (res.bpm * 2.0 - gt_bpm).abs() <= 0.5
                || (res.bpm * 0.5 - gt_bpm).abs() <= 0.5
                || (res.bpm - gt_bpm * 2.0).abs() <= 0.5
                || (res.bpm - gt_bpm * 0.5).abs() <= 0.5;
            if is_exact {
                bpm_exact += 1;
            }
            if is_oct {
                bpm_octave += 1;
            } else if verbose {
                eprintln!(
                    "[BPM MISS] pred={:.2} gt={:.2} | {} - {} ({})",
                    res.bpm,
                    gt_bpm,
                    entry.artist,
                    entry.title,
                    path.display()
                );
            }
        }

        if let Some(gt_key) = entry.musical_key_value {
            if let Some(gt_open) = KeyWheel::to_open_key(gt_key) {
                key_total += 1;
                let pred_open = res.key_open.as_str();
                if res.key_value == gt_key {
                    key_exact += 1;
                    key_compat += 1;
                    if verbose {
                        eprintln!(
                            "[KEY HIT ] pred={}({}) gt={}({}) | {} - {}",
                            res.key_value, pred_open, gt_key, gt_open, entry.artist, entry.title
                        );
                    }
                } else if KeyWheel::are_harmonically_compatible(pred_open, gt_open) {
                    key_compat += 1;
                    if let (Some((na, ma)), Some((nb, mb))) = (
                        KeyWheel::parse_open_key(pred_open),
                        KeyWheel::parse_open_key(gt_open),
                    ) {
                        if na == nb && ma != mb {
                            key_rel_err += 1;
                        } else if ma == mb {
                            key_fifth_err += 1;
                        }
                    }
                    if verbose {
                        eprintln!(
                            "[KEY COMP] pred={}({}) gt={}({}) | {} - {}",
                            res.key_value, pred_open, gt_key, gt_open, entry.artist, entry.title
                        );
                    }
                } else if verbose {
                    eprintln!(
                        "[KEY MISS] pred={}({}) gt={}({}) | {} - {}",
                        res.key_value, pred_open, gt_key, gt_open, entry.artist, entry.title
                    );
                }
            }
        }
    }

    let pct = |num: usize, den: usize| -> f64 {
        if den == 0 {
            0.0
        } else {
            100.0 * (num as f64) / (den as f64)
        }
    };
    let mean_ms = if analyzed_tracks > 0 {
        (total_analysis_sec * 1000.0) / (analyzed_tracks as f64)
    } else {
        0.0
    };

    println!("┌──────────────────────────────────────┬──────────────────────────┐");
    println!("│ ReTraktor Offline Analysis Benchmark │ Result                   │");
    println!("├──────────────────────────────────────┼──────────────────────────┤");
    println!(
        "│ Tracks decoded & analyzed            │ {:>4} / {:<4}              │",
        analyzed_tracks,
        selected.len()
    );
    println!(
        "│ BPM exact (±0.5 BPM)                 │ {:>4}/{:<4} ({:>5.1}%)       │",
        bpm_exact,
        bpm_total,
        pct(bpm_exact, bpm_total)
    );
    println!(
        "│ BPM octave-tolerant (±0.5, ½x/2x)    │ {:>4}/{:<4} ({:>5.1}%)       │",
        bpm_octave,
        bpm_total,
        pct(bpm_octave, bpm_total)
    );
    println!(
        "│ Key exact (MUSICAL_KEY 0..=23)       │ {:>4}/{:<4} ({:>5.1}%)       │",
        key_exact,
        key_total,
        pct(key_exact, key_total)
    );
    println!(
        "│ Key compatible (KeyWheel)            │ {:>4}/{:<4} ({:>5.1}%)       │",
        key_compat,
        key_total,
        pct(key_compat, key_total)
    );
    println!(
        "│ Key relative / fifth errors          │ {:>4} rel / {:<4} fifth    │",
        key_rel_err, key_fifth_err
    );
    println!(
        "│ Mean analysis time per track         │ {:>7.1} ms               │",
        mean_ms
    );
    println!("└──────────────────────────────────────┴──────────────────────────┘");
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_mapping_accepts_the_example_and_rejects_bad_or_missing_files() {
        let dir = std::env::temp_dir().join(format!("retraktor-cli-validate-{}", process::id()));
        fs::create_dir_all(&dir).unwrap();
        let good = dir.join("good.json");
        fs::write(&good, controller_rules::EXAMPLE_RULES).unwrap();
        assert_eq!(validate_mapping(&good), 0);
        let bad = dir.join("bad.json");
        fs::write(&bad, controller_rules::EXAMPLE_RULES.replace("\"deck\": 2", "\"deck\": 7")).unwrap();
        assert_eq!(validate_mapping(&bad), 1);
        assert_eq!(validate_mapping(&dir.join("missing.json")), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// The import command: list, refuse an ambiguous device or a missing port, preview, write a draft without
    /// overwriting, and exit 2 with the reason when nothing converts.
    #[test]
    fn import_mapping_lists_previews_writes_new_files_and_reports_empty_conversions() {
        let dir = std::env::temp_dir().join(format!("retraktor-cli-import-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let tsi = dir.join("settings.tsi");
        fs::write(&tsi, mapping_import::fixtures::traktor_three_devices()).unwrap();
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = import_mapping(&args, &mut out, &mut err);
            (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
        };
        let t = tsi.to_str().unwrap();
        let (code, out, _) = run(&[t, "--list"]);
        assert_eq!(code, 0);
        assert!(out.contains("3 device(s)") && out.contains("2  Generic MIDI: K2 layout and browser"), "{out}");
        let (code, _, err) = run(&[t, "--port", "xonek2"]);
        assert!(code == 1 && err.contains("choose one with --device N"), "{err}");
        let (code, _, err) = run(&[t, "--device", "2"]);
        assert!(code == 1 && err.contains("--port is required"), "{err}");
        let (code, out, _) = run(&[t, "--device", "2", "--port", "xonek2"]);
        assert!(code == 0 && out.contains("17 source assignments, 17 report entries: 4 translated, 13 approximated") && out.contains("Preview only"), "{out}");
        let alias = dir.join("same.txt");
        let (code, _, err) = run(&[t, "--device", "2", "--port", "xonek2", "--out", alias.to_str().unwrap(), "--report", &format!("{}/./same.txt", dir.display())]);
        assert!(code == 1 && err.contains("name the same file") && !alias.exists(), "{err}");

        let draft = dir.join("draft.json");
        let d = draft.to_str().unwrap();
        let (code, out, _) = run(&[t, "--device", "2", "--port", "xonek2", "--out", d]);
        assert!(code == 0 && out.contains("Draft written to"), "{out}");
        assert_eq!(validate_mapping(&draft), 0);
        let before = fs::read_to_string(&draft).unwrap();
        let (code, _, err) = run(&[t, "--device", "3", "--port", "mixer", "--out", d]);
        assert!(code == 1 && err.contains("already exists"), "{err}");
        assert_eq!(fs::read_to_string(&draft).unwrap(), before, "an existing draft was overwritten");

        let (code, out, err) = run(&[t, "--device", "2", "--port", "xonek2", "--out", "-"]);
        assert!(code == 0 && out.starts_with('{') && err.contains("ReTraktor mapping import"), "{out}");
        let report = dir.join("x1.txt");
        let (code, out, err) = run(&[t, "--device", "1", "--port", "x1", "--report", report.to_str().unwrap()]);
        assert!(code == 2 && err.contains("No draft: ") && out.contains("Draft:       none.") && out.contains("0 source assignments, 1 report entry"), "{err}");
        assert!(fs::read_to_string(&report).unwrap().contains("built-in QML script"));
        let (code, _, err) = run(&[dir.join("missing.tsi").to_str().unwrap(), "--list"]);
        assert!(code == 1 && err.contains("can't read"), "{err}");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Two complementary drafts go in together, in order and with their pad lights; a repeat, a collision between
    /// drafts, or a bad later draft writes nothing; a dry run writes nothing; usage errors exit 2.
    #[test]
    fn apply_mapping_adds_drafts_all_or_nothing() {
        let dir = std::env::temp_dir().join(format!("retraktor-cli-apply-{}", process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rules = dir.join("controller-rules.json");
        let draft = |name: &str, note: u8, pad: u8| {
            let path = dir.join(name);
            fs::write(&path, format!(r#"{{"format": "retraktor-controller-rules", "version": 1, "rules": [{{"input": "midi.xonek2.ch15.note{note}", "do": [{{"action": "play", "deck": 1}}]}}], "leds": [{{"control": "surface.right.pads.{pad}", "colour": "cyan"}}]}}"#)).unwrap();
            path.to_str().unwrap().to_string()
        };
        let (a, b, a_again) = (draft("a.json", 36, 1), draft("b.json", 37, 2), draft("a-again.json", 36, 3));
        let broken = dir.join("broken.json");
        fs::write(&broken, "{\"format\": \"retraktor-controller-rules\", \"version\": 1, \"rules\": [{\"input\": \"midi.xonek2.ch15.note40\", \"do\": [{\"action\": \"plai\"}]}]}").unwrap();
        let broken = broken.to_str().unwrap();
        let run = |args: &[&str]| {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let code = apply_mapping(&args, &rules, &mut out, &mut err);
            (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
        };
        let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text).unwrap();
        let files = || {
            let mut names: Vec<_> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
            names.sort();
            names
        };

        let (code, out, _) = run(&["--help"]);
        assert!(code == 0 && out.starts_with("Usage: retraktor-cli apply-mapping"), "{out}");
        let (code, out, _) = run(&["--json"]);
        assert_eq!((code, parse(&out)["outcome"].as_str()), (2, Some("usage")));
        let (code, _, err) = run(&["--force", &a]);
        assert!(code == 2 && err.contains("unknown option --force") && err.contains("Usage:"), "{err}");

        // Dry run on a missing rules file: counts, nothing written.
        let (code, out, _) = run(&["--dry-run", "--json", &a, &b]);
        let v = parse(&out);
        assert_eq!(code, 0);
        assert_eq!((v["outcome"].as_str(), v["dry_run"].as_bool(), v["counts"]["rules_added"].as_u64(), v["counts"]["leds_after"].as_u64()), (Some("preview"), Some(true), Some(2), Some(2)));
        assert!(!rules.exists());

        // Both drafts at once, in order, with their pad lights.
        let (code, out, _) = run(&["--json", &a, &b]);
        let v = parse(&out);
        assert_eq!(code, 0, "{out}");
        assert_eq!((v["outcome"].as_str(), v["backup"].is_null(), v["counts"]["rules_after"].as_u64(), v["counts"]["leds_after"].as_u64()), (Some("applied"), true, Some(2), Some(2)));
        let applied = fs::read_to_string(&rules).unwrap();
        assert!(applied.find("note36").unwrap() < applied.find("note37").unwrap() && applied.find("pads.1").unwrap() < applied.find("pads.2").unwrap(), "{applied}");
        assert_eq!(validate_mapping(&rules), 0);
        let other = draft("c.json", 38, 4);
        let before = files();

        // A repeat, a collision between two drafts, and a broken later draft are all refused and write nothing.
        let (code, out, _) = run(&["--json", &a]);
        let v = parse(&out);
        assert_eq!((code, v["outcome"].as_str(), v["counts"].is_null()), (1, Some("refused"), true));
        let messages: Vec<&str> = v["diagnostics"].as_array().unwrap().iter().map(|d| d["message"].as_str().unwrap()).collect();
        assert!(messages.contains(&"midi.xonek2.ch15.note36 already has rules in the rules file"), "{out}");
        assert!(messages.contains(&"pad light surface.right.pads.1 is already coloured by the rules file"), "{out}");
        assert!(messages.last().unwrap().contains("already in the rules file is refused like this"), "{out}");
        fs::remove_file(&rules).unwrap();
        let (code, _, err) = run(&[&a, &a_again]);
        assert!(code == 1 && err.contains("a-again.json: midi.xonek2.ch15.note36 already has rules") && err.contains("includes the drafts named before this one"), "{err}");
        assert!(!rules.exists(), "a refused apply created the rules file");
        fs::write(&rules, &applied).unwrap();
        let (code, _, err) = run(&[&other, broken]);
        assert!(code == 1 && err.contains("broken.json: the draft has problems") && err.contains("unknown action \"plai\""), "{err}");
        let (code, _, err) = run(&[&other, dir.join("missing.json").to_str().unwrap()]);
        assert!(code == 1 && err.contains("missing.json: can't read"), "{err}");
        assert_eq!(fs::read_to_string(&rules).unwrap(), applied, "a refused apply changed the rules file");
        assert_eq!(files(), before, "a refused apply left a backup or temporary file");

        // A broken rules file is refused as the file's own problem, not the draft's, and left as it is.
        let broken_file = applied.replace("\"play\"", "\"plai\"");
        fs::write(&rules, &broken_file).unwrap();
        let (code, out, _) = run(&["--json", &other]);
        let v = parse(&out);
        assert_eq!((code, v["outcome"].as_str(), v["counts"].is_null()), (1, Some("refused"), true), "{out}");
        let diagnostics = v["diagnostics"].as_array().unwrap();
        assert!(diagnostics.len() >= 2 && diagnostics.iter().all(|d| d["draft"].is_null() && d["message"].as_str().unwrap().starts_with(rules.to_str().unwrap())), "{out}");
        assert!(diagnostics.iter().any(|d| d["message"].as_str().unwrap().contains("unknown action \"plai\"")), "{out}");
        assert_eq!(fs::read_to_string(&rules).unwrap(), broken_file, "a broken rules file was changed");
        assert_eq!(files(), before);
        fs::write(&rules, &applied).unwrap();

        // Added after the existing rules, with the previous file kept as a backup.
        let (code, out, _) = run(&[&other]);
        assert!(code == 0 && out.contains("Added 1 rules and 1 pad lights from 1 draft(s)") && out.contains("now 3 rules and 3 pad lights") && out.contains("The previous file is kept as"), "{out}");
        let backup = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).find(|p| p.to_string_lossy().contains(".before-import-")).unwrap();
        assert_eq!(fs::read_to_string(backup).unwrap(), applied);
        fs::remove_dir_all(&dir).unwrap();
    }
}
