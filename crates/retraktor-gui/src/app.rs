use crate::audio::{AudioSystem, LoadedTrackAudio};
use crate::library_ui::LibraryManager;
use crate::preferences::{PreferencesDialog, PrefsTab};
use crate::theme::TraktorTheme;
use crate::widgets::{
    crossfader, deck_transport_bar, icon_button, render_shortcuts_hud, rotary_knob_sized,
    scrolling_stem_waveforms, scrolling_waveform, stripe_overview, vertical_fader, vu_meter, Icon,
    TransportAction,
};
use eframe::egui::{self, Align, Color32, Key, Layout, RichText, Sense, Stroke, Ui, Vec2};
use retraktor_core::{PropValue, PropertyBus, SurfaceEvent};
use retraktor_dsp::MixerFxKind;
use retraktor_hid::{
    scan_linux_ni_hidraw_devices, Mx2BottomShellState, Mx2HidDriver,
    TRAKTOR_MX2_PID,
};
use retraktor_library::KeyWheel;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::thread;

/// Traktor Pro 4 + Next-Gen Workspace Layouts (switchable from Top Header or F1..F6)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraktorLayout {
    /// Stacked full-width parallel waveforms at top + transport/mixer row + browser
    Parallel,
    /// All 4 Decks (A, B, C, D) visible + 4-channel mixer + browser
    FourDeck,
    /// DJ.Studio-style Horizontal Timeline Transition Editor + Stem Automation
    Timeline,
    /// Compact mini-decks at top + maximized full-screen library & file explorer
    Browser,
}

impl TraktorLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Parallel => "Mix",
            Self::FourDeck => "4-Deck",
            Self::Timeline => "Timeline",
            Self::Browser => "Browser",
        }
    }
}

/// Traktor MX2 & Deck Performance Pad Modes
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeckPadMode {
    Hotcues,
    Stems,
    Pattern,
    FluxLoop,
}

impl DeckPadMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hotcues => "HOTCUES",
            Self::Stems => "STEMS",
            Self::Pattern => "PATTERN",
            Self::FluxLoop => "LOOP ROLL",
        }
    }
}

pub struct ReTraktorApp {
    pub audio: AudioSystem,
    pub library: LibraryManager,
    pub preferences: PreferencesDialog,
    pub demucs: crate::demucs::DemucsManager,
    pub bus: PropertyBus,
    pub layout: TraktorLayout,
    pub show_shortcuts_hud: bool,

    // Mixer UI State
    pub channel_gain: [f32; 4],
    pub channel_eq_hi: [f32; 4],
    pub channel_eq_mid: [f32; 4],
    pub channel_eq_low: [f32; 4],
    pub eq_kill_hi: [bool; 4],
    pub eq_kill_mid: [bool; 4],
    pub eq_kill_low: [bool; 4],
    pub channel_filter: [f32; 4],
    pub channel_filter_on: [bool; 4],
    pub channel_fx_kind: [MixerFxKind; 4],
    pub channel_volume: [f32; 4],
    pub channel_cue: [bool; 4],
    pub channel_xfader_assign: [usize; 4], // 0: Left, 1: Thru, 2: Right

    pub master_volume: f32,
    pub master_tempo: f64,
    pub crossfader_pos: f32,

    // FX Racks UI State
    pub fx1_dry_wet: f32,
    pub fx1_param: [f32; 3],
    pub fx1_on: bool,
    pub fx2_dry_wet: f32,
    pub fx2_param: [f32; 3],
    pub fx2_on: bool,

    // Decks Performance State
    pub deck_pitch_fader: [f32; 4], // -8% to +8%
    pub deck_flux_active: [bool; 4],
    pub deck_loop_active: [bool; 4],
    pub deck_loop_start_sec: [f64; 4],
    pub deck_loop_size_beats: [u32; 4],
    pub deck_hotcues: [Vec<(u8, f64)>; 4],
    pub deck_pad_mode: [DeckPadMode; 4],
    pub deck_sync_enabled: [bool; 4],

    // Hybrid 4-Stem Separation State ([Drums, Bass, Instruments, Vocals])
    pub deck_stem_volumes: [[f32; 4]; 4],
    pub deck_stem_muted: [[bool; 4]; 4],
    pub deck_stem_fx_send: [[bool; 4]; 4],
    pub deck_stem_view: [bool; 4],
    /// Per-deck "More": hotcue pads, pad modes, cue-play, flux and all loop sizes.
    pub deck_expanded: [bool; 4],
    /// Switched from Browser to Mix after the first track load.
    first_load_done: bool,

    // Traktor Pro 4 16-Step Pattern Player State
    pub show_pattern_player: bool,
    pub pattern_enabled: bool,
    pub pattern_selected_voice: usize,
    pub pattern_steps: [[bool; 16]; 4],
    pub pattern_volume: f32,
    pub pattern_pitch: f32,
    pub pattern_decay: f32,

    // DJ.Studio Timeline Transition Editor State
    pub timeline_transition_bars: u32,
    pub timeline_preset_idx: usize, // 0: Smooth Crossfade, 1: Bass Swap, 2: Vocal Stem Bridge

    // Traktor MX2 (VID 0x17CC, PID 0x2420) Live Hardware State
    pub mx2_driver: Mx2HidDriver,
    pub mx2_hidraw_file: Option<File>,
    pub mx2_dev_node: Option<String>,
    pub mx2_model_name: Option<String>,
    pub mx2_scan_counter: u32,
    pub mx2_access_status: Option<String>,
    pub mx2_packets_received: u64,
    pub mx2_last_report_id: Option<u8>,
    pub mx2_last_packet_hex: Option<String>,
    pub mx2_last_events: Vec<String>,
    pub mx2_led_test_active: bool,
    pub mx2_led_test_phase: u8,
    pub mx2_led_test_timer: f32,
    pub discovered_hardware: Vec<retraktor_hid::DiscoveredHardwareItem>,

    // Live Scratching & Controller Remapping State
    pub deck_scratch_active: [bool; 4],
    pub deck_turntable_mode: [bool; 4],
    pub jog_scratch_sensitivity: f32,
    pub custom_mappings: std::collections::HashMap<String, String>,

    // Exit Collection Save Prompt State
    pub exit_confirmed: bool,
    startup_scan_done: bool,
    /// Dev screenshot mode (RETRAKTOR_SHOT_DIR): output dir and steps.
    shot: Option<(String, Vec<String>)>,
}

impl Default for ReTraktorApp {
    fn default() -> Self {
        let bus = PropertyBus::new_traktor_pro4_default();
        let mut default_steps = [[false; 16]; 4];
        // Default 4x4 Kick + Offbeat Hat + Clap on 5 & 13
        for s in (0..16).step_by(4) {
            default_steps[0][s] = true;
        }
        default_steps[1][4] = true;
        default_steps[1][12] = true;
        for s in (2..16).step_by(4) {
            default_steps[2][s] = true;
        }
        default_steps[3][7] = true;
        default_steps[3][15] = true;

        Self {
            audio: AudioSystem::new(),
            library: LibraryManager::default(),
            preferences: PreferencesDialog::load(),
            demucs: crate::demucs::DemucsManager::new(),
            bus,
            layout: TraktorLayout::Browser, // Library first; switches to Mix on the first load
            show_shortcuts_hud: false,
            channel_gain: [0.5, 0.5, 0.5, 0.5],
            channel_eq_hi: [0.5, 0.5, 0.5, 0.5],
            channel_eq_mid: [0.5, 0.5, 0.5, 0.5],
            channel_eq_low: [0.5, 0.5, 0.5, 0.5],
            eq_kill_hi: [false, false, false, false],
            eq_kill_mid: [false, false, false, false],
            eq_kill_low: [false, false, false, false],
            channel_filter: [0.5, 0.5, 0.5, 0.5],
            channel_filter_on: [false, false, false, false],
            channel_fx_kind: [
                MixerFxKind::Filter,
                MixerFxKind::Filter,
                MixerFxKind::Filter,
                MixerFxKind::Filter,
            ],
            channel_volume: [0.85, 0.85, 0.75, 0.75],
            channel_cue: [false, false, false, false],
            channel_xfader_assign: [0, 2, 1, 1], // Deck A: Left, Deck B: Right, C/D: Thru
            master_volume: 0.8,
            master_tempo: 128.00,
            crossfader_pos: 0.5,
            fx1_dry_wet: 0.3,
            fx1_param: [0.5, 0.5, 0.5],
            fx1_on: false,
            fx2_dry_wet: 0.3,
            fx2_param: [0.5, 0.5, 0.5],
            fx2_on: false,
            deck_pitch_fader: [0.0, 0.0, 0.0, 0.0],
            deck_flux_active: [false, false, false, false],
            deck_loop_active: [false, false, false, false],
            deck_loop_start_sec: [0.0, 0.0, 0.0, 0.0],
            deck_loop_size_beats: [4, 4, 4, 4],
            deck_hotcues: [
                vec![(1, 0.0), (2, 15.0), (3, 30.0), (4, 60.0)],
                vec![(1, 0.0), (2, 16.0), (3, 32.0), (4, 64.0)],
                vec![(1, 0.0), (2, 15.0)],
                vec![(1, 0.0), (2, 15.0)],
            ],
            deck_pad_mode: [
                DeckPadMode::Hotcues,
                DeckPadMode::Hotcues,
                DeckPadMode::Hotcues,
                DeckPadMode::Hotcues,
            ],
            deck_sync_enabled: [false; 4],
            deck_stem_volumes: [[1.0; 4]; 4],
            deck_stem_muted: [[false; 4]; 4],
            deck_stem_fx_send: [[false; 4]; 4],
            deck_stem_view: [false, false, false, false],
            deck_expanded: [false; 4],
            first_load_done: false,
            show_pattern_player: false,
            pattern_enabled: false,
            pattern_selected_voice: 0,
            pattern_steps: default_steps,
            pattern_volume: 0.75,
            pattern_pitch: 0.5,
            pattern_decay: 0.5,
            timeline_transition_bars: 16,
            timeline_preset_idx: 1,
            mx2_driver: Mx2HidDriver::new(),
            mx2_hidraw_file: None,
            mx2_dev_node: None,
            mx2_model_name: None,
            mx2_scan_counter: 0,
            mx2_access_status: None,
            mx2_packets_received: 0,
            mx2_last_report_id: None,
            mx2_last_packet_hex: None,
            mx2_last_events: Vec::new(),
            mx2_led_test_active: false,
            mx2_led_test_phase: 0,
            mx2_led_test_timer: 0.0,
            discovered_hardware: Vec::new(),
            deck_scratch_active: [false; 4],
            deck_turntable_mode: [true; 4],
            jog_scratch_sensitivity: 1.0,
            custom_mappings: std::collections::HashMap::new(),
            exit_confirmed: false,
            startup_scan_done: false,
            shot: std::env::var("RETRAKTOR_SHOT_DIR").ok().map(|d| {
                let steps = std::env::var("SHOT_STEPS").unwrap_or_else(|_| "Mix,FourDeck,Browser,prefs".into());
                (d, steps.split(',').map(str::to_string).collect())
            }),
        }
    }
}

/// Maps action ids saved by older builds to the current `verb:deck[:arg]` form.
fn legacy_action(a: &str) -> String {
    let deck = |s: &str| if s.ends_with("_a") { 1 } else { 2 };
    match a {
        "play_deck_a" | "play_deck_b" => format!("play:{}", deck(a)),
        "cue_deck_a" | "cue_deck_b" => format!("cue:{}", deck(a)),
        "sync_deck_a" | "sync_deck_b" => format!("sync:{}", deck(a)),
        "stem_mute_drums_a" => "stem:1:1".into(),
        "stem_mute_bass_a" => "stem:1:2".into(),
        "stem_mute_inst_a" => "stem:1:3".into(),
        "stem_mute_vocals_a" => "stem:1:4".into(),
        "filter_toggle_a" | "filter_toggle_b" => format!("filter_on:{}", deck(a)),
        other => other.to_string(),
    }
}

impl eframe::App for ReTraktorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request 60 FPS continuous repaint for smooth scrolling waveforms and VU meters
        ctx.request_repaint();
        if self.shot.is_some() {
            self.screenshot_step(ctx);
        }

        // Flush the autosaved collection and preferences on close.
        if ctx.input(|i| i.viewport().close_requested()) && !self.exit_confirmed {
            self.save_collection_according_to_prefs();
            let _ = self.preferences.save();
            self.exit_confirmed = true;
        }

        if self.library.open_prefs_requested {
            self.library.open_prefs_requested = false;
            self.preferences.is_open = true;
            if self.library.open_prefs_file_management {
                self.library.open_prefs_file_management = false;
                self.preferences.current_tab = PrefsTab::FileManagement;
            } else if self.library.open_prefs_browser_columns {
                self.library.open_prefs_browser_columns = false;
                self.preferences.current_tab = PrefsTab::BrowserColumns;
            } else {
                self.preferences.current_tab = PrefsTab::Controllers;
            }
        }

        self.handle_keyboard_shortcuts(ctx);
        self.poll_mx2_hardware();
        self.sync_audio_state();

        crate::theme::apply_visuals(ctx);

        // One 4 px gutter everywhere: window edge to panel, and panel to panel.
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(TraktorTheme::BG_WINDOW()).inner_margin(4.0))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);

                // If Preferences is open, render as a full-page workspace view with maximum screen real estate
                let prefs_t = crate::theme::fade(ctx, "prefs_page", self.preferences.is_open);
                let work_t = crate::theme::fade(ctx, "workspace", !self.preferences.is_open);
                if self.preferences.is_open {
                    ui.set_opacity(prefs_t);
                    self.preferences.show_full_page(ui, ctx);
                    return;
                }
                ui.set_opacity(work_t);

                // 1. Top FX Rack, Master Clock, Layout Switcher, Pattern Toggle & Mix Recorder
                self.show_top_bar(ui);

                // 1b. Optional Collapsible Traktor Pro 4 16-Step Pattern Player Strip
                if self.show_pattern_player {
                    self.show_pattern_player_bar(ui);
                }

                // 2. Active Workspace Layout (each fades in when chosen)
                let mut layout_t = 1.0;
                for lay in [TraktorLayout::Parallel, TraktorLayout::FourDeck, TraktorLayout::Timeline, TraktorLayout::Browser] {
                    let t = crate::theme::fade(ctx, ("layout", lay.label()), self.layout == lay);
                    if self.layout == lay {
                        layout_t = t;
                    }
                }
                ui.set_opacity(work_t * layout_t);
                match self.layout {
                    TraktorLayout::Parallel => {
                        self.show_parallel_layout(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::FourDeck => {
                        self.show_four_deck_layout(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Timeline => {
                        self.show_timeline_studio_layout(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Browser => {
                        self.show_mini_decks_bar(ui);
                        self.show_browser_panel(ui);
                    }
                }
            });

        // Synchronize live hardware state with Preferences Controller Dialog
        self.preferences.mx2_connected_node = self.mx2_dev_node.clone();
        self.preferences.mx2_access_status = self.mx2_access_status.clone();
        self.preferences.mx2_packets_count = self.mx2_packets_received;
        self.preferences.mx2_last_report_id = self.mx2_last_report_id;
        self.preferences.mx2_last_hex = self.mx2_last_packet_hex.clone();
        self.preferences.mx2_recent_events = self.mx2_last_events.clone();
        if self.preferences.trigger_rescan {
            self.discovered_hardware = retraktor_hid::scan_all_dj_hardware();
            self.preferences.discovered_hardware = self.discovered_hardware.clone();
            self.preferences.trigger_rescan = false;
        }
        if let Some(target_node) = self.preferences.trigger_connect_node.take() {
            self.connect_hidraw_node(&target_node);
        }
        if self.preferences.trigger_led_test {
            self.mx2_led_test_active = true;
            self.mx2_led_test_phase = 0;
            self.mx2_led_test_timer = 0.0;
            self.preferences.trigger_led_test = false;
        }
        if self.preferences.trigger_self_test {
            let report = self.mx2_driver.run_full_self_test();
            self.preferences.self_test_report = Some(report);
            self.preferences.trigger_self_test = false;
        }

        // Handle File Management background music folder rescan trigger
        self.library.auto_analyze = self.preferences.analyze_new_tracks;
        if std::mem::take(&mut self.preferences.request_folder_pick) {
            self.library.pick(crate::library_ui::Pick::MusicFolder);
        }
        if !self.startup_scan_done {
            self.startup_scan_done = true;
            if self.preferences.scan_on_startup && !self.library.tracks.is_empty() {
                self.library.start_scan(self.preferences.music_folders.clone());
            }
        }
        self.library.apply_picks(&mut self.preferences.music_folders);
        if self.preferences.trigger_music_rescan {
            self.preferences.trigger_music_rescan = false;
            self.library.start_scan(self.preferences.music_folders.clone());
            self.preferences.file_management_status = "Scanning in the background — see the browser status bar.".to_string();
        }

        // Synchronize visual remapper and controller settings from Preferences
        self.jog_scratch_sensitivity = self.preferences.jog_scratch_sensitivity;
        self.deck_turntable_mode[0] = self.preferences.jog_mode_turntable;
        self.deck_turntable_mode[1] = self.preferences.jog_mode_turntable;
        self.custom_mappings = self.preferences.mapping.action.clone();

        // Demucs background installer & separation worker synchronization
        self.preferences.demucs_install_state = self.demucs.get_install_state();
        if self.preferences.trigger_demucs_install {
            self.preferences.trigger_demucs_install = false;
            self.demucs.start_background_install();
        }

        // Poll Demucs background stem separation worker
        let (sep_status, active_desc) = self.demucs.poll_separation();
        self.preferences.demucs_active_separation = active_desc;

        match sep_status {
            crate::demucs::DemucsSeparationStatus::Running { track_id, percent, .. } => {
                self.library.stem_job_progress = Some((track_id, percent));
            }
            crate::demucs::DemucsSeparationStatus::Completed { track_id, stem_dir } => {
                self.library.stem_job_progress = None;
                if let Some(t) = self.library.tracks.iter_mut().find(|t| t.id == track_id) {
                    t.stems_ready = true;
                    self.library.status_message = format!("✔ AI Demucs separated 4 stems for '{}'!", t.title);
                }
                // Reload stems asynchronously without stalling audio or UI
                if let Some(track) = self.library.tracks.iter().find(|t| t.id == track_id) {
                    let track_title = track.title.clone();
                    let track_artist = track.artist.clone();
                    let shared_audio = self.audio.shared.clone();
                    let stem_dir_buf = stem_dir.clone();
                    thread::spawn(move || {
                        if let Ok(mut state) = shared_audio.lock() {
                            for d in 0..4 {
                                if let Some(ref mut loaded) = state.loaded_tracks[d] {
                                    if loaded.title == track_title && loaded.artist == track_artist {
                                        let _ = loaded.load_stems_from_dir(&stem_dir_buf);
                                    }
                                }
                            }
                        }
                    });
                    for d in 0..4 {
                        self.deck_stem_view[d] = true;
                    }
                }
                *self.demucs.separation_status.lock().unwrap() = crate::demucs::DemucsSeparationStatus::Idle;
            }
            crate::demucs::DemucsSeparationStatus::Failed { track_id: _, error } => {
                self.library.stem_job_progress = None;
                self.library.status_message = format!("⚠ Stem separation failed: {error}");
                *self.demucs.separation_status.lock().unwrap() = crate::demucs::DemucsSeparationStatus::Idle;
            }
            crate::demucs::DemucsSeparationStatus::Idle => {}
        }

        // Handle user request to separate stems for a track
        if let Some(track_id) = self.library.request_stem_separation.take() {
            if let Some(track) = self.library.tracks.iter().find(|t| t.id == track_id).cloned() {
                if let Some(ref file_path) = track.file_path {
                    if self.demucs.is_installed() {
                        let started = self.demucs.start_separation(track.id, &track.title, file_path);
                        if started {
                            self.library.status_message = format!("Queued Meta Demucs AI 4-stem separation for '{}'...", track.title);
                        } else {
                            self.library.status_message = format!("Could not start Demucs separation for '{}'.", track.title);
                        }
                    } else {
                        self.library.status_message =
                            "Install Demucs in Preferences > Stems & AI to separate stems (live crossover stems work without it).".to_string();
                    }
                }
            }
        }

        // Preferences Dialog
        self.preferences.show(ctx);

        // Live Performance Shortcuts HUD Overlay
        render_shortcuts_hud(ctx, &mut self.show_shortcuts_hud);

    }
}

impl ReTraktorApp {
    fn save_collection_according_to_prefs(&mut self) {
        let fmt = self.preferences.collection_save_format;
        // The .nml is the library's own store, so it is always written.
        let nml_path = LibraryManager::default_nml_path();
        if let Err(e) = self.library.save_collection_nml(&nml_path) {
            eprintln!("retraktor: failed to save collection: {e}");
        }
        if fmt == 1 || fmt == 2 {
            let m3u_path = LibraryManager::default_m3u8_path();
            let _ = self.library.save_collection_m3u8(&m3u_path);
        }
    }

    /// Dev-only: walks SHOT_STEPS (90 frames each after SHOT_WARM frames) saving a PNG per step, then exits.
    /// Steps: Mix FourDeck Browser Timeline prefs prefs:<Tab> nav:Missing|Suggestions|Files loadA loadB expandA stemsA pattern relocate wait.
    fn screenshot_step(&mut self, ctx: &egui::Context) {
        let Some((dir, steps)) = self.shot.clone() else { return };
        let warm: usize = std::env::var("SHOT_WARM").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
        let n = ctx.cumulative_pass_nr() as usize;
        if n < warm {
            return;
        }
        let idx = (n - warm) / 90;
        let Some(step) = steps.get(idx) else { std::process::exit(0) };
        let first = (n - warm) % 90 == 0;
        use crate::library_ui::NavItem;
        match step.as_str() {
            "Mix" => self.layout = TraktorLayout::Parallel,
            "FourDeck" => self.layout = TraktorLayout::FourDeck,
            "Browser" => self.layout = TraktorLayout::Browser,
            "Timeline" => self.layout = TraktorLayout::Timeline,
            "prefs" => self.preferences.is_open = true,
            "noprefs" => self.preferences.is_open = false,
            "expandA" => self.deck_expanded[0] = true,
            s if s.starts_with("select:") => self.preferences.mapper.selected = Some(s[7..].to_string()),
            s if s.starts_with("prefs:") => {
                self.preferences.is_open = true;
                if let Some((_, tabs)) = PreferencesDialog::GROUPS.iter().find(|(name, _)| name.replace(' ', "") == s[6..]) {
                    self.preferences.current_tab = tabs[0];
                }
            }
            s if s.starts_with("nav:") => {
                self.preferences.is_open = false;
                self.layout = TraktorLayout::Browser;
                self.library.selected_nav = match &s[4..] {
                    "Missing" => NavItem::Missing,
                    "Suggestions" => NavItem::Suggestions,
                    "Files" => NavItem::Explorer,
                    _ => NavItem::Collection,
                };
            }
            "loadA" | "loadB" if first => {
                let deck = if step == "loadA" { 1 } else { 2 };
                let mut playable = self.library.tracks.iter().filter(|t| !t.missing && t.file_path.as_ref().is_some_and(|p| p.is_file()));
                if let Some(id) = playable.nth(deck - 1).map(|t| t.id) {
                    self.library.load_to_deck(deck, id, [false; 4]);
                    self.drain_pending_loads();
                }
            }
            "stemsA" => self.deck_stem_view[0] = true,
            "pattern" => self.show_pattern_player = true,
            "relocate" if first => self.library.start_relocate(self.preferences.music_folders.clone()),
            _ => {}
        }
        // SHOT_AT picks the frame within a step (default 80 = settled; small = mid-transition).
        let at: usize = std::env::var("SHOT_AT").ok().and_then(|v| v.parse().ok()).unwrap_or(80);
        if (n - warm) % 90 == at.min(88) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot);
        }
        let shots: Vec<_> = ctx.input(|i| {
            i.raw.events.iter().filter_map(|e| if let egui::Event::Screenshot { image, .. } = e { Some(image.clone()) } else { None }).collect()
        });
        for img in shots {
            let name = format!("{dir}/{idx:02}-{}.png", step.replace(':', "-"));
            let write = || -> Result<(), Box<dyn std::error::Error>> {
                let mut enc = png::Encoder::new(std::fs::File::create(&name)?, img.size[0] as u32, img.size[1] as u32);
                enc.set_color(png::ColorType::Rgba);
                enc.set_depth(png::BitDepth::Eight);
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
                enc.write_header()?.write_image_data(&bytes)?;
                Ok(())
            };
            if let Err(e) = write() {
                eprintln!("screenshot {name}: {e}");
            }
        }
    }

    /// Executes a mapped button action (`play:1`, `hotcue:2:5`, …). Unknown verbs are ignored.
    fn run_button_action(&mut self, verb: &str, deck: usize, arg: &str) {
        let d = deck.clamp(1, 4) - 1;
        let n: usize = arg.parse().unwrap_or(1);
        match verb {
            "play" => self.toggle_play(deck),
            "cue" => self.jump_to_cue(deck, 1),
            "cueplay" => {
                self.jump_to_cue(deck, 1);
                if let Ok(mut st) = self.audio.shared.lock() {
                    st.is_playing[d] = true;
                }
            }
            "sync" => self.sync_deck(deck),
            "flux" => self.deck_flux_active[d] = !self.deck_flux_active[d],
            "loop" => self.toggle_loop(deck),
            "loop_half" => self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] / 2).max(1),
            "loop_double" => self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] * 2).min(32),
            "hotcue" => self.jump_to_cue(deck, n.clamp(1, 8) as u8),
            "stem" => self.deck_stem_muted[d][n.clamp(1, 4) - 1] ^= true,
            "stem_view" => self.deck_stem_view[d] ^= true,
            "pads" => {
                self.deck_pad_mode[d] = match arg {
                    "stems" => DeckPadMode::Stems,
                    "pattern" => DeckPadMode::Pattern,
                    "loop" => DeckPadMode::FluxLoop,
                    _ => DeckPadMode::Hotcues,
                }
            }
            "load" => {
                if let Some(id) = self.library.selected_track_id {
                    let locked = self.locked_decks();
                    self.library.load_to_deck(deck, id, locked);
                    self.drain_pending_loads();
                }
            }
            "browse_down" => self.library.select_next_track(),
            "browse_up" => self.library.select_prev_track(),
            "filter_on" => self.channel_filter_on[d] ^= true,
            "headphone" => self.channel_cue[d] ^= true,
            "fx_on" if deck == 2 => self.fx2_on ^= true,
            "fx_on" => self.fx1_on ^= true,
            _ => {}
        }
    }

    /// Applies a mapped knob/fader value (0..=1).
    fn set_mapped_value(&mut self, target: &str, deck: usize, v: f32) {
        let d = deck.clamp(1, 4) - 1;
        match target {
            "volume" => self.channel_volume[d] = v,
            "gain" => self.channel_gain[d] = v,
            "eq_hi" => self.channel_eq_hi[d] = v,
            "eq_mid" => self.channel_eq_mid[d] = v,
            "eq_low" => self.channel_eq_low[d] = v,
            "filter" => self.channel_filter[d] = v,
            "pitch" => self.deck_pitch_fader[d] = v * 16.0 - 8.0,
            "xfader" => self.crossfader_pos = v,
            "master_volume" => self.master_volume = v,
            "fx_mix" if deck == 2 => self.fx2_dry_wet = v,
            "fx_mix" => self.fx1_dry_wet = v,
            _ => {}
        }
    }

    fn locked_decks(&self) -> [bool; 4] {
        let playing = self.audio.shared.lock().map(|s| s.is_playing).unwrap_or([false; 4]);
        playing.map(|p| self.preferences.lock_playing_decks && p)
    }

    /// Decodes queued library loads on worker threads; real files only, never synthetic audio.
    fn drain_pending_loads(&mut self) {
        let library_tx = self.library.sender();
        if !self.library.pending_loads.is_empty() && !self.first_load_done {
            self.first_load_done = true;
            if self.layout == TraktorLayout::Browser {
                self.layout = TraktorLayout::Parallel;
            }
        }
        for (deck_idx, track) in std::mem::take(&mut self.library.pending_loads) {
            let d = (deck_idx - 1).min(3);
            let shared_audio = self.audio.shared.clone();
            let library_tx = library_tx.clone();
            thread::spawn(move || {
                let Some(path) = track.file_path.as_ref() else { return };
                match LoadedTrackAudio::from_file(path, &track.title, &track.artist, track.bpm, &track.musical_key) {
                    Ok(mut loaded) => {
                        // Unanalysed track: analyse the PCM we just decoded and report back to the library.
                        if let Some(grid) = track.cues.iter().find(|c| c.cue_type == retraktor_core::CueType::Grid) {
                            loaded.grid_offset_sec = grid.start_ms / 1000.0;
                        }
                        if track.bpm <= 0.0 || track.musical_key.is_empty() {
                            let mono = crate::library_ui::stereo_to_mono(&loaded.pcm_stereo);
                            let a = retraktor_library::analyze_mono(&mono, loaded.sample_rate);
                            if track.bpm <= 0.0 {
                                loaded.bpm = a.bpm;
                                loaded.grid_offset_sec = a.beatgrid_offset_sec;
                            }
                            if track.musical_key.is_empty() {
                                loaded.key = a.key_open.clone();
                            }
                            let _ = library_tx.send(crate::library_ui::Msg::Analyzed(track.id, a, false));
                        }
                        if let Ok(mut state) = shared_audio.lock() {
                            state.loaded_tracks[d] = Some(loaded);
                            state.playhead_sec[d] = 0.0;
                        }
                    }
                    Err(e) => eprintln!("retraktor: could not load {}: {e}", path.display()),
                }
            });
        }
    }

    fn sync_audio_state(&mut self) {
        if let Ok(mut state) = self.audio.shared.lock() {
            state.crossfader = self.crossfader_pos;
            state.crossfader_curve = if self.preferences.xfader_curve_idx == 2 {
                0.95
            } else if self.preferences.xfader_curve_idx == 1 {
                0.6
            } else {
                0.5
            };

            state.stem_volumes = self.deck_stem_volumes;
            state.stem_muted = self.deck_stem_muted;
            state.stem_fx_send = self.deck_stem_fx_send;

            state.pattern_enabled = self.pattern_enabled;
            state.pattern_steps = self.pattern_steps;
            state.pattern_volume = self.pattern_volume;
            state.pattern_pitch = self.pattern_pitch;
            state.pattern_decay = self.pattern_decay;

            let grids: [Option<(f64, f64)>; 4] =
                std::array::from_fn(|i| state.loaded_tracks[i].as_ref().map(|t| (t.bpm.max(20.0), t.grid_offset_sec)));
            for d in 0..4 {
                state.channel_gain[d] = self.channel_gain[d];
                state.engine.strips[d].volume = self.channel_volume[d];
                state.engine.strips[d].cue_enabled = self.channel_cue[d];
                state.engine.strips[d].filter.knob = self.channel_filter[d];
                state.engine.strips[d].filter.enabled = self.channel_filter_on[d];
                state.engine.strips[d].filter.kind = self.channel_fx_kind[d];
                state.engine.strips[d].eq.high = self.channel_eq_hi[d];
                state.engine.strips[d].eq.mid = self.channel_eq_mid[d];
                state.engine.strips[d].eq.low = self.channel_eq_low[d];
                state.eq_kill_hi[d] = self.eq_kill_hi[d];
                state.eq_kill_mid[d] = self.eq_kill_mid[d];
                state.eq_kill_low[d] = self.eq_kill_low[d];

                state.engine.strips[d].xfader_left = self.channel_xfader_assign[d] == 0;
                state.engine.strips[d].xfader_right = self.channel_xfader_assign[d] == 2;

                let mut nudge = 0.0;
                // Sync holds the tempo to the other deck (or the master tempo) and eases the beats together.
                if self.deck_sync_enabled[d] {
                    if let Some((bpm, offset)) = grids[d] {
                        let other = if d == 0 { 1 } else { 0 };
                        let target = grids[other]
                            .map(|(b, _)| b * (1.0 + self.deck_pitch_fader[other] as f64 / 100.0))
                            .unwrap_or(self.master_tempo);
                        self.deck_pitch_fader[d] = ((sync_ratio(bpm, target) - 1.0) * 100.0) as f32;
                        if let (Some((other_bpm, other_offset)), true, true) = (grids[other], state.is_playing[d], state.is_playing[other]) {
                            let behind = beat_phase_diff(
                                (state.playhead_sec[other] - other_offset) * other_bpm / 60.0,
                                (state.playhead_sec[d] - offset) * bpm / 60.0,
                            );
                            nudge = (behind * 0.2).clamp(-0.03, 0.03);
                        }
                    }
                }
                let ratio = 1.0 + self.deck_pitch_fader[d] as f64 / 100.0 + nudge;
                state.pitch_ratio[d] = ratio;

                let bpm = state.loaded_tracks[d]
                    .as_ref()
                    .map(|t| t.bpm)
                    .unwrap_or(128.0);
                state.loop_active[d] = self.deck_loop_active[d];
                state.loop_start_sec[d] = self.deck_loop_start_sec[d];
                state.loop_len_sec[d] = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
            }

            state.engine.master_volume = self.master_volume;
            state.engine.clock.bpm = self.master_tempo;

            // Sync telemetry into PropertyBus for MX2 105-LED & 9-segment VU meter feedback
            for d in 0..4 {
                let idx = d + 1;
                self.bus.set(
                    &format!("app.traktor.decks.{idx}.play"),
                    PropValue::Bool(state.is_playing[d]),
                );
                self.bus.set(
                    &format!("app.traktor.decks.{idx}.flux.enabled"),
                    PropValue::Bool(self.deck_flux_active[d]),
                );
                self.bus.set(
                    &format!("app.traktor.mixer.channels.{idx}.cue"),
                    PropValue::Bool(self.channel_cue[d]),
                );
                self.bus.set(
                    &format!("app.traktor.mixer.channels.{idx}.fx.on"),
                    PropValue::Bool(self.channel_filter_on[d]),
                );
                self.bus.set(
                    &format!("app.traktor.mixer.channels.{idx}.level.prefader.linear.sum"),
                    PropValue::Float(state.channel_peaks[d] as f64),
                );
            }
            self.bus.set(
                "app.traktor.mixer.master.level.clip.left",
                PropValue::Bool(state.master_peak_l >= 0.98),
            );
            self.bus.set(
                "app.traktor.mixer.master.level.clip.right",
                PropValue::Bool(state.master_peak_r >= 0.98),
            );
        }
    }

    /// Connects to a specific `/dev/hidraw*` endpoint for Traktor MX2 / Kontrol hardware.
    pub fn connect_hidraw_node(&mut self, dev_node: &str) {
        let mut opts = OpenOptions::new();
        opts.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.custom_flags(0x0800); // O_NONBLOCK
        }
        match opts.open(dev_node) {
            Ok(mut file) => {
                // Send initial Output Report 0x80 to illuminate deck and bottom shell
                let initial_leds = self.mx2_driver.encode_led_report(
                    &self.bus,
                    Mx2BottomShellState::DefaultGlow,
                    Mx2BottomShellState::DefaultGlow,
                );
                let _ = file.write_all(&initial_leds);
                self.mx2_dev_node = Some(dev_node.to_string());
                self.mx2_model_name = Some("Traktor MX2".to_string());
                self.mx2_access_status = Some("Connected (Read/Write OK)".to_string());
                self.mx2_hidraw_file = Some(file);
            }
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                // Try read-only so button/fader/jog inputs work even if udev write rule is missing
                let mut ro_opts = OpenOptions::new();
                ro_opts.read(true);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    ro_opts.custom_flags(0x0800);
                }
                if let Ok(file) = ro_opts.open(dev_node) {
                    self.mx2_dev_node = Some(dev_node.to_string());
                    self.mx2_model_name = Some("Traktor MX2".to_string());
                    self.mx2_access_status = Some("Connected (Read-Only: inputs work; write permissions needed for LEDs)".to_string());
                    self.mx2_hidraw_file = Some(file);
                } else {
                    self.mx2_dev_node = Some(dev_node.to_string());
                    self.mx2_model_name = Some("Traktor MX2".to_string());
                    self.mx2_access_status = Some(format!("Permission Denied: cannot open {} (udev rule required)", dev_node));
                }
            }
            Err(e) => {
                self.mx2_access_status = Some(format!("Error opening {}: {}", dev_node, e));
            }
        }
    }

    /// Polls Linux `/dev/hidraw*` for a connected Traktor MX2 (`17CC:2420`)
    /// or other Traktor Kontrol surface, decodes Input Reports `0x01`/`0x02`/`0x03`, and writes
    /// Output Report `0x80` (105 LEDs + 6 RGB Bottom-Shell LEDs per deck).
    fn poll_mx2_hardware(&mut self) {
        if self.mx2_hidraw_file.is_none() {
            self.mx2_scan_counter = self.mx2_scan_counter.wrapping_add(1);
            if self.mx2_scan_counter % 90 == 1 {
                let detected = scan_linux_ni_hidraw_devices();
                // Prefer Traktor MX2 if multiple NI devices are attached
                if let Some(dev) = detected
                    .iter()
                    .find(|d| d.descriptor.pid == TRAKTOR_MX2_PID)
                    .or_else(|| detected.first())
                {
                    self.connect_hidraw_node(&dev.dev_node);
                }
            }
            return;
        }

        // Non-blocking read of up to 16 queued HID input reports per frame
        let mut pending_events = Vec::new();
        let mut disconnected = false;
        if let Some(ref mut file) = self.mx2_hidraw_file {
            let mut buf = [0u8; 256];
            for _ in 0..16 {
                match file.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        self.mx2_packets_received = self.mx2_packets_received.wrapping_add(1);
                        self.mx2_last_report_id = Some(buf[0]);

                        // Format hex preview for Settings inspector
                        let hex_bytes: Vec<String> = buf[..n.min(16)].iter().map(|b| format!("{:02X}", b)).collect();
                        self.mx2_last_packet_hex = Some(format!("{} ({} bytes)", hex_bytes.join(" "), n));

                        let evs = self.mx2_driver.decode_input_report(&buf[..n]);
                        for ev in &evs {
                            let desc = match ev {
                                SurfaceEvent::Button { control, pressed } => format!("Button {control} = {pressed}"),
                                SurfaceEvent::Fader { control, value } => format!("Fader {control} = {value:.3}"),
                                SurfaceEvent::Encoder { control, delta } => format!("Encoder {control} += {delta}"),
                            };
                            self.mx2_last_events.push(desc);
                            if self.mx2_last_events.len() > 8 {
                                self.mx2_last_events.remove(0);
                            }
                        }
                        pending_events.extend(evs);
                    }
                    Ok(_) => {
                        // EOF -> device unplugged
                        disconnected = true;
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::Interrupted => {
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::BrokenPipe || e.kind() == std::io::ErrorKind::NotConnected => {
                        disconnected = true;
                        break;
                    }
                    Err(_) => {
                        // Transient I/O error, do not immediately disconnect
                        break;
                    }
                }
            }
        }

        if disconnected {
            self.mx2_hidraw_file = None;
            self.mx2_dev_node = None;
            self.mx2_access_status = Some("Disconnected (USB cable unplugged)".to_string());
            return;
        }

        for ev in pending_events {
            self.apply_mx2_surface_event(ev);
        }

        // Handle Interactive LED Light Test if requested from Preferences
        if self.mx2_led_test_active {
            self.mx2_led_test_timer += 1.0 / 60.0;
            if self.mx2_led_test_timer > 0.6 {
                self.mx2_led_test_timer = 0.0;
                self.mx2_led_test_phase += 1;
                if self.mx2_led_test_phase >= 4 {
                    self.mx2_led_test_active = false;
                }
            }
            let test_report = self.mx2_driver.generate_led_test_pattern(self.mx2_led_test_phase);
            if let Some(ref mut file) = self.mx2_hidraw_file {
                let _ = file.write_all(&test_report);
            }
            return;
        }

        // Normal Operational LED Update
        let left_shell = if self.deck_loop_active[0] {
            Mx2BottomShellState::ActiveLoopGreen
        } else {
            Mx2BottomShellState::DefaultGlow
        };
        let right_shell = if self.deck_loop_active[1] {
            Mx2BottomShellState::ActiveLoopGreen
        } else {
            Mx2BottomShellState::DefaultGlow
        };

        let led_report = self
            .mx2_driver
            .encode_led_report(&self.bus, left_shell, right_shell);
        if let Some(ref mut file) = self.mx2_hidraw_file {
            let _ = file.write_all(&led_report);
        }
    }

    pub fn apply_mx2_surface_event(&mut self, ev: SurfaceEvent) {
        let touched = match &ev {
            SurfaceEvent::Button { control, pressed: true } => Some(control.clone()),
            SurfaceEvent::Fader { control, .. } => Some(control.clone()),
            _ => None,
        };
        if let Some(t) = touched {
            self.preferences.last_surface_control = Some(t);
        }
        // User mapping overrides the built-in behaviour.
        use crate::controller_map::{parse, Mapped};
        let mapped = match &ev {
            SurfaceEvent::Button { control, pressed } => self.custom_mappings.get(control).and_then(|a| parse(&legacy_action(a), false)).map(|m| (m, *pressed, 0.0)),
            SurfaceEvent::Fader { control, value } => self.custom_mappings.get(control).and_then(|a| parse(a, true)).map(|m| (m, true, *value as f32)),
            _ => None,
        };
        if let Some((m, pressed, v)) = mapped {
            match m {
                Mapped::Nothing => {}
                Mapped::Button { .. } if !pressed => {}
                Mapped::Button { verb, deck, arg } => self.run_button_action(&verb, deck, &arg),
                Mapped::Value { target, deck } => self.set_mapped_value(&target, deck, v),
            }
            return;
        }

        let neural_eq = self.preferences.neural_mix_eq_mode;
        match ev {
            SurfaceEvent::Fader { control, value } => {
                let v = value as f32;
                match control.as_str() {
                    "surface.mixer.xfader" => self.crossfader_pos = v,
                    "surface.mixer.channels.1.volume" => self.channel_volume[0] = v,
                    "surface.mixer.channels.2.volume" => self.channel_volume[1] = v,
                    "surface.mixer.channels.1.gain" => self.channel_gain[0] = v,
                    "surface.mixer.channels.2.gain" => self.channel_gain[1] = v,
                    "surface.mixer.channels.1.eq.high" => {
                        if neural_eq {
                            self.deck_stem_volumes[0][3] = v; // Vocals
                        } else {
                            self.channel_eq_hi[0] = v;
                        }
                    }
                    "surface.mixer.channels.1.eq.mid" => {
                        if neural_eq {
                            self.deck_stem_volumes[0][2] = v; // Instruments
                        } else {
                            self.channel_eq_mid[0] = v;
                        }
                    }
                    "surface.mixer.channels.1.eq.low" => {
                        if neural_eq {
                            self.deck_stem_volumes[0][0] = v; // Drums
                            self.deck_stem_volumes[0][1] = v; // Bass
                        } else {
                            self.channel_eq_low[0] = v;
                        }
                    }
                    "surface.mixer.channels.2.eq.high" => {
                        if neural_eq {
                            self.deck_stem_volumes[1][3] = v; // Vocals
                        } else {
                            self.channel_eq_hi[1] = v;
                        }
                    }
                    "surface.mixer.channels.2.eq.mid" => {
                        if neural_eq {
                            self.deck_stem_volumes[1][2] = v; // Instruments
                        } else {
                            self.channel_eq_mid[1] = v;
                        }
                    }
                    "surface.mixer.channels.2.eq.low" => {
                        if neural_eq {
                            self.deck_stem_volumes[1][0] = v; // Drums
                            self.deck_stem_volumes[1][1] = v; // Bass
                        } else {
                            self.channel_eq_low[1] = v;
                        }
                    }
                    "surface.mixer.channels.1.channel_fx.amount" => self.channel_filter[0] = v,
                    "surface.mixer.channels.2.channel_fx.amount" => self.channel_filter[1] = v,
                    "surface.mixer.main.volume" => self.master_volume = v,
                    "surface.left.pitch.fader" => self.deck_pitch_fader[0] = (v * 16.0) - 8.0,
                    "surface.right.pitch.fader" => self.deck_pitch_fader[1] = (v * 16.0) - 8.0,
                    "surface.left.fx.knobs.1" => self.fx1_dry_wet = v,
                    "surface.left.fx.knobs.2" => self.fx1_param[0] = v,
                    "surface.left.fx.knobs.3" => self.fx1_param[1] = v,
                    "surface.left.fx.knobs.4" => self.fx1_param[2] = v,
                    "surface.right.fx.knobs.1" => self.fx2_dry_wet = v,
                    "surface.right.fx.knobs.2" => self.fx2_param[0] = v,
                    "surface.right.fx.knobs.3" => self.fx2_param[1] = v,
                    "surface.right.fx.knobs.4" => self.fx2_param[2] = v,
                    _ => {}
                }
            }
            SurfaceEvent::Button { control, pressed } => {
                // Handle Capacitive Jogwheel Platter Touch (Scratching on/off)
                match control.as_str() {
                    "surface.left.jogwheel.touch" => {
                        if self.deck_turntable_mode[0] {
                            self.deck_scratch_active[0] = pressed;
                            if let Ok(mut state) = self.audio.shared.lock() {
                                if pressed {
                                    state.enable_scratch(0, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
                                } else {
                                    state.disable_scratch(0);
                                }
                            }
                        }
                        return;
                    }
                    "surface.right.jogwheel.touch" => {
                        if self.deck_turntable_mode[1] {
                            self.deck_scratch_active[1] = pressed;
                            if let Ok(mut state) = self.audio.shared.lock() {
                                if pressed {
                                    state.enable_scratch(1, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
                                } else {
                                    state.disable_scratch(1);
                                }
                            }
                        }
                        return;
                    }
                    _ => {}
                }

                // Handle standard button press triggers
                if !pressed {
                    return;
                }
                match control.as_str() {
                    "surface.left.play" => self.toggle_play(1),
                    "surface.right.play" => self.toggle_play(2),
                    "surface.left.cue" => self.jump_to_cue(1, 1),
                    "surface.right.cue" => self.jump_to_cue(2, 1),
                    "surface.left.sync" => self.sync_deck(1),
                    "surface.right.sync" => self.sync_deck(2),
                    "surface.left.flux" => self.deck_flux_active[0] = !self.deck_flux_active[0],
                    "surface.right.flux" => self.deck_flux_active[1] = !self.deck_flux_active[1],
                    "surface.left.loop_size.push" => self.toggle_loop(1),
                    "surface.right.loop_size.push" => self.toggle_loop(2),

                    // Turntable Vinyl Scratch Mode vs CDJ Jog Bend Mode
                    "surface.left.mode.turntable" => {
                        self.deck_turntable_mode[0] = true;
                        self.bus.set("app.traktor.decks.1.jog.turntable", PropValue::Bool(true));
                    }
                    "surface.left.mode.jogwheel" => {
                        self.deck_turntable_mode[0] = false;
                        self.deck_scratch_active[0] = false;
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.scratch_active[0] = false;
                        }
                        self.bus.set("app.traktor.decks.1.jog.turntable", PropValue::Bool(false));
                    }
                    "surface.right.mode.turntable" => {
                        self.deck_turntable_mode[1] = true;
                        self.bus.set("app.traktor.decks.2.jog.turntable", PropValue::Bool(true));
                    }
                    "surface.right.mode.jogwheel" => {
                        self.deck_turntable_mode[1] = false;
                        self.deck_scratch_active[1] = false;
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.scratch_active[1] = false;
                        }
                        self.bus.set("app.traktor.decks.2.jog.turntable", PropValue::Bool(false));
                    }

                    // Deck Pad Mode Selectors (Hotcues, Stems, Pattern, Flux Loop)
                    "surface.left.stems" | "surface.left.pad_mode.stems" => {
                        self.deck_pad_mode[0] = DeckPadMode::Stems;
                        self.deck_stem_view[0] = true;
                        self.bus.set("app.traktor.decks.1.pad_mode", PropValue::Int(1));
                    }
                    "surface.right.stems" | "surface.right.pad_mode.stems" => {
                        self.deck_pad_mode[1] = DeckPadMode::Stems;
                        self.deck_stem_view[1] = true;
                        self.bus.set("app.traktor.decks.2.pad_mode", PropValue::Int(1));
                    }
                    "surface.left.hotcues" | "surface.left.pad_mode.hotcues" => {
                        self.deck_pad_mode[0] = DeckPadMode::Hotcues;
                        self.bus.set("app.traktor.decks.1.pad_mode", PropValue::Int(0));
                    }
                    "surface.right.hotcues" | "surface.right.pad_mode.hotcues" => {
                        self.deck_pad_mode[1] = DeckPadMode::Hotcues;
                        self.bus.set("app.traktor.decks.2.pad_mode", PropValue::Int(0));
                    }
                    "surface.left.pattern" | "surface.left.pad_mode.pattern" => {
                        self.deck_pad_mode[0] = DeckPadMode::Pattern;
                        self.show_pattern_player = true;
                        self.bus.set("app.traktor.decks.1.pad_mode", PropValue::Int(2));
                    }
                    "surface.right.pattern" | "surface.right.pad_mode.pattern" => {
                        self.deck_pad_mode[1] = DeckPadMode::Pattern;
                        self.show_pattern_player = true;
                        self.bus.set("app.traktor.decks.2.pad_mode", PropValue::Int(2));
                    }
                    "surface.left.flux_loop" | "surface.left.pad_mode.flux_loop" => {
                        self.deck_pad_mode[0] = DeckPadMode::FluxLoop;
                        self.bus.set("app.traktor.decks.1.pad_mode", PropValue::Int(3));
                    }
                    "surface.right.flux_loop" | "surface.right.pad_mode.flux_loop" => {
                        self.deck_pad_mode[1] = DeckPadMode::FluxLoop;
                        self.bus.set("app.traktor.decks.2.pad_mode", PropValue::Int(3));
                    }

                    "surface.left.browse.view" | "surface.right.browse.view" => {
                        self.layout = if self.layout == TraktorLayout::Browser {
                            TraktorLayout::Parallel
                        } else {
                            TraktorLayout::Browser
                        };
                    }
                    "surface.left.fx.buttons.1" => self.fx1_on = !self.fx1_on,
                    "surface.right.fx.buttons.1" => self.fx2_on = !self.fx2_on,
                    "surface.mixer.channels.1.cue" => self.channel_cue[0] = !self.channel_cue[0],
                    "surface.mixer.channels.2.cue" => self.channel_cue[1] = !self.channel_cue[1],
                    "surface.mixer.channels.1.channel_fx.on" => {
                        self.channel_filter_on[0] = !self.channel_filter_on[0];
                    }
                    "surface.mixer.channels.2.channel_fx.on" => {
                        self.channel_filter_on[1] = !self.channel_filter_on[1];
                    }
                    "surface.mixer.channel_fx.filter" => {
                        self.channel_fx_kind[0] = MixerFxKind::Filter;
                        self.channel_fx_kind[1] = MixerFxKind::Filter;
                    }
                    "surface.mixer.channel_fx.fx1" => {
                        self.channel_fx_kind[0] = MixerFxKind::Reverb;
                        self.channel_fx_kind[1] = MixerFxKind::Reverb;
                    }
                    "surface.mixer.channel_fx.fx2" => {
                        self.channel_fx_kind[0] = MixerFxKind::DualDelay;
                        self.channel_fx_kind[1] = MixerFxKind::DualDelay;
                    }
                    "surface.mixer.channel_fx.fx3" => {
                        self.channel_fx_kind[0] = MixerFxKind::Noise;
                        self.channel_fx_kind[1] = MixerFxKind::Noise;
                    }
                    "surface.mixer.channel_fx.fx4" => {
                        self.channel_fx_kind[0] = MixerFxKind::TimeGater;
                        self.channel_fx_kind[1] = MixerFxKind::TimeGater;
                    }
                    s if s.starts_with("surface.left.pads.") => {
                        if let Ok(pad) = s.trim_start_matches("surface.left.pads.").parse::<u8>() {
                            self.trigger_performance_pad(1, pad);
                        }
                    }
                    s if s.starts_with("surface.right.pads.") => {
                        if let Ok(pad) = s.trim_start_matches("surface.right.pads.").parse::<u8>() {
                            self.trigger_performance_pad(2, pad);
                        }
                    }
                    _ => {}
                }
            }
            SurfaceEvent::Encoder { control, delta } => match control.as_str() {
                "surface.left.jogwheel.rotation" => {
                    let sens = (self.jog_scratch_sensitivity as f64).clamp(0.2, 3.0);
                    if let Ok(mut state) = self.audio.shared.lock() {
                        if state.scratch_active[0] {
                            let scaled_ticks = ((delta as f64) * sens).round() as i64;
                            state.scratch_tick(0, scaled_ticks);
                        } else {
                            // Nudge / Phase bend when outer rim is turned without touching top
                            let nudge_sec = (delta as f64 / 1024.0) * 0.08 * sens;
                            state.playhead_sec[0] = (state.playhead_sec[0] + nudge_sec).max(0.0);
                        }
                    }
                }
                "surface.right.jogwheel.rotation" => {
                    let sens = (self.jog_scratch_sensitivity as f64).clamp(0.2, 3.0);
                    if let Ok(mut state) = self.audio.shared.lock() {
                        if state.scratch_active[1] {
                            let scaled_ticks = ((delta as f64) * sens).round() as i64;
                            state.scratch_tick(1, scaled_ticks);
                        } else {
                            let nudge_sec = (delta as f64 / 1024.0) * 0.08 * sens;
                            state.playhead_sec[1] = (state.playhead_sec[1] + nudge_sec).max(0.0);
                        }
                    }
                }
                "surface.left.loop_size" => {
                    if delta > 0 {
                        self.deck_loop_size_beats[0] = (self.deck_loop_size_beats[0] * 2).min(32);
                    } else if delta < 0 {
                        self.deck_loop_size_beats[0] = (self.deck_loop_size_beats[0] / 2).max(1);
                    }
                }
                "surface.right.loop_size" => {
                    if delta > 0 {
                        self.deck_loop_size_beats[1] = (self.deck_loop_size_beats[1] * 2).min(32);
                    } else if delta < 0 {
                        self.deck_loop_size_beats[1] = (self.deck_loop_size_beats[1] / 2).max(1);
                    }
                }
                "surface.left.browse.encoder" => {
                    if delta > 0 {
                        self.library.select_next_track();
                    } else if delta < 0 {
                        self.library.select_prev_track();
                    }
                }
                "surface.right.browse.encoder" => {
                    if delta > 0 {
                        self.library.select_next_track();
                    } else if delta < 0 {
                        self.library.select_prev_track();
                    }
                }
                _ => {}
            },
        }
    }

    fn trigger_performance_pad(&mut self, deck_idx: usize, pad_num: u8) {
        let d = (deck_idx - 1).min(3);
        let pad_idx = (pad_num.saturating_sub(1) as usize).min(7);
        match self.deck_pad_mode[d] {
            DeckPadMode::Hotcues => {
                self.jump_to_cue(deck_idx, pad_num);
            }
            DeckPadMode::Stems => {
                if pad_idx < 4 {
                    self.deck_stem_muted[d][pad_idx] = !self.deck_stem_muted[d][pad_idx];
                    let prop = format!("app.traktor.decks.{}.stems.{}.muted", d + 1, pad_idx);
                    self.bus.set(&prop, PropValue::Bool(self.deck_stem_muted[d][pad_idx]));
                    if let Ok(mut state) = self.audio.shared.lock() {
                        state.stem_muted[d][pad_idx] = self.deck_stem_muted[d][pad_idx];
                    }
                } else {
                    let s = pad_idx - 4;
                    self.deck_stem_fx_send[d][s] = !self.deck_stem_fx_send[d][s];
                }
            }
            DeckPadMode::Pattern => {
                self.pattern_enabled = true;
                self.show_pattern_player = true;
                let step_offset = if deck_idx == 2 { 8 } else { 0 };
                let step = (step_offset + pad_idx).min(15);
                let v = self.pattern_selected_voice.min(3);
                self.pattern_steps[v][step] = !self.pattern_steps[v][step];
            }
            DeckPadMode::FluxLoop => {
                let sizes = [1, 1, 2, 4, 8, 16, 32, 32];
                self.deck_loop_size_beats[d] = sizes[pad_idx];
                self.deck_flux_active[d] = true;
                self.toggle_loop(deck_idx);
            }
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Comma)) {
            self.preferences.is_open = !self.preferences.is_open;
        }

        if ctx.input(|i| (i.modifiers.shift && i.key_pressed(Key::Slash)) || i.key_pressed(Key::Questionmark)) {
            self.show_shortcuts_hud = !self.show_shortcuts_hud;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) && self.show_shortcuts_hud {
            self.show_shortcuts_hud = false;
        }

        if ctx.input(|i| i.key_pressed(Key::Escape)) && self.preferences.is_open {
            self.preferences.save();
            self.preferences.is_open = false;
        }

        // Layout shortcuts F1..F4
        for (key, lay) in [
            (Key::F1, TraktorLayout::Parallel),
            (Key::F2, TraktorLayout::FourDeck),
            (Key::F3, TraktorLayout::Browser),
            (Key::F4, TraktorLayout::Timeline),
        ] {
            if ctx.input(|i| i.key_pressed(key)) {
                self.layout = lay;
                self.preferences.four_deck_mode = lay == TraktorLayout::FourDeck;
            }
        }

        // Only process single-key deck shortcuts when not typing in a text field
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::Space)) {
                self.toggle_play(1);
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::Space)) {
                self.toggle_play(2);
            }
            if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::C)) {
                self.jump_to_cue(1, 1);
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::C)) {
                self.jump_to_cue(2, 1);
            }
            if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::S)) {
                self.sync_deck(1);
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::S)) {
                self.sync_deck(2);
            }
        }
    }

    fn toggle_play(&mut self, deck_idx: usize) {
        if let Ok(mut state) = self.audio.shared.lock() {
            let d = deck_idx - 1;
            state.is_playing[d] = !state.is_playing[d];
        }
    }

    fn jump_to_cue(&mut self, deck_idx: usize, cue_num: u8) {
        let d = deck_idx - 1;
        if let Some(&(_, cue_sec)) = self.deck_hotcues[d].iter().find(|(id, _)| *id == cue_num) {
            if let Ok(mut state) = self.audio.shared.lock() {
                state.playhead_sec[d] = cue_sec;
            }
        } else if let Ok(state) = self.audio.shared.lock() {
            let current = state.playhead_sec[d];
            self.deck_hotcues[d].push((cue_num, current));
        }
    }

    fn sync_deck(&mut self, deck_idx: usize) {
        let d = (deck_idx.saturating_sub(1)).min(3);
        self.deck_sync_enabled[d] = !self.deck_sync_enabled[d];

        // Update PropertyBus for hardware LED backlight
        let bus_key = format!("app.traktor.decks.{}.sync.enabled", d + 1);
        self.bus.set(&bus_key, PropValue::Bool(self.deck_sync_enabled[d]));

        if !self.deck_sync_enabled[d] {
            return;
        }

        // Tempo follows in sync_audio_state; here the deck jumps straight onto the other deck's beat.
        if let Ok(mut state) = self.audio.shared.lock() {
            let other = if d == 0 { 1 } else { 0 };
            let grid = |i: usize| state.loaded_tracks[i].as_ref().map(|t| (t.bpm.max(20.0), t.grid_offset_sec));
            if let (Some((bpm, offset)), Some((other_bpm, other_offset)), true) = (grid(d), grid(other), state.is_playing[other]) {
                let behind = beat_phase_diff(
                    (state.playhead_sec[other] - other_offset) * other_bpm / 60.0,
                    (state.playhead_sec[d] - offset) * bpm / 60.0,
                );
                state.playhead_sec[d] = (state.playhead_sec[d] + behind * 60.0 / bpm).max(0.0);
            }
        }
    }

    fn toggle_loop(&mut self, deck_idx: usize) {
        let d = deck_idx - 1;
        self.deck_loop_active[d] = !self.deck_loop_active[d];
        if self.deck_loop_active[d] {
            if let Ok(state) = self.audio.shared.lock() {
                self.deck_loop_start_sec[d] = state.playhead_sec[d];
            }
        }
    }

    /// Top bar, left to right: layout tabs, master tempo, FX units, pattern player;
    /// right-aligned: status lamps, recorder, main level, help and settings.
    fn show_top_bar(&mut self, ui: &mut Ui) {
        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            row(ui, 40.0, |ui| {
                // The wordmark is the first thing to go when the window is narrow.
                if ui.available_width() >= 1180.0 {
                    ui.label(RichText::new("ReTraktor").size(14.0).color(TraktorTheme::TEXT_HIGHLIGHT()));
                    ui.separator();
                }

                for (lay, key) in [
                    (TraktorLayout::Parallel, "F1"),
                    (TraktorLayout::FourDeck, "F2"),
                    (TraktorLayout::Browser, "F3"),
                    (TraktorLayout::Timeline, "F4"),
                ] {
                    if pill(ui, lay.label(), self.layout == lay, TraktorTheme::ROW_SELECTED(), 54.0)
                        .on_hover_text(format!("{} layout ({key})", lay.label()))
                        .clicked()
                    {
                        self.layout = lay;
                        self.preferences.four_deck_mode = lay == TraktorLayout::FourDeck;
                    }
                }
                ui.separator();

                display(ui, |ui, ink| {
                    readout(ui, "MASTER BPM", &format!("{:.2}", self.master_tempo), 20.0, ink);
                });
                if pill(ui, "−", false, Color32::TRANSPARENT, HIT).on_hover_text("Master tempo down 0.5 BPM").clicked() {
                    self.master_tempo = (self.master_tempo - 0.5).max(40.0);
                }
                if pill(ui, "+", false, Color32::TRANSPARENT, HIT).on_hover_text("Master tempo up 0.5 BPM").clicked() {
                    self.master_tempo = (self.master_tempo + 0.5).min(240.0);
                }
                // Below ~1150 px the bar drops its least-used items rather than overlap.
                let roomy = ui.ctx().screen_rect().width() >= 1150.0;
                if roomy
                    && pill(ui, "ROUND", false, Color32::TRANSPARENT, 0.0)
                        .on_hover_text("Round the master tempo to a whole BPM")
                        .clicked()
                {
                    self.master_tempo = self.master_tempo.round().clamp(60.0, 200.0);
                }
                ui.separator();

                fx_unit(ui, 1, &mut self.fx1_on, &mut self.fx1_dry_wet, &mut self.fx1_param[0], "REV", "reverb amount");
                ui.separator();
                fx_unit(ui, 2, &mut self.fx2_on, &mut self.fx2_dry_wet, &mut self.fx2_param[0], "DLY", "delay amount");
                ui.separator();

                // Lit in the signal colour while the pattern is sounding, even if its strip is hidden.
                let pattern_fill = if self.pattern_enabled { TraktorTheme::ACCENT() } else { TraktorTheme::ROW_SELECTED() };
                if pill(ui, "PATTERN", self.show_pattern_player || self.pattern_enabled, pattern_fill, 0.0)
                    .on_hover_text("Show or hide the 16-step pattern player (drum machine)")
                    .clicked()
                {
                    self.show_pattern_player = !self.show_pattern_player;
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(egui::Button::new(RichText::new("⚙").size(15.0)).min_size(Vec2::new(28.0, HIT)))
                        .on_hover_text("Preferences (Ctrl+,)")
                        .clicked()
                    {
                        self.preferences.is_open = !self.preferences.is_open;
                    }
                    if ui
                        .add(egui::Button::new(RichText::new("?").size(14.0)).min_size(Vec2::new(28.0, HIT)))
                        .on_hover_text("Keyboard shortcuts (?)")
                        .clicked()
                    {
                        self.show_shortcuts_hud = !self.show_shortcuts_hud;
                    }

                    rotary_knob_sized(ui, &mut self.master_volume, 0.0, 1.0, 0.8, "MAIN", TraktorTheme::TEXT_PRIMARY(), 28.0)
                        .on_hover_text(format!("Main output level. {KNOB_TIP}"));

                    let (peak_l, peak_r, is_rec, rec_sec) = if let Ok(state) = self.audio.shared.lock() {
                        let secs = state.recorded_samples.len() as f64 / (48_000.0 * 2.0);
                        (state.master_peak_l, state.master_peak_r, state.is_recording, secs)
                    } else {
                        (0.0, 0.0, false, 0.0)
                    };
                    vu_meter(ui, peak_l, peak_r, 30.0);

                    // Recording shows as a filled button with a running clock, not by colour alone.
                    let rec_label = if is_rec {
                        format!("REC {:02}:{:02}", (rec_sec / 60.0).floor() as u32, (rec_sec % 60.0).floor() as u32)
                    } else {
                        "REC".to_string()
                    };
                    let (rec_fill, rec_fg) = if is_rec {
                        (TraktorTheme::ACCENT(), on(TraktorTheme::ACCENT()))
                    } else {
                        (TraktorTheme::CONTROL(), TraktorTheme::TEXT_SECONDARY())
                    };
                    if icon_pill(ui, Icon::Dot, &rec_label, rec_fill, rec_fg, Vec2::new(0.0, HIT))
                        .on_hover_text(if is_rec {
                            "Stop and save the recording to ~/Music/ReTraktor_Master_Mix.wav"
                        } else {
                            "Record the main mix (16-bit 48 kHz WAV in ~/Music)"
                        })
                        .clicked()
                    {
                        let mut samples_to_save: Option<Vec<i16>> = None;
                        if let Ok(mut state) = self.audio.shared.lock() {
                            if state.is_recording {
                                state.is_recording = false;
                                samples_to_save = Some(std::mem::take(&mut state.recorded_samples));
                            } else {
                                state.recorded_samples = Vec::with_capacity(48_000 * 2 * 1800);
                                state.is_recording = true;
                            }
                        }
                        if let Some(samples) = samples_to_save {
                            let home = env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
                            let music_dir = home.join("Music");
                            let out_dir = if music_dir.is_dir() { music_dir } else { home };
                            let out_path = out_dir.join("ReTraktor_Master_Mix.wav");
                            match AudioSystem::write_stereo_wav_file(&out_path, &samples, 48_000) {
                                Ok(dur_s) => {
                                    self.library.status_message =
                                        format!("✔ Saved Master Mix recording ({dur_s:.1}s) to {}", out_path.display());
                                }
                                Err(e) => {
                                    self.library.status_message = format!("⚠ Failed to save WAV recording: {e}");
                                }
                            }
                        }
                    }

                    let controller_tip = match &self.mx2_dev_node {
                        Some(node) => format!("Traktor MX2 connected ({node}). Click for controller settings"),
                        None => "No controller connected. Click for controller settings".to_string(),
                    };
                    if lamp(ui, "MX2", self.mx2_dev_node.is_some()).on_hover_text(controller_tip).clicked() {
                        self.preferences.is_open = true;
                        self.preferences.current_tab = PrefsTab::Controllers;
                    }
                    if roomy
                        && lamp(ui, "48 kHz", true)
                            .on_hover_text("Audio out: PipeWire, 48 kHz. Click for audio settings")
                            .clicked()
                    {
                        self.preferences.is_open = true;
                        self.preferences.current_tab = PrefsTab::AudioSetup;
                    }
                });
            });
        });
    }

    /// 16-step pattern player (drum machine locked to the master tempo).
    fn show_pattern_player_bar(&mut self, ui: &mut Ui) {
        let cur_step = self.audio.shared.lock().map(|s| s.pattern_current_step).unwrap_or(0);

        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            row(ui, 33.0, |ui| {
                let running = self.pattern_enabled;
                let (fill, fg) = if running {
                    (TraktorTheme::ACCENT(), on(TraktorTheme::ACCENT()))
                } else {
                    (TraktorTheme::CONTROL(), TraktorTheme::TEXT_SECONDARY())
                };
                let icon = if running { Icon::Pause } else { Icon::Play };
                if icon_pill(ui, icon, "PATTERN", fill, fg, Vec2::new(0.0, HIT))
                    .on_hover_text(if running { "Stop the pattern" } else { "Start the pattern (follows the master tempo)" })
                    .clicked()
                {
                    self.pattern_enabled = !running;
                }
                ui.separator();

                for (v, (label, name)) in
                    [("KICK", "808 kick"), ("CLAP", "909 clap"), ("HAT", "909 hat"), ("PERC", "rave percussion")].into_iter().enumerate()
                {
                    if pill(ui, label, self.pattern_selected_voice == v, TraktorTheme::ROW_SELECTED(), 44.0)
                        .on_hover_text(format!("Edit the {name} steps"))
                        .clicked()
                    {
                        self.pattern_selected_voice = v;
                    }
                }
                ui.separator();

                // Steps share whatever is left after the fixed controls to their right.
                let v = self.pattern_selected_voice.min(3);
                let step_w = ((ui.available_width() - 250.0) / 16.0).clamp(HIT, 40.0);
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for step in 0..16 {
                        if step > 0 && step % 4 == 0 {
                            ui.add_space(4.0); // one gap per beat
                        }
                        let resp = pill(ui, &format!("{}", step + 1), self.pattern_steps[v][step], TraktorTheme::ACCENT(), step_w)
                            .on_hover_text(format!("Step {}", step + 1));
                        if running && cur_step == step {
                            ui.painter().rect_stroke(resp.rect, 2.0, Stroke::new(2.0_f32, TraktorTheme::TEXT_HIGHLIGHT()));
                        }
                        if resp.clicked() {
                            self.pattern_steps[v][step] = !self.pattern_steps[v][step];
                        }
                    }
                });
                ui.separator();

                if pill(ui, "4X4", false, Color32::TRANSPARENT, 0.0).on_hover_text("Fill with a four-on-the-floor pattern").clicked() {
                    self.pattern_steps[v] = [false; 16];
                    for s in (0..16).step_by(4) {
                        self.pattern_steps[v][s] = true;
                    }
                }
                if pill(ui, "ROLL", false, Color32::TRANSPARENT, 0.0).on_hover_text("Fill every step (16th-note roll)").clicked() {
                    self.pattern_steps[v] = [true; 16];
                }
                if pill(ui, "CLEAR", false, Color32::TRANSPARENT, 0.0).on_hover_text("Clear this voice's steps").clicked() {
                    self.pattern_steps[v] = [false; 16];
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    rotary_knob_sized(ui, &mut self.pattern_volume, 0.0, 1.0, 0.75, "LVL", TraktorTheme::TEXT_PRIMARY(), 28.0)
                        .on_hover_text(format!("Pattern level. {KNOB_TIP}"));
                });
            });
        });
    }

    /// Timeline layout: decks A and B as stacked waveforms with transition presets.
    fn show_timeline_studio_layout(&mut self, ui: &mut Ui) {
        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(caps("TRANSITION"));
                for bars in [8u32, 16, 32] {
                    if pill(ui, &format!("{bars}"), self.timeline_transition_bars == bars, TraktorTheme::ROW_SELECTED(), 32.0)
                        .on_hover_text(format!("Transition over {bars} bars"))
                        .clicked()
                    {
                        self.timeline_transition_bars = bars;
                    }
                }
                ui.label(caps("BARS"));
                ui.separator();
                ui.label(caps("CURVE"));
                for (idx, name, tip) in [
                    (0usize, "CROSSFADE", "Smooth crossfade: every stem plays on both decks"),
                    (1usize, "BASS SWAP", "Deck A drops its bass as deck B brings its bass in"),
                    (2usize, "VOCAL BRIDGE", "Deck A's vocals over deck B's instrumental"),
                ] {
                    if pill(ui, name, self.timeline_preset_idx == idx, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.timeline_preset_idx = idx;
                        // Apply stem automation preset to Decks A & B
                        match idx {
                            0 => {
                                self.deck_stem_muted[0] = [false; 4];
                                self.deck_stem_muted[1] = [false; 4];
                            }
                            1 => {
                                // Outgoing Deck A drops Bass; Incoming Deck B brings Bass
                                self.deck_stem_muted[0][1] = true;
                                self.deck_stem_muted[1][1] = false;
                            }
                            _ => {
                                // Outgoing Deck A isolates Vocals over Incoming Deck B Instrumental
                                self.deck_stem_muted[0] = [true, true, true, false];
                                self.deck_stem_muted[1] = [false, false, false, true];
                            }
                        }
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (fill, fg) = (TraktorTheme::CONTROL(), TraktorTheme::TEXT_PRIMARY());
                    if icon_pill(ui, Icon::Play, "AUDITION", fill, fg, Vec2::new(0.0, HIT))
                        .on_hover_text("Sync deck B to deck A and play both")
                        .clicked()
                    {
                        self.sync_deck(2);
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.is_playing[0] = true;
                            state.is_playing[1] = true;
                        }
                    }
                });
            });

            let wave_h = mix_wave_height(ui) * 1.25;
            self.show_parallel_waveform_row(ui, 1, TraktorTheme::DECK_A(), wave_h, true);
            self.show_parallel_waveform_row(ui, 2, TraktorTheme::DECK_B(), wave_h, true);
        });
    }

    /// Mix layout: full-width waveforms stacked so the beatgrids line up, then deck A | mixer | deck B.
    fn show_parallel_layout(&mut self, ui: &mut Ui) {
        let four_deck = self.layout == TraktorLayout::FourDeck || self.preferences.four_deck_mode;
        let wave_h = mix_wave_height(ui) * if four_deck { 0.7 } else { 1.0 };

        // Waveforms only for loaded decks; an empty deck's own panel already says how to load it.
        let decks = if four_deck { 4 } else { 2 };
        let loaded: Vec<usize> = match self.audio.shared.lock() {
            Ok(s) => (1..=decks).filter(|d| s.loaded_tracks[d - 1].is_some()).collect(),
            Err(_) => Vec::new(),
        };
        if !loaded.is_empty() {
            panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for deck_idx in loaded {
                    self.show_parallel_waveform_row(ui, deck_idx, TraktorTheme::deck_color(deck_idx), wave_h, false);
                }
            });
        }

        ui.horizontal_top(|ui| {
            let mixer_w = 320.0_f32;
            let deck_w = ((ui.available_width() - mixer_w - 8.0) / 2.0).max(260.0);
            // Deck panels stretch to the mixer's height (from the previous frame) so the row has one bottom edge.
            let height_id = egui::Id::new("mix_row_height");
            let row_h = ui.data(|d| d.get_temp::<f32>(height_id)).unwrap_or(0.0);

            ui.allocate_ui_with_layout(Vec2::new(deck_w, row_h), Layout::top_down(Align::Min), |ui| {
                ui.set_width(deck_w);
                self.show_single_deck(ui, 1, TraktorTheme::DECK_A(), false, 0.0, row_h);
            });
            let mixer = ui.allocate_ui_with_layout(Vec2::new(mixer_w, 0.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(mixer_w);
                self.show_mixer_section(ui, true);
            });
            ui.data_mut(|d| d.insert_temp(height_id, mixer.response.rect.height()));
            ui.allocate_ui_with_layout(Vec2::new(deck_w, row_h), Layout::top_down(Align::Min), |ui| {
                ui.set_width(deck_w);
                self.show_single_deck(ui, 2, TraktorTheme::DECK_B(), false, 0.0, row_h);
            });
        });
    }

    /// One full-width scrolling waveform. `header` adds the track line and readouts for layouts
    /// with no deck panel underneath (Timeline).
    fn show_parallel_waveform_row(&mut self, ui: &mut Ui, deck_idx: usize, accent_color: Color32, height: f32, header: bool) {
        let rect = ui.scope(|ui| self.show_parallel_waveform_row_inner(ui, deck_idx, accent_color, height, header)).response.rect;
        self.deck_drop(ui, rect, deck_idx);
    }

    fn show_parallel_waveform_row_inner(&mut self, ui: &mut Ui, deck_idx: usize, accent_color: Color32, height: f32, header: bool) {
        let d = deck_idx - 1;
        let deck_letter = DECK_LETTERS[d.min(3)];
        let Some(deck) = self.deck_view(d) else {
            row(ui, (height * 0.5).max(HIT), |ui| {
                empty_deck_hint(ui, deck_letter, accent_color, "Drop a track here, or double-click one in the library");
            });
            return;
        };

        if header {
            row(ui, 40.0, |ui| {
                deck_badge(ui, deck_letter, accent_color);
                self.stem_buttons(ui, d);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    display(ui, |ui, ink| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        readout(ui, "BPM", &format!("{:.2}", deck.bpm * (1.0 + self.deck_pitch_fader[d] as f64 / 100.0)), 20.0, ink);
                        readout(ui, "KEY", &deck.key, 20.0, ink);
                        readout(ui, "REMAIN", &deck.remaining(), 20.0, ink);
                    });
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| title_label(ui, &deck.title, &deck.artist));
                });
            });
        }

        let cues = self.deck_hotcues[d].clone();
        let loop_len = self.deck_loop_size_beats[d] as f64 * (60.0 / deck.bpm.max(30.0));
        let wave_resp = if self.deck_stem_view[d] {
            scrolling_stem_waveforms(
                ui,
                deck.elapsed_sec,
                deck.bpm,
                deck.grid_offset_sec,
                &cues,
                &deck.stem_peaks,
                &self.deck_stem_volumes[d],
                &mut self.deck_stem_muted[d],
                self.deck_loop_active[d],
                self.deck_loop_start_sec[d],
                loop_len,
                Some((deck_letter, accent_color)),
                height,
            )
        } else {
            scrolling_waveform(
                ui,
                deck.elapsed_sec,
                deck.bpm,
                deck.grid_offset_sec,
                &cues,
                &deck.peaks,
                self.deck_loop_active[d],
                self.deck_loop_start_sec[d],
                loop_len,
                Some((deck_letter, accent_color)),
                height,
            )
        };
        self.scrub_by_drag(d, &wave_resp, deck.duration_sec);
    }

    /// Dragging a scrolling waveform moves the playhead.
    fn scrub_by_drag(&mut self, d: usize, wave_resp: &egui::Response, duration_sec: f64) {
        if wave_resp.dragged() {
            let dt = -wave_resp.drag_delta().x as f64 / 85.0;
            if let Ok(mut state) = self.audio.shared.lock() {
                state.playhead_sec[d] = (state.playhead_sec[d] + dt).clamp(0.0, duration_sec);
            }
        }
    }

    /// 4-Deck layout: decks A + C | four-channel mixer | decks B + D, each deck with its own waveform.
    fn show_four_deck_layout(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            let mixer_w = 348.0_f32;
            let deck_w = ((ui.available_width() - mixer_w - 8.0) / 2.0).max(260.0);
            // Shorter waveforms on short windows keep the browser on screen.
            let wave_h = (36.0 + (ui.ctx().screen_rect().height() - 640.0) * 0.1).clamp(36.0, 80.0);

            ui.allocate_ui_with_layout(Vec2::new(deck_w, 340.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(deck_w);
                self.show_single_deck(ui, 1, TraktorTheme::DECK_A(), true, wave_h, 0.0);
                self.show_single_deck(ui, 3, TraktorTheme::DECK_C(), true, wave_h, 0.0);
            });
            ui.allocate_ui_with_layout(Vec2::new(mixer_w, 340.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(mixer_w);
                self.show_mixer_section(ui, false);
            });
            ui.allocate_ui_with_layout(Vec2::new(deck_w, 340.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(deck_w);
                self.show_single_deck(ui, 2, TraktorTheme::DECK_B(), true, wave_h, 0.0);
                self.show_single_deck(ui, 4, TraktorTheme::DECK_D(), true, wave_h, 0.0);
            });
        });
    }

    /// Browser layout: decks A and B as one-line strips either side of the crossfader.
    fn show_mini_decks_bar(&mut self, ui: &mut Ui) {
        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            row(ui, 40.0, |ui| {
                let xfader_w = 140.0;
                // Two equal halves: what is left after the crossfader, two separators and four gaps.
                let side = ((ui.available_width() - xfader_w - 2.0 * 6.0 - 4.0 * ui.spacing().item_spacing.x) / 2.0).max(200.0);
                for deck_idx in [1, 2] {
                    if deck_idx == 2 {
                        ui.separator();
                        crossfader(ui, &mut self.crossfader_pos, xfader_w).on_hover_text(XFADER_TIP);
                        ui.separator();
                    }
                    ui.allocate_ui_with_layout(Vec2::new(side, 40.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.set_width(side);
                        self.show_mini_deck_strip(ui, deck_idx, TraktorTheme::deck_color(deck_idx));
                    });
                }
            });
        });
    }

    fn show_mini_deck_strip(&mut self, ui: &mut Ui, deck_idx: usize, accent: Color32) {
        let rect = ui.scope(|ui| self.show_mini_deck_strip_inner(ui, deck_idx, accent)).response.rect;
        self.deck_drop(ui, rect, deck_idx);
    }

    /// Loads a track dragged from the library onto this deck's area.
    fn deck_drop(&mut self, ui: &Ui, rect: egui::Rect, deck_idx: usize) {
        if let Some(id) = crate::library_ui::deck_drop_target(ui, rect) {
            let locked = self.locked_decks();
            self.library.load_to_deck(deck_idx, id, locked);
            self.drain_pending_loads();
        }
    }

    fn show_mini_deck_strip_inner(&mut self, ui: &mut Ui, deck_idx: usize, accent: Color32) {
        let d = deck_idx - 1;
        let letter = DECK_LETTERS[d.min(3)];
        let Some(deck) = self.deck_view(d) else {
            empty_deck_hint(ui, letter, accent, "Empty. Drop a track here");
            return;
        };

        deck_badge(ui, letter, accent);
        // Play state reads from the icon as well as the fill.
        let (icon, fill, fg) = if deck.is_playing {
            (Icon::Pause, TraktorTheme::PLAY_ACTIVE(), on(TraktorTheme::PLAY_ACTIVE()))
        } else {
            (Icon::Play, TraktorTheme::PLAY_INACTIVE(), TraktorTheme::TEXT_PRIMARY())
        };
        let shortcut = ["(Space)", "(Shift+Space)", "", ""][d.min(3)];
        if icon_pill(ui, icon, "", fill, fg, Vec2::new(34.0, HIT))
            .on_hover_text(format!("{} deck {letter} {shortcut}", if deck.is_playing { "Pause" } else { "Play" }))
            .clicked()
        {
            self.toggle_play(deck_idx);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            display(ui, |ui, ink| {
                ui.spacing_mut().item_spacing.x = 14.0;
                readout(ui, "BPM", &format!("{:.2}", deck.bpm * (1.0 + self.deck_pitch_fader[d] as f64 / 100.0)), 20.0, ink);
                readout(ui, "REMAIN", &deck.remaining(), 20.0, ink);
            });
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| title_label(ui, &deck.title, &deck.artist));
        });
    }

    /// `min_height` stretches the panel to line up with its neighbours (0 = as tall as its content).
    fn show_single_deck(
        &mut self,
        ui: &mut Ui,
        deck_idx: usize,
        accent_color: Color32,
        include_scrolling_waveform: bool,
        waveform_height: f32,
        min_height: f32,
    ) {
        let rect = ui
            .scope(|ui| self.show_single_deck_inner(ui, deck_idx, accent_color, include_scrolling_waveform, waveform_height, min_height))
            .response
            .rect;
        self.deck_drop(ui, rect, deck_idx);
    }

    /// Snapshot of what a deck needs to draw itself; `None` while the deck is empty.
    fn deck_view(&self, d: usize) -> Option<DeckView> {
        let state = self.audio.shared.lock().ok()?;
        let t = state.loaded_tracks[d].as_ref()?;
        Some(DeckView {
            title: t.title.clone(),
            artist: t.artist.clone(),
            bpm: t.bpm,
            grid_offset_sec: t.grid_offset_sec,
            key: t.key.clone(),
            duration_sec: t.duration_sec,
            elapsed_sec: state.playhead_sec[d],
            is_playing: state.is_playing[d],
            peaks: t.waveform_peaks.clone(),
            stem_peaks: t.stem_peaks.clone(),
        })
    }

    /// STEMS view toggle, plus the one-click isolators while the view is on.
    fn stem_buttons(&mut self, ui: &mut Ui, d: usize) {
        let view_on = self.deck_stem_view[d];
        if pill(ui, "STEMS", view_on, TraktorTheme::ROW_SELECTED(), 0.0)
            .on_hover_text("Show the track as four stem lanes: drums, bass, other, vocals")
            .clicked()
        {
            self.deck_stem_view[d] = !view_on;
        }
        if !view_on {
            return;
        }
        for (label, mutes, tip) in [
            ("ACAPELLA", [true, true, true, false], "Vocals only: mute drums, bass and other"),
            ("INST", [false, false, false, true], "Instrumental: mute the vocals"),
        ] {
            let active = self.deck_stem_muted[d] == mutes;
            if pill(ui, label, active, TraktorTheme::ACCENT(), 0.0).on_hover_text(tip).clicked() {
                self.deck_stem_muted[d] = if active { [false; 4] } else { mutes };
            }
        }
    }

    /// An unloaded deck says how to load it, and shows nothing else.
    fn show_empty_deck(&self, ui: &mut Ui, deck_idx: usize, accent: Color32, min_height: f32) {
        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height((min_height - 12.0).max(HIT));
            ui.horizontal(|ui| {
                empty_deck_hint(ui, DECK_LETTERS[(deck_idx - 1).min(3)], accent, "Drop a track here, or double-click one in the library");
            });
        });
    }

    fn show_single_deck_inner(
        &mut self,
        ui: &mut Ui,
        deck_idx: usize,
        accent_color: Color32,
        include_scrolling_waveform: bool,
        waveform_height: f32,
        min_height: f32,
    ) {
        let d = deck_idx - 1;
        let deck_letter = DECK_LETTERS[d.min(3)];
        let is_loaded = self.audio.shared.lock().is_ok_and(|s| s.loaded_tracks[d].is_some());
        let loaded_t = crate::theme::fade(ui.ctx(), ("deck_loaded", d), is_loaded);
        let pads_t = crate::theme::fade(ui.ctx(), ("deck_pads", d), self.deck_expanded[d]);
        let Some(deck) = self.deck_view(d) else {
            self.show_empty_deck(ui, deck_idx, accent_color, min_height);
            return;
        };
        let bpm = deck.bpm;
        // Keys are compared against deck A, the reference deck.
        let in_key = deck_idx != 1
            && self.audio.shared.lock().is_ok_and(|s| {
                s.loaded_tracks[0].as_ref().is_some_and(|a| KeyWheel::are_harmonically_compatible(&a.key, &deck.key))
            });

        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            ui.set_min_height((min_height - 12.0).max(0.0));
            ui.multiply_opacity(loaded_t);

            // 1. Which deck, which track.
            ui.horizontal(|ui| {
                deck_badge(ui, deck_letter, accent_color);
                title_label(ui, &deck.title, &deck.artist);
            });

            // 2. Display: the numbers read at arm's length, as large as the deck's width allows.
            display(ui, |ui, ink| {
                ui.spacing_mut().item_spacing.x = 14.0;
                let advance = ui.fonts(|f| f.glyph_width(&crate::theme::dot_font(100.0), '0')) / 100.0;
                let size = ((ui.available_width() - 3.0 * 14.0) / (20.0 * advance)).floor().clamp(18.0, 30.0);
                let pitch_pct = self.deck_pitch_fader[d];
                beat_lamp(ui, ink, deck.is_playing, deck.elapsed_sec - deck.grid_offset_sec, bpm);
                readout(ui, "BPM", &format!("{:.2}", bpm * (1.0 + pitch_pct as f64 / 100.0)), size, ink);
                readout(ui, "PITCH", &format!("{pitch_pct:+.1}%"), size, ink);
                // A compatible key says so in words, not just in brightness.
                readout(ui, if in_key { "IN KEY" } else { "KEY" }, &deck.key, size, ink);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    readout(ui, "REMAIN", &deck.remaining(), size, ink);
                });
            });

            // 3. Sync, tempo master, stems; "MORE" discloses the performance pads.
            ui.horizontal(|ui| {
                if pill(ui, "SYNC", self.deck_sync_enabled[d], TraktorTheme::ACCENT(), 0.0)
                    .on_hover_text("Match tempo and beat phase to the other deck")
                    .clicked()
                {
                    self.sync_deck(deck_idx);
                }
                if pill(ui, "MASTER", (self.master_tempo - bpm).abs() < 0.05, TraktorTheme::ROW_SELECTED(), 0.0)
                    .on_hover_text("Use this track's tempo as the master tempo")
                    .clicked()
                {
                    self.master_tempo = bpm;
                }
                self.stem_buttons(ui, d);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let expanded = self.deck_expanded[d];
                    if pill(ui, "MORE", expanded, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text("Hotcue pads, pad modes, cue-play, flux and every loop size")
                        .clicked()
                    {
                        self.deck_expanded[d] = !expanded;
                    }
                });
            });

            // 4. Scrolling waveform (4-Deck; Mix draws them full width above).
            let cues = self.deck_hotcues[d].clone();
            let loop_len_sec = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
            if include_scrolling_waveform && waveform_height > 10.0 {
                let wave_resp = if self.deck_stem_view[d] {
                    scrolling_stem_waveforms(
                        ui,
                        deck.elapsed_sec,
                        bpm,
                        deck.grid_offset_sec,
                        &cues,
                        &deck.stem_peaks,
                        &self.deck_stem_volumes[d],
                        &mut self.deck_stem_muted[d],
                        self.deck_loop_active[d],
                        self.deck_loop_start_sec[d],
                        loop_len_sec,
                        None,
                        waveform_height,
                    )
                } else {
                    scrolling_waveform(
                        ui,
                        deck.elapsed_sec,
                        bpm,
                        deck.grid_offset_sec,
                        &cues,
                        &deck.peaks,
                        self.deck_loop_active[d],
                        self.deck_loop_start_sec[d],
                        loop_len_sec,
                        None,
                        waveform_height,
                    )
                };
                self.scrub_by_drag(d, &wave_resp, deck.duration_sec);
            }

            // 5. Stem mutes, while the stems view is on.
            if self.deck_stem_view[d] {
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 3.0 * ui.spacing().item_spacing.x) / 4.0;
                    for (s, name) in ["DRUMS", "BASS", "OTHER", "VOCALS"].into_iter().enumerate() {
                        let label = if self.deck_stem_fx_send[d][s] { format!("{name} · FX") } else { name.to_string() };
                        let resp = pill(ui, &label, !self.deck_stem_muted[d][s], accent_color, w)
                            .on_hover_text("Click to mute or unmute this stem. Right-click to send it to the FX");
                        if resp.clicked() {
                            self.deck_stem_muted[d][s] = !self.deck_stem_muted[d][s];
                        }
                        if resp.secondary_clicked() {
                            self.deck_stem_fx_send[d][s] = !self.deck_stem_fx_send[d][s];
                        }
                    }
                });
            }

            // 6. Whole-track overview with scrubber.
            let mut scrub_pos = deck.elapsed_sec;
            let stripe_resp = stripe_overview(
                ui,
                &mut scrub_pos,
                deck.duration_sec,
                &deck.peaks,
                &cues,
                self.deck_loop_active[d],
                self.deck_loop_start_sec[d],
                loop_len_sec,
                accent_color,
            );
            if stripe_resp.changed() {
                if let Ok(mut state) = self.audio.shared.lock() {
                    state.playhead_sec[d] = scrub_pos;
                }
            }

            // 7. Transport and loop; the pitch fader shares the row when there is room for it.
            let mut pitch_inline = false;
            row(ui, HIT, |ui| {
                if let Some(action) = deck_transport_bar(
                    ui,
                    deck.is_playing,
                    self.deck_flux_active[d],
                    self.deck_loop_active[d],
                    self.deck_loop_size_beats[d],
                    !self.deck_expanded[d],
                ) {
                    match action {
                        TransportAction::TogglePlay => self.toggle_play(deck_idx),
                        TransportAction::Cue => self.jump_to_cue(deck_idx, 1),
                        TransportAction::Cup => {
                            self.jump_to_cue(deck_idx, 1);
                            if let Ok(mut state) = self.audio.shared.lock() {
                                state.is_playing[d] = true;
                            }
                        }
                        TransportAction::ToggleFlux => {
                            self.deck_flux_active[d] = !self.deck_flux_active[d];
                        }
                        TransportAction::HalveLoop => {
                            self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] / 2).max(1);
                        }
                        TransportAction::DoubleLoop => {
                            self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] * 2).min(32);
                        }
                        TransportAction::SelectLoop(beats) => {
                            if self.deck_loop_size_beats[d] == beats {
                                self.toggle_loop(deck_idx);
                            } else {
                                self.deck_loop_size_beats[d] = beats;
                                if !self.deck_loop_active[d] {
                                    self.toggle_loop(deck_idx);
                                }
                            }
                        }
                    }
                }
                pitch_inline = ui.available_width() >= 120.0;
                if pitch_inline {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| pitch_fader(ui, &mut self.deck_pitch_fader[d]));
                }
            });
            if !pitch_inline {
                ui.horizontal(|ui| {
                    ui.label(caps("PITCH"));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| pitch_fader(ui, &mut self.deck_pitch_fader[d]));
                });
            }

            // 8. Pad mode and the eight performance pads, behind "MORE".
            if !self.deck_expanded[d] {
                return;
            }
            ui.multiply_opacity(pads_t);
            let gap = ui.spacing().item_spacing.x;
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 3.0 * gap) / 4.0;
                for mode in [DeckPadMode::Hotcues, DeckPadMode::Stems, DeckPadMode::Pattern, DeckPadMode::FluxLoop] {
                    if pill(ui, mode.label(), self.deck_pad_mode[d] == mode, TraktorTheme::ROW_SELECTED(), w)
                        .on_hover_text(match mode {
                            DeckPadMode::Hotcues => "Pads jump to hotcues 1-8",
                            DeckPadMode::Stems => "Pads 1-4 mute stems, pads 5-8 send them to the FX",
                            DeckPadMode::Pattern => "Pads toggle pattern player steps",
                            DeckPadMode::FluxLoop => "Pads hold a loop roll of 1 to 32 beats",
                        })
                        .clicked()
                    {
                        self.deck_pad_mode[d] = mode;
                        if mode == DeckPadMode::Pattern {
                            self.show_pattern_player = true;
                        }
                    }
                }
            });
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 7.0 * gap) / 8.0;
                let mode = self.deck_pad_mode[d];
                for i in 1..=8usize {
                    let (lit, fill, tip) = match mode {
                        DeckPadMode::Hotcues => (
                            self.deck_hotcues[d].iter().any(|(c, _)| *c == i as u8),
                            TraktorTheme::hotcue_color(i),
                            "Jump to this hotcue, or set it here if empty. Right-click to clear",
                        ),
                        DeckPadMode::Stems if i <= 4 => (!self.deck_stem_muted[d][i - 1], TraktorTheme::ACCENT(), "Mute or unmute a stem"),
                        DeckPadMode::Stems => (self.deck_stem_fx_send[d][i - 5], TraktorTheme::ACCENT(), "Send a stem to the FX"),
                        DeckPadMode::Pattern => {
                            let v = self.pattern_selected_voice.min(3);
                            let step_offset = if deck_idx == 2 { 8 } else { 0 };
                            (self.pattern_steps[v][(step_offset + i - 1).min(15)], TraktorTheme::ACCENT(), "Toggle a pattern step")
                        }
                        DeckPadMode::FluxLoop => (self.deck_loop_active[d], TraktorTheme::ACCENT(), "Loop roll"),
                    };
                    let pad = pill(ui, &i.to_string(), lit, fill, w).on_hover_text(tip);
                    if pad.clicked() {
                        self.trigger_performance_pad(deck_idx, i as u8);
                    }
                    if pad.secondary_clicked() && mode == DeckPadMode::Hotcues {
                        self.deck_hotcues[d].retain(|(c, _)| *c != i as u8);
                    }
                }
            });
        });
    }

    /// Centre mixer: knob-mode switch, channel strips, crossfader.
    fn show_mixer_section(&mut self, ui: &mut Ui, two_channel_only: bool) {
        panel(TraktorTheme::BG_PANEL_ALT()).show(ui, |ui| {
            // What the three knobs per channel control.
            ui.horizontal(|ui| {
                let neural = self.preferences.neural_mix_eq_mode;
                ui.label(caps("KNOBS"));
                for (label, selected, tip) in [
                    ("EQ", !neural, "Knobs are a high / mid / low equaliser"),
                    ("STEMS", neural, "Knobs are vocals / instruments / drums + bass levels"),
                ] {
                    if pill(ui, label, selected, TraktorTheme::ROW_SELECTED(), 52.0).on_hover_text(tip).clicked() && !selected {
                        self.preferences.neural_mix_eq_mode = !neural;
                    }
                }
            });

            if two_channel_only {
                ui.columns(2, |cols| {
                    self.show_channel_strip(&mut cols[0], 0, true);
                    self.show_channel_strip(&mut cols[1], 1, true);
                });
            } else {
                // Same order as the decks either side: A, C | D, B.
                ui.columns(4, |cols| {
                    self.show_channel_strip(&mut cols[0], 0, false);
                    self.show_channel_strip(&mut cols[1], 2, false);
                    self.show_channel_strip(&mut cols[2], 3, false);
                    self.show_channel_strip(&mut cols[3], 1, false);
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                ui.label(caps("A"));
                let w = (ui.available_width() - 20.0).max(90.0);
                crossfader(ui, &mut self.crossfader_pos, w).on_hover_text(XFADER_TIP);
                ui.label(caps("B"));
            });
        });
    }

    /// One channel: header (deck, headphone cue), EQ or stem knobs, gain, filter, fader with meter and
    /// crossfader side. `compact` (two-channel Mix) spreads the knobs over two columns to stay short.
    fn show_channel_strip(&mut self, ui: &mut Ui, ch: usize, compact: bool) {
        let accent = TraktorTheme::deck_color(ch + 1);
        let knob_h = 33.0; // rotary_knob_sized(28) is 28 + 5 tall
        let fader_h = 3.0 * knob_h + 2.0 * 2.0;

        ui.horizontal(|ui| {
            deck_badge(ui, DECK_LETTERS[ch.min(3)], accent);
            if pill(ui, "CUE", self.channel_cue[ch], TraktorTheme::CUE_ACTIVE(), 36.0)
                .on_hover_text("Headphone cue: hear this channel in the headphones")
                .clicked()
            {
                self.channel_cue[ch] = !self.channel_cue[ch];
            }
            if compact {
                ui.add_space((ui.available_width() - XFADER_ASSIGN_W).max(0.0));
                self.xfader_assign(ui, ch);
            }
        });

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(8.0, 2.0);
            // Centre the knob and fader block in the strip.
            let block_w = if compact { 28.0 + 8.0 + 28.0 + 8.0 + 38.0 } else { 28.0 + 8.0 + 38.0 };
            ui.add_space(((ui.available_width() - block_w) / 2.0).max(0.0));

            ui.vertical(|ui| {
                self.band_knobs(ui, ch, accent);
                if !compact {
                    self.filter_knob(ui, ch);
                }
            });
            if compact {
                ui.vertical(|ui| {
                    self.gain_knob(ui, ch, accent);
                    ui.add_space(knob_h + 2.0);
                    self.filter_knob(ui, ch);
                });
            }
            ui.vertical(|ui| {
                if !compact {
                    self.gain_knob(ui, ch, accent);
                }
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    vertical_fader(ui, &mut self.channel_volume[ch], 0.0, 1.0, accent, fader_h)
                        .on_hover_text("Channel volume. Drag to adjust; double-click to reset");
                    let peak = self.audio.shared.lock().map(|s| s.channel_peaks[ch]).unwrap_or(0.0);
                    vu_meter(ui, peak, peak * 0.94, fader_h);
                });
            });
        });

        if !compact {
            ui.horizontal(|ui| {
                ui.add_space(((ui.available_width() - XFADER_ASSIGN_W) / 2.0).max(0.0));
                self.xfader_assign(ui, ch);
            });
        }
    }

    /// The three knobs that are either the EQ or, in stems mode, stem levels. Right-click cuts or mutes;
    /// the label then says so, so the state does not rest on the arc colour alone.
    fn band_knobs(&mut self, ui: &mut Ui, ch: usize, accent: Color32) {
        let off = TraktorTheme::METER_RED();
        if self.preferences.neural_mix_eq_mode {
            for (s, label, name) in [(3usize, "VOC", "Vocals level"), (2usize, "INST", "Instruments level")] {
                let muted = self.deck_stem_muted[ch][s];
                let (label, col) = if muted { ("MUTE", off) } else { (label, accent) };
                if rotary_knob_sized(ui, &mut self.deck_stem_volumes[ch][s], 0.0, 1.0, 1.0, label, col, 28.0)
                    .on_hover_text(format!("{name}. {KNOB_TIP} Right-click to mute"))
                    .secondary_clicked()
                {
                    self.deck_stem_muted[ch][s] = !muted;
                }
            }
            let mut level = (self.deck_stem_volumes[ch][0] + self.deck_stem_volumes[ch][1]) * 0.5;
            let muted = self.deck_stem_muted[ch][0] && self.deck_stem_muted[ch][1];
            let (label, col) = if muted { ("MUTE", off) } else { ("D+B", accent) };
            let resp = rotary_knob_sized(ui, &mut level, 0.0, 1.0, 1.0, label, col, 28.0)
                .on_hover_text(format!("Drums and bass level. {KNOB_TIP} Right-click to mute"));
            if resp.changed() {
                self.deck_stem_volumes[ch][0] = level;
                self.deck_stem_volumes[ch][1] = level;
            }
            if resp.secondary_clicked() {
                self.deck_stem_muted[ch][0] = !muted;
                self.deck_stem_muted[ch][1] = !muted;
            }
        } else {
            for (label, name, value, cut) in [
                ("HI", "High EQ", &mut self.channel_eq_hi[ch], &mut self.eq_kill_hi[ch]),
                ("MID", "Mid EQ", &mut self.channel_eq_mid[ch], &mut self.eq_kill_mid[ch]),
                ("LOW", "Low EQ", &mut self.channel_eq_low[ch], &mut self.eq_kill_low[ch]),
            ] {
                let (label, col) = if *cut { ("CUT", off) } else { (label, accent) };
                if rotary_knob_sized(ui, value, 0.0, 1.0, 0.5, label, col, 28.0)
                    .on_hover_text(format!("{name}. {KNOB_TIP} Right-click to cut"))
                    .secondary_clicked()
                {
                    *cut = !*cut;
                }
            }
        }
    }

    fn gain_knob(&mut self, ui: &mut Ui, ch: usize, accent: Color32) {
        rotary_knob_sized(ui, &mut self.channel_gain[ch], 0.0, 1.0, 0.5, "GAIN", accent, 28.0)
            .on_hover_text(format!("Input gain. {KNOB_TIP}"));
    }

    /// The filter engages as soon as its knob leaves the centre.
    fn filter_knob(&mut self, ui: &mut Ui, ch: usize) {
        let col = if self.channel_filter_on[ch] { TraktorTheme::ACCENT() } else { TraktorTheme::TEXT_MUTED() };
        if rotary_knob_sized(ui, &mut self.channel_filter[ch], 0.0, 1.0, 0.5, "FLTR", col, 28.0)
            .on_hover_text(format!("Filter: left of centre cuts highs, right cuts lows. {KNOB_TIP}"))
            .changed()
        {
            self.channel_filter_on[ch] = (self.channel_filter[ch] - 0.5).abs() > 0.02;
        }
    }

    /// Which side of the crossfader this channel sits on.
    fn xfader_assign(&mut self, ui: &mut Ui, ch: usize) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (label, val, tip) in [
                ("A", 0usize, "Crossfader side A (left)"),
                ("T", 1usize, "Thru: bypass the crossfader"),
                ("B", 2usize, "Crossfader side B (right)"),
            ] {
                if pill(ui, label, self.channel_xfader_assign[ch] == val, TraktorTheme::ROW_SELECTED(), HIT)
                    .on_hover_text(tip)
                    .clicked()
                {
                    self.channel_xfader_assign[ch] = val;
                }
            }
        });
    }

    fn show_browser_panel(&mut self, ui: &mut Ui) {
        let avail_h = ui.available_height().max(160.0);
        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            ui.set_min_height(avail_h - 12.0);
            ui.horizontal_top(|ui| {
                // Left Navigation Tree
                ui.allocate_ui_with_layout(Vec2::new(178.0, avail_h - 16.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(178.0);
                    self.library.show_nav_tree(ui);
                });

                ui.separator();

                let four_deck_mode = self.layout == TraktorLayout::FourDeck || self.preferences.four_deck_mode;
                let locked_decks = self.locked_decks();
                self.library.show_browser_content(
                    ui,
                    four_deck_mode,
                    locked_decks,
                    &mut self.preferences.library_columns,
                    &mut self.preferences.music_folders,
                );
                self.drain_pending_loads();

                // If a track was loaded and has stems ready, automatically enable deck stems view
                if let Some((d, stems_ready)) = self.library.last_loaded_deck.take() {
                    if stems_ready {
                        self.deck_stem_view[d] = true;
                    }
                }

                // If stems were generated for a track, enable stems view if it is currently playing/loaded on any deck
                if let Some(stemmed_id) = self.library.last_stemmed_track_id.take() {
                    if let Some(track) = self.library.tracks.iter().find(|t| t.id == stemmed_id) {
                        if let Ok(mut state) = self.audio.shared.lock() {
                            for d in 0..4 {
                                if let Some(ref mut loaded) = state.loaded_tracks[d] {
                                    if loaded.title == track.title && loaded.artist == track.artist {
                                        loaded.stems_ready = true;
                                        self.deck_stem_view[d] = true;
                                    }
                                }
                            }
                        }
                    }
                }
            });
        });
    }
}

/// Playback ratio that brings `bpm` to `target`, folding the target by octaves so 70 follows 140 at half time.
fn sync_ratio(bpm: f64, target: f64) -> f64 {
    let mut ratio = target.max(20.0) / bpm.max(20.0);
    while ratio > std::f64::consts::SQRT_2 {
        ratio /= 2.0;
    }
    while ratio < std::f64::consts::FRAC_1_SQRT_2 {
        ratio *= 2.0;
    }
    ratio
}

/// How far (in beats, -0.5..=0.5) a deck at `beats` must move forward to land on the master's beat.
fn beat_phase_diff(master_beats: f64, beats: f64) -> f64 {
    let diff = (master_beats - beats).rem_euclid(1.0);
    if diff > 0.5 { diff - 1.0 } else { diff }
}

/// What a loaded deck needs to draw one frame.
struct DeckView {
    title: String,
    artist: String,
    bpm: f64,
    grid_offset_sec: f64,
    key: String,
    duration_sec: f64,
    elapsed_sec: f64,
    is_playing: bool,
    peaks: Vec<[f32; 3]>,
    stem_peaks: Vec<[f32; 4]>,
}

impl DeckView {
    fn remaining(&self) -> String {
        let rem = (self.duration_sec - self.elapsed_sec).max(0.0);
        format!("-{:02}:{:02}", (rem / 60.0).floor() as u32, (rem % 60.0).floor() as u32)
    }
}

// ---- The performance screen's small design system: one panel, one button, one readout. ----

const DECK_LETTERS: [&str; 4] = ["A", "B", "C", "D"];
/// Smallest side of anything clickable.
const HIT: f32 = 24.0;
/// Three 24 px crossfader-side buttons and their two 2 px gaps.
const XFADER_ASSIGN_W: f32 = 3.0 * HIT + 2.0 * 2.0;
const KNOB_TIP: &str = "Drag up or down; double-click to reset.";
const XFADER_TIP: &str = "Crossfader. Drag to blend A and B; double-click to centre";

/// WCAG relative luminance.
fn luminance(c: Color32) -> f32 {
    let linear = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * linear(c.r()) + 0.7152 * linear(c.g()) + 0.0722 * linear(c.b())
}

/// Text colour for any fill: black or white, whichever contrasts more (never below 4.5:1).
fn on(fill: Color32) -> Color32 {
    // Above this luminance black wins; at it both give 4.58:1.
    if luminance(fill) > 0.179 {
        Color32::BLACK
    } else {
        Color32::WHITE
    }
}

/// Every surface on the screen: same padding, border and corner.
fn panel(fill: Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()))
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
}

/// A horizontal row of known height, so everything in it shares one centre line.
fn row<R>(ui: &mut Ui, height: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(Vec2::new(ui.available_width(), height), Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_height(height);
        add_contents(ui)
    })
    .inner
}

/// Small caps label for a group of controls.
fn caps(text: &str) -> RichText {
    RichText::new(text).size(10.0).color(TraktorTheme::TEXT_SECONDARY())
}

/// Hover and keyboard-focus outline, the same on every button.
fn ring(ui: &Ui, resp: egui::Response) -> egui::Response {
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_stroke(resp.rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::TEXT_SECONDARY()));
    }
    resp
}

/// `icon_button` with the shared hover ring.
fn icon_pill(ui: &mut Ui, icon: Icon, label: &str, fill: Color32, fg: Color32, min_size: Vec2) -> egui::Response {
    let resp = icon_button(ui, icon, label, fill, fg, min_size);
    ring(ui, resp)
}

/// The one text button: 24 px tall, filled with `on_fill` while engaged (its text flips to stay readable).
fn pill(ui: &mut Ui, label: &str, engaged: bool, on_fill: Color32, min_width: f32) -> egui::Response {
    // The fill eases in and out; the text flips halfway so it is readable throughout.
    // Keyed to this button's own position, so same-named buttons never share an animation.
    let t = crate::theme::fade(ui.ctx(), ui.next_auto_id(), engaged);
    let idle = TraktorTheme::CONTROL();
    let fill = crate::theme::mix(idle, on_fill, t);
    let fg = if t > 0.5 { on(on_fill) } else { TraktorTheme::TEXT_SECONDARY() };
    let mut button = egui::Button::new(RichText::new(label).size(11.5).color(fg)).min_size(Vec2::new(min_width, HIT));
    if t > 0.0 && on_fill != Color32::TRANSPARENT {
        button = button.fill(fill);
    }
    let resp = ui.add(button);
    ring(ui, resp)
}

/// Deck identity: its letter on its colour, so colour alone never names a deck.
fn deck_badge(ui: &mut Ui, letter: &str, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(HIT), Sense::hover());
    ui.painter().rect_filled(rect, 3.0, color);
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, letter, egui::FontId::proportional(14.0), on(color));
}

/// "Title  Artist" on one line, cut with an ellipsis rather than pushing the layout wider.
fn title_label(ui: &mut Ui, title: &str, artist: &str) {
    let mut job = egui::text::LayoutJob::default();
    job.append(title, 0.0, egui::TextFormat::simple(egui::FontId::proportional(13.0), TraktorTheme::TEXT_PRIMARY()));
    job.append(artist, 10.0, egui::TextFormat::simple(egui::FontId::proportional(12.0), TraktorTheme::TEXT_SECONDARY()));
    ui.add(egui::Label::new(job).truncate().selectable(false)).on_hover_text(format!("{title}\n{artist}"));
}

/// Badge plus a one-line hint for a deck with nothing loaded.
fn empty_deck_hint(ui: &mut Ui, letter: &str, accent: Color32, hint: &str) {
    deck_badge(ui, letter, accent);
    ui.add(egui::Label::new(RichText::new(hint).size(12.0).color(TraktorTheme::TEXT_SECONDARY())).truncate().selectable(false));
}

/// Dark inset holding dot-matrix readouts, like the display window on a piece of hardware.
/// `add_contents` gets the ink colour that reads on it.
fn display(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui, Color32)) {
    let bg = TraktorTheme::WAVEFORM_BG();
    egui::Frame::none()
        .fill(bg)
        .rounding(3.0)
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| ui.horizontal(|ui| add_contents(ui, on(bg))));
}

/// Beat lamp: flashes on every beat while the deck plays; a hollow ring when stopped,
/// so the state reads without colour or motion.
fn beat_lamp(ui: &mut Ui, ink: Color32, playing: bool, position_sec: f64, bpm: f64) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(10.0, 28.0), Sense::hover());
    let centre = rect.center();
    if playing {
        ui.painter().circle_filled(centre, 4.0, ink.gamma_multiply(crate::theme::beat_pulse(position_sec, bpm)));
    } else {
        ui.painter().circle_stroke(centre, 3.5, Stroke::new(1.2_f32, ink.gamma_multiply(0.6)));
    }
    resp.on_hover_text(if playing { "Playing — flashes on the beat" } else { "Stopped" });
}

/// A number glanced at mid-mix: small caps label over the value in the dot-matrix face.
fn readout(ui: &mut Ui, label: &str, value: &str, size: f32, ink: Color32) {
    let label_galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(10.0), ink.gamma_multiply(0.65));
    let value_galley = ui.painter().layout_no_wrap(value.to_owned(), crate::theme::dot_font(size), ink);
    let size = Vec2::new(label_galley.size().x.max(value_galley.size().x), label_galley.size().y + value_galley.size().y);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let value_pos = egui::pos2(rect.left(), rect.bottom() - value_galley.size().y);
    ui.painter().galley(rect.left_top(), label_galley, ink);
    ui.painter().galley(value_pos, value_galley, ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label} {value}")));
}

/// Status lamp with a label; clicking opens the matching settings. Filled = OK, hollow = not,
/// so the state never rests on colour alone.
fn lamp(ui: &mut Ui, label: &str, ok: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(11.5), TraktorTheme::TEXT_SECONDARY());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 22.0, HIT), Sense::click());
    let centre = egui::pos2(rect.left() + 8.0, rect.center().y);
    if ok {
        ui.painter().circle_filled(centre, 3.5, TraktorTheme::TEXT_PRIMARY());
    } else {
        ui.painter().circle_stroke(centre, 3.0, Stroke::new(1.2_f32, TraktorTheme::TEXT_SECONDARY()));
    }
    ui.painter().galley(egui::pos2(rect.left() + 17.0, rect.center().y - galley.size().y / 2.0), galley, TraktorTheme::TEXT_SECONDARY());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    ring(ui, resp)
}

/// One FX unit in the top bar: on/off, dry/wet and one amount knob. The arcs light up with the unit.
fn fx_unit(ui: &mut Ui, n: u8, enabled: &mut bool, dry_wet: &mut f32, amount: &mut f32, amount_label: &str, amount_name: &str) {
    if pill(ui, &format!("FX {n}"), *enabled, TraktorTheme::ACCENT(), 40.0)
        .on_hover_text(format!("FX unit {n} on / off"))
        .clicked()
    {
        *enabled = !*enabled;
    }
    let arc = if *enabled { TraktorTheme::ACCENT() } else { TraktorTheme::TEXT_MUTED() };
    rotary_knob_sized(ui, dry_wet, 0.0, 1.0, 0.3, "D/W", arc, 28.0).on_hover_text(format!("FX {n} dry / wet mix. {KNOB_TIP}"));
    rotary_knob_sized(ui, amount, 0.0, 1.0, 0.5, amount_label, arc, 28.0).on_hover_text(format!("FX {n} {amount_name}. {KNOB_TIP}"));
}

/// Pitch fader and its reset. Call inside a right-to-left layout: the fader takes the width that is left.
fn pitch_fader(ui: &mut Ui, pitch: &mut f32) {
    if pill(ui, "0%", false, Color32::TRANSPARENT, 32.0).on_hover_text("Reset pitch to 0%").clicked() {
        *pitch = 0.0;
    }
    ui.spacing_mut().interact_size.y = HIT;
    ui.spacing_mut().slider_width = (ui.available_width() - ui.spacing().item_spacing.x).clamp(48.0, 220.0);
    ui.add(egui::Slider::new(pitch, -8.0..=8.0).clamp_to_range(false).show_value(false)).on_hover_text("Pitch fader: tempo ±8%");
}

/// Height of the full-width waveforms: 64 px at the minimum window, growing with the window.
fn mix_wave_height(ui: &Ui) -> f32 {
    (64.0 + (ui.ctx().screen_rect().height() - 640.0) * 0.14).clamp(64.0, 124.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Palette;

    #[test]
    fn sync_matches_tempo_and_phase() {
        assert!((sync_ratio(91.0, 68.5) - 68.5 / 91.0).abs() < 1e-9);
        assert!((sync_ratio(68.5, 137.0) - 1.0).abs() < 1e-9); // half time, not +100%
        assert!((sync_ratio(140.0, 128.0) * 140.0 - 128.0).abs() < 1e-9);
        assert!((beat_phase_diff(10.25, 3.0) - 0.25).abs() < 1e-9);
        assert!((beat_phase_diff(10.0, 3.9) - 0.1).abs() < 1e-9);
        assert!((beat_phase_diff(10.0, 3.2) + 0.2).abs() < 1e-9);
    }

    fn contrast(a: Color32, b: Color32) -> f32 {
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn button_text_is_readable_in_every_preset() {
        for (name, p) in Palette::PRESETS {
            // Every fill a pill, badge or display can take.
            for fill in [
                p.ACCENT, p.ROW_SELECTED, p.CUE_ACTIVE, p.PLAY_ACTIVE, p.WAVEFORM_BG, p.DECK_A, p.DECK_B, p.DECK_C, p.DECK_D,
                p.HOTCUE_CUE, p.HOTCUE_LOAD, p.HOTCUE_LOOP, p.HOTCUE_FADEIN, p.HOTCUE_FADEOUT, p.HOTCUE_GENERIC,
            ] {
                assert!(contrast(on(fill), fill) >= 4.5, "{name}: text on {fill:?}");
            }
            // Idle buttons and labels.
            for bg in [p.CONTROL, p.CONTROL_HOVER, p.BG_PANEL, p.BG_PANEL_ALT] {
                assert!(contrast(p.TEXT_SECONDARY, bg) >= 4.5, "{name}: secondary text on {bg:?}");
            }
        }
    }
}
