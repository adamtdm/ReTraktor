use crate::audio::{AudioSystem, LoadedTrackAudio, PEAKS_PER_SEC};
use crate::dot;
use crate::library_ui::LibraryManager;
use crate::preferences::{PreferencesDialog, PrefsTab};
use crate::theme::TraktorTheme;
use crate::widgets::{
    crossfader, deck_transport_bar, icon_button, render_shortcuts_hud, rotary_knob_sized,
    scrolling_stem_waveforms, scrolling_waveform, stripe_overview, Icon, TransportAction,
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
    /// Build → Perform set sequencing, harmonic flow arc and warnings
    Set,
}

impl TraktorLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Parallel => "Mix",
            Self::FourDeck => "4-Deck",
            Self::Timeline => "Timeline",
            Self::Browser => "Browser",
            Self::Set => "Set",
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
    pub show_next_track_panel: bool,
    pub next_track_picks: Vec<(usize, f32, String)>,
    pub next_track_target_deck: usize,
    pub ai_rerank_runner: Option<crate::ai::AiRerankRunner>,
    pub ai_rerank_status: Option<String>,

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
    pub xfader_cheer_sm: dot::CrossfadeCheerStateMachine,
    pub active_cheer: Option<dot::ActiveCheer>,

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
    pub deck_track_ids: [Option<usize>; 4],
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
    pub set_state: crate::setlist::SetUiState,
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
            show_next_track_panel: false,
            next_track_picks: Vec::new(),
            next_track_target_deck: 2,
            ai_rerank_runner: None,
            ai_rerank_status: None,
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
            xfader_cheer_sm: dot::CrossfadeCheerStateMachine::new(),
            active_cheer: None,
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
            deck_track_ids: [None; 4],
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
            set_state: crate::setlist::SetUiState::default(),
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
        if let Some(ids) = self.library.add_to_set_requested.take() {
            self.set_state.add_tracks(&ids);
        }

        if self.layout == TraktorLayout::Set {
            let idle = self.idle_deck();
            if let Some((deck, id)) = self.set_state.handle_keys(ctx, &self.library, idle) {
                self.library.load_to_deck(deck, id, [false; 4]);
                self.drain_pending_loads();
            }
        }

        let dt = ctx.input(|i| i.stable_dt);
        if let Ok(state) = self.audio.shared.lock() {
            let is_rec = state.is_recording;
            let rec_offset = if is_rec {
                (state.recorded_samples.len() as f64) / (2.0 * 48_000.0)
            } else {
                0.0
            };

            for d in 0..2 {
                let playing = state.is_playing[d];
                self.set_state.update_playback(d, playing, dt);

                if is_rec {
                    let vol = self.channel_volume[d];
                    let xfader = self.crossfader_pos;
                    let (artist, title) = if let Some(ref loaded) = state.loaded_tracks[d] {
                        (loaded.artist.clone(), loaded.title.clone())
                    } else {
                        (String::new(), String::new())
                    };
                    self.set_state.check_audible_track(d, playing, vol, xfader, &artist, &title, rec_offset);
                }
            }
        }

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
                for lay in [
                    TraktorLayout::Parallel,
                    TraktorLayout::FourDeck,
                    TraktorLayout::Timeline,
                    TraktorLayout::Browser,
                    TraktorLayout::Set,
                ] {
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
                    TraktorLayout::Set => {
                        let idle = self.idle_deck();
                        self.set_state.show_set_view(ui, &mut self.library, idle);
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
        for entry in &self.preferences.generic_manager.active_mapping.entries {
            self.custom_mappings.insert(entry.control_id.clone(), entry.action.clone());
        }

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
                        if self.deck_track_ids[d] == Some(track_id) {
                            self.deck_stem_view[d] = true;
                        }
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

        // Open Preferences AI tab if requested by library
        if self.library.open_prefs_ai {
            self.library.open_prefs_ai = false;
            self.preferences.is_open = true;
            self.preferences.current_tab = PrefsTab::Ai;
        }

        // Suggest Next Track Window
        if self.show_next_track_panel {
            self.render_next_track_window(ctx);
        }
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
            "Set" => {
                self.preferences.is_open = false;
                self.layout = TraktorLayout::Set;
                if self.set_state.set_list.track_ids.is_empty() {
                    let sample_ids: Vec<usize> = self.library.tracks.iter().take(5).map(|t| t.id).collect();
                    if sample_ids.is_empty() {
                        let sample_tracks = vec![
                            crate::library_ui::UiTrackEntry {
                                id: 9001,
                                title: "Solaris (Extended Mix)".to_string(),
                                artist: "Articuno".to_string(),
                                bpm: 124.0,
                                musical_key: "8m".to_string(),
                                energy: 7,
                                duration_sec: 384.0,
                                ..Default::default()
                            },
                            crate::library_ui::UiTrackEntry {
                                id: 9002,
                                title: "Midnight Sun (Original)".to_string(),
                                artist: "Helios".to_string(),
                                bpm: 125.0,
                                musical_key: "9m".to_string(),
                                energy: 8,
                                duration_sec: 412.0,
                                ..Default::default()
                            },
                            crate::library_ui::UiTrackEntry {
                                id: 9003,
                                title: "Deep Echoes".to_string(),
                                artist: "Subway 21".to_string(),
                                bpm: 126.0,
                                musical_key: "10m".to_string(),
                                energy: 6,
                                duration_sec: 350.0,
                                ..Default::default()
                            },
                            crate::library_ui::UiTrackEntry {
                                id: 9004,
                                title: "Velocity Peak".to_string(),
                                artist: "Apex Drift".to_string(),
                                bpm: 128.0,
                                musical_key: "10d".to_string(),
                                energy: 9,
                                duration_sec: 295.0,
                                ..Default::default()
                            },
                        ];
                        for t in sample_tracks {
                            self.library.tracks.push(t);
                        }
                        self.set_state.add_tracks(&[9001, 9002, 9003, 9004]);
                    } else {
                        self.set_state.add_tracks(&sample_ids);
                    }
                }
            }
            "prefs" => self.preferences.is_open = true,
            "noprefs" => self.preferences.is_open = false,
            "expandA" => self.deck_expanded[0] = true,
            s if s.starts_with("select:") => self.preferences.mapper.selected = Some(s[7..].to_string()),
            "prefs:other" | "prefs:OtherControllers" | "prefs:Other" => {
                self.preferences.is_open = true;
                self.preferences.current_tab = PrefsTab::OtherControllers;
            }
            s if s.starts_with("prefs:") => {
                self.preferences.is_open = true;
                self.show_next_track_panel = false;
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
            "playA" | "playB" if first => self.toggle_play(if step == "playA" { 1 } else { 2 }),
            "stemsA" => self.deck_stem_view[0] = true,
            "stemsB" => self.deck_stem_view[1] = true,
            "pattern" => self.show_pattern_player = true,
            "help" => self.show_shortcuts_hud = true,
            "ai" => {
                self.preferences.is_open = false;
                self.show_next_track_panel = false;
                self.layout = TraktorLayout::Browser;
                self.library.show_ai_playlist_panel = true;
            }
            "next" => {
                self.preferences.is_open = false;
                if self.library.tracks.is_empty() {
                    self.library.tracks = vec![
                        crate::library_ui::UiTrackEntry {
                            id: 1,
                            title: "Pulse Width".into(),
                            artist: "Aphex Twin".into(),
                            bpm: 124.0,
                            musical_key: "8m".into(),
                            genre: "Techno".into(),
                            energy: 6,
                            file_path: Some(std::path::PathBuf::from("/music/pulse_width.flac")),
                            ..Default::default()
                        },
                        crate::library_ui::UiTrackEntry {
                            id: 2,
                            title: "Windowlicker".into(),
                            artist: "Aphex Twin".into(),
                            bpm: 125.0,
                            musical_key: "8m".into(),
                            genre: "Electronic".into(),
                            energy: 7,
                            file_path: Some(std::path::PathBuf::from("/music/windowlicker.flac")),
                            ..Default::default()
                        },
                        crate::library_ui::UiTrackEntry {
                            id: 3,
                            title: "Blue Monday".into(),
                            artist: "New Order".into(),
                            bpm: 126.0,
                            musical_key: "9m".into(),
                            genre: "Synthpop".into(),
                            energy: 7,
                            file_path: Some(std::path::PathBuf::from("/music/blue_monday.flac")),
                            ..Default::default()
                        },
                        crate::library_ui::UiTrackEntry {
                            id: 4,
                            title: "Around The World".into(),
                            artist: "Daft Punk".into(),
                            bpm: 121.0,
                            musical_key: "8m".into(),
                            genre: "House".into(),
                            energy: 6,
                            file_path: Some(std::path::PathBuf::from("/music/around_the_world.flac")),
                            ..Default::default()
                        },
                        crate::library_ui::UiTrackEntry {
                            id: 5,
                            title: "Voodoo Ray".into(),
                            artist: "A Guy Called Gerald".into(),
                            bpm: 122.0,
                            musical_key: "7m".into(),
                            genre: "Acid House".into(),
                            energy: 6,
                            file_path: Some(std::path::PathBuf::from("/music/voodoo_ray.flac")),
                            ..Default::default()
                        },
                    ];
                    self.deck_track_ids[0] = Some(1);
                }
                self.show_next_track_panel = true;
                self.compute_next_track_picks();
            }
            // Held mid-animation so a still can show it.
            s if s.starts_with("cheer:") => {
                let cheer_index = s[6..].parse().unwrap_or(0);
                self.active_cheer = Some(dot::ActiveCheer { deck: 1, cheer_index, start_sec: ctx.input(|i| i.time) - 0.7 });
            }
            s if s.starts_with("xf:") => self.crossfader_pos = s[3..].parse().unwrap_or(0.5),
            "muteA" if first => self.deck_stem_muted[0][0] ^= true,
            "relocate" if first => self.library.start_relocate(self.preferences.music_folders.clone()),
            "import" | "import_playlist" | "playlist_import" if first => {
                self.preferences.is_open = false;
                self.layout = TraktorLayout::Browser;
                self.library.demo_import_playlist();
            }
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

    /// The deck (1 = A, 2 = B) that is currently idle / not playing to receive the next track.
    fn idle_deck(&self) -> usize {
        if let Ok(state) = self.audio.shared.lock() {
            // An empty deck beats a loaded one.
            match (state.loaded_tracks[0].is_some(), state.loaded_tracks[1].is_some()) {
                (true, false) => return 2,
                (false, true) => return 1,
                _ => {}
            }
            if state.is_playing[0] && !state.is_playing[1] {
                return 2;
            } else if !state.is_playing[0] && state.is_playing[1] {
                return 1;
            }
        }
        if self.crossfader_pos < 0.5 { 2 } else { 1 }
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
            self.deck_track_ids[d] = Some(track.id);
            self.set_state.on_track_loaded(d, track.id);
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
        // Non-blocking poll of connected generic MIDI & NI HID controllers
        let generic_events = self.preferences.generic_manager.poll_events();
        for ev in generic_events {
            self.apply_mx2_surface_event(ev);
        }

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
            SurfaceEvent::Encoder { control, .. } => Some(control.clone()),
            _ => None,
        };
        if let Some(t) = touched {
            self.preferences.last_surface_control = Some(t);
        }

        // Handle Encoder actions (e.g. browse, jog turn, loop size)
        if let SurfaceEvent::Encoder { ref control, delta } = ev {
            if let Some(act) = self.custom_mappings.get(control) {
                match act.as_str() {
                    "browse" => {
                        if delta > 0 {
                            self.library.select_next_track();
                        } else if delta < 0 {
                            self.library.select_prev_track();
                        }
                        return;
                    }
                    "jog:1" | "jog:2" => {
                        let d = if act == "jog:2" { 1 } else { 0 };
                        let sens = (self.jog_scratch_sensitivity as f64).clamp(0.2, 3.0);
                        if let Ok(mut state) = self.audio.shared.lock() {
                            if state.scratch_active[d] {
                                let scaled_ticks = ((delta as f64) * sens).round() as i64;
                                state.scratch_tick(d, scaled_ticks);
                            } else {
                                let nudge_sec = (delta as f64 / 1024.0) * 0.08 * sens;
                                state.playhead_sec[d] = (state.playhead_sec[d] + nudge_sec).max(0.0);
                            }
                        }
                        return;
                    }
                    "loop_size:1" | "loop_size:2" => {
                        let d = if act == "loop_size:2" { 1 } else { 0 };
                        if delta > 0 {
                            self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] * 2).min(32);
                        } else if delta < 0 {
                            self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] / 2).max(1);
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        // Handle Jog Touch (scratch enable / disable)
        if let SurfaceEvent::Button { ref control, pressed } = ev {
            if let Some(act) = self.custom_mappings.get(control) {
                if act.starts_with("jog_touch:") || act == "jog:1" || act == "jog:2" {
                    let d = if act.ends_with(":2") { 1 } else { 0 };
                    if self.deck_turntable_mode[d] {
                        self.deck_scratch_active[d] = pressed;
                        if let Ok(mut state) = self.audio.shared.lock() {
                            if pressed {
                                state.enable_scratch(d, 1024, 33.333333333333336, 0.125, 0.125 / 32.0);
                            } else {
                                state.disable_scratch(d);
                            }
                        }
                    }
                    return;
                }
            }
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

        if ctx.input(|i| i.key_pressed(Key::Escape)) && self.show_next_track_panel {
            self.show_next_track_panel = false;
        }

        if self.show_next_track_panel && ctx.input(|i| i.key_pressed(Key::Enter)) {
            if let Some(&(id, _, _)) = self.next_track_picks.first() {
                let locked = self.locked_decks();
                self.library.load_to_deck(self.next_track_target_deck, id, locked);
                self.show_next_track_panel = false;
            }
        }

        // Layout shortcuts F1..F4
        for (key, lay) in [
            (Key::F1, TraktorLayout::Parallel),
            (Key::F2, TraktorLayout::FourDeck),
            (Key::F3, TraktorLayout::Browser),
            (Key::F4, TraktorLayout::Timeline),
            (Key::F5, TraktorLayout::Set),
        ] {
            if ctx.input(|i| i.key_pressed(key)) {
                self.layout = lay;
                self.preferences.four_deck_mode = lay == TraktorLayout::FourDeck;
            }
        }

        // Only process single-key deck and mixer shortcuts when not typing in a text field or in Preferences
        if !ctx.wants_keyboard_input() && !self.preferences.is_open {
            ctx.input(|i| {
                if i.modifiers.command || i.modifiers.ctrl || i.modifiers.alt {
                    return;
                }
                let shift = i.modifiers.shift;
                let deck = if shift { 2 } else { 1 };
                let ch = deck - 1;

                if i.key_pressed(Key::Space) {
                    self.toggle_play(deck);
                }
                if i.key_pressed(Key::C) {
                    self.jump_to_cue(deck, 1);
                }
                if i.key_pressed(Key::S) {
                    self.sync_deck(deck);
                }
                if i.key_pressed(Key::L) {
                    self.toggle_loop(deck);
                }

                // Hotcues 1..4 on Deck A (1..4) and Deck B (5..8)
                for (key, d_idx, cue) in [
                    (Key::Num1, 1, 1u8),
                    (Key::Num2, 1, 2),
                    (Key::Num3, 1, 3),
                    (Key::Num4, 1, 4),
                    (Key::Num5, 2, 1),
                    (Key::Num6, 2, 2),
                    (Key::Num7, 2, 3),
                    (Key::Num8, 2, 4),
                ] {
                    if !shift && i.key_pressed(key) {
                        self.jump_to_cue(d_idx, cue);
                    }
                }

                // EQ / Stems view toggle
                if i.key_pressed(Key::T) {
                    self.deck_stem_view[ch] = !self.deck_stem_view[ch];
                }
                // Create Demucs AI Stems
                if i.key_pressed(Key::G) {
                    self.request_deck_stems(deck);
                }

                // Stem Mute 1..4 (Drums, Bass, Other, Vocals)
                for (s, key) in [(0usize, Key::Q), (1, Key::W), (2, Key::E), (3, Key::R)] {
                    if i.key_pressed(key) {
                        self.deck_stem_muted[ch][s] = !self.deck_stem_muted[ch][s];
                    }
                }

                // EQ Kill Low / Mid / Hi
                if i.key_pressed(Key::Z) {
                    self.eq_kill_low[ch] = !self.eq_kill_low[ch];
                }
                if i.key_pressed(Key::X) {
                    self.eq_kill_mid[ch] = !self.eq_kill_mid[ch];
                }
                if i.key_pressed(Key::V) {
                    self.eq_kill_hi[ch] = !self.eq_kill_hi[ch];
                }

                // Headphone Cue On/Off
                if i.key_pressed(Key::H) {
                    self.channel_cue[ch] = !self.channel_cue[ch];
                }

                // Filter On/Off
                if i.key_pressed(Key::F) {
                    self.channel_filter_on[ch] = !self.channel_filter_on[ch];
                }

                // FX Unit On/Off
                if i.key_pressed(Key::D) {
                    if ch == 0 {
                        self.fx1_on = !self.fx1_on;
                    } else {
                        self.fx2_on = !self.fx2_on;
                    }
                }

                // Channel Fader Down / Up
                if i.key_pressed(Key::Minus) {
                    self.channel_volume[ch] = (self.channel_volume[ch] - 0.05).clamp(0.0, 1.0);
                }
                if i.key_pressed(Key::Equals) || i.key_pressed(Key::Plus) {
                    self.channel_volume[ch] = (self.channel_volume[ch] + 0.05).clamp(0.0, 1.0);
                }

                // Crossfader Nudge Left / Centre / Right
                if i.key_pressed(Key::OpenBracket) {
                    self.crossfader_pos = (self.crossfader_pos - 0.08).clamp(0.0, 1.0);
                }
                if i.key_pressed(Key::Backslash) {
                    self.crossfader_pos = 0.5;
                }
                if i.key_pressed(Key::CloseBracket) {
                    self.crossfader_pos = (self.crossfader_pos + 0.08).clamp(0.0, 1.0);
                }

                // Timeline actions: Cycle Curve (U), Cycle Bar Length (B), Audition (A)
                if i.key_pressed(Key::U) {
                    let next = (self.timeline_preset_idx + 1) % 3;
                    self.timeline_preset_idx = next;
                    match next {
                        0 => {
                            self.deck_stem_muted[0] = [false; 4];
                            self.deck_stem_muted[1] = [false; 4];
                        }
                        1 => {
                            self.deck_stem_muted[0][1] = true;
                            self.deck_stem_muted[1][1] = false;
                        }
                        _ => {
                            self.deck_stem_muted[0] = [true, true, true, false];
                            self.deck_stem_muted[1] = [false, false, false, true];
                        }
                    }
                }
                if i.key_pressed(Key::B) {
                    self.timeline_transition_bars = match self.timeline_transition_bars {
                        8 => 16,
                        16 => 32,
                        _ => 8,
                    };
                }
                if i.key_pressed(Key::A) {
                    self.sync_deck(2);
                    if let Ok(mut state) = self.audio.shared.lock() {
                        state.is_playing[0] = true;
                        state.is_playing[1] = true;
                    }
                }

                // Perform: Shift+N loads next unplayed set track into the deck that is not playing
                if shift && i.key_pressed(Key::N) {
                    if let Some((_, _, track)) = self.set_state.next_unplayed_track(&self.library) {
                        let target_deck = self.idle_deck();
                        self.library.load_to_deck(target_deck, track.id, [false; 4]);
                        self.drain_pending_loads();
                    }
                }
                if i.key_pressed(Key::N) && !shift {
                    self.show_next_track_panel = !self.show_next_track_panel;
                    if self.show_next_track_panel {
                        self.compute_next_track_picks();
                    }
                }
            });
        }
    }

    fn request_deck_stems(&mut self, deck_idx: usize) {
        let d = (deck_idx.saturating_sub(1)).min(3);
        let track_id = self.deck_track_ids[d].or_else(|| {
            let state = self.audio.shared.lock().ok()?;
            let loaded = state.loaded_tracks[d].as_ref()?;
            self.library
                .tracks
                .iter()
                .find(|t| t.title == loaded.title && t.artist == loaded.artist)
                .map(|t| t.id)
        });
        if let Some(id) = track_id {
            self.library.request_stem_separation = Some(id);
            self.deck_stem_view[d] = true;
        } else {
            self.library.status_message = format!("Load a track onto Deck {} first to create stems.", DECK_LETTERS[d]);
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

    /// Shifts a loaded track's beatgrid and/or scales its tempo, on the deck and in the library entry.
    fn adjust_deck_grid(&mut self, d: usize, shift_sec: f64, bpm_factor: f64) {
        let Ok(mut state) = self.audio.shared.lock() else { return };
        let Some(t) = state.loaded_tracks[d].as_mut() else { return };
        t.grid_offset_sec += shift_sec;
        t.bpm = (t.bpm * bpm_factor).clamp(20.0, 400.0);
        let (offset, bpm) = (t.grid_offset_sec, t.bpm);
        drop(state);
        if let Some(id) = self.deck_track_ids[d] {
            self.library.set_grid(id, offset, bpm);
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

    /// Top bar: Deck A strip on the left (`A`, `GAIN`, `FILTER`, `FX` + `ON`), Master tempo & layout tabs
    /// in the centre, Deck B strip mirrored on the right (`ON`, `FX`, `FILTER`, `GAIN`, `B`) plus utilities.
    fn show_top_bar(&mut self, ui: &mut Ui) {
        let screen_w = ui.ctx().screen_rect().width();
        let roomy = screen_w >= 1180.0;
        let show_rec = screen_w >= 1080.0;

        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            row(ui, 56.0, |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                self.deck_strip(ui, 0);
                ui.separator();

                // ---- Centre-left: compact layout tabs ----
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for (lay, short, key) in [
                        (TraktorLayout::Parallel, "Mix", "F1"),
                        (TraktorLayout::FourDeck, "4-Deck", "F2"),
                        (TraktorLayout::Browser, "Browse", "F3"),
                        (TraktorLayout::Timeline, "Timeline", "F4"),
                        (TraktorLayout::Set, "Set", "F5"),
                    ] {
                        let label = if roomy { lay.label() } else { short };
                        let min_w = if roomy { 46.0 } else { 36.0 };
                        if pill(ui, label, self.layout == lay, TraktorTheme::ROW_SELECTED(), min_w)
                            .on_hover_text(format!("{} layout ({key})", lay.label()))
                            .clicked()
                        {
                            self.layout = lay;
                            self.preferences.four_deck_mode = lay == TraktorLayout::FourDeck;
                        }
                    }
                });

                // ---- Centre: Master tempo readout in dot font ----
                display(ui, |ui, ink| {
                    readout(ui, "MASTER BPM", &format!("{:.1}", self.master_tempo), 20.0, ink);
                });
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if pill(ui, "−", false, Color32::TRANSPARENT, HIT)
                        .on_hover_text("Master tempo down 0.5 BPM")
                        .clicked()
                    {
                        self.master_tempo = (self.master_tempo - 0.5).max(40.0);
                    }
                    if pill(ui, "+", false, Color32::TRANSPARENT, HIT)
                        .on_hover_text("Master tempo up 0.5 BPM")
                        .clicked()
                    {
                        self.master_tempo = (self.master_tempo + 0.5).min(240.0);
                    }
                });

                let pat_label = if roomy { "PATTERN" } else { "PAT" };
                let pattern_fill = if self.pattern_enabled { TraktorTheme::ACCENT() } else { TraktorTheme::ROW_SELECTED() };
                if pill(ui, pat_label, self.show_pattern_player || self.pattern_enabled, pattern_fill, 0.0)
                    .on_hover_text("Show or hide the 16-step pattern player (drum machine)")
                    .clicked()
                {
                    self.show_pattern_player = !self.show_pattern_player;
                }

                let next_fill = if self.show_next_track_panel { TraktorTheme::ACCENT() } else { TraktorTheme::ROW_SELECTED() };
                if roomy && pill(ui, "NEXT", self.show_next_track_panel, next_fill, 0.0)
                    .on_hover_text("Suggest next track for the idle deck (N)")
                    .clicked()
                {
                    self.toggle_next_track_panel();
                }

                // ---- Right: utilities + mirrored Deck B strip (ON/OFF + FX | FILTER | GAIN | B) ----
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // Same order in a right-to-left layout mirrors the strip.
                    self.deck_strip(ui, 1);
                    ui.separator();

                    if pill(ui, if roomy { "SETUP" } else { "PREF" }, self.preferences.is_open, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text("Preferences (Ctrl+,)")
                        .clicked()
                    {
                        self.preferences.is_open = !self.preferences.is_open;
                    }
                    if pill(ui, "KEYS", self.show_shortcuts_hud, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text("Keyboard shortcuts (?)")
                        .clicked()
                    {
                        self.show_shortcuts_hud = !self.show_shortcuts_hud;
                    }

                    let main_txt = format!("{}", (self.master_volume * 100.0).round() as i32);
                    dot::dot_ring(ui, &mut self.master_volume, 0.0, 1.0, 0.8, &main_txt, "MAIN", TraktorTheme::TEXT_PRIMARY(), true)
                        .on_hover_text(format!("Main output level ({main_txt}%). {KNOB_TIP}"));

                    let is_rec = self.audio.shared.lock().map(|s| s.is_recording).unwrap_or(false);
                    let rec_sec = self
                        .audio
                        .shared
                        .lock()
                        .map(|s| s.recorded_samples.len() as f64 / (48_000.0 * 2.0))
                        .unwrap_or(0.0);
                    if show_rec || is_rec {
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
                                "Stop and save recording, CUE & tracklist to ~/Music/ReTraktor"
                            } else {
                                "Record master mix to ~/Music/ReTraktor (WAV, CUE & TXT)"
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
                                    self.set_state.recorded_audible_tracks.clear();
                                    self.set_state.recording_start = Some(std::time::Instant::now());
                                }
                            }
                            if let Some(samples) = samples_to_save {
                                let home = env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
                                let rec_dir = home.join("Music").join("ReTraktor");
                                let _ = std::fs::create_dir_all(&rec_dir);
                                let (out_wav, out_cue, out_txt) = crate::setlist::unique_recording_paths(&rec_dir, std::time::SystemTime::now());
                                match AudioSystem::write_stereo_wav_file(&out_wav, &samples, 48_000) {
                                    Ok(dur_s) => {
                                        let entries: Vec<(f64, &str, &str)> = self
                                            .set_state
                                            .recorded_audible_tracks
                                            .iter()
                                            .map(|(s, a, t)| (*s, a.as_str(), t.as_str()))
                                            .collect();
                                        let wav_name = out_wav.file_name().and_then(|n| n.to_str()).unwrap_or("recording.wav");
                                        let cue_str = crate::setlist::cue_sheet(&entries, wav_name);
                                        let _ = std::fs::write(&out_cue, cue_str);
                                        let txt_str = crate::setlist::plain_tracklist(&entries);
                                        let _ = std::fs::write(&out_txt, txt_str);

                                        self.library.status_message = format!(
                                            "✔ Saved recording ({dur_s:.1}s), CUE & tracklist to {}",
                                            rec_dir.display()
                                        );
                                    }
                                    Err(e) => {
                                        self.library.status_message = format!("⚠ Failed to save WAV recording: {e}");
                                    }
                                }
                            }
                        }
                    }

                    if roomy {
                        let (ctl_label, ctl_ok, ctl_tab, controller_tip) = if let Some(ref node) = self.mx2_dev_node {
                            ("MX2", true, PrefsTab::Controllers, format!("Traktor MX2 connected ({node}). Click for controller settings"))
                        } else {
                            let generic_devs = self.preferences.generic_manager.discovered.lock().map(|d| d.clone()).unwrap_or_default();
                            if let Some(dev) = generic_devs.iter().find(|d| d.connected) {
                                let label = if dev.slug.contains("flx4") {
                                    "FLX4"
                                } else if dev.slug.contains("x1") {
                                    "X1"
                                } else if dev.slug.contains("z1") {
                                    "Z1"
                                } else if dev.slug.contains("s4") {
                                    "S4"
                                } else if dev.slug.contains("s2") {
                                    "S2"
                                } else if dev.slug.contains("s3") {
                                    "S3"
                                } else {
                                    "MIDI"
                                };
                                (label, true, PrefsTab::OtherControllers, format!("{} connected ({}). Click for controller settings", dev.name, dev.dev_node))
                            } else {
                                ("MX2", false, PrefsTab::Controllers, "No controller connected. Click for controller settings".to_string())
                            }
                        };
                        if lamp(ui, ctl_label, ctl_ok).on_hover_text(controller_tip).clicked() {
                            self.preferences.is_open = true;
                            self.preferences.current_tab = ctl_tab;
                        }
                    }


                });
            });
        });
    }

    /// One deck's top-bar strip: letter, then GAIN, FILTER and FX as dot rings with the FX on/off lamp.
    fn deck_strip(&mut self, ui: &mut Ui, d: usize) {
        let col = TraktorTheme::deck_color(d + 1);
        let letter = DECK_LETTERS[d];
        let shift = if d == 1 { "Shift+" } else { "" };
        deck_badge(ui, letter, col);
        let gain_txt = dot::format_db(self.channel_gain[d], 0.5);
        dot::dot_ring(ui, &mut self.channel_gain[d], 0.0, 1.0, 0.5, &gain_txt, "GAIN", col, true)
            .on_hover_text(format!("Deck {letter} input gain. {KNOB_TIP}"));
        let filter_txt = dot::format_filter_val(self.channel_filter[d], self.channel_filter_on[d]);
        let filter = dot::dot_ring(ui, &mut self.channel_filter[d], 0.0, 1.0, 0.5, &filter_txt, "FILTER", col, self.channel_filter_on[d])
            .on_hover_text(format!("Deck {letter} filter ({shift}F or right-click turns it on or off). {KNOB_TIP}"));
        if filter.changed() {
            self.channel_filter_on[d] = (self.channel_filter[d] - 0.5).abs() > 0.02;
        }
        if filter.secondary_clicked() {
            self.channel_filter_on[d] ^= true;
        }
        let (mix, fx_on) = if d == 0 { (&mut self.fx1_dry_wet, &mut self.fx1_on) } else { (&mut self.fx2_dry_wet, &mut self.fx2_on) };
        let fx_txt = format!("{}", (*mix * 100.0).round() as i32);
        dot::dot_ring(ui, mix, 0.0, 1.0, 0.3, &fx_txt, "FX", col, *fx_on)
            .on_hover_text(format!("Deck {letter} effect amount. {KNOB_TIP}"));
        dot::dot_lamp(ui, fx_on, col, &format!("Deck {letter} effect on or off ({shift}D)"));
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

                // All four voices at once as a 4 x 16 dot grid; the playing step sweeps across as a bright column.
                let v = self.pattern_selected_voice.min(3);
                let grid_w = (ui.available_width() - 250.0).clamp(16.0 * 14.0, 16.0 * 34.0);
                let (grid, resp) = ui.allocate_exact_size(Vec2::new(grid_w, 4.0 * 18.0), Sense::click());
                let cell = Vec2::new(grid_w / 16.0, 18.0);
                if let (true, Some(pos)) = (resp.clicked(), resp.interact_pointer_pos()) {
                    let step = (((pos.x - grid.left()) / cell.x) as usize).min(15);
                    let voice = (((pos.y - grid.top()) / cell.y) as usize).min(3);
                    self.pattern_steps[voice][step] ^= true;
                    self.pattern_selected_voice = voice;
                }
                let sweep = crate::theme::animations() && running;
                for voice in 0..4 {
                    for step in 0..16 {
                        let centre = grid.min + Vec2::new((step as f32 + 0.5) * cell.x, (voice as f32 + 0.5) * cell.y);
                        let on_step = running && cur_step == step;
                        // The column just played fades out behind the sweep.
                        let trail = if sweep && (cur_step + 16 - step) % 16 == 1 { 0.35 } else { 0.0 };
                        let lit = self.pattern_steps[voice][step];
                        let colour = if lit {
                            if on_step { TraktorTheme::TEXT_HIGHLIGHT() } else { TraktorTheme::ACCENT() }
                        } else if on_step {
                            TraktorTheme::TEXT_SECONDARY()
                        } else {
                            crate::theme::mix(dot::unlit_on_panel(), TraktorTheme::TEXT_SECONDARY(), trail + if voice == v { 0.25 } else { 0.0 })
                        };
                        let radius = if lit { 6.0 } else if step % 4 == 0 { 3.2 } else { 2.4 };
                        ui.painter().circle_filled(centre, if on_step { radius + 1.0 } else { radius }, colour);
                    }
                }
                resp.on_hover_text("Rows are kick, clap, hat and percussion; click a dot to switch that step on or off");
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
            row(ui, 50.0, |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                ui.label(caps("CURVE"));
                for (idx, name, tip) in [
                    (0usize, "CROSSFADE", "Smooth crossfade: every stem plays on both decks (U)"),
                    (1usize, "BASS SWAP", "Deck A drops its bass as deck B brings its bass in (U)"),
                    (2usize, "VOCAL BRIDGE", "Deck A's vocals over deck B's instrumental (U)"),
                ] {
                    if pill(ui, name, self.timeline_preset_idx == idx, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.timeline_preset_idx = idx;
                        match idx {
                            0 => {
                                self.deck_stem_muted[0] = [false; 4];
                                self.deck_stem_muted[1] = [false; 4];
                            }
                            1 => {
                                self.deck_stem_muted[0][1] = true;
                                self.deck_stem_muted[1][1] = false;
                            }
                            _ => {
                                self.deck_stem_muted[0] = [true, true, true, false];
                                self.deck_stem_muted[1] = [false, false, false, true];
                            }
                        }
                    }
                }

                ui.separator();

                // Live animated crossfader cursor position (smoothed when animations are enabled)
                let anim = crate::theme::animations();
                let cursor_xfader = if anim {
                    ui.ctx().animate_value_with_time(egui::Id::new("timeline_cursor_xfader"), self.crossfader_pos, 0.08)
                } else {
                    self.crossfader_pos
                };

                // Curve wipe animation when preset changes
                let prev_preset_id = egui::Id::new("timeline_prev_preset");
                let wipe_start_id = egui::Id::new("timeline_wipe_start");
                let prev_preset = ui.data(|d| d.get_temp::<usize>(prev_preset_id));
                let cur_time = ui.input(|i| i.time);
                if let Some(prev) = prev_preset {
                    if prev != self.timeline_preset_idx {
                        ui.data_mut(|d| {
                            d.insert_temp(prev_preset_id, self.timeline_preset_idx);
                            d.insert_temp(wipe_start_id, cur_time);
                        });
                    }
                } else {
                    ui.data_mut(|d| d.insert_temp(prev_preset_id, self.timeline_preset_idx));
                }
                let wipe_start = ui.data(|d| d.get_temp::<f64>(wipe_start_id)).unwrap_or(0.0);
                let wipe_elapsed = (cur_time - wipe_start) as f32;
                let wipe_duration = 0.28;
                let wipe_progress = if anim && wipe_elapsed < wipe_duration {
                    ui.ctx().request_repaint();
                    (wipe_elapsed / wipe_duration).clamp(0.0, 1.0)
                } else {
                    0.0
                };

                // Compact dot-matrix transition display
                let disp_w = (ui.available_width() - 260.0).clamp(240.0, 360.0);
                let mut disp_resp = dot::dot_transition_display(
                    ui,
                    self.timeline_preset_idx,
                    self.timeline_transition_bars,
                    cursor_xfader,
                    wipe_progress,
                    Vec2::new(disp_w, 42.0),
                );
                disp_resp = disp_resp.on_hover_text("Transition curve: Deck A falls, Deck B rises. Drag to move crossfader, click to cycle curve (U)");
                if disp_resp.clicked() {
                    let next = (self.timeline_preset_idx + 1) % 3;
                    self.timeline_preset_idx = next;
                    match next {
                        0 => {
                            self.deck_stem_muted[0] = [false; 4];
                            self.deck_stem_muted[1] = [false; 4];
                        }
                        1 => {
                            self.deck_stem_muted[0][1] = true;
                            self.deck_stem_muted[1][1] = false;
                        }
                        _ => {
                            self.deck_stem_muted[0] = [true, true, true, false];
                            self.deck_stem_muted[1] = [false, false, false, true];
                        }
                    }
                }
                if disp_resp.dragged() {
                    let delta = disp_resp.drag_delta().x / (disp_w - 44.0).max(20.0);
                    self.crossfader_pos = (self.crossfader_pos + delta).clamp(0.0, 1.0);
                }

                ui.separator();

                // Bar length choice (8/16/32) shown as lit segments
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for bars in [8u32, 16, 32] {
                        let is_selected = self.timeline_transition_bars == bars;
                        let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(28.0, HIT), Sense::click());
                        let fill = if is_selected { TraktorTheme::ROW_SELECTED() } else { TraktorTheme::CONTROL() };
                        let border = if is_selected { TraktorTheme::BORDER_ACCENT() } else { TraktorTheme::BORDER_COLOR() };
                        ui.painter().rect(rect, 3.0, fill, Stroke::new(1.0_f32, border));
                        let lamp_col = if is_selected { TraktorTheme::ACCENT() } else { dot::unlit_on_panel() };
                        ui.painter().circle_filled(egui::pos2(rect.center().x, rect.top() + 4.5), 1.6, lamp_col);
                        let fg = if is_selected { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_MUTED() };
                        ui.painter().text(
                            egui::pos2(rect.center().x, rect.bottom() - 3.5),
                            egui::Align2::CENTER_BOTTOM,
                            &format!("{bars}"),
                            egui::FontId::monospace(10.0),
                            fg,
                        );
                        resp = resp.on_hover_text(format!("Transition over {bars} bars (B)"));
                        if resp.clicked() {
                            self.timeline_transition_bars = bars;
                        }
                        ring(ui, resp);
                    }
                });
                ui.label(caps("BARS"));

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (fill, fg) = (TraktorTheme::CONTROL(), TraktorTheme::TEXT_PRIMARY());
                    if icon_pill(ui, Icon::Play, "AUDITION", fill, fg, Vec2::new(0.0, HIT))
                        .on_hover_text("Sync deck B to deck A and play both (A)")
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

        let idle = self.idle_deck();
        self.set_state.show_next_up_strip(ui, &mut self.library, idle);

        ui.horizontal_top(|ui| {
            let mixer_w = (ui.available_width() * 0.38).clamp(380.0, 560.0);
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
            let phase = self.audio.shared.lock().ok().and_then(|state| {
                let other = if d == 0 { 1 } else { 0 };
                let beats = |i: usize| {
                    let t = state.loaded_tracks[i].as_ref().filter(|_| state.is_playing[i])?;
                    Some((state.playhead_sec[i] - t.grid_offset_sec) * t.bpm.max(20.0) / 60.0)
                };
                Some(beat_phase_diff(beats(other)?, beats(d)?))
            });
            let in_key = deck_idx != 1
                && self.audio.shared.lock().is_ok_and(|s| {
                    s.loaded_tracks[0].as_ref().is_some_and(|a| KeyWheel::are_harmonically_compatible(&a.key, &deck.key))
                });
            let is_loaded = self.audio.shared.lock().is_ok_and(|s| s.loaded_tracks[d].is_some());
            let loaded_t = crate::theme::fade(ui.ctx(), ("deck_loaded", d), is_loaded);
            let looping = crate::theme::fade(ui.ctx(), ("deck_loop", d), self.deck_loop_active[d]);

            row(ui, 48.0, |ui| {
                deck_badge(ui, deck_letter, accent_color);
                self.stem_buttons(ui, d);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let display_w = 340.0;
                    let panel_rect = display(ui, |ui, ink| {
                        ui.set_width(display_w);
                        ui.vertical(|ui| {
                            // Beat strip: the bar counter sweeps with the music, the phase meter shows drift from the other deck.
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 10.0;
                                let bar_w = ((display_w - 10.0) * 0.60).max(60.0);
                                let (bar, _) = ui.allocate_exact_size(Vec2::new(bar_w, 11.0), Sense::hover());
                                dot::bar_counter(ui.painter(), bar, (deck.elapsed_sec - deck.grid_offset_sec) * deck.bpm / 60.0, deck.is_playing, accent_color, ink);
                                let meter_w = (display_w - bar_w - 10.0).max(40.0);
                                let (meter, resp) = ui.allocate_exact_size(Vec2::new(meter_w, 11.0), Sense::hover());
                                dot::phase_meter(ui.painter(), meter, phase, accent_color, ink);
                                resp.on_hover_text("Beat phase against the other deck: centre means the beats are locked");
                            });
                            // Readout numbers in dot font: BPM, KEY, REMAIN with fixed columns so A and B headers align
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let size = 18.0;
                                let pitch_pct = self.deck_pitch_fader[d];
                                let bpm_txt = format!("{:.2}", deck.bpm * (1.0 + pitch_pct as f64 / 100.0));
                                ui.allocate_ui_with_layout(Vec2::new(95.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                    readout(ui, "BPM", &bpm_txt, size, ink);
                                });
                                ui.allocate_ui_with_layout(Vec2::new(85.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                    readout(ui, if in_key { "IN KEY" } else { "KEY" }, &deck.key, size, ink);
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    readout(ui, "REMAIN", &deck.remaining(), size, ink);
                                });
                            });
                        });
                    });
                    dot::paint_dot_wipe(ui.painter(), panel_rect, (1.0 - loaded_t).max(looping), accent_color);
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
            let mixer_w = (ui.available_width() * 0.36).clamp(460.0, 560.0);
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
            // The other deck's beat position, for the phase meter (only while both play).
            let phase = self.audio.shared.lock().ok().and_then(|state| {
                let other = if d == 0 { 1 } else { 0 };
                let beats = |i: usize| {
                    let t = state.loaded_tracks[i].as_ref().filter(|_| state.is_playing[i])?;
                    Some((state.playhead_sec[i] - t.grid_offset_sec) * t.bpm.max(20.0) / 60.0)
                };
                Some(beat_phase_diff(beats(other)?, beats(d)?))
            });
            let looping = crate::theme::fade(ui.ctx(), ("deck_loop", d), self.deck_loop_active[d]);
            let panel_rect = display(ui, |ui, ink| {
                ui.vertical(|ui| {
                    // Beat strip: the bar counter sweeps with the music, the phase meter shows drift from the other deck.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let w = ui.available_width();
                        let (bar, _) = ui.allocate_exact_size(Vec2::new((w - 12.0) * 0.62, 12.0), Sense::hover());
                        dot::bar_counter(ui.painter(), bar, (deck.elapsed_sec - deck.grid_offset_sec) * bpm / 60.0, deck.is_playing, accent_color, ink);
                        let (meter, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 12.0), Sense::hover());
                        dot::phase_meter(ui.painter(), meter, phase, accent_color, ink);
                        resp.on_hover_text("Beat phase against the other deck: centre means the beats are locked");
                    });
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        let advance = ui.fonts(|f| f.glyph_width(&crate::theme::dot_font(100.0), '0')) / 100.0;
                        let size = ((ui.available_width() - 3.0 * 14.0) / (20.0 * advance)).floor().clamp(16.0, 30.0);
                        let pitch_pct = self.deck_pitch_fader[d];
                        readout(ui, "BPM", &format!("{:.2}", bpm * (1.0 + pitch_pct as f64 / 100.0)), size, ink);
                        readout(ui, "PITCH", &format!("{pitch_pct:+.1}%"), size, ink);
                        // A compatible key says so in words, not just in brightness.
                        readout(ui, if in_key { "IN KEY" } else { "KEY" }, &deck.key, size, ink);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            readout(ui, "REMAIN", &deck.remaining(), size, ink);
                        });
                    });
                });
            });
            // A sweep of dots crosses the display when a track lands or a loop engages.
            dot::paint_dot_wipe(ui.painter(), panel_rect, loaded_t, accent_color);
            dot::paint_dot_wipe(ui.painter(), panel_rect, looping, accent_color);

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
                    if pill(ui, "TOOLS", expanded, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text("Track tools: create stems, fix the beatgrid or tempo, and the hotcue pads")
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

            // 8. Track tools and the hotcue pads, behind "TOOLS".
            if !self.deck_expanded[d] {
                return;
            }
            ui.multiply_opacity(pads_t);
            let gap = ui.spacing().item_spacing.x;
            ui.horizontal(|ui| {
                let shift = if d == 1 { "Shift+" } else { "" };
                let progress = self.library.stem_job_progress.filter(|(id, _)| self.deck_track_ids[d] == Some(*id));
                let label = match progress {
                    Some((_, p)) => format!("STEMS {:.0}%", p * 100.0),
                    None => "CREATE STEMS".to_string(),
                };
                if pill(ui, &label, progress.is_some(), TraktorTheme::ACCENT(), 0.0)
                    .on_hover_text(format!("Separate this track into drums, bass, other and vocals with Demucs ({shift}G). Takes a few minutes; saved for next time."))
                    .clicked()
                    && progress.is_none()
                {
                    self.request_deck_stems(deck_idx);
                }
                ui.separator();
                ui.label(caps("GRID"));
                for (label, step, tip) in [("◀", -0.005, "Move the beatgrid 5 ms earlier"), ("▶", 0.005, "Move the beatgrid 5 ms later")] {
                    if pill(ui, label, false, Color32::TRANSPARENT, HIT).on_hover_text(tip).clicked() {
                        self.adjust_deck_grid(d, step, 1.0);
                    }
                }
                ui.label(caps("BPM"));
                for (label, factor, tip) in [("×½", 0.5, "Halve the track's tempo (analysis found double time)"), ("×2", 2.0, "Double the track's tempo (analysis found half time)")] {
                    if pill(ui, label, false, Color32::TRANSPARENT, HIT).on_hover_text(tip).clicked() {
                        self.adjust_deck_grid(d, 0.0, factor);
                    }
                }
            });
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 7.0 * gap) / 8.0;
                // Always hotcues on screen; the other pad modes belong to the hardware pads.
                let mode = DeckPadMode::Hotcues;
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

    /// Centre mixer, mirrored around the main meter: unified continuous dot-matrix canvas
    /// holding per-deck EQ/stems zones, channel fader-on-meter columns, MAIN meter,
    /// and crossfader ribbon on a single continuous dot lattice.
    fn show_mixer_section(&mut self, ui: &mut Ui, two_channel_only: bool) {
        // Live levels for the displays: band and stem peaks at the playhead, silent when stopped.
        let mut bands = [[0.0f32; 3]; 4];
        let mut stems = [[0.0f32; 4]; 4];
        let (mut peaks, mut main) = ([0.0f32; 4], (0.0, 0.0));
        let mut both_playing = false;
        let mut is_recording = false;
        let mut in_key = false;
        if let Ok(state) = self.audio.shared.lock() {
            peaks = state.channel_peaks;
            main = (state.master_peak_l, state.master_peak_r);
            is_recording = state.is_recording;
            both_playing = state.is_playing[0] || state.is_playing[1];
            in_key = state.loaded_tracks[0].as_ref().is_some_and(|a| {
                state.loaded_tracks[1].as_ref().is_some_and(|b| {
                    retraktor_library::KeyWheel::are_harmonically_compatible(&a.key, &b.key)
                })
            });
            for d in 0..4 {
                if let (Some(t), true) = (&state.loaded_tracks[d], state.is_playing[d]) {
                    let at = (state.playhead_sec[d].max(0.0) * PEAKS_PER_SEC as f64) as usize;
                    bands[d] = t.waveform_peaks.get(at).copied().unwrap_or_default();
                    stems[d] = t.stem_peaks.get(at).copied().unwrap_or_default();
                }
            }
        }

        // Crossfade cheers state machine
        let now_sec = ui.input(|i| i.time);
        if crate::theme::animations() {
            if let Some(cheer_idx) = self.xfader_cheer_sm.update(self.crossfader_pos, both_playing, now_sec) {
                let deck = self.xfader_cheer_sm.target_deck;
                self.active_cheer = Some(dot::ActiveCheer {
                    deck,
                    cheer_index: cheer_idx,
                    start_sec: now_sec,
                });
            }
        } else {
            self.active_cheer = None;
        }

        if let Some(cheer) = self.active_cheer {
            if !crate::theme::animations() || now_sec - cheer.start_sec >= 1.2 {
                self.active_cheer = None;
            } else {
                ui.ctx().request_repaint();
            }
        }

        let stem_progress = [
            self.library.stem_job_progress.filter(|(id, _)| self.deck_track_ids[0] == Some(*id)).map(|(_, p)| p),
            self.library.stem_job_progress.filter(|(id, _)| self.deck_track_ids[1] == Some(*id)).map(|(_, p)| p),
            self.library.stem_job_progress.filter(|(id, _)| self.deck_track_ids[2] == Some(*id)).map(|(_, p)| p),
            self.library.stem_job_progress.filter(|(id, _)| self.deck_track_ids[3] == Some(*id)).map(|(_, p)| p),
        ];

        let screen_h = ui.ctx().screen_rect().height();
        let h = (175.0 + (screen_h - 640.0) * 0.35).clamp(175.0, 270.0);

        panel(TraktorTheme::BG_PANEL_ALT()).show(ui, |ui| {
            let mut mixer = dot::ContinuousMixer {
                two_channel_only,
                volume: &mut self.channel_volume,
                cue: &mut self.channel_cue,
                eq_low: &mut self.channel_eq_low,
                eq_mid: &mut self.channel_eq_mid,
                eq_hi: &mut self.channel_eq_hi,
                eq_kill_low: &mut self.eq_kill_low,
                eq_kill_mid: &mut self.eq_kill_mid,
                eq_kill_hi: &mut self.eq_kill_hi,
                filter: self.channel_filter,
                filter_on: self.channel_filter_on,
                stem_view: self.deck_stem_view,
                stem_muted: &mut self.deck_stem_muted,
                stem_fx_send: &mut self.deck_stem_fx_send,
                crossfader: &mut self.crossfader_pos,
                channel_peaks: peaks,
                band_peaks: bands,
                stem_peaks: stems,
                main_peaks: main,
                stem_progress,
                active_cheer: &mut self.active_cheer,
                deck_track_ids: self.deck_track_ids,
                deck_sync_enabled: self.deck_sync_enabled,
                in_key,
                is_recording,
                size: Vec2::new(ui.available_width(), h),
            };
            mixer.show(ui);
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

    fn toggle_next_track_panel(&mut self) {
        self.show_next_track_panel = !self.show_next_track_panel;
        if self.show_next_track_panel {
            self.compute_next_track_picks();
        }
    }

    fn compute_next_track_picks(&mut self) {
        let (live_d, idle_d) = {
            let (play_a, play_b) = if let Ok(state) = self.audio.shared.lock() {
                (state.is_playing[0], state.is_playing[1])
            } else {
                (false, false)
            };
            if play_a && !play_b {
                (0, 1)
            } else if play_b && !play_a {
                (1, 0)
            } else if self.crossfader_pos < 0.5 {
                (0, 1)
            } else {
                (1, 0)
            }
        };
        self.next_track_target_deck = idle_d + 1;

        let live_ref_opt = self.deck_track_ids[live_d]
            .and_then(|id| self.library.track(id).cloned())
            .or_else(|| self.library.history.last().and_then(|&id| self.library.track(id).cloned()))
            .or_else(|| self.library.tracks.iter().find(|t| !t.missing && t.file_path.is_some()).cloned());

        if let Some(live_ref) = live_ref_opt {
            let mut scored: Vec<(usize, f32, String)> = self.library.tracks
                .iter()
                .filter(|t| !t.missing && t.file_path.is_some())
                .filter_map(|t| {
                    let s = crate::ai::score_next_candidate(t, &live_ref, &self.library.history);
                    if s > 0.0 {
                        let reason = crate::ai::explain_candidate(t, &live_ref);
                        Some((t.id, s, reason))
                    } else {
                        None
                    }
                })
                .collect();
            scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            if scored.is_empty() {
                scored = self.library.tracks
                    .iter()
                    .filter(|t| !t.missing && t.file_path.is_some() && t.id != live_ref.id)
                    .take(5)
                    .map(|t| (t.id, 0.5f32, format!("{} BPM · {}", if t.bpm > 0.0 { format!("{:.1}", t.bpm) } else { "-".into() }, if !t.musical_key.is_empty() { &t.musical_key } else { "-" })))
                    .collect();
            }
            self.next_track_picks = scored.into_iter().take(5).collect();
        }
    }

    fn render_next_track_window(&mut self, ctx: &egui::Context) {
        if let Some(runner) = &self.ai_rerank_runner {
            if let Some(res) = runner.poll() {
                match res {
                    Ok(ranked) => {
                        let mut new_picks = Vec::new();
                        for (id, reason) in ranked.into_iter().take(5) {
                            new_picks.push((id, 1.0f32, reason));
                        }
                        if !new_picks.is_empty() {
                            self.next_track_picks = new_picks;
                        }
                        self.ai_rerank_status = None;
                        self.ai_rerank_runner = None;
                    }
                    Err(e) => {
                        self.ai_rerank_status = Some(format!("AI error: {e}"));
                        self.ai_rerank_runner = None;
                    }
                }
            }
        }

        if !self.show_next_track_panel || self.preferences.is_open {
            return;
        }

        let deck_letter = if self.next_track_target_deck == 1 { "A" } else { "B" };
        let deck_accent = if self.next_track_target_deck == 1 { TraktorTheme::DECK_A() } else { TraktorTheme::DECK_B() };

        let mut load_id = None;

        egui::Window::new("Suggest next track")
            .open(&mut self.show_next_track_panel)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::new(0.0, -20.0))
            .default_width(520.0)
            .frame(
                egui::Frame::window(&ctx.style())
                    .fill(TraktorTheme::BG_PANEL())
                    .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT()))
                    .rounding(6.0)
                    .inner_margin(14.0),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("NEXT TRACK PICKS")
                            .color(TraktorTheme::TEXT_PRIMARY())
                            .font(egui::FontId::monospace(13.0))
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("→ DECK {deck_letter}"))
                            .color(deck_accent)
                            .font(egui::FontId::monospace(13.0))
                            .strong(),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ai_settings = crate::ai::AiSettings::load();
                        if ai_settings.is_configured() {
                            let is_busy = self.ai_rerank_runner.is_some();
                            let ai_btn = egui::Button::new(
                                RichText::new(if is_busy { "AI THINKING…" } else { "✦ ASK AI" })
                                    .size(11.0)
                                    .color(crate::theme::on(TraktorTheme::ROW_SELECTED()))
                                    .strong(),
                            )
                            .fill(TraktorTheme::ROW_SELECTED())
                            .rounding(3.0)
                            .min_size(Vec2::new(76.0, 22.0));

                            if ui.add_enabled(!is_busy, ai_btn).on_hover_text("Re-rank candidates using your configured AI model").clicked() {
                                let (live_d, _) = if self.next_track_target_deck == 1 { (1, 0) } else { (0, 1) };
                                let live_ref_opt = self.deck_track_ids[live_d]
                                    .and_then(|id| self.library.track(id).cloned())
                                    .or_else(|| self.library.history.last().and_then(|&id| self.library.track(id).cloned()))
                                    .or_else(|| self.library.tracks.first().cloned());
                                if let Some(live_ref) = live_ref_opt {
                                    let candidates: Vec<crate::ai::TrackBrief> = self.library.tracks
                                        .iter()
                                        .filter(|t| !t.missing && t.file_path.is_some() && t.id != live_ref.id)
                                        .take(30)
                                        .map(crate::ai::TrackBrief::from_ui)
                                        .collect();
                                    let live_brief = crate::ai::TrackBrief::from_ui(&live_ref);
                                    let prompt = crate::ai::build_rerank_prompt(&live_brief, &candidates);
                                    self.ai_rerank_status = Some("Asking AI model to evaluate flow…".to_string());
                                    self.ai_rerank_runner = Some(crate::ai::AiRerankRunner::start(ai_settings, prompt, candidates));
                                }
                            }
                        }
                    });
                });

                ui.add_space(4.0);
                ui.separator();
                ui.add_space(6.0);

                if let Some(err) = &self.ai_rerank_status {
                    ui.label(RichText::new(err).color(TraktorTheme::METER_YELLOW()).size(11.0));
                    ui.add_space(4.0);
                }

                if self.next_track_picks.is_empty() {
                    ui.label(RichText::new("No recommendations available. Load a track onto the live deck first.").color(TraktorTheme::TEXT_MUTED()));
                } else {
                    for (rank, &(id, _score, ref reason)) in self.next_track_picks.iter().enumerate() {
                        if let Some(track) = self.library.track(id).cloned() {
                            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::click());
                            let hovered = resp.hovered();

                            let bg = if rank == 0 {
                                TraktorTheme::ROW_SELECTED().gamma_multiply(0.35)
                            } else if hovered {
                                TraktorTheme::BG_PANEL_ALT()
                            } else {
                                Color32::TRANSPARENT
                            };
                            ui.painter().rect_filled(rect, 4.0, bg);

                            // Left dot indicator & rank
                            let dot_pos = rect.min + Vec2::new(12.0, 17.0);
                            ui.painter().circle_filled(dot_pos, 4.0, deck_accent);
                            ui.painter().text(
                                rect.min + Vec2::new(22.0, 10.0),
                                egui::Align2::LEFT_TOP,
                                format!("{}.", rank + 1),
                                egui::FontId::monospace(11.0),
                                TraktorTheme::TEXT_SECONDARY(),
                            );

                            // Title & Artist
                            let text_pos = rect.min + Vec2::new(42.0, 4.0);
                            // Clipped so a long title cannot run under the BPM and key on the right.
                            ui.painter().with_clip_rect(egui::Rect::from_min_max(rect.min, egui::Pos2::new(rect.right() - 96.0, rect.bottom()))).text(
                                text_pos,
                                egui::Align2::LEFT_TOP,
                                format!("{} – {}", track.title, track.artist),
                                egui::FontId::proportional(12.5),
                                TraktorTheme::TEXT_PRIMARY(),
                            );

                            // Reason line
                            let reason_pos = rect.min + Vec2::new(42.0, 19.0);
                            ui.painter().text(
                                reason_pos,
                                egui::Align2::LEFT_TOP,
                                reason,
                                egui::FontId::proportional(11.0),
                                TraktorTheme::ACCENT(),
                            );

                            // Right: BPM & Key badge
                            let badge_text = format!("{} · {}", if track.bpm > 0.0 { format!("{:.1}", track.bpm) } else { "-".into() }, if !track.musical_key.is_empty() { &track.musical_key } else { "-" });
                            let badge_pos = egui::pos2(rect.max.x - 10.0, rect.min.y + 10.0);
                            ui.painter().text(
                                badge_pos,
                                egui::Align2::RIGHT_TOP,
                                badge_text,
                                egui::FontId::monospace(11.5),
                                TraktorTheme::TEXT_SECONDARY(),
                            );

                            if resp.double_clicked() || (resp.clicked() && rank == 0) {
                                load_id = Some(id);
                            }
                        }
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Press Enter or double-click to load top pick. Esc to close.")
                            .color(TraktorTheme::TEXT_MUTED())
                            .font(egui::FontId::proportional(11.0)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some(&(top_id, _, _)) = self.next_track_picks.first() {
                            let btn = egui::Button::new(
                                RichText::new(format!("LOAD TO DECK {deck_letter} (↵)"))
                                    .color(crate::theme::on(TraktorTheme::ROW_SELECTED()))
                                    .strong(),
                            )
                            .fill(TraktorTheme::ROW_SELECTED())
                            .rounding(4.0)
                            .min_size(Vec2::new(140.0, 24.0));
                            if ui.add(btn).clicked() {
                                load_id = Some(top_id);
                            }
                        }
                    });
                });
            });

        if let Some(id) = load_id {
            let locked = self.locked_decks();
            self.library.load_to_deck(self.next_track_target_deck, id, locked);
            self.show_next_track_panel = false;
        }
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
fn display(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui, Color32)) -> egui::Rect {
    let bg = TraktorTheme::WAVEFORM_BG();
    egui::Frame::none()
        .fill(bg)
        .rounding(3.0)
        .inner_margin(egui::Margin::symmetric(8.0, 3.0))
        .show(ui, |ui| ui.horizontal(|ui| add_contents(ui, on(bg))))
        .response
        .rect
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
    ui.add(egui::Slider::new(pitch, -8.0..=8.0).clamping(egui::SliderClamping::Never).show_value(false)).on_hover_text("Pitch fader: tempo ±8%");
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

    #[test]
    fn crossfade_cheer_state_machine_transitions() {
        let mut sm = dot::CrossfadeCheerStateMachine::new();

        // 1. Initial state at Deck A side (< 0.1): no cheer yet
        assert_eq!(sm.update(0.05, true, 0.0), None);

        // 2. Crossfader moves through middle with both playing
        assert_eq!(sm.update(0.5, true, 1.0), None);

        // 3. Reaches Deck B side (> 0.9) at t=2.0
        assert_eq!(sm.update(0.95, true, 2.0), None);

        // 4. Still resting at t=3.5 (< 2.0s resting)
        assert_eq!(sm.update(0.95, true, 3.5), None);

        // 5. Rests for 2.0s at t=4.0 -> cheer triggers on Deck B!
        let cheer1 = sm.update(0.95, true, 4.0);
        assert!(cheer1.is_some());
        assert_eq!(sm.target_deck, 1); // Deck B

        // 6. Continues resting: does not re-trigger
        assert_eq!(sm.update(0.95, true, 4.5), None);

        // 7. Transition back from Right to Left, but paused halfway: no cheer!
        assert_eq!(sm.update(0.5, false, 5.0), None); // paused!
        assert_eq!(sm.update(0.05, true, 6.0), None); // reached Left
        assert_eq!(sm.update(0.05, true, 8.5), None); // rested 2.5s, but transit was invalid -> None!

        // 8. Now a valid transit from Left to Right:
        assert_eq!(sm.update(0.05, true, 9.0), None);
        assert_eq!(sm.update(0.5, true, 9.5), None);
        assert_eq!(sm.update(0.95, true, 10.0), None);
        let cheer2 = sm.update(0.95, true, 12.0);
        assert!(cheer2.is_some());
        assert_eq!(sm.target_deck, 1);
        // Consecutive cheers must not repeat the same index
        assert_ne!(cheer1, cheer2);

        // 9. Valid transit from Right to Left: triggers cheer on Deck A!
        assert_eq!(sm.update(0.5, true, 13.0), None);
        assert_eq!(sm.update(0.05, true, 14.0), None);
        let cheer3 = sm.update(0.05, true, 16.0);
        assert!(cheer3.is_some());
        assert_eq!(sm.target_deck, 0); // Deck A
        assert_ne!(cheer2, cheer3);
    }

    #[test]
    fn timeline_preset_and_bar_cycling() {
        let cycle_curve = |idx: usize| (idx + 1) % 3;
        assert_eq!(cycle_curve(0), 1);
        assert_eq!(cycle_curve(1), 2);
        assert_eq!(cycle_curve(2), 0);

        let cycle_bars = |bars: u32| match bars {
            8 => 16,
            16 => 32,
            _ => 8,
        };
        assert_eq!(cycle_bars(8), 16);
        assert_eq!(cycle_bars(16), 32);
        assert_eq!(cycle_bars(32), 8);
    }
}
