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
use std::sync::{Arc, mpsc};

/// A library track in the headphone preview: its audio, streamed in as it decodes, and the engine's serial for it.
#[derive(Clone)]
struct Preview {
    track: usize,
    pcm: Arc<crate::audio::PreviewPcm>,
    serial: u64,
}

/// What a deck load's worker sends, in this order, for the load (deck, generation) it was started for: the track,
/// playable (or why not); then, after it is playable, its overview as kept in the stripe cache (when the cache
/// didn't have it yet), the analysis of a track that lacked BPM or key and its cached AI stems. One channel, so they
/// arrive in that order.
enum DeckResult {
    Loaded(usize, u64, crate::library_ui::UiTrackEntry, Result<LoadedTrackAudio, String>),
    /// The track's overview as its browser row draws it, for the audio it was made from.
    Overview(usize, u64, Arc<Vec<f32>>, crate::overview::Overview),
    /// For library track (id): BPM, grid, key and phrases, the audio they were worked out from, and what the library
    /// already had (kept, not replaced).
    Analysis(usize, u64, usize, retraktor_library::TrackAnalysis, Arc<Vec<f32>>, Kept),
    Stems(usize, u64, LoadedTrackAudio),
}

/// What a track already had when its deck load started, which a late analysis must not replace.
#[derive(Clone, Copy, Debug)]
struct Kept {
    bpm: bool,
    /// A grid cue (imported or set by hand), whatever its offset, even 0.
    grid: bool,
    phrases: bool,
    /// The library track's hot cue revision at the load (`LibraryManager::cue_revision`): a hand edit since, on any
    /// deck, means no auto hot cues.
    cue_revision: u64,
}

/// How long a selection rests before its overview is decoded, so scrolling through the browser decodes nothing.
const BROWSE_SETTLE: std::time::Duration = std::time::Duration::from_millis(150);

/// Row waveforms kept in memory (8 KB each; finished ones are also in the stripe cache): many screens of browsing.
const ROW_WAVEFORMS_KEPT: usize = 1000;

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
    /// The highlighted pick: arrows move it, Enter loads it.
    pub next_track_selected: usize,
    /// Read when the picker opens, not every frame.
    cached_ai_settings: crate::ai::AiSettings,
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
    /// Headphone blend (0 = cue, 1 = main) and level.
    pub cue_mix: f32,
    pub cue_volume: f32,
    pub deck_keylock: [bool; 4],
    /// The sound-card settings (device, rate, buffer) last opened, and the width it asked for then.
    audio_applied: Option<((String, u32, u32), u16)>,
    pub master_tempo: f64,
    /// The deck whose tempo the MASTER button made the clock, if any.
    tempo_master_deck: Option<usize>,
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
    pub fx_kind: [MixerFxKind; 2],
    /// Which FX units (1, 2) each deck feeds.
    pub deck_fx_assign: [[bool; 2]; 4],

    // Decks Performance State
    pub deck_pitch_fader: [f32; 4], // -8% to +8%
    pub deck_flux_active: [bool; 4],
    pub deck_loop_active: [bool; 4],
    pub deck_loop_start_sec: [f64; 4],
    pub deck_loop_size_beats: [u32; 4],
    /// Beat jump size per deck, one of `JUMP_SIZES`.
    pub deck_jump_beats: [u32; 4],
    pub deck_hotcues: [Vec<(u8, f64)>; 4],
    pub deck_pad_mode: [DeckPadMode; 4],
    pub deck_sync_enabled: [bool; 4],
    /// Play state last frame, so a synced deck can snap onto the bar when either deck starts.
    sync_was_playing: [bool; 4],

    // Hybrid 4-Stem Separation State ([Drums, Bass, Instruments, Vocals])
    pub deck_stem_volumes: [[f32; 4]; 4],
    pub deck_stem_muted: [[bool; 4]; 4],
    pub deck_stem_fx_send: [[bool; 4]; 4],
    pub deck_stem_view: [bool; 4],
    pub deck_track_ids: [Option<usize>; 4],
    /// Per-deck "More": hotcue pads, pad modes, cue-play, flux and all loop sizes.
    pub deck_expanded: [bool; 4],
    pub deck_auto_cue_status: [Option<String>; 4],
    /// Switched from Browser to Mix after the first track load.
    first_load_done: bool,
    load_generation: [u64; 4],
    /// Generation of the load that actually succeeded on each deck (unaffected by later failed load attempts).
    deck_loaded_generation: [u64; 4],
    /// From worker dispatch until that generation's result arrives.
    deck_loading: [bool; 4],
    /// When the last load on a deck failed, shown on the deck for a few seconds.
    deck_load_failed: [Option<std::time::Instant>; 4],
    load_channel: (mpsc::Sender<DeckResult>, mpsc::Receiver<DeckResult>),
    /// The library track in the headphone preview, from its press until it ends or is stopped.
    preview: Option<Preview>,
    preview_loader: crate::audio::PreviewLoader,
    /// Where finished tracks' overview stripes are kept, for the rows and the decks (the preview worker has its own copy).
    stripes: crate::overview::StripeCache,
    /// Decoded (or decoding) audio of recent tracks: the selection's overview, the preview, and one more, for reuse.
    prepared: Vec<(usize, Arc<crate::audio::PreviewPcm>)>,
    /// The selection as last seen, and since when: its overview is decoded once it has rested [`BROWSE_SETTLE`].
    browse_seen: (Option<usize>, std::time::Instant),
    /// The resting selection (track, path) whose file was last looked at: once per selection, not every frame.
    browse_looked: Option<(usize, Option<PathBuf>)>,
    /// The tracks in `library.row_waveforms`, oldest first, to drop the oldest past [`ROW_WAVEFORMS_KEPT`].
    row_waveform_order: std::collections::VecDeque<usize>,
    /// The row in view whose waveform the preview worker is making (peaks only, no audio kept), after the preview
    /// and the selection.
    row_job: Option<(usize, Arc<crate::audio::PreviewPcm>)>,
    /// A deck whose BPM and key are still being worked out (shown as pending, not as zero).
    deck_analysing: [bool; 4],
    /// From REC until its WAV is finished (or left unsaved for RETRY SAVE).
    recording: Option<crate::recorder::Recording>,
    /// Close the window once the recording has been finished.
    close_after_recording: bool,

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
    /// SHIFT held on the MX2 left / right deck.
    deck_shift_held: [bool; 2],
    /// Preferences as last applied, so a change on either side (Preferences or the deck / library) wins.
    last_pref_jog_mode_turntable: bool,
    last_pref_hide_missing: bool,
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
            next_track_selected: 0,
            cached_ai_settings: crate::ai::AiSettings::load(),
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
            cue_mix: 0.0,
            cue_volume: 0.8,
            deck_keylock: [false; 4],
            audio_applied: None,
            master_tempo: 128.00,
            tempo_master_deck: None,
            crossfader_pos: 0.5,
            xfader_cheer_sm: dot::CrossfadeCheerStateMachine::new(),
            active_cheer: None,
            fx1_dry_wet: 0.3,
            fx1_param: [0.5, 0.5, 0.5],
            fx1_on: false,
            fx2_dry_wet: 0.3,
            fx2_param: [0.5, 0.5, 0.5],
            fx2_on: false,
            fx_kind: [MixerFxKind::Reverb, MixerFxKind::DualDelay],
            deck_fx_assign: [[true, false], [false, true], [true, false], [false, true]],
            deck_pitch_fader: [0.0, 0.0, 0.0, 0.0],
            deck_flux_active: [false, false, false, false],
            deck_loop_active: [false, false, false, false],
            deck_loop_start_sec: [0.0, 0.0, 0.0, 0.0],
            deck_loop_size_beats: [4, 4, 4, 4],
            deck_jump_beats: [4; 4],
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
            sync_was_playing: [false; 4],
            deck_stem_volumes: [[1.0; 4]; 4],
            deck_stem_muted: [[false; 4]; 4],
            deck_stem_fx_send: [[false; 4]; 4],
            deck_stem_view: [false, false, false, false],
            deck_expanded: [false; 4],
            deck_auto_cue_status: [None, None, None, None],
            deck_track_ids: [None; 4],
            first_load_done: false,
            load_generation: [0; 4],
            deck_loaded_generation: [0; 4],
            deck_loading: [false; 4],
            deck_load_failed: [None; 4],
            load_channel: mpsc::channel(),
            preview: None,
            preview_loader: crate::audio::PreviewLoader::new(crate::overview::StripeCache::user()),
            stripes: crate::overview::StripeCache::user(),
            prepared: Vec::new(),
            browse_seen: (None, std::time::Instant::now()),
            browse_looked: None,
            row_waveform_order: Default::default(),
            row_job: None,
            deck_analysing: [false; 4],
            recording: None,
            close_after_recording: false,
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
            deck_shift_held: [false; 2],
            last_pref_jog_mode_turntable: true, // deck_turntable_mode's default
            last_pref_hide_missing: false,
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
        // Full frame rate only while something moves or hardware needs polling; otherwise a slow tick.
        if self.needs_continuous_repaint() {
            ctx.request_repaint();
        } else {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if self.shot.is_some() {
            self.screenshot_step(ctx);
        }

        self.apply_audio_output();

        // Flush the autosaved collection and preferences on close.
        if self.poll_recording() && self.close_after_recording {
            if !self.exit_confirmed {
                self.save_collection_according_to_prefs();
                let _ = self.preferences.save();
                self.exit_confirmed = true;
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if self.recording.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if ctx.input(|i| i.viewport().close_requested()) && self.recording.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if self.recording.as_ref().is_some_and(|r| r.is_unsaved()) {
                self.library.status_message = "Recording has not been saved. Click RETRY SAVE before closing.".into();
            } else {
                self.stop_recording();
                self.close_after_recording = true;
                self.library.status_message = "Finishing the recording before closing…".into();
            }
        } else if ctx.input(|i| i.viewport().close_requested()) && !self.exit_confirmed {
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

        self.finish_loads();
        self.poll_preview();
        if self.deck_loading.iter().any(|&l| l) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Keys belong to the topmost layer: decide once, before any handler can close an overlay.
        self.dispatch_keys(ctx);
        if let Some(ids) = self.library.add_to_set_requested.take() {
            self.set_state.add_tracks(&ids);
        }

        let dt = ctx.input(|i| i.stable_dt);
        let (is_rec, rec_offset) = match &self.recording {
            Some(rec) if rec.is_active() => (true, rec.seconds()),
            _ => (false, 0.0),
        };
        if let Ok(state) = self.audio.shared.lock() {
            for d in 0..4 {
                let playing = state.is_playing[d];
                self.set_state.update_playback(d, playing, dt);

                if is_rec {
                    let vol = self.channel_volume[d];
                    let xfader = self.crossfader_pos;
                    let (artist, title) = state.loaded_tracks[d].as_ref().map_or(("", ""), |t| (t.artist.as_str(), t.title.as_str()));
                    self.set_state.check_audible_track(d, playing, vol, xfader, artist, title, rec_offset);
                }
            }
        }

        self.poll_mx2_hardware();
        self.sync_audio_state();

        crate::theme::apply_visuals(ctx);
        crate::theme::set_waveform_colours(self.preferences.waveform_color_mode);

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
                        // Planning views keep play, cue and time on screen.
                        self.show_mini_decks_bar(ui);
                        self.show_timeline_studio_layout(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Browser => {
                        self.show_mini_decks_bar(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Set => {
                        let idle = self.idle_deck();
                        let locks = self.locked_decks();
                        self.show_mini_decks_bar(ui);
                        self.set_state.show_set_view(ui, &mut self.library, idle, locks);
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
        // Only a changed preference resets the decks, so the MX2's TURNTABLE / JOG buttons stick.
        if self.preferences.jog_mode_turntable != self.last_pref_jog_mode_turntable {
            self.last_pref_jog_mode_turntable = self.preferences.jog_mode_turntable;
            self.deck_turntable_mode[0] = self.preferences.jog_mode_turntable;
            self.deck_turntable_mode[1] = self.preferences.jog_mode_turntable;
        }
        self.sync_hide_missing();
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
                // The decks holding this library track now: their stems decode on a worker and install only where that same audio still plays.
                let decks: Vec<(usize, LoadedTrackAudio)> = match self.audio.shared.lock() {
                    Ok(state) => (0..4)
                        .filter(|&d| self.deck_track_ids[d] == Some(track_id))
                        .filter_map(|d| Some((d, state.loaded_tracks[d].clone()?)))
                        .collect(),
                    Err(_) => Vec::new(),
                };
                for &(d, _) in &decks {
                    self.deck_stem_view[d] = true;
                }
                install_stems(self.audio.shared.clone(), decks, stem_dir.clone());
                *self.demucs.separation_status.lock().unwrap() = crate::demucs::DemucsSeparationStatus::Idle;
            }
            crate::demucs::DemucsSeparationStatus::Failed { track_id: _, error } => {
                self.library.stem_job_progress = None;
                self.library.status_message = format!("⚠ Stem separation failed: {error}");
                *self.demucs.separation_status.lock().unwrap() = crate::demucs::DemucsSeparationStatus::Idle;
            }
            crate::demucs::DemucsSeparationStatus::Idle => {}
        }

        // Start the next queued separation once Demucs is free.
        let demucs_idle = matches!(*self.demucs.separation_status.lock().unwrap(), crate::demucs::DemucsSeparationStatus::Idle);
        if demucs_idle && !self.demucs.is_installed() && !self.library.stem_queue.is_empty() {
            self.library.stem_queue.clear();
            self.library.status_message =
                "Install Demucs in Preferences > Stems & AI to separate stems (live crossover stems work without it).".to_string();
        }
        if let Some(track_id) = demucs_idle.then(|| self.library.stem_queue.pop_front()).flatten() {
            if let Some(track) = self.library.tracks.iter().find(|t| t.id == track_id).cloned() {
                if let Some(ref file_path) = track.file_path {
                    if self.demucs.is_installed() {
                        let started = self.demucs.start_separation(track.id, &track.title, file_path);
                        if started {
                            let more = self.library.stem_queue.len();
                            self.library.status_message = if more > 0 {
                                format!("Separating stems for '{}' ({more} more queued)...", track.title)
                            } else {
                                format!("Separating stems for '{}'...", track.title)
                            };
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
        self.preferences.audio_output = self.audio.status_line();
        self.audio.log_stream_trouble();
        self.preferences.audio_output_channels = self.audio.output_channels;
        self.preferences.audio_route = self.audio.output_route;
        self.preferences.audio_on_system_default = self.audio.on_system_default();

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
    /// Steps: Mix FourDeck Browser Timeline prefs prefs:<Tab> nav:Missing|Suggestions|Files loadA..loadD expandA expandAll stemsA pattern relocate wait.
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
            "Timeline" => {
                self.preferences.is_open = false;
                self.layout = TraktorLayout::Timeline;
                if let Ok(mut state) = self.audio.shared.lock() {
                    for d in 0..2 {
                        if state.loaded_tracks[d].is_none() {
                            state.loaded_tracks[d] = Some(crate::audio::LoadedTrackAudio {
                                title: if d == 0 { "Solaris (Extended Mix)".into() } else { "Midnight Sun (Original)".into() },
                                artist: if d == 0 { "Articuno".into() } else { "Helios".into() },
                                bpm: if d == 0 { 124.0 } else { 125.0 },
                                grid_offset_sec: 0.0,
                                key: if d == 0 { "8m".into() } else { "9m".into() },
                                duration_sec: 384.0,
                                pcm_stereo: Arc::new(vec![0.0; 48000]),
                                sample_rate: 48000,
                                waveform_peaks: vec![[0.4, 0.5, 0.3]; 300].into(),
                                stem_peaks: vec![[0.4, 0.5, 0.3, 0.2]; 300].into(),
                                overview: crate::overview::from_peaks(&[[0.4, 0.5, 0.3]; 300]),
                                stems_ready: true,
                                pcm_drums: None,
                                pcm_bass: None,
                                pcm_other: None,
                                pcm_vocals: None,
                                phrases: Vec::new(),
                                auto_gain: 1.0,
                                played_peaks: vec![[0.4, 0.5, 0.3]; 300].into(),
                                played_stem_peaks: vec![[0.4, 0.5, 0.3, 0.2]; 300].into(),
                                played_overview: crate::overview::from_peaks(&[[0.4, 0.5, 0.3]; 300]),
                            });
                        }
                    }
                }
            }
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
            "expandAll" => self.deck_expanded = [true; 4],
            "pick" => self.library.selected_track_id = self.library.tracks.first().map(|t| t.id),
            "master" => self.tempo_master_deck = Some(0),
            "setpick" => self.set_state.selected_index = Some(1),
            s if s.starts_with("lib:") => {
                self.layout = TraktorLayout::Browser;
                self.library.selected_nav = crate::library_ui::NavItem::Library(s[4..].parse().unwrap_or(0));
            }
            s if s.starts_with("faderB:") => self.channel_volume[1] = s[7..].parse().unwrap_or(0.85),
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
            "loadA" | "loadB" | "loadC" | "loadD" if first => {
                let deck = step.as_bytes()[4] as usize - b'A' as usize + 1;
                let mut playable = self.library.tracks.iter().filter(|t| !t.missing && t.file_path.as_ref().is_some_and(|p| p.is_file()));
                if let Some(id) = playable.nth(deck - 1).map(|t| t.id) {
                    self.library.load_to_deck(deck, id, [false; 4]);
                    self.drain_pending_loads();
                } else if let Ok(mut state) = self.audio.shared.lock() {
                    let d = deck - 1;
                    let peaks: Arc<[[f32; 3]]> = (0..1200).map(|i| {
                        let phase = (i as f32 / 1200.0) * 8.0;
                        let bass = ((phase * 3.14).sin().abs()).clamp(0.1, 0.9);
                        [bass, 0.5, 0.3]
                    }).collect::<Vec<_>>().into();
                    let overview = crate::overview::from_peaks(&peaks);
                    state.loaded_tracks[d] = Some(crate::audio::LoadedTrackAudio {
                        title: if d == 0 { "Solaris (Extended Mix)".into() } else { "Midnight Sun".into() },
                        artist: if d == 0 { "Articuno".into() } else { "Helios".into() },
                        duration_sec: 240.0,
                        bpm: 128.0,
                        grid_offset_sec: 0.0,
                        key: if d == 0 { "8m".into() } else { "9m".into() },
                        sample_rate: 44100,
                        overview: overview.clone(),
                        waveform_peaks: peaks.clone(),
                        stem_peaks: Vec::new().into(),
                        pcm_stereo: Arc::default(),
                        stems_ready: false,
                        pcm_drums: None,
                        pcm_bass: None,
                        pcm_other: None,
                        pcm_vocals: None,
                        phrases: vec![
                            retraktor_library::Phrase { start_sec: 0.0, kind: retraktor_library::PhraseKind::Intro },
                            retraktor_library::Phrase { start_sec: 30.0, kind: retraktor_library::PhraseKind::Build },
                            retraktor_library::Phrase { start_sec: 60.0, kind: retraktor_library::PhraseKind::Drop },
                            retraktor_library::Phrase { start_sec: 120.0, kind: retraktor_library::PhraseKind::Breakdown },
                            retraktor_library::Phrase { start_sec: 150.0, kind: retraktor_library::PhraseKind::Drop },
                            retraktor_library::Phrase { start_sec: 210.0, kind: retraktor_library::PhraseKind::Outro },
                        ],
                        auto_gain: 1.0,
                        played_peaks: peaks,
                        played_stem_peaks: Vec::new().into(),
                        played_overview: overview,
                    });
                    state.seek_to(d, 75.0);
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
                    st.start_playing(d);
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
            "keylock" => self.deck_keylock[d] ^= true,
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
            "pitch" => self.set_deck_pitch_fader(d, v * 16.0 - 8.0),
            "xfader" => self.crossfader_pos = v,
            "master_volume" => self.master_volume = v,
            "cue_mix" => self.cue_mix = v,
            "cue_vol" => self.cue_volume = v,
            "fx_mix" if deck == 2 => self.fx2_dry_wet = v,
            "fx_mix" => self.fx1_dry_wet = v,
            _ => {}
        }
    }

    fn needs_continuous_repaint(&self) -> bool {
        let engine_busy = self.audio.shared.lock().is_ok_and(|s| {
            (0..4).any(|d| s.is_playing[d] || s.scratch_active[d] || s.scratch_ramping[d])
        });
        let generic_connected = self.preferences.generic_manager.discovered.lock().is_ok_and(|d| d.iter().any(|x| x.connected));
        engine_busy
            || self.preview.is_some()
            || self.deck_loading.iter().chain(&self.deck_analysing).any(|&b| b)
            || self.pattern_enabled
            || self.recording.as_ref().is_some_and(|r| r.is_active())
            || self.mx2_hidraw_file.is_some()
            || generic_connected
            || self.active_cheer.is_some()
            || self.ai_rerank_runner.is_some()
    }

    /// Hide-missing lives in Preferences and in the library toolbar ("Hiding missing ×", "Clear"): whichever changed wins.
    /// Saved with the other preferences on close.
    fn sync_hide_missing(&mut self) {
        if self.preferences.hide_missing_files != self.last_pref_hide_missing {
            self.library.hide_missing_files = self.preferences.hide_missing_files;
        } else if self.library.hide_missing_files != self.last_pref_hide_missing {
            self.preferences.hide_missing_files = self.library.hide_missing_files;
        }
        self.last_pref_hide_missing = self.preferences.hide_missing_files;
    }

    fn locked_decks(&self) -> [bool; 4] {
        let playing = self.audio.shared.lock().map(|s| std::array::from_fn(|d| s.is_playing[d] && s.loaded_tracks[d].is_some())).unwrap_or([false; 4]);
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
        for (deck_idx, track) in std::mem::take(&mut self.library.pending_loads) {
            let d = deck_idx.clamp(1, 4) - 1;
            self.load_generation[d] += 1;
            self.deck_loading[d] = true;
            self.deck_load_failed[d] = None;
            self.library.deck_refused[d] = None;
            let generation = self.load_generation[d];
            let tx = self.load_channel.0.clone();
            let stripes = self.stripes.clone();
            let cue_revision = self.library.cue_revision(track.id);
            let autogain = self.preferences.autogain;
            // A complete, exact preview or overview decode of the same, unchanged file saves decoding it again.
            // shortcut: one still under way is ignored and the file decoded afresh, wait on it if loads straight after selecting matter.
            let reuse = track.file_path.as_ref().and_then(|path| {
                let mut decoded = self.prepared.iter().map(|(_, p)| p).chain(self.preview.as_ref().map(|p| &p.pcm));
                decoded.find(|p| &p.path == path && p.complete()).cloned()
            });
            thread::spawn(move || {
                let Some(path) = track.file_path.clone() else {
                    let _ = tx.send(DeckResult::Loaded(d, generation, track, Err("No file path".into())));
                    return;
                };
                // Before decoding, so a file replaced meanwhile isn't kept under its new version's name.
                let identity = crate::audio::file_identity(&path);
                // Only a decode that is exactly what `from_file` gives, of the file as it is now (see `to_vec`).
                let reused = reuse.as_ref().and_then(|p| Some((p.to_vec()?, p.layout()?.sample_rate, p.tags()?.clone())));
                drop(reuse);
                let decoded = match reused {
                    Some((pcm, sr, (title, artist))) => {
                        let mut loaded = LoadedTrackAudio::from_pcm_with_autogain(pcm, sr, &track.title, &track.artist, track.bpm, &track.musical_key, autogain);
                        loaded.title = title.unwrap_or(loaded.title);
                        loaded.artist = artist.unwrap_or(loaded.artist);
                        Ok(loaded)
                    }
                    None => LoadedTrackAudio::from_file_with_autogain(&path, &track.title, &track.artist, track.bpm, &track.musical_key, autogain),
                };
                let mut loaded = match decoded {
                    Ok(loaded) => loaded,
                    Err(e) => {
                        let _ = tx.send(DeckResult::Loaded(d, generation, track, Err(e)));
                        return;
                    }
                };
                if let Some(grid) = track.cues.iter().find(|c| c.cue_type == retraktor_core::CueType::Grid) {
                    loaded.grid_offset_sec = grid.start_ms / 1000.0;
                }
                let unanalysed = track.bpm <= 0.0 || track.musical_key.is_empty();
                if !track.phrases.is_empty() {
                    loaded.phrases = track.phrases.clone();
                }
                let kept = Kept {
                    bpm: track.bpm > 0.0,
                    grid: track.cues.iter().any(|c| c.cue_type == retraktor_core::CueType::Grid),
                    phrases: !track.phrases.is_empty(),
                    cue_revision,
                };
                // A bounded read: the deck draws the kept overview from the start, as the track's row does.
                let kept_overview = stripes.load(&path, identity).map(|(overview, _)| overview);
                if let Some(overview) = &kept_overview {
                    loaded.overview = Arc::clone(overview);
                }
                // Playable now. Its overview, analysis and cached stems follow, for this load only (`finish_loads` checks).
                let (id, later) = (track.id, loaded.clone());
                let _ = tx.send(DeckResult::Loaded(d, generation, track, Ok(loaded)));
                if kept_overview.is_none() {
                    // The load's own peaks, no second pass: whichever stripe is kept first (this or a row's) is drawn by both.
                    let overview = stripes.store(&path, identity, &later.waveform_peaks, later.duration_sec).unwrap_or_else(|| Arc::clone(&later.overview));
                    let _ = tx.send(DeckResult::Overview(d, generation, Arc::clone(&later.pcm_stereo), overview));
                }
                if unanalysed {
                    let a = retraktor_library::analyze_mono(&crate::library_ui::stereo_to_mono(&later.pcm_stereo), later.sample_rate);
                    let _ = tx.send(DeckResult::Analysis(d, generation, id, a, Arc::clone(&later.pcm_stereo), kept));
                }
                if let Some(dir) = crate::demucs::DemucsManager::get_cached_stem_dir(&path) {
                    let mut stemmed = later;
                    if stemmed.load_stems_from_dir(&dir) {
                        let _ = tx.send(DeckResult::Stems(d, generation, stemmed));
                    }
                }
            });
        }
    }

    /// REC: streams the master to a fresh WAV in `dir`, or says why it could not start.
    fn start_recording(&mut self, dir: &std::path::Path) {
        match crate::recorder::Recording::start(dir, std::time::SystemTime::now()) {
            Ok((rec, tap)) => {
                // Poisoned only if the callback panicked mid-block: there is no engine left to record.
                let stale = match self.audio.recorder.lock() {
                    Ok(mut slot) => slot.replace(tap),
                    Err(_) => {
                        rec.discard();
                        self.library.status_message = "Recording did not start: the audio engine is not available".into();
                        return;
                    }
                };
                drop(stale); // freed after the callback's lock is released
                self.set_state.begin_recording();
                self.library.status_message = format!("Recording to {}", rec.wav.display());
                self.recording = Some(rec);
            }
            Err(e) => self.library.status_message = format!("Recording did not start: {e}"),
        }
    }

    /// Takes the tap out of the audio callback, keeping its last partial chunk, and lets the
    /// writer finish the file. The tap is flushed after the callback's lock is released.
    fn stop_recording(&mut self) {
        let tap = self.audio.recorder.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(rec) = &self.recording {
            rec.stop(tap);
        }
    }

    /// Stops a recording that ended on its own and finishes one whose writer is done.
    /// True once the WAV and its CUE and tracklist are saved with nothing to report.
    fn poll_recording(&mut self) -> bool {
        let Some(rec) = &self.recording else { return false };
        if !rec.is_active() && !rec.is_stopping() {
            self.stop_recording();
        }
        match self.recording.as_mut().and_then(|rec| rec.poll()) {
            Some(outcome) => self.finish_recording(outcome),
            None => false,
        }
    }

    fn finish_recording(&mut self, outcome: Result<crate::recorder::Saved, String>) -> bool {
        let Some(rec) = &self.recording else { return false };
        let saved = match outcome {
            Ok(saved) => saved,
            Err(e) => {
                self.close_after_recording = false;
                self.library.status_message =
                    format!("Could not finish {}: {e}. Click RETRY SAVE; keep the app open.", rec.wav.display());
                return false;
            }
        };
        // A failed write can leave the WAV shorter than the clock when a track was logged:
        // list only tracks that start inside the saved audio.
        let end = saved.seconds();
        let entries: Vec<(f64, &str, &str)> = self
            .set_state
            .recorded_audible_tracks
            .iter()
            .filter(|(s, _, _)| *s < end)
            .map(|(s, a, t)| (*s, a.as_str(), t.as_str()))
            .collect();
        let wav_name = rec.wav.file_name().and_then(|n| n.to_str()).unwrap_or("recording.wav");
        let mut written = vec!["WAV"];
        let mut failed = Vec::new();
        match std::fs::write(&rec.cue, crate::setlist::cue_sheet(&entries, wav_name)) {
            Ok(()) => written.push("CUE"),
            Err(e) => failed.push(format!("CUE not written: {e}")),
        }
        match std::fs::write(&rec.txt, crate::setlist::plain_tracklist(&entries)) {
            Ok(()) => written.push("tracklist"),
            Err(e) => failed.push(format!("tracklist not written: {e}")),
        }
        let dir = rec.wav.parent().unwrap_or(std::path::Path::new(".")).display();
        let clean = saved.reason.is_none() && failed.is_empty();
        let mut msg = match saved.reason {
            None if clean => format!("✔ Saved recording ({end:.1}s), CUE & tracklist to {dir}"),
            None => format!("Saved recording ({end:.1}s): {} to {dir}", written.join(", ")),
            Some(reason) => format!(
                "Recording stopped early: {}{}. Saved {end:.1}s: {} to {dir}",
                reason.describe(),
                saved.error.as_deref().map(|e| format!(" ({e})")).unwrap_or_default(),
                written.join(", "),
            ),
        };
        for failure in &failed {
            msg.push_str(&format!(". {failure}"));
        }
        if !clean && self.close_after_recording {
            self.close_after_recording = false;
            msg.push_str(". Close again to quit");
        }
        self.library.status_message = msg;
        self.recording = None;
        clean
    }

    fn finish_loads(&mut self) {
        while let Ok(result) = self.load_channel.1.try_recv() {
            let (d, generation, track, result) = match result {
                DeckResult::Loaded(d, generation, track, result) => (d, generation, track, result),
                DeckResult::Analysis(d, generation, id, a, audio, kept) => {
                    self.apply_deck_analysis(d, generation, id, &a, &audio, kept);
                    continue;
                }
                DeckResult::Overview(d, generation, audio, overview) => {
                    let Ok(mut state) = self.audio.shared.lock() else { continue };
                    let current = generation == self.deck_loaded_generation[d];
                    if let Some(loaded) = state.loaded_tracks[d].as_mut().filter(|l| current && Arc::ptr_eq(&l.pcm_stereo, &audio)) {
                        loaded.overview = overview.clone();
                        if loaded.auto_gain == 1.0 {
                            loaded.played_overview = overview;
                        }
                    }
                    continue;
                }
                DeckResult::Stems(d, generation, stemmed) => {
                    let Ok(mut state) = self.audio.shared.lock() else { continue };
                    // Only on the load they were decoded for, still holding that audio.
                    let current = generation == self.deck_loaded_generation[d];
                    if let Some(loaded) = state.loaded_tracks[d].as_mut().filter(|l| current && Arc::ptr_eq(&l.pcm_stereo, &stemmed.pcm_stereo)) {
                        loaded.pcm_drums = stemmed.pcm_drums;
                        loaded.pcm_bass = stemmed.pcm_bass;
                        loaded.pcm_other = stemmed.pcm_other;
                        loaded.pcm_vocals = stemmed.pcm_vocals;
                        loaded.stem_peaks = stemmed.stem_peaks.clone();
                        loaded.played_stem_peaks = if loaded.auto_gain == 1.0 { stemmed.stem_peaks } else { stemmed.played_stem_peaks };
                        loaded.stems_ready = true;
                        self.deck_stem_view[d] = true;
                    }
                    continue;
                }
            };
            if generation != self.load_generation[d] { continue; }
            self.deck_loading[d] = false;
            match result {
                Ok(mut loaded) => {
                    self.deck_loaded_generation[d] = generation;
                    // Read on each load, so switching it never jumps the level of a track on air.
                    if !self.preferences.autogain {
                        loaded.auto_gain = 1.0;
                        loaded.played_peaks = loaded.waveform_peaks.clone();
                        loaded.played_stem_peaks = loaded.stem_peaks.clone();
                        loaded.played_overview = loaded.overview.clone();
                    }
                    // The first successful load opens the decks; after that the view is the DJ's choice.
                    let mut switched = false;
                    if !self.first_load_done {
                        self.first_load_done = true;
                        if self.layout == TraktorLayout::Browser {
                            self.layout = TraktorLayout::Parallel;
                            switched = true;
                        }
                    }
                    let stems_ready = loaded.stems_ready;
                    if let Ok(mut state) = self.audio.shared.lock() {
                        state.loaded_tracks[d] = Some(loaded);
                        state.seek_to(d, 0.0);
                        state.is_playing[d] = false;
                        state.reset_jump_ease(d);
                    } else { continue; }
                    self.deck_track_ids[d] = Some(track.id);
                    self.deck_analysing[d] = track.bpm <= 0.0 || track.musical_key.is_empty();
                    self.deck_auto_cue_status[d] = None;
                    // The library entry may have changed while the file decoded; the queued copy is the fallback.
                    self.deck_hotcues[d] = self.deck_library_track(d).unwrap_or(&track).hotcue_slots();
                    self.deck_loop_active[d] = false;
                    // A new track starts with every stem on, at full level, dry.
                    self.deck_stem_muted[d] = [false; 4];
                    self.deck_stem_volumes[d] = [1.0; 4];
                    self.deck_stem_fx_send[d] = [false; 4];
                    self.library.finish_deck_load(d, track.id, stems_ready);
                    self.set_state.on_track_loaded(d, track.id);
                    self.library.status_message = format!("Loaded '{}' on deck {}", track.title, DECK_LETTERS[d]);
                    if switched {
                        self.library.status_message += " · opened the Mix view (F3 returns to the Browser)";
                    }
                }
                Err(e) => {
                    self.deck_load_failed[d] = Some(std::time::Instant::now());
                    self.library.status_message = format!("Could not load '{}' on deck {}: {e}", track.title, DECK_LETTERS[d]);
                }
            }
        }
    }

    /// BPM, grid, key and phrases worked out after deck `d` became playable, for library track `id` that lacked BPM
    /// or key. The library entry takes it whatever the deck holds now. The deck takes it only on the load it was worked
    /// out for, still holding that audio, and only what is still missing: a BPM, grid cue (even at 0) or phrases the
    /// library had stay, as does anything set meanwhile; the playhead and transport don't move. Auto hot cues, for
    /// entry and deck alike, only if no slot is taken and the track's hot cues weren't set or cleared by hand since
    /// the load, on any deck.
    fn apply_deck_analysis(&mut self, d: usize, generation: u64, id: usize, a: &retraktor_library::TrackAnalysis, audio: &Arc<Vec<f32>>, kept: Kept) {
        let on_deck = generation == self.deck_loaded_generation[d]
            && self.audio.shared.lock().is_ok_and(|s| s.loaded_tracks[d].as_ref().is_some_and(|t| Arc::ptr_eq(&t.pcm_stereo, audio)));
        let hot_cues = self.library.cue_revision(id) == kept.cue_revision;
        self.library.apply_late_analysis(id, a, hot_cues);
        if !on_deck {
            return;
        }
        let Ok(mut state) = self.audio.shared.lock() else { return };
        let Some(t) = state.loaded_tracks[d].as_mut() else { return };
        self.deck_analysing[d] = false;
        if !kept.bpm && t.bpm <= 0.0 {
            t.bpm = a.bpm;
            if !kept.grid {
                t.grid_offset_sec = a.beatgrid_offset_sec;
            }
        }
        if t.key.is_empty() {
            t.key = a.key_open.clone();
        }
        if !kept.phrases && !a.phrases.is_empty() {
            t.phrases = a.phrases.clone();
        }
        drop(state);
        if hot_cues {
            self.refresh_deck_hotcue_copies(id);
        }
    }

    /// Opens (or reopens) the sound card whenever its settings change. A routing the open stream has the
    /// channels for (swapping main and headphone pairs) switches at the next block instead, so playback and
    /// recording carry on; so does one needing a width already asked for, so a narrower card isn't reopened
    /// every frame. Rescan tries the chosen card again once, only while it isn't the one playing.
    fn apply_audio_output(&mut self) {
        let p = &self.preferences;
        let (route, settings) = (p.output_route(), (p.audio_device_name.clone(), p.sample_rate, p.buffer_size));
        let stranded = self.audio.output_channels == 0 || (!settings.0.is_empty() && self.audio.on_system_default());
        let retry = std::mem::take(&mut self.preferences.audio_rescanned) && stranded;
        let fits = |asked: u16| self.audio.output_channels >= route.width() || asked == route.width();
        match &self.audio_applied {
            Some((open, asked)) if *open == settings && fits(*asked) && !retry => self.audio.reroute(route),
            _ => {
                self.audio.open(&settings.0, settings.1, settings.2, route);
                self.audio_applied = Some((settings, route.width()));
            }
        }
    }

    /// A Preview press, from the browser toolbar or a controller, for the `selected` track: the track already in
    /// the preview stops, any other playable one replaces it (from its start), and no selection stops whatever
    /// plays. A track that can't be previewed leaves the current preview playing. Decks, history and the set are
    /// never touched.
    fn press_preview(&mut self, selected: Option<usize>) {
        let current = self.preview.as_ref().map(|p| p.track);
        let Some(id) = selected.filter(|&id| current != Some(id)) else {
            match current {
                Some(id) => {
                    self.stop_preview();
                    self.library.status_message = format!("Stopped the preview of '{}'", self.track_title(id));
                }
                None => self.library.status_message = "Select a track to preview it".into(),
            }
            return;
        };
        self.start_preview(id, 0.0);
    }

    /// A click on track `id`'s row waveform at `at` (a fraction of the track): the preview of that track moves
    /// there, otherwise that track's preview starts there. Never touches a deck.
    fn click_overview(&mut self, id: usize, at: f64) {
        let at = if at.is_finite() { at.clamp(0.0, 1.0) } else { 0.0 };
        match self.preview.as_ref().filter(|p| p.track == id) {
            Some(p) => {
                p.pcm.want(at);
                self.preview_loader.nudge();
                if let Ok(mut state) = self.audio.shared.lock() {
                    state.seek_preview(at);
                }
            }
            None => self.start_preview(id, at),
        }
    }

    /// Starts track `id` in the headphones from `at` (a fraction of the track), replacing any preview, once it
    /// is known to be playable; otherwise says why and leaves the current preview alone.
    fn start_preview(&mut self, id: usize, at: f64) {
        let refused = match self.library.track(id) {
            None => Err("That track is no longer in the collection".to_string()),
            Some(t) => match (t.file_path.clone().filter(|p| p.is_file()), self.preview_route_missing()) {
                (None, _) => Err(format!("File not found for '{}' — use Relocate", t.title)),
                (_, Some(why)) => Err(format!("Can't preview '{}': {why}", t.title)),
                (Some(path), None) => Ok((t.title.clone(), path)),
            },
        };
        let (title, path) = match refused {
            Ok(found) => found,
            Err(why) => {
                let still = self.preview.as_ref().map(|p| format!(" · still previewing '{}'", self.track_title(p.track))).unwrap_or_default();
                self.library.status_message = why + &still;
                return;
            }
        };
        self.stop_preview();
        // The selection's overview (or an earlier preview) may have decoded this already.
        let pcm = match self.prepared.iter().find(|(t, p)| *t == id && p.path == path && p.failed().is_none() && p.same_file()) {
            Some((_, p)) => Arc::clone(p),
            None => crate::audio::PreviewPcm::new(path, at),
        };
        pcm.want(at);
        let serial = match self.audio.shared.lock() {
            Ok(mut state) => state.start_preview(Arc::clone(&pcm), at),
            Err(_) => {
                self.library.status_message = format!("Could not preview '{title}': the audio engine is not available");
                return;
            }
        };
        self.remember_prepared(id, &pcm);
        self.preview = Some(Preview { track: id, pcm, serial });
        self.update_preview_jobs();
        self.library.status_message = format!("Previewing '{title}' in the headphones");
    }

    /// Stops the preview, whether it is still decoding or playing; nothing more of it is heard.
    fn stop_preview(&mut self) {
        if self.preview.take().is_none() {
            return;
        }
        if let Ok(mut state) = self.audio.shared.lock() {
            state.stop_preview();
        }
        self.update_preview_jobs();
    }

    /// Keeps `pcm` for track `id` for reuse, alongside at most one other (the preview playing, or the selection);
    /// the oldest that is neither playing nor selected goes first.
    fn remember_prepared(&mut self, id: usize, pcm: &Arc<crate::audio::PreviewPcm>) {
        self.prepared.retain(|(t, p)| *t != id || Arc::ptr_eq(p, pcm));
        if !self.prepared.iter().any(|(_, p)| Arc::ptr_eq(p, pcm)) {
            self.prepared.push((id, Arc::clone(pcm)));
        }
        while self.prepared.len() > 2 {
            let keep = |t: usize| self.preview.as_ref().is_some_and(|p| p.track == t) || t == id || self.library.selected_track_id == Some(t);
            match self.prepared.iter().position(|(t, _)| !keep(*t)) {
                Some(i) => drop(self.prepared.remove(i)),
                None => break,
            }
        }
    }

    /// The preview worker decodes the playing preview first, then the selected track's overview, then a row's waveform.
    fn update_preview_jobs(&self) {
        let selected = self.library.selected_track_id;
        let browse = self.prepared.iter().find(|(t, _)| Some(*t) == selected).map(|(_, p)| p);
        self.preview_loader.set(self.preview.as_ref().map(|p| &p.pcm), browse, self.row_job.as_ref().map(|(_, p)| p));
    }

    /// A finished (or in-progress) decode of track `id` at its current path, for the overview. No file check here
    /// (it runs every frame): `prepare_selection` drops a replaced file's decode once, when the selection rests.
    fn prepared_for(&self, id: usize) -> Option<&Arc<crate::audio::PreviewPcm>> {
        let path = self.library.track(id)?.file_path.as_ref()?;
        self.prepared.iter().find(|(t, p)| *t == id && &p.path == path).map(|(_, p)| p)
    }

    /// Once the selection has rested, decodes it for its overview (only that: browsing never plays anything).
    fn prepare_selection(&mut self) {
        let selected = self.library.selected_track_id;
        if selected != self.browse_seen.0 {
            self.browse_seen = (selected, std::time::Instant::now());
            self.browse_looked = None; // a newly selected track's file is looked at again
            self.update_preview_jobs(); // the old selection's decode stops
            return;
        }
        let Some(id) = selected.filter(|_| self.browse_seen.1.elapsed() >= BROWSE_SETTLE) else { return };
        // The file is looked at once per resting selection (again if its path changes), not every frame.
        let path = self.library.track(id).and_then(|t| t.file_path.clone());
        if self.browse_looked.as_ref().is_some_and(|(t, p)| *t == id && *p == path) {
            return;
        }
        self.browse_looked = Some((id, path.clone()));
        self.prepared.retain(|(t, p)| *t != id || p.same_file()); // a file replaced since is decoded again
        if self.prepared_for(id).is_some() {
            self.update_preview_jobs();
            return;
        }
        let Some(path) = path.filter(|p| p.is_file()) else { return };
        let pcm = crate::audio::PreviewPcm::new(path, 0.0);
        self.remember_prepared(id, &pcm);
        self.update_preview_jobs();
    }

    /// Why the cue mix can't reach headphones on the open output, if it can't.
    fn preview_route_missing(&self) -> Option<String> {
        use crate::audio::OutputRoute;
        let (route, channels) = (self.audio.output_route, self.audio.output_channels as usize);
        let fix = "Choose another pair or an audio interface with a separate headphone pair, or turn on Split cue in Preferences > Audio > Outputs";
        match route {
            _ if route.place(channels).headphones_heard() => None,
            _ if channels == 0 => Some("no audio output is open".into()),
            OutputRoute::Split => Some("Split cue needs a stereo output".into()),
            OutputRoute::Pairs { headphones: Some(h), .. } => {
                Some(format!("this {channels}-channel output has no free channels {} for headphones. {fix}", crate::audio::pair_label(2 * h as usize)))
            }
            OutputRoute::Pairs { headphones: None, .. } => Some(format!("the system default output has no headphone pair. {fix}")),
        }
    }

    fn track_title(&self, id: usize) -> String {
        self.library.track(id).map_or_else(|| "the track".into(), |t| t.title.clone())
    }

    /// Each frame: ends a preview that lost its headphone output, couldn't be read or ran out (so the same track
    /// can be started again), and decodes the resting selection for its overview.
    fn poll_preview(&mut self) {
        if let Some(p) = self.preview.clone() {
            let title = self.track_title(p.track);
            let progress = self.audio.shared.lock().map(|s| s.preview_progress).unwrap_or_default();
            if let Some(why) = self.preview_route_missing() {
                self.stop_preview();
                self.library.status_message = format!("Stopped the preview of '{title}': {why}");
            } else if let Some(e) = p.pcm.failed() {
                self.stop_preview();
                self.library.status_message = format!("Could not preview '{title}': {e}");
            } else if progress.serial == p.serial && progress.ended {
                self.stop_preview();
                self.library.status_message = format!("Preview of '{title}' finished");
            }
        }
        self.prepare_selection();
        self.prepare_rows();
    }

    /// Each frame: every decode under way (the preview, the selection, a row) updates its row's waveform; then
    /// the first row in view still without a finished one gets a peaks-only decode, unless the preview or the
    /// selection is decoding it anyway. That decode goes on while its row stays in view; scrolled away, it stops.
    /// Missing files and finished waveforms (or failures) are never decoded again, so nothing is retried every frame.
    fn prepare_rows(&mut self) {
        let visible = std::mem::take(&mut self.library.visible_rows);
        let selected = self.library.selected_track_id;
        // A stopped preview or an earlier selection is no longer decoded: only what it finished counts.
        let decodes: Vec<(usize, Arc<crate::audio::PreviewPcm>)> = self
            .prepared
            .iter()
            .filter(|(t, p)| Some(*t) == selected || p.complete() || p.failed().is_some())
            .cloned()
            .chain(self.preview.as_ref().map(|p| (p.track, Arc::clone(&p.pcm))))
            .chain(self.row_job.clone())
            .collect();
        for (id, pcm) in &decodes {
            self.note_row_waveform(*id, pcm);
        }
        let decoding_anyway = |app: &Self, id: usize| app.preview.as_ref().is_some_and(|p| p.track == id) || (selected == Some(id) && app.prepared_for(id).is_some());
        let wanted = |app: &Self, id: usize| {
            let t = app.library.track(id).filter(|t| !t.missing)?;
            let path = t.file_path.as_ref()?;
            let finished = app.library.row_waveforms.get(&id).is_some_and(|w| &w.path == path && w.made_from.is_none());
            (!finished && !decoding_anyway(app, id)).then(|| path.clone())
        };
        // Kept only while its row is in view and still wants this very file (not one it has been relinked away from).
        if self.row_job.as_ref().is_some_and(|(id, job)| visible.contains(id) && wanted(self, *id).as_ref() == Some(&job.path)) {
            return;
        }
        let next = visible.iter().find_map(|&id| Some((id, crate::audio::PreviewPcm::peaks_only(wanted(self, id)?))));
        if self.row_job.is_some() || next.is_some() {
            self.row_job = next;
            self.update_preview_jobs();
        }
    }

    /// Track `id`'s row waveform from `pcm`, a decode of its file (or its stripe, found in the cache instead), if
    /// more has been decoded since it was last made. A finished waveform stands until a decode finds the file
    /// changed (replaced or repaired); a finished error also gives way to a decode of the same file that reads fine.
    /// One from a decode of another path (before a relink) is never made. No file is looked at here: decodes carry
    /// what they found.
    fn note_row_waveform(&mut self, id: usize, pcm: &Arc<crate::audio::PreviewPcm>) {
        // A complete decode is finished once the worker has kept its overview (`cached`), so rows draw what decks do.
        let finished = pcm.failed().is_some() || pcm.cached().is_some();
        let made_from = (!finished).then(|| (Arc::as_ptr(pcm) as usize, pcm.filled(), pcm.playable_frames()));
        let identity = pcm.identity();
        let current_path = self.library.track(id).and_then(|t| t.file_path.as_ref()) == Some(&pcm.path);
        let kept = self.library.row_waveforms.get(&id).filter(|w| w.path == pcm.path);
        let keep = kept.is_some_and(|w| {
            w.identity == identity
                && match w.made_from {
                    None => w.peaks.is_ok() || pcm.failed().is_some(),
                    Some(_) => w.made_from == made_from,
                }
        });
        if !current_path || keep || (pcm.filled() == 0 && !finished) {
            return;
        }
        let peaks = match (pcm.failed(), pcm.cached()) {
            (Some(e), _) => Err(e.to_string()),
            (None, Some((overview, length))) => Ok((Arc::clone(overview), Some(*length))),
            (None, None) => Ok((crate::overview::from_peaks(&pcm.overview_peaks().unwrap_or_default()), pcm.duration_sec())),
        };
        let waveform = crate::library_ui::RowWaveform { path: pcm.path.clone(), peaks, made_from, identity };
        if self.library.row_waveforms.insert(id, waveform).is_none() {
            self.row_waveform_order.push_back(id);
            while self.row_waveform_order.len() > ROW_WAVEFORMS_KEPT {
                if let Some(oldest) = self.row_waveform_order.pop_front() {
                    self.library.row_waveforms.remove(&oldest);
                }
            }
        }
    }

    /// Where the preview is, as a fraction of its track, once it has found its place.
    fn preview_position(&self, p: &Preview) -> Option<f64> {
        let progress = self.audio.shared.lock().map(|s| s.preview_progress).ok()?;
        let frames = p.pcm.playable_frames()?.max(1) as f64;
        (progress.serial == p.serial && progress.started).then(|| (progress.frame / frames).clamp(0.0, 1.0))
    }

    /// What the browser shows about the preview: the Stop state, and where the track previewing is.
    fn browser_preview_view(&mut self) {
        self.library.previewing = self.preview.as_ref().map(|p| (p.track, p.pcm.filled() == 0 && p.pcm.failed().is_none()));
        self.library.preview_position = self.preview.as_ref().and_then(|p| self.preview_position(p));
    }

    fn sync_audio_state(&mut self) {
        // Track audio the callback has let go of is freed here, after the lock is released, never in the callback.
        drop(self.audio.shared.lock().map(|mut s| s.take_retired()));
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
            state.fx_enabled = [self.fx1_on, self.fx2_on];
            state.fx_kind = self.fx_kind;
            state.fx_dry_wet = [self.fx1_dry_wet, self.fx2_dry_wet];
            state.fx_params = [self.fx1_param, self.fx2_param];
            state.deck_fx_assign = self.deck_fx_assign;
            state.limiter_enabled = self.preferences.limiter_enabled;

            state.pattern_enabled = self.pattern_enabled;
            state.pattern_steps = self.pattern_steps;
            state.pattern_volume = self.pattern_volume;
            state.pattern_pitch = self.pattern_pitch;
            state.pattern_decay = self.pattern_decay;

            // Only a known BPM is a grid to sync with; a track still being analysed has none yet.
            let grids: [Option<(f64, f64)>; 4] =
                std::array::from_fn(|i| state.loaded_tracks[i].as_ref().filter(|t| t.bpm > 0.0).map(|t| (t.bpm.max(20.0), t.grid_offset_sec)));
            for d in 0..4 {
                state.channel_gain[d] = self.channel_gain[d];
                state.engine.strips[d].volume = self.channel_volume[d];
                state.engine.strips[d].cue_enabled = self.channel_cue[d];
                state.engine.strips[d].filter.knob = self.channel_filter[d];
                state.engine.strips[d].filter.enabled = self.channel_filter_on[d];
                state.engine.strips[d].filter.kind = self.channel_fx_kind[d];
                let knob = |kill: bool, v: f32| if kill { 0.0 } else { v };
                state.engine.strips[d].eq.high = knob(self.eq_kill_hi[d], self.channel_eq_hi[d]);
                state.engine.strips[d].eq.mid = knob(self.eq_kill_mid[d], self.channel_eq_mid[d]);
                state.engine.strips[d].eq.low = knob(self.eq_kill_low[d], self.channel_eq_low[d]);
                state.keylock[d] = self.deck_keylock[d];

                state.engine.strips[d].xfader_left = self.channel_xfader_assign[d] == 0;
                state.engine.strips[d].xfader_right = self.channel_xfader_assign[d] == 2;

                let mut nudge = 0.0;
                // Sync holds the tempo to the other deck (or the master tempo) and eases the beats together.
                let leader = sync_leader(self.tempo_master_deck, d);
                if let (true, Some(other)) = (self.deck_sync_enabled[d], leader) {
                    if let Some((bpm, offset)) = grids[d] {
                        let target = grids[other]
                            .map(|(b, _)| b * (1.0 + self.deck_pitch_fader[other] as f64 / 100.0))
                            .unwrap_or(self.master_tempo);
                        self.deck_pitch_fader[d] = ((sync_ratio(bpm, target) - 1.0) * 100.0) as f32;
                        if let (Some((other_bpm, other_offset)), true, true) = (grids[other], state.is_playing[d], state.is_playing[other]) {
                            // Either deck just started: jump onto the bar like SYNC does; after that only drift is nudged.
                            if !self.sync_was_playing[d] || !self.sync_was_playing[other] {
                                snap_bars(&mut state, d, other);
                            }
                            let behind = beat_phase_diff(
                                (state.playhead_sec[other] - other_offset) * other_bpm / 60.0,
                                (state.playhead_sec[d] - offset) * bpm / 60.0,
                                1.0,
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

            self.sync_was_playing = state.is_playing;
            state.engine.master_volume = self.master_volume;
            state.cue_mix = self.cue_mix;
            state.cue_volume = self.cue_volume;
            state.keylock_hq = self.preferences.keylock_quality == 0;
            // A master deck drives the clock at its live tempo; an ejected deck releases it.
            if let Some(m) = self.tempo_master_deck {
                match grids[m] {
                    Some((bpm, _)) => self.master_tempo = bpm * (1.0 + self.deck_pitch_fader[m] as f64 / 100.0),
                    None if state.loaded_tracks[m].is_some() => {} // still analysing: the clock holds its tempo
                    None => self.tempo_master_deck = None,
                }
            }
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
                self.bus.set(&format!("app.traktor.decks.{idx}.sync.enabled"), PropValue::Bool(self.deck_sync_enabled[d]));
                for (s, &muted) in self.deck_stem_muted[d].iter().enumerate() {
                    self.bus.set(&format!("app.traktor.decks.{idx}.stems.{}.muted", s + 1), PropValue::Bool(muted));
                }
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
                        self.jog_turn(if act == "jog:2" { 1 } else { 0 }, delta);
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
                    self.jog_touch(if act.ends_with(":2") { 1 } else { 0 }, pressed);
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
                    "surface.mixer.cue_mix" => self.cue_mix = v,
                    "surface.mixer.cue_vol" => self.cue_volume = v,
                    "surface.left.pitch.fader" => self.set_deck_pitch_fader(0, v * 16.0 - 8.0),
                    "surface.right.pitch.fader" => self.set_deck_pitch_fader(1, v * 16.0 - 8.0),
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
                // Held controls: platter touch (scratch on / off) and SHIFT, on press and release.
                match control.as_str() {
                    "surface.left.jogwheel.touch" => return self.jog_touch(0, pressed),
                    "surface.right.jogwheel.touch" => return self.jog_touch(1, pressed),
                    "surface.left.shift" | "surface.right.shift" => {
                        self.deck_shift_held[usize::from(control == "surface.right.shift")] = pressed;
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
                    "surface.left.master" => self.toggle_master(0),
                    "surface.right.master" => self.toggle_master(1),
                    "surface.left.browse.encoder.push" | "surface.right.browse.encoder.push" => {
                        if self.show_next_track_panel {
                            self.load_next_pick(self.next_track_selected);
                        } else {
                            self.run_button_action("load", if control.starts_with("surface.left") { 1 } else { 2 }, "");
                        }
                    }
                    "surface.left.key_lock" => self.deck_keylock[0] ^= true,
                    "surface.right.key_lock" => self.deck_keylock[1] ^= true,
                    "surface.left.flux" => self.deck_flux_active[0] = !self.deck_flux_active[0],
                    "surface.right.flux" => self.deck_flux_active[1] = !self.deck_flux_active[1],
                    "surface.left.loop_size.push" => self.toggle_loop(1),
                    "surface.right.loop_size.push" => self.toggle_loop(2),
                    "surface.left.browse.preview" | "surface.right.browse.preview" => self.press_preview(self.library.selected_track_id),

                    // Turntable Vinyl Scratch Mode vs CDJ Jog Bend Mode
                    "surface.left.mode.turntable" => {
                        self.deck_turntable_mode[0] = true;
                        self.bus.set("app.traktor.decks.1.jog.turntable", PropValue::Bool(true));
                    }
                    "surface.left.mode.jogwheel" => {
                        self.deck_turntable_mode[0] = false;
                        self.deck_scratch_active[0] = false;
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.cancel_scratch(0);
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
                            state.cancel_scratch(1);
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
                    "surface.left.fx.buttons.2" => self.cycle_fx_kind(0),
                    "surface.right.fx.buttons.2" => self.cycle_fx_kind(1),
                    // Buttons 3 / 4 send deck A / B into this side's FX unit.
                    "surface.left.fx.buttons.3" => self.deck_fx_assign[0][0] ^= true,
                    "surface.left.fx.buttons.4" => self.deck_fx_assign[1][0] ^= true,
                    "surface.right.fx.buttons.3" => self.deck_fx_assign[0][1] ^= true,
                    "surface.right.fx.buttons.4" => self.deck_fx_assign[1][1] ^= true,
                    "surface.mixer.channels.1.deck_fx_assign.1" => self.deck_fx_assign[0][0] ^= true,
                    "surface.mixer.channels.1.deck_fx_assign.2" => self.deck_fx_assign[0][1] ^= true,
                    "surface.mixer.channels.2.deck_fx_assign.1" => self.deck_fx_assign[1][0] ^= true,
                    "surface.mixer.channels.2.deck_fx_assign.2" => self.deck_fx_assign[1][1] ^= true,
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
                    // FX1–FX4 pick the effects chosen in Preferences > Effects.
                    s if s.starts_with("surface.mixer.channel_fx.fx") => {
                        if let Some(&slot) = s["surface.mixer.channel_fx.fx".len()..].parse::<usize>().ok().and_then(|n| self.preferences.mixer_fx_slots.get(n.wrapping_sub(1))) {
                            let kind = MixerFxKind::from_i64(slot as i64);
                            self.channel_fx_kind[0] = kind;
                            self.channel_fx_kind[1] = kind;
                        }
                    }
                    s if s.starts_with("surface.left.pads.") || s.starts_with("surface.right.pads.") => {
                        let d = if s.starts_with("surface.left") { 0 } else { 1 };
                        if let Ok(pad) = s.rsplit('.').next().unwrap_or_default().parse::<u8>() {
                            // SHIFT + a hotcue pad deletes that cue so it can be set again.
                            if self.deck_shift_held[d] && self.deck_pad_mode[d] == DeckPadMode::Hotcues {
                                self.clear_deck_hotcue(d, pad);
                            } else {
                                self.trigger_performance_pad(d + 1, pad);
                            }
                        }
                    }
                    _ => {}
                }
            }
            SurfaceEvent::Encoder { control, delta } => match control.as_str() {
                "surface.left.jogwheel.rotation" => self.jog_turn(0, delta),
                "surface.right.jogwheel.rotation" => self.jog_turn(1, delta),
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

    /// Platter turned: SHIFT scrubs fast, a touched platter in turntable mode scratches, otherwise it bends
    /// a playing deck's tempo or nudges a paused one.
    fn jog_turn(&mut self, d: usize, delta: i64) {
        let sens = (self.jog_scratch_sensitivity as f64).clamp(0.2, 3.0);
        let shift = self.deck_shift_held[d.min(1)];
        if let Ok(mut state) = self.audio.shared.lock() {
            if shift {
                state.fast_scrub(d, delta, sens);
            } else if state.scratch_active[d] {
                state.scratch_tick(d, (delta as f64 * sens).round() as i64);
            } else {
                state.jog_nudge(d, delta, sens);
            }
        }
    }

    /// Platter top touched / let go: scratch on / off, in turntable mode only.
    fn jog_touch(&mut self, d: usize, pressed: bool) {
        if !self.deck_turntable_mode[d] {
            return;
        }
        self.deck_scratch_active[d] = pressed;
        if let Ok(mut state) = self.audio.shared.lock() {
            if pressed {
                state.enable_scratch(d, retraktor_hid::TRAKTOR_MX2_JOG_TICKS_PER_REV as u32, 33.333333333333336, 0.125, 0.125 / 32.0);
            } else {
                state.disable_scratch(d);
            }
        }
    }

    /// Steps FX unit `u` through every effect.
    fn cycle_fx_kind(&mut self, u: usize) {
        self.fx_kind[u] = MixerFxKind::from_i64((self.fx_kind[u] as i64 + 1) % FX_SHORT.len() as i64);
        self.library.status_message = format!("FX {}: {}", u + 1, crate::preferences::FX_NAMES[self.fx_kind[u] as usize]);
    }

    fn trigger_performance_pad(&mut self, deck_idx: usize, pad_num: u8) {
        let d = deck_idx.clamp(1, 4) - 1;
        let pad_idx = (pad_num.saturating_sub(1) as usize).min(7);
        match self.deck_pad_mode[d] {
            DeckPadMode::Hotcues => {
                self.jump_to_cue(deck_idx, pad_num);
            }
            DeckPadMode::Stems => {
                if pad_idx < 4 {
                    // The engine and the MX2 LEDs follow in sync_audio_state.
                    self.deck_stem_muted[d][pad_idx] ^= true;
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

    /// Every keyboard handler, in order. Overlay state is taken once, before any handler can close an overlay,
    /// so one key press never reaches both an overlay and what lies under it.
    fn dispatch_keys(&mut self, ctx: &egui::Context) {
        let overlay = self.show_shortcuts_hud || self.show_next_track_panel || self.preferences.is_open;
        self.library.keys_blocked = overlay;
        self.handle_keyboard_shortcuts(ctx);
        self.set_keys(ctx, overlay);
    }

    /// Set view keys (arrows, Enter, Delete, O); inert while help, the picker or Preferences is on top.
    fn set_keys(&mut self, ctx: &egui::Context, overlay: bool) {
        if self.layout != TraktorLayout::Set || overlay {
            return;
        }
        let idle = self.idle_deck();
        if let Some((deck, id)) = self.set_state.handle_keys(ctx, &self.library, idle) {
            let locks = self.locked_decks();
            self.library.load_to_deck(deck, id, locks);
            self.drain_pending_loads();
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        let typing = ctx.wants_keyboard_input();
        let overlay = self.show_shortcuts_hud || self.show_next_track_panel || self.preferences.is_open;
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Comma)) {
            self.preferences.is_open = !self.preferences.is_open;
        }
        if !typing && ctx.input(|i| (i.modifiers.shift && i.key_pressed(Key::Slash)) || i.key_pressed(Key::Questionmark)) {
            self.show_shortcuts_hud = !self.show_shortcuts_hud;
        }
        // Escape closes only the topmost layer.
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.show_shortcuts_hud {
                self.show_shortcuts_hud = false;
            } else if self.show_next_track_panel {
                self.show_next_track_panel = false;
            } else if self.preferences.is_open {
                self.preferences.save();
                self.preferences.is_open = false;
            }
        }
        // Settings take the screen: no help sheet or picker floating over them.
        if self.preferences.is_open {
            self.show_next_track_panel = false;
            self.show_shortcuts_hud = false;
        }

        if self.preferences.is_open {
            let num_keys = [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
                Key::Num7,
                Key::Num8,
            ];
            for (idx, &k) in num_keys.iter().enumerate() {
                if ctx.input(|i| (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(k)) {
                    if let Some(group) = PreferencesDialog::GROUPS.get(idx) {
                        if let Some(&first_tab) = group.1.first() {
                            self.preferences.current_tab = first_tab;
                        }
                    }
                }
            }
        }

        if self.show_next_track_panel && !self.show_shortcuts_hud {
            let last = self.next_track_picks.len().saturating_sub(1);
            let (down, up, enter, digit) = ctx.input(|i| {
                let digit = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5].iter().position(|&k| i.key_pressed(k));
                (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Enter), digit)
            });
            if down {
                self.next_track_selected = (self.next_track_selected + 1).min(last);
            }
            if up {
                self.next_track_selected = self.next_track_selected.saturating_sub(1);
            }
            if let Some(pick) = digit.or(enter.then_some(self.next_track_selected)) {
                self.load_next_pick(pick);
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
        // Deck keys only act on the decks: never while typing, in Preferences, or with the help sheet or picker open.
        if !typing && !overlay {
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
                    if i.key_pressed(key) {
                        // Shift deletes the cue, so the pad can be set again.
                        if shift {
                            self.clear_deck_hotcue(d_idx - 1, cue);
                        } else {
                            self.jump_to_cue(d_idx, cue);
                        }
                    }
                }

                // Auto Cues (K for Deck A, Shift+K for Deck B)
                if i.key_pressed(Key::K) {
                    self.apply_auto_cues(ch);
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
                        state.start_playing(0);
                        state.start_playing(1);
                    }
                }

                // Perform: Shift+N loads next unplayed set track into the deck that is not playing
                if shift && i.key_pressed(Key::N) {
                    if let Some((_, _, track)) = self.set_state.next_unplayed_track(&self.library) {
                        let target_deck = self.idle_deck();
                        let locks = self.locked_decks();
                        self.library.load_to_deck(target_deck, track.id, locks);
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
            self.library.queue_stems(&[id]);
            self.deck_stem_view[d] = true;
        } else {
            self.library.status_message = format!("Load a track onto Deck {} first to create stems.", DECK_LETTERS[d]);
        }
    }

    fn toggle_play(&mut self, deck_idx: usize) {
        if let Ok(mut state) = self.audio.shared.lock() {
            let d = deck_idx - 1;
            if state.is_playing[d] {
                state.is_playing[d] = false;
            } else {
                state.start_playing(d);
            }
            if !state.is_playing[d] {
                state.reset_jump_ease(d);
            }
        }
    }

    fn jump_to_cue(&mut self, deck_idx: usize, cue_num: u8) {
        let d = deck_idx - 1;
        if !(1..=8).contains(&cue_num) {
            return;
        }
        if let Some(&(_, cue_sec)) = self.deck_hotcue_slots(d).iter().find(|(id, _)| *id == cue_num) {
            if let Ok(mut state) = self.audio.shared.lock() {
                state.seek_to(d, cue_sec);
            }
            return;
        }
        let Some(current) = self.audio.shared.lock().ok().map(|state| state.playhead_sec[d]) else { return };
        self.add_deck_hotcue(d, cue_num, current);
    }

    /// The library track a deck plays, if it is still in the library; its cues are the deck's cues.
    fn deck_library_track(&self, d: usize) -> Option<&crate::library_ui::UiTrackEntry> {
        let id = self.deck_track_ids[d]?;
        self.library.track(id).filter(|t| t.id == id)
    }

    /// Hotcue pads for a deck, read from the library track so two decks with the same track, or a
    /// load that finished after an edit, agree. Tracks outside the library keep deck-only pads.
    /// Each read also updates the deck's own copy, so the pads it last showed (pad edits, analysis)
    /// stay if the track is removed from the library while loaded.
    fn deck_hotcue_slots(&mut self, d: usize) -> Vec<(u8, f64)> {
        if let Some(slots) = self.deck_library_track(d).map(|t| t.hotcue_slots()) {
            if self.deck_hotcues[d] != slots {
                self.deck_hotcues[d] = slots.clone();
            }
            return slots;
        }
        self.deck_hotcues[d].clone()
    }

    /// Sets an empty pad (1..=8); a filled pad is never overwritten.
    fn add_deck_hotcue(&mut self, d: usize, slot: u8, sec: f64) {
        match self.deck_library_track(d).map(|t| t.id) {
            Some(id) => {
                self.library.add_hotcue(id, slot, sec);
                self.refresh_deck_hotcue_copies(id);
            }
            None => {
                if (1..=8).contains(&slot) && !self.deck_hotcues[d].iter().any(|&(s, _)| s == slot) {
                    self.deck_hotcues[d].push((slot, sec));
                }
            }
        }
    }

    fn clear_deck_hotcue(&mut self, d: usize, slot: u8) {
        match self.deck_library_track(d).map(|t| t.id) {
            Some(id) => {
                self.library.clear_hotcue(id, slot);
                self.refresh_deck_hotcue_copies(id);
            }
            None => self.deck_hotcues[d].retain(|&(s, _)| s != slot),
        }
    }

    /// Updates the deck-only pad copy on every deck playing this track, including decks not on screen.
    fn refresh_deck_hotcue_copies(&mut self, id: usize) {
        for d in 0..4 {
            if self.deck_track_ids[d] == Some(id) {
                self.deck_hotcue_slots(d);
            }
        }
    }

    fn apply_auto_cues(&mut self, d: usize) {
        let Some(deck) = self.deck_view(d) else {
            self.deck_auto_cue_status[d] = Some("No track loaded".to_string());
            return;
        };
        if deck.phrases.is_empty() {
            self.deck_auto_cue_status[d] = Some("No phrases detected".to_string());
            return;
        }
        let existing = self.deck_hotcue_slots(d);
        let proposed = retraktor_library::auto_cues(&deck.phrases, &existing);
        if proposed.is_empty() {
            if existing.len() >= 8 {
                self.deck_auto_cue_status[d] = Some("All 8 hotcue slots filled".to_string());
            } else {
                self.deck_auto_cue_status[d] = Some("No new cue points found".to_string());
            }
            return;
        }
        let mut added_names = Vec::new();
        for (slot, sec, label) in proposed {
            self.add_deck_hotcue(d, slot, sec);
            added_names.push(label.to_lowercase());
        }
        let status = format!("Added {} cues: {}", added_names.len(), added_names.join(", "));
        self.deck_auto_cue_status[d] = Some(status);
    }

    /// SYNC on: this deck jumps to the leader's tempo (and bar, when the leader plays) at once. The leader is the
    /// MASTER deck, or else the other loaded deck (a playing one first), which becomes MASTER so this deck never
    /// pulls it along. sync_audio_state keeps it locked from then on.
    fn sync_deck(&mut self, deck_idx: usize) {
        let d = (deck_idx.saturating_sub(1)).min(3);
        self.deck_sync_enabled[d] = !self.deck_sync_enabled[d];
        if !self.deck_sync_enabled[d] {
            return;
        }
        let Ok(mut state) = self.audio.shared.lock() else { return };
        let bpm = |i: usize| state.loaded_tracks[i].as_ref().map_or(0.0, |t| t.bpm);
        let pitched = |i: usize| bpm(i) * (1.0 + self.deck_pitch_fader[i] as f64 / 100.0);
        let leader = match self.tempo_master_deck {
            Some(m) if m != d && bpm(m) > 0.0 => Some(m),
            _ => {
                let partner = if d == 0 { 1 } else { 0 };
                let other = (0..4)
                    .find(|&i| i != d && state.is_playing[i] && bpm(i) > 0.0)
                    .or((bpm(partner) > 0.0).then_some(partner));
                if let Some(o) = other {
                    self.tempo_master_deck = Some(o);
                    self.master_tempo = pitched(o);
                }
                other
            }
        };
        let target = leader.map_or(self.master_tempo, pitched);
        if bpm(d) > 0.0 {
            self.deck_pitch_fader[d] = ((sync_ratio(bpm(d), target) - 1.0) * 100.0) as f32;
        }
        if let Some(o) = leader.filter(|&o| state.is_playing[o]) {
            snap_bars(&mut state, d, o);
        }
    }

    /// MASTER: this deck's live tempo becomes the clock; pressed again, the clock is free.
    fn toggle_master(&mut self, d: usize) {
        if self.tempo_master_deck == Some(d) {
            self.tempo_master_deck = None;
            return;
        }
        self.tempo_master_deck = Some(d);
        let bpm = self.audio.shared.lock().ok().and_then(|s| s.loaded_tracks[d].as_ref().map(|t| t.bpm)).unwrap_or(0.0);
        if bpm > 0.0 {
            self.master_tempo = bpm * (1.0 + self.deck_pitch_fader[d] as f64 / 100.0);
        }
    }

    /// A pitch fader moved (on screen or on the controller). On a deck in a sync group (synced, or the deck others
    /// follow) the whole group takes the new tempo, so sync never pulls the fader back.
    fn set_deck_pitch_fader(&mut self, d: usize, pitch: f32) {
        self.deck_pitch_fader[d] = pitch;
        let leads = (0..4).any(|o| o != d && self.deck_sync_enabled[o] && sync_leader(self.tempo_master_deck, o) == Some(d));
        if !(self.deck_sync_enabled[d] || leads) {
            return;
        }
        let Ok(state) = self.audio.shared.lock() else { return };
        let bpm = |i: usize| state.loaded_tracks[i].as_ref().map_or(0.0, |t| t.bpm);
        if bpm(d) <= 0.0 {
            return;
        }
        let target = bpm(d) * (1.0 + pitch as f64 / 100.0);
        self.master_tempo = target;
        for o in (0..4).filter(|&o| o != d && (self.deck_sync_enabled[o] || self.tempo_master_deck == Some(o)) && bpm(o) > 0.0) {
            self.deck_pitch_fader[o] = ((sync_ratio(bpm(o), target) - 1.0) * 100.0) as f32;
        }
    }

    /// The pitch fader as drawn on a deck; a move goes through `set_deck_pitch_fader`.
    fn deck_pitch_fader_ui(&mut self, ui: &mut Ui, d: usize) {
        let mut pitch = self.deck_pitch_fader[d];
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| pitch_fader(ui, &mut pitch));
        if pitch != self.deck_pitch_fader[d] {
            self.set_deck_pitch_fader(d, pitch);
        }
    }

    /// Back to the track's own tempo. Sync has to let go too, or it would pull the pitch straight back.
    fn reset_tempo(&mut self, d: usize) {
        if self.deck_sync_enabled[d] {
            self.sync_deck(d + 1);
        }
        self.deck_pitch_fader[d] = 0.0;
    }

    /// Shifts a loaded track's beatgrid and/or scales its tempo, on the deck and in the library entry.
    fn adjust_deck_grid(&mut self, d: usize, shift_sec: f64, bpm_factor: f64) {
        let Ok(mut state) = self.audio.shared.lock() else { return };
        // No grid to move or scale until the BPM is known.
        let Some(t) = state.loaded_tracks[d].as_mut().filter(|t| t.bpm > 0.0) else { return };
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
        // A loop is a number of beats: with no BPM yet it would be a guess, and change length when it arrives.
        let no_bpm = self.audio.shared.lock().map_or(true, |s| s.loaded_tracks[d].as_ref().is_some_and(|t| t.bpm <= 0.0));
        if !self.deck_loop_active[d] && no_bpm {
            self.library.status_message = format!("Loops on deck {} wait for its BPM", DECK_LETTERS[d]);
            return;
        }
        self.deck_loop_active[d] = !self.deck_loop_active[d];
        if self.deck_loop_active[d] {
            if let Ok(state) = self.audio.shared.lock() {
                self.deck_loop_start_sec[d] = state.playhead_sec[d];
            }
        }
    }

    /// Where deck `d`'s beat jump lands from `playhead` (`dir` -1 back, 1 forward), plus the loop length
    /// the audio uses for this deck (as in sync_audio_state). The one check behind both the arrows and the jump.
    fn plan_beat_jump(&self, d: usize, dir: f64, playhead: f64, bpm: f64, duration: f64) -> Option<(f64, Option<f64>, f64)> {
        let loop_len = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
        let active_loop = (self.deck_loop_active[d] && loop_len > 0.05).then_some((self.deck_loop_start_sec[d], loop_len));
        let (to, loop_start) = beat_jump_target(playhead, bpm, duration, dir * self.deck_jump_beats[d] as f64, active_loop)?;
        Some((to, loop_start, loop_len))
    }

    /// Beat jump on deck `d` from the live playhead. False, with the playhead and loop untouched, when it would leave the track.
    fn beat_jump(&mut self, d: usize, dir: f64) -> bool {
        let Ok(mut state) = self.audio.shared.lock() else { return false };
        let Some(t) = state.loaded_tracks[d].as_ref() else { return false };
        let Some((to, loop_start, loop_len)) = self.plan_beat_jump(d, dir, state.playhead_sec[d], t.bpm, t.duration_sec) else { return false };
        if let Some(start) = loop_start {
            self.deck_loop_start_sec[d] = start;
        }
        // Playhead and loop change in one lock, so the next callback never wraps into the old loop.
        let from = state.playhead_sec[d];
        state.seek_by(d, to - from);
        if state.is_playing[d] {
            state.ease_jump(d);
        } else {
            state.reset_jump_ease(d);
        }
        state.loop_active[d] = self.deck_loop_active[d];
        state.loop_start_sec[d] = self.deck_loop_start_sec[d];
        state.loop_len_sec[d] = loop_len;
        true
    }

    /// JUMP ◀ [size] ▶ for one deck. The arrows grey out (and say why) from this frame's snapshot;
    /// a click checks again against the live playhead.
    fn beat_jump_controls(&mut self, ui: &mut Ui, d: usize, deck: &DeckView) {
        let beats = self.deck_jump_beats[d];
        let timed = beat_jump_target(0.0, deck.bpm, deck.duration_sec, 0.0, None).is_some(); // usable BPM and length
        ui.label(caps("JUMP"));
        let mut jump = None;
        for (dir, arrow, word) in [(-1.0, "◀", "back"), (1.0, "▶", "forward")] {
            if dir > 0.0 {
                let combo = egui::ComboBox::from_id_salt(("beat_jump_size", d))
                    .selected_text(format!("{beats} beats"))
                    .width(66.0)
                    .show_ui(ui, |ui| {
                        for n in JUMP_SIZES {
                            ui.selectable_value(&mut self.deck_jump_beats[d], n, format!("{n} beats"));
                        }
                    });
                let combo = combo.response.on_hover_text("Beat jump size, the distance each arrow moves the playhead and any active loop");
                combo.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, format!("Deck {} beat jump size {beats} beats", DECK_LETTERS[d])));
            }
            let enabled = self.plan_beat_jump(d, dir, deck.elapsed_sec, deck.bpm, deck.duration_sec).is_some();
            let why = if timed {
                format!("Can't jump {word} {beats} beats here. The playhead and any active loop must stay inside the track.")
            } else {
                "Beat jump needs the track's tempo and length".to_string()
            };
            let resp = ui
                .add_enabled_ui(enabled, |ui| pill(ui, arrow, false, Color32::TRANSPARENT, HIT))
                .inner
                .on_hover_text(format!("Jump {word} {beats} beats, keeping the beat (moves an active loop too)"))
                .on_disabled_hover_text(why);
            let label = format!("Deck {} beat jump {word} {beats} beats", DECK_LETTERS[d]);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));
            if resp.clicked() {
                jump = Some(dir);
            }
        }
        if let Some(dir) = jump {
            if !self.beat_jump(d, dir) {
                self.library.status_message = format!("Jump would leave the track on deck {}", DECK_LETTERS[d]);
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
                        self.tempo_master_deck = None;
                        self.master_tempo = (self.master_tempo - 0.5).max(40.0);
                    }
                    if pill(ui, "+", false, Color32::TRANSPARENT, HIT)
                        .on_hover_text("Master tempo up 0.5 BPM")
                        .clicked()
                    {
                        self.tempo_master_deck = None;
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
                    let phones_txt = format!("{}", (self.cue_volume * 100.0).round() as i32);
                    dot::dot_ring(ui, &mut self.cue_volume, 0.0, 1.0, 0.8, &phones_txt, "PHONES", TraktorTheme::TEXT_PRIMARY(), true)
                        .on_hover_text(format!("Headphone level ({phones_txt}%). {KNOB_TIP}"));
                    let mix_txt = match self.cue_mix {
                        m if m < 0.05 => "CUE".to_string(),
                        m if m > 0.95 => "MAIN".to_string(),
                        m => format!("{}", (m * 100.0).round() as i32),
                    };
                    dot::dot_ring(ui, &mut self.cue_mix, 0.0, 1.0, 0.0, &mix_txt, "CUE/MIX", TraktorTheme::TEXT_PRIMARY(), true)
                        .on_hover_text(format!("Headphones hear the cued decks (left) or the main mix (right). {KNOB_TIP}"));

                    let rec_state = self.recording.as_ref().map(|r| (r.is_active(), r.is_unsaved(), r.seconds()));
                    let is_rec = matches!(rec_state, Some((true, _, _)));
                    if show_rec || rec_state.is_some() {
                        let rec_label = match rec_state {
                            Some((true, _, sec)) => format!("REC {:02}:{:02}", (sec / 60.0).floor() as u32, (sec % 60.0).floor() as u32),
                            Some((_, true, _)) => "RETRY SAVE".to_string(),
                            Some(_) => "SAVING…".to_string(),
                            None => "REC".to_string(),
                        };
                        let (rec_fill, rec_fg) = if is_rec {
                            (TraktorTheme::ACCENT(), on(TraktorTheme::ACCENT()))
                        } else {
                            (TraktorTheme::CONTROL(), TraktorTheme::TEXT_SECONDARY())
                        };
                        if icon_pill(ui, Icon::Dot, &rec_label, rec_fill, rec_fg, Vec2::new(0.0, HIT))
                            .on_hover_text(if is_rec {
                                format!("Stop and save recording, CUE & tracklist to {}", self.preferences.recording_dir.display())
                            } else {
                                format!("Record master mix to {} (WAV, CUE & TXT)", self.preferences.recording_dir.display())
                            })
                            .clicked()
                        {
                            match rec_state {
                                None => {
                                    let dir = recording_dir(&self.preferences.recording_dir);
                                    self.start_recording(&dir);
                                }
                                Some((true, _, _)) => self.stop_recording(),
                                Some((_, true, _)) => {
                                    if let Some(outcome) = self.recording.as_mut().map(|r| r.retry()) {
                                        self.finish_recording(outcome);
                                    }
                                }
                                Some(_) => {} // the writer is still finishing the file
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
        // Stacked, so the two pickers take one pill's width: mixer effect (the FILTER knob) over the FX unit.
        let (rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 2.0 * HIT + 2.0), Sense::hover());
        let mut stack = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Center)));
        stack.spacing_mut().item_spacing.y = 2.0;
        fx_pick(&mut stack, &mut self.channel_fx_kind[d], &format!("Deck {letter} mixer effect (the FILTER knob)"));
        fx_pick(&mut stack, &mut self.fx_kind[d], &format!("FX unit {}", d + 1));
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

                // All four voices at once as a 4 x 16 dot grid; the playing step sweeps across as a bright column.
                let v = self.pattern_selected_voice.min(3);
                // Each row is named on its left; clicking a name picks the voice the 4X4 / ROLL / CLEAR buttons edit.
                const NAMES: [&str; 4] = ["KICK", "CLAP", "HAT", "PERC"];
                let label_w = 40.0;
                let grid_w = (ui.available_width() - 250.0).clamp(16.0 * 14.0, 16.0 * 34.0);
                let (area, resp) = ui.allocate_exact_size(Vec2::new(label_w + grid_w, 4.0 * 18.0 + 11.0), Sense::click());
                // A row of beat numbers on top, then the four voice rows.
                let grid = egui::Rect::from_min_max(area.min + Vec2::new(label_w, 11.0), area.max);
                let cell = Vec2::new(grid_w / 16.0, 18.0);
                if let (true, Some(pos)) = (resp.clicked(), resp.interact_pointer_pos().filter(|p| p.y >= grid.top())) {
                    let voice = (((pos.y - grid.top()) / cell.y) as usize).min(3);
                    if pos.x >= grid.left() {
                        let step = (((pos.x - grid.left()) / cell.x) as usize).min(15);
                        self.pattern_steps[voice][step] ^= true;
                    }
                    self.pattern_selected_voice = voice;
                }
                for (step, n) in [(0, "1"), (4, "5"), (8, "9"), (12, "13")] {
                    ui.painter().text(egui::pos2(grid.left() + (step as f32 + 0.5) * cell.x, area.top()), egui::Align2::CENTER_TOP, n, egui::FontId::proportional(9.0), TraktorTheme::TEXT_SECONDARY());
                }
                for (voice, name) in NAMES.iter().enumerate() {
                    let selected = voice == self.pattern_selected_voice;
                    if selected {
                        let y = grid.top() + (voice as f32 + 0.5) * cell.y;
                        ui.painter().circle_filled(egui::pos2(area.left() - 5.0, y), 2.5, TraktorTheme::ACCENT());
                    }
                    ui.painter().text(
                        egui::pos2(area.left(), grid.top() + (voice as f32 + 0.5) * cell.y),
                        egui::Align2::LEFT_CENTER,
                        *name,
                        egui::FontId::proportional(10.5),
                        if selected { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_SECONDARY() },
                    );
                }
                // Faint lines between the four beats.
                for beat in 1..4 {
                    let x = grid.left() + beat as f32 * 4.0 * cell.x;
                    ui.painter().line_segment([egui::pos2(x, grid.top()), egui::pos2(x, grid.bottom())], Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));
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
                resp.on_hover_text("Click a dot to switch that step on or off; click a name to pick the voice the buttons on the right edit");
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
                    if icon_pill(ui, Icon::Play, "SYNC + PLAY BOTH", fill, fg, Vec2::new(0.0, HIT))
                        .on_hover_text("Sync deck B to deck A and play both through the main output (A)")
                        .clicked()
                    {
                        self.sync_deck(2);
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.start_playing(0);
                            state.start_playing(1);
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
        let locks = self.locked_decks();
        self.set_state.show_next_up_strip(ui, &mut self.library, idle, locks);

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
                    let t = state.loaded_tracks[i].as_ref().filter(|t| state.is_playing[i] && t.bpm > 0.0)?;
                    Some((state.playhead_sec[i] - t.grid_offset_sec) * t.bpm.max(20.0) / 60.0)
                };
                Some(beat_phase_diff(beats(other)?, beats(d)?, 1.0))
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
                let display_w = 340.0;
                let avail = (ui.available_width() - display_w - 12.0).max(60.0);
                ui.allocate_ui_with_layout(Vec2::new(avail, 24.0), Layout::left_to_right(Align::Center), |ui| {
                    title_label(ui, &deck.title, &deck.artist);
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let panel_rect = display(ui, |ui, ink| {
                        ui.set_width(display_w);
                        ui.vertical(|ui| {
                            beat_strip(ui, d, &deck, phase, accent_color, ink, display_w - 16.0, 11.0);
                            // Readout numbers in dot font: BPM, KEY, REMAIN with fixed columns so A and B headers align
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let size = 18.0;
                                let pitch_pct = self.deck_pitch_fader[d];
                                let bpm_txt = bpm_text(deck.bpm, pitch_pct, self.deck_analysing[d]);
                                ui.allocate_ui_with_layout(Vec2::new(95.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                    if bpm_reset_clicked(readout(ui, "BPM", &bpm_txt, size, ink)) {
                                        self.reset_tempo(d);
                                    }
                                });
                                ui.allocate_ui_with_layout(Vec2::new(85.0, 24.0), Layout::left_to_right(Align::Center), |ui| {
                                    readout(ui, if in_key { "IN KEY" } else { "KEY" }, key_text(&deck.key, self.deck_analysing[d]), size, ink);
                                });
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    readout(ui, "REMAIN", &deck.remaining(), size, ink);
                                });
                            });
                        });
                    });
                    dot::paint_dot_wipe(ui.painter(), panel_rect, (1.0 - loaded_t).max(looping), accent_color);
                });
            });
        }

        let cues = self.deck_hotcue_slots(d);
        let loop_len = self.deck_loop_size_beats[d] as f64 * (60.0 / deck.bpm.max(30.0));
        let wave_resp = if self.deck_stem_view[d] {
            scrolling_stem_waveforms(
                ui,
                deck.elapsed_sec,
                1.0 + self.deck_pitch_fader[d] as f64 / 100.0,
                deck.bpm,
                deck.grid_offset_sec,
                &cues,
                &deck.phrases,
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
                1.0 + self.deck_pitch_fader[d] as f64 / 100.0,
                deck.bpm,
                deck.grid_offset_sec,
                &cues,
                &deck.phrases,
                &deck.peaks,
                &deck.played_peaks,
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
                let from = state.playhead_sec[d];
                state.seek_by(d, (from + dt).clamp(0.0, duration_sec) - from);
            }
        }
    }

    /// 4-Deck layout: decks A + C | four-channel mixer | decks B + D, each deck with its own waveform.
    fn show_four_deck_layout(&mut self, ui: &mut Ui) {
        // Scrolls when C and D are taller than the window (1024 x 640), so every deck control stays reachable.
        egui::ScrollArea::vertical().id_salt("four_deck").auto_shrink([false, true]).drag_to_scroll(false).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let mixer_w = (ui.available_width() * 0.36).clamp(460.0, 560.0);
                let deck_w = ((ui.available_width() - mixer_w - 8.0) / 2.0).max(260.0);
                // Shorter waveforms on short windows keep the browser on screen.
                let wave_h = (36.0 + (ui.ctx().screen_rect().height() - 640.0) * 0.1).clamp(36.0, 80.0);

                // A and B differ in height when one shows tools or stems; pad the shorter one so C and D line up.
                let heights_id = ui.id().with("four_deck_top_heights");
                let prev: [f32; 2] = ui.ctx().data(|d| d.get_temp(heights_id)).unwrap_or([0.0; 2]);
                let row_h = prev[0].max(prev[1]);
                let mut now = [0.0f32; 2];
                ui.allocate_ui_with_layout(Vec2::new(deck_w, 340.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(deck_w);
                    now[0] = self.show_single_deck(ui, 1, TraktorTheme::DECK_A(), true, wave_h, 0.0);
                    ui.add_space((row_h - now[0]).max(0.0));
                    self.show_single_deck(ui, 3, TraktorTheme::DECK_C(), true, wave_h, 0.0);
                });
                ui.allocate_ui_with_layout(Vec2::new(mixer_w, 340.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(mixer_w);
                    self.show_mixer_section(ui, false);
                });
                ui.allocate_ui_with_layout(Vec2::new(deck_w, 340.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(deck_w);
                    now[1] = self.show_single_deck(ui, 2, TraktorTheme::DECK_B(), true, wave_h, 0.0);
                    ui.add_space((row_h - now[1]).max(0.0));
                    self.show_single_deck(ui, 4, TraktorTheme::DECK_D(), true, wave_h, 0.0);
                });
                if now != prev {
                    ui.ctx().data_mut(|d| d.insert_temp(heights_id, now));
                    ui.ctx().request_repaint();
                }
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
            let loading = self.deck_loading[d] || self.library.pending_loads.iter().any(|(deck, _)| *deck == deck_idx);
            deck_badge(ui, letter, accent);
            self.deck_notice(ui, d);
            if !loading {
                ui.add(egui::Label::new(RichText::new("Drop a track here, or double-click one in the library").size(12.0).color(TraktorTheme::TEXT_SECONDARY())).truncate().selectable(false));
            }
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
        let cue_key = ["(C)", "(Shift+C)", "", ""][d.min(3)];
        if pill(ui, "CUE", false, TraktorTheme::CUE_ACTIVE(), 40.0).on_hover_text(format!("Jump to cue 1 on deck {letter} {cue_key}")).clicked() {
            self.jump_to_cue(deck_idx, 1);
        }
        self.deck_notice(ui, d);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            display(ui, |ui, ink| {
                ui.spacing_mut().item_spacing.x = 14.0;
                if bpm_reset_clicked(readout(ui, "BPM", &bpm_text(deck.bpm, self.deck_pitch_fader[d], self.deck_analysing[d]), 20.0, ink)) {
                    self.reset_tempo(d);
                }
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
    ) -> f32 {
        let rect = ui
            .scope(|ui| self.show_single_deck_inner(ui, deck_idx, accent_color, include_scrolling_waveform, waveform_height, min_height))
            .response
            .rect;
        self.deck_drop(ui, rect, deck_idx);
        rect.height()
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
            // Stem lanes have fixed colours, so they only need the played level.
            stem_peaks: t.played_stem_peaks.clone(),
            played_peaks: t.played_peaks.clone(),
            overview: t.overview.clone(),
            played_overview: t.played_overview.clone(),
            phrases: t.phrases.clone(),
        })
    }

    /// STEMS view toggle, plus the one-click isolators while the view is on.
    fn stem_buttons(&mut self, ui: &mut Ui, d: usize) {
        let view_on = self.deck_stem_view[d];
        if pill(ui, "STEMS", view_on, TraktorTheme::ROW_SELECTED(), 0.0)
            .on_hover_text("Stem mode: four lanes (drums, bass, other, vocals) on the deck, and the mixer's EQ becomes four stem controls")
            .clicked()
        {
            self.deck_stem_view[d] = !view_on;
        }
        if !view_on {
            return;
        }
        // Say which separation is playing: clean Demucs stems, or the instant frequency split.
        let saved = self.audio.shared.lock().is_ok_and(|st| st.loaded_tracks[d].as_ref().is_some_and(|t| t.stems_ready));
        let (src, src_tip) = if saved {
            ("SAVED STEMS", "Stems separated by Demucs and saved with the track; quality depends on the track")
        } else {
            ("LIVE SPLIT · BANDS", "Instant frequency split: ACAPELLA and VOCALS keep the vocal frequencies, not an isolated voice, so expect bleed. Use CREATE STEMS under TOOLS for separated stems")
        };
        ui.label(RichText::new(src).font(egui::FontId::proportional(10.5)).color(if saved { TraktorTheme::PLAY_ACTIVE() } else { TraktorTheme::METER_YELLOW() }))
            .on_hover_text(src_tip);
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
                // LOADING / LOAD FAILED / LOCKED sit beside the letter, as on a loaded deck.
                deck_badge(ui, DECK_LETTERS[(deck_idx - 1).min(3)], accent);
                self.deck_notice(ui, deck_idx - 1);
                if !(self.deck_loading[deck_idx - 1] || self.library.pending_loads.iter().any(|(deck, _)| *deck == deck_idx)) {
                    ui.add(egui::Label::new(RichText::new("Drop a track here, or double-click one in the library").size(12.0).color(TraktorTheme::TEXT_SECONDARY())).truncate().selectable(false));
                }
            });
        });
    }

    /// LOADING or LOCKED beside the deck letter, so the answer to a load shows where the DJ is looking.
    fn deck_notice(&self, ui: &mut Ui, d: usize) {
        let loading = self.deck_loading[d] || self.library.pending_loads.iter().any(|(deck, _)| *deck == d + 1);
        let refused = self.library.deck_refused[d].is_some_and(|t| t.elapsed().as_secs_f32() < 3.0);
        let failed = self.deck_load_failed[d].is_some_and(|t| t.elapsed().as_secs_f32() < 5.0);
        let (text, colour, tip) = if loading {
            ("LOADING", TraktorTheme::TEXT_PRIMARY(), "A new track is loading on this deck; the current one keeps playing until it is ready")
        } else if failed {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            ("LOAD FAILED", TraktorTheme::METER_RED(), "The file could not be decoded; see the status line under the library")
        } else if refused {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
            ("LOCKED", TraktorTheme::METER_RED(), "This deck is playing, so the load was refused (Preferences → Decks: lock playing decks)")
        } else if self.locked_decks()[d] {
            ("LOCK", TraktorTheme::TEXT_SECONDARY(), "Playing and protected: new tracks won't load here until it stops (Preferences → Decks)")
        } else {
            return;
        };
        ui.label(RichText::new(text).font(egui::FontId::proportional(10.5)).strong().color(colour)).on_hover_text(tip);
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
                self.deck_notice(ui, d);
                title_label(ui, &deck.title, &deck.artist);
            });

            // 2. Display: the numbers read at arm's length, as large as the deck's width allows.
            // The other deck's beat position, for the phase meter (only while both play).
            let phase = self.audio.shared.lock().ok().and_then(|state| {
                let other = if d == 0 { 1 } else { 0 };
                let beats = |i: usize| {
                    let t = state.loaded_tracks[i].as_ref().filter(|t| state.is_playing[i] && t.bpm > 0.0)?;
                    Some((state.playhead_sec[i] - t.grid_offset_sec) * t.bpm.max(20.0) / 60.0)
                };
                Some(beat_phase_diff(beats(other)?, beats(d)?, 1.0))
            });
            let looping = crate::theme::fade(ui.ctx(), ("deck_loop", d), self.deck_loop_active[d]);
            let panel_rect = display(ui, |ui, ink| {
                ui.vertical(|ui| {
                    beat_strip(ui, d, &deck, phase, accent_color, ink, ui.available_width(), 12.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 14.0;
                        let advance = ui.fonts(|f| f.glyph_width(&crate::theme::dot_font(100.0), '0')) / 100.0;
                        let size = ((ui.available_width() - 3.0 * 14.0) / (20.0 * advance)).floor().clamp(16.0, 30.0);
                        let pitch_pct = self.deck_pitch_fader[d];
                        if bpm_reset_clicked(readout(ui, "BPM", &bpm_text(bpm, pitch_pct, self.deck_analysing[d]), size, ink)) {
                            self.reset_tempo(d);
                        }
                        readout(ui, "PITCH", &format!("{pitch_pct:+.1}%"), size, ink);
                        // A compatible key says so in words, not just in brightness.
                        readout(ui, if in_key { "IN KEY" } else { "KEY" }, key_text(&deck.key, self.deck_analysing[d]), size, ink);
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
                if pill(ui, "KEY", self.deck_keylock[d], TraktorTheme::KEYLOCK_ACTIVE(), 0.0)
                    .on_hover_text("Key lock: change tempo without changing the musical key")
                    .clicked()
                {
                    self.deck_keylock[d] ^= true;
                }
                if pill(ui, "MASTER", self.tempo_master_deck == Some(d), TraktorTheme::ROW_SELECTED(), 0.0)
                    .on_hover_text("Make this deck's tempo the master clock; click again to release")
                    .clicked()
                {
                    self.toggle_master(d);
                }
                self.stem_buttons(ui, d);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let expanded = self.deck_expanded[d];
                    if pill(ui, "TOOLS", expanded, TraktorTheme::ROW_SELECTED(), 0.0)
                        .on_hover_text("Track tools: create stems, fix the beatgrid or tempo, auto cues")
                        .clicked()
                    {
                        self.deck_expanded[d] = !expanded;
                    }
                });
            });

            // 4. Scrolling waveform (4-Deck; Mix draws them full width above).
            let cues = self.deck_hotcue_slots(d);
            let loop_len_sec = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
            if include_scrolling_waveform && waveform_height > 10.0 {
                let wave_resp = if self.deck_stem_view[d] {
                    scrolling_stem_waveforms(
                        ui,
                        deck.elapsed_sec,
                        1.0 + self.deck_pitch_fader[d] as f64 / 100.0,
                        bpm,
                        deck.grid_offset_sec,
                        &cues,
                        &deck.phrases,
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
                        1.0 + self.deck_pitch_fader[d] as f64 / 100.0,
                        bpm,
                        deck.grid_offset_sec,
                        &cues,
                        &deck.phrases,
                        &deck.peaks,
                        &deck.played_peaks,
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
                    let gap = ui.spacing().item_spacing.x;
                    let w = (ui.available_width() - 10.0 * gap) / 4.0;
                    for (s, name) in ["DRUMS", "BASS", "OTHER", "VOCALS"].into_iter().enumerate() {
                        if s > 0 {
                            ui.add_space(gap);
                        }
                        let muted = self.deck_stem_muted[d][s];
                        // The word carries the state too, not just the fill.
                        let label = if muted { format!("{name} OFF") } else { name.to_string() };
                        if pill(ui, &label, !muted, accent_color, w * 0.72).on_hover_text("Click to mute or unmute this stem").clicked() {
                            self.deck_stem_muted[d][s] = !muted;
                        }
                        let fx = self.deck_stem_fx_send[d][s];
                        if pill(ui, "FX", fx, TraktorTheme::ACCENT(), w * 0.28).on_hover_text(format!("Send {} to the deck FX", name.to_lowercase())).clicked() {
                            self.deck_stem_fx_send[d][s] = !fx;
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
                &deck.overview,
                &deck.played_overview,
                &cues,
                &deck.phrases,
                self.deck_loop_active[d],
                self.deck_loop_start_sec[d],
                loop_len_sec,
                accent_color,
            );
            if stripe_resp.changed() {
                if let Ok(mut state) = self.audio.shared.lock() {
                    state.seek_to(d, scrub_pos);
                }
            }

            // 7. Transport and loop; the pitch fader shares the row when there is room for it.
            let mut pitch_inline = false;
            row(ui, 30.0, |ui| {
                if let Some(action) = deck_transport_bar(
                    ui,
                    deck.is_playing,
                    self.deck_flux_active[d],
                    self.deck_loop_active[d],
                    self.deck_loop_size_beats[d],
                    !self.deck_expanded[d],
                    [Some(""), Some("Shift+"), None, None][d],
                ) {
                    match action {
                        TransportAction::TogglePlay => self.toggle_play(deck_idx),
                        TransportAction::Cue => self.jump_to_cue(deck_idx, 1),
                        TransportAction::Cup => {
                            self.jump_to_cue(deck_idx, 1);
                            if let Ok(mut state) = self.audio.shared.lock() {
                                state.start_playing(d);
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
                    self.deck_pitch_fader_ui(ui, d);
                }
            });
            // 7b. Beat jump, with the pitch fader beside it when the transport row had no room for it.
            row(ui, HIT, |ui| {
                self.beat_jump_controls(ui, d, &deck);
                if !pitch_inline && ui.available_width() >= 120.0 {
                    pitch_inline = true;
                    self.deck_pitch_fader_ui(ui, d);
                }
            });
            if !pitch_inline {
                ui.horizontal(|ui| {
                    ui.label(caps("PITCH"));
                    self.deck_pitch_fader_ui(ui, d);
                });
            }

            // 8. Hotcue pads: always on screen, they are played, not configured.
            let gap = ui.spacing().item_spacing.x;
            let hotcues = self.deck_hotcue_slots(d);
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 7.0 * gap) / 8.0;
                // Always hotcues on screen; the other pad modes belong to the hardware pads.
                let mode = DeckPadMode::Hotcues;
                for i in 1..=8usize {
                    let (lit, fill, tip) = match mode {
                        DeckPadMode::Hotcues => (
                            hotcues.iter().any(|(c, _)| *c == i as u8),
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
                        self.clear_deck_hotcue(d, i as u8);
                    }
                }
            });
            // 9. Track tools, behind "TOOLS".
            if !self.deck_expanded[d] {
                return;
            }
            ui.multiply_opacity(pads_t);
            // Wraps rather than widening a narrow deck past the window edge.
            ui.horizontal_wrapped(|ui| {
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
                ui.separator();
                if pill(ui, "AUTO CUES", false, TraktorTheme::ACCENT(), 0.0)
                    .on_hover_text(format!("Automatically detect phrases and set hotcues ({shift}K)"))
                    .clicked()
                {
                    self.apply_auto_cues(d);
                }
                if let Some(status) = &self.deck_auto_cue_status[d] {
                    ui.label(RichText::new(status).color(TraktorTheme::ACCENT()).font(egui::FontId::proportional(11.0)));
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
        let is_recording = self.recording.as_ref().is_some_and(|r| r.is_active());
        let mut in_key = false;
        if let Ok(state) = self.audio.shared.lock() {
            peaks = state.channel_peaks;
            main = (state.master_peak_l, state.master_peak_r);
            both_playing = state.is_playing[0] || state.is_playing[1];
            in_key = state.loaded_tracks[0].as_ref().is_some_and(|a| {
                state.loaded_tracks[1].as_ref().is_some_and(|b| {
                    retraktor_library::KeyWheel::are_harmonically_compatible(&a.key, &b.key)
                })
            });
            for d in 0..4 {
                if let (Some(t), true) = (&state.loaded_tracks[d], state.is_playing[d]) {
                    let at = (state.playhead_sec[d].max(0.0) * PEAKS_PER_SEC as f64) as usize;
                    bands[d] = t.played_peaks.get(at).copied().unwrap_or_default();
                    stems[d] = t.played_stem_peaks.get(at).copied().unwrap_or_default();
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
                self.browser_preview_view();
                self.library.show_browser_content(
                    ui,
                    four_deck_mode,
                    locked_decks,
                    &mut self.preferences.library_columns,
                    &mut self.preferences.music_folders,
                );
                self.drain_pending_loads();
                if let Some(selected) = self.library.preview_requested.take() {
                    self.press_preview(selected);
                }
                if let Some((id, at)) = self.library.overview_clicked.take() {
                    self.click_overview(id, at);
                }

                // If a track was loaded and has stems ready, automatically enable deck stems view
                if let Some((d, stems_ready)) = self.library.last_loaded_deck.take() {
                    if stems_ready {
                        self.deck_stem_view[d] = true;
                    }
                }

                // If stems were generated for a track, enable stems view if it is currently playing/loaded on any deck
                if let Some(stemmed_id) = self.library.last_stemmed_track_id.take() {
                    if let Ok(mut state) = self.audio.shared.lock() {
                        for d in (0..4).filter(|&d| self.deck_track_ids[d] == Some(stemmed_id)) {
                            if let Some(loaded) = state.loaded_tracks[d].as_mut() {
                                loaded.stems_ready = true;
                                self.deck_stem_view[d] = true;
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
        self.next_track_selected = 0;
        self.cached_ai_settings = crate::ai::AiSettings::load();

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
                            self.next_track_selected = 0;
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

        let mut load_pick = None;

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
                        if self.cached_ai_settings.is_configured() {
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
                                    self.ai_rerank_runner = Some(crate::ai::AiRerankRunner::start(self.cached_ai_settings.clone(), prompt, candidates));
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

                            let bg = if rank == self.next_track_selected {
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
                                egui::FontId::proportional(11.5),
                                TraktorTheme::TEXT_SECONDARY(),
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

                            if resp.clicked() {
                                self.next_track_selected = rank;
                            }
                            if resp.double_clicked() {
                                load_pick = Some(rank);
                            }
                        }
                    }
                }

                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("↑/↓ select · Enter or 1–5 loads · Esc closes")
                            .color(TraktorTheme::TEXT_SECONDARY())
                            .font(egui::FontId::proportional(11.0)),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if !self.next_track_picks.is_empty() {
                            let btn = egui::Button::new(
                                RichText::new(format!("LOAD TO DECK {deck_letter} (↵)"))
                                    .color(crate::theme::on(TraktorTheme::ROW_SELECTED()))
                                    .strong(),
                            )
                            .fill(TraktorTheme::ROW_SELECTED())
                            .rounding(4.0)
                            .min_size(Vec2::new(140.0, 24.0));
                            if ui.add(btn).clicked() {
                                load_pick = Some(self.next_track_selected);
                            }
                        }
                    });
                });
            });

        if let Some(pick) = load_pick {
            self.load_next_pick(pick);
        }
    }

    /// Loads pick `pick` (0-based) onto the picker's deck and closes the picker; no such pick, nothing happens.
    fn load_next_pick(&mut self, pick: usize) {
        let Some(&(id, _, _)) = self.next_track_picks.get(pick) else { return };
        let locked = self.locked_decks();
        self.library.load_to_deck(self.next_track_target_deck, id, locked);
        self.drain_pending_loads();
        self.show_next_track_panel = false;
    }
}

/// Beat jump sizes, in beats.
const JUMP_SIZES: [u32; 4] = [4, 8, 16, 32];

/// Where a jump of `beats` (negative = back) lands: the new playhead and, with an active loop
/// `(start, len)`, the loop's new start. Counted in the track's own beats (source seconds), so pitch,
/// sync and play state never change the distance and the beat and bar phase are kept. `None` when the
/// tempo or length is unusable or the playhead or the whole loop would leave [0, duration).
fn beat_jump_target(playhead: f64, bpm: f64, duration: f64, beats: f64, active_loop: Option<(f64, f64)>) -> Option<(f64, Option<f64>)> {
    if !(bpm.is_finite() && bpm > 0.0 && duration.is_finite() && duration > 0.0 && playhead.is_finite()) {
        return None;
    }
    let shift = beats * 60.0 / bpm;
    let to = playhead + shift;
    if !(0.0..duration).contains(&to) {
        return None;
    }
    match active_loop {
        None => Some((to, None)),
        Some((start, len)) => {
            let start = start + shift;
            (start >= 0.0 && start + len <= duration).then_some((to, Some(start)))
        }
    }
}

/// Playback ratio that brings `bpm` to `target`, folding the target by octaves so 70 follows 140 at half time.
/// A deck's hot cues (slot 1..=8, seconds) from a library entry's cues, of any kind, in its first eight slots.
fn deck_hotcues_of(cues: &[retraktor_formats::nml::NmlCue]) -> Vec<(u8, f64)> {
    cues.iter().filter(|c| (0..8).contains(&c.hotcue)).map(|c| ((c.hotcue + 1) as u8, c.start_ms / 1000.0)).collect()
}

/// A deck's tempo as its readout shows it: still being worked out, or unknown, rather than 0.00.
fn bpm_text(bpm: f64, pitch_pct: f32, analysing: bool) -> String {
    match bpm > 0.0 {
        true => format!("{:.2}", bpm * (1.0 + pitch_pct as f64 / 100.0)),
        false if analysing => "…".into(),
        false => "—".into(),
    }
}

/// A deck's key as its readout shows it, likewise.
fn key_text(key: &str, analysing: bool) -> &str {
    match key.is_empty() {
        false => key,
        true if analysing => "…",
        true => "—",
    }
}

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

/// How far (in beats, within ±period/2) a deck at `beats` must move forward to line up with the master
/// every `period` beats: 1 for the beat, 4 for the bar.
fn beat_phase_diff(master_beats: f64, beats: f64, period: f64) -> f64 {
    let diff = (master_beats - beats).rem_euclid(period);
    if diff > period / 2.0 { diff - period } else { diff }
}

/// Moves deck `d` so its bar lines meet `other`'s (at most two beats either way).
/// shortcut: counts bars in each track's own beats, so a half-time pairing (70 under 140) lines up every other bar.
/// Decodes a finished separation's stems on a worker and installs them on each listed deck that still holds
/// the very audio it held when asked (same loaded PCM, so the same library track and load). A deck reloaded
/// meanwhile, even with a same-named track of the same length, keeps what it has. The lock is held only to install.
fn install_stems(
    shared: Arc<std::sync::Mutex<crate::audio::AudioSharedState>>,
    decks: Vec<(usize, LoadedTrackAudio)>,
    stem_dir: PathBuf,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Some((_, mut stemmed)) = decks.first().cloned() else { return };
        if !stemmed.load_stems_from_dir(&stem_dir) {
            return;
        }
        let Ok(mut state) = shared.lock() else { return };
        for (d, asked) in &decks {
            if let Some(loaded) = state.loaded_tracks[*d].as_mut().filter(|l| Arc::ptr_eq(&l.pcm_stereo, &asked.pcm_stereo)) {
                loaded.pcm_drums = stemmed.pcm_drums.clone();
                loaded.pcm_bass = stemmed.pcm_bass.clone();
                loaded.pcm_other = stemmed.pcm_other.clone();
                loaded.pcm_vocals = stemmed.pcm_vocals.clone();
                loaded.stem_peaks = Arc::clone(&stemmed.stem_peaks);
                // Each deck draws its stems at its own latched gain, as load_stems_from_dir does.
                loaded.played_stem_peaks = if (loaded.auto_gain - 1.0).abs() < 1e-4 {
                    Arc::clone(&stemmed.stem_peaks)
                } else {
                    stemmed.stem_peaks.iter().map(|p| p.map(|m| (m * loaded.auto_gain).clamp(0.04, 1.0))).collect()
                };
                loaded.stems_ready = true;
            }
        }
    })
}

/// Where REC saves: the Preferences folder with a leading `~` expanded, or ~/Music/ReTraktor when it is empty.
fn recording_dir(pref: &std::path::Path) -> PathBuf {
    let home = env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("."));
    match pref.strip_prefix("~") {
        _ if pref.as_os_str().is_empty() => home.join("Music").join("ReTraktor"),
        Ok(rest) => home.join(rest),
        Err(_) => pref.to_path_buf(),
    }
}

fn snap_bars(state: &mut crate::audio::AudioSharedState, d: usize, other: usize) {
    let grid = |i: usize| state.loaded_tracks[i].as_ref().filter(|t| t.bpm > 0.0).map(|t| (t.bpm.max(20.0), t.grid_offset_sec));
    if let (Some((bpm, offset)), Some((other_bpm, other_offset))) = (grid(d), grid(other)) {
        let behind = beat_phase_diff(
            (state.playhead_sec[other] - other_offset) * other_bpm / 60.0,
            (state.playhead_sec[d] - offset) * bpm / 60.0,
            4.0,
        );
        let len = state.loaded_tracks[d].as_ref().map_or(0.0, |t| t.duration_sec);
        let from = state.playhead_sec[d];
        state.seek_by(d, (from + behind * 60.0 / bpm).clamp(0.0, len) - from);
    }
}

/// Helper to determine the current phrase and bars remaining until the next phrase boundary.
fn current_phrase_info(
    phrases: &[retraktor_library::Phrase],
    elapsed_sec: f64,
    bpm: f64,
    _grid_offset_sec: f64,
) -> Option<(String, usize)> {
    if phrases.is_empty() || bpm <= 0.0 {
        return None;
    }
    let current_idx = phrases.iter().rposition(|p| p.start_sec <= elapsed_sec)?;
    let cur = &phrases[current_idx];
    let next_boundary_sec = phrases.get(current_idx + 1).map(|p| p.start_sec);
    let sec_per_bar = (60.0 / bpm) * 4.0;
    let bars_rem = if let Some(next_sec) = next_boundary_sec {
        let diff_sec = (next_sec - elapsed_sec).max(0.0);
        (diff_sec / sec_per_bar).ceil() as usize
    } else {
        0
    };
    Some((cur.kind.as_str().to_uppercase(), bars_rem))
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
    peaks: Arc<[[f32; 3]]>,
    stem_peaks: Arc<[[f32; 4]]>,
    /// `peaks` at the level the deck plays (after auto gain): waveform heights come from these,
    /// colours from `peaks`.
    played_peaks: Arc<[[f32; 3]]>,
    overview: crate::overview::Overview,
    played_overview: crate::overview::Overview,
    phrases: Vec<retraktor_library::Phrase>,
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
/// The deck a syncing deck follows: the MASTER deck when one is set (the master itself follows no one),
/// otherwise its partner (B for A, A for every other deck).
fn sync_leader(master: Option<usize>, d: usize) -> Option<usize> {
    match master {
        Some(m) if m == d => None,
        Some(m) => Some(m),
        None => Some(if d == 0 { 1 } else { 0 }),
    }
}

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

/// Beat strip atop a deck display, `width` wide: the bar counter sweeps with the music, the phase meter
/// shows drift from the other deck, and the phrase reads like `BREAKDOWN · 3` (bars to the next phrase).
/// A track with phrases keeps one phrase slot throughout, so a changing label never moves the deck or the mixer.
#[allow(clippy::too_many_arguments)]
fn beat_strip(ui: &mut Ui, d: usize, deck: &DeckView, phase: Option<f64>, accent: Color32, ink: Color32, width: f32, height: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let font = crate::theme::dot_font(9.0);
        let slot_w = if deck.phrases.is_empty() || deck.bpm <= 0.0 { 0.0 } else { phrase_slot_width(ui, &font) };
        let avail = (width - if slot_w > 0.0 { slot_w + 8.0 } else { 0.0 }).max(100.0);
        let bar_w = (avail * 0.58).max(50.0);
        let meter_w = (avail - bar_w - 8.0).max(35.0);
        let (bar, _) = ui.allocate_exact_size(Vec2::new(bar_w, height), Sense::hover());
        dot::bar_counter(ui.painter(), bar, (deck.elapsed_sec - deck.grid_offset_sec) * deck.bpm / 60.0, deck.is_playing, accent, ink);
        let (meter, resp) = ui.allocate_exact_size(Vec2::new(meter_w, height), Sense::hover());
        dot::phase_meter(ui.painter(), meter, phase, accent, ink);
        resp.on_hover_text("Beat phase against the other deck: centre means the beats are locked");
        if slot_w <= 0.0 {
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(slot_w, ui.fonts(|f| f.row_height(&font))), Sense::hover());
        let Some((kind, bars)) = current_phrase_info(&deck.phrases, deck.elapsed_sec, deck.bpm, deck.grid_offset_sec) else { return };
        let text = if bars > 0 { format!("{kind} · {bars}") } else { kind };
        let mut job = egui::text::LayoutJob::simple_singleline(text.clone(), font, ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width(slot_w);
        let galley = ui.fonts(|f| f.layout_job(job));
        let cut = galley.elided;
        ui.painter().galley(egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, ink);
        let resp = if cut { resp.on_hover_text(&text) } else { resp };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("Deck {} phrase {text}", DECK_LETTERS[d])));
    });
}

/// Width in `font` of the longest realistic phrase label: the longest phrase name, BREAKDOWN, plus a two-digit countdown.
// shortcut: a phrase over 99 bars is cut with an ellipsis (full text on hover), widen the sample if such phrases become common.
fn phrase_slot_width(ui: &Ui, font: &egui::FontId) -> f32 {
    ui.fonts(|f| f.layout_no_wrap("BREAKDOWN · 88".to_owned(), font.clone(), Color32::WHITE).size().x).ceil()
}

/// A number glanced at mid-mix: small caps label over the value in the dot-matrix face.
/// True when a BPM readout was double-clicked (back to the track's own tempo).
fn bpm_reset_clicked(resp: egui::Response) -> bool {
    resp.interact(Sense::click()).on_hover_text("Double-click to return to the track's original BPM").double_clicked()
}

fn readout(ui: &mut Ui, label: &str, value: &str, size: f32, ink: Color32) -> egui::Response {
    let label_galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(10.0), ink.gamma_multiply(0.65));
    let value_galley = ui.painter().layout_no_wrap(value.to_owned(), crate::theme::dot_font(size), ink);
    let size = Vec2::new(label_galley.size().x.max(value_galley.size().x), label_galley.size().y + value_galley.size().y);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    let value_pos = egui::pos2(rect.left(), rect.bottom() - value_galley.size().y);
    ui.painter().galley(rect.left_top(), label_galley, ink);
    ui.painter().galley(value_pos, value_galley, ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, format!("{label} {value}")));
    resp
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

/// Short effect names for the deck strip, in `MixerFxKind` order.
const FX_SHORT: [&str; 9] = ["FLT", "REV", "DLY", "NOI", "GAT", "FLG", "BRB", "DOT", "CRU"];

/// The effect `kind` as a short pill: click steps to the next effect, right-click lists them all.
fn fx_pick(ui: &mut Ui, kind: &mut MixerFxKind, what: &str) {
    use crate::preferences::FX_NAMES;
    let resp = pill(ui, FX_SHORT[*kind as usize], false, Color32::TRANSPARENT, 30.0)
        .on_hover_text(format!("{what}: {}. Click for the next effect, right-click to choose", FX_NAMES[*kind as usize]));
    if resp.clicked() {
        *kind = MixerFxKind::from_i64((*kind as i64 + 1) % FX_SHORT.len() as i64);
    }
    resp.context_menu(|ui| {
        for (i, name) in FX_NAMES.iter().enumerate() {
            if ui.selectable_label(*kind as usize == i, *name).clicked() {
                *kind = MixerFxKind::from_i64(i as i64);
                ui.close_menu();
            }
        }
    });
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
    fn bpm_double_click_resets_tempo() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        let mut time = 0.0;
        let mut frame = |events: Vec<egui::Event>| {
            time += 0.05;
            let (mut rect, mut hit) = (egui::Rect::NOTHING, false);
            let _ = ctx.run(egui::RawInput { time: Some(time), events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let r = readout(ui, "BPM", "128.00", 18.0, Color32::WHITE);
                    rect = r.rect;
                    hit = bpm_reset_clicked(r);
                });
            });
            (rect, hit)
        };
        let p = frame(vec![]).0.center();
        let mut hits = Vec::new();
        let click = |pressed| egui::Event::PointerButton { pos: p, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(vec![egui::Event::PointerMoved(p)]);
        for _ in 0..2 {
            hits.push(frame(vec![click(true)]).1);
            hits.push(frame(vec![click(false)]).1);
        }
        assert!(hits.iter().any(|&h| h), "double-click on the BPM readout was not seen");

        let mut app = ReTraktorApp::default();
        app.deck_pitch_fader[0] = 3.5;
        app.deck_sync_enabled[0] = true;
        app.reset_tempo(0);
        assert_eq!(app.deck_pitch_fader[0], 0.0);
        assert!(!app.deck_sync_enabled[0], "sync would pull the pitch straight back");
    }

    #[test]
    fn set_keys_and_locked_loads_never_replace_a_protected_deck() {
        let ctx = egui::Context::default();
        let mut app = ReTraktorApp::default();
        app.library.tracks.push(crate::library_ui::UiTrackEntry { id: 77, title: "x".into(), file_path: Some("/nonexistent.wav".into()), ..Default::default() });
        app.library.rebuild_index();
        // A locked destination is refused with a visible deck notice, through the shared load path.
        app.library.load_to_deck(1, 77, [true, false, false, false]);
        assert!(app.library.pending_loads.is_empty());
        assert!(app.library.deck_refused[0].is_some());

        // Through the real dispatch order: Enter, Delete and O under help or Preferences leave the set alone
        // and load nothing (the set is never changed here, so nothing is saved).
        app.library.tracks.push(crate::library_ui::UiTrackEntry { id: 78, title: "y".into(), file_path: Some("/nonexistent2.wav".into()), ..Default::default() });
        app.library.rebuild_index();
        app.layout = TraktorLayout::Set;
        app.set_state.set_list.track_ids = vec![77, 78];
        app.set_state.selected_index = Some(0);
        let key = |k| egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        for (help, prefs) in [(true, false), (false, true)] {
            app.show_shortcuts_hud = help;
            app.preferences.is_open = prefs;
            let _ = ctx.run(egui::RawInput { events: vec![key(Key::Enter), key(Key::Delete), key(Key::O)], ..Default::default() }, |ctx| app.dispatch_keys(ctx));
            assert_eq!(app.set_state.set_list.track_ids, vec![77, 78], "the set changed under an overlay");
            assert!(app.library.pending_loads.is_empty() && !app.deck_loading.iter().any(|&l| l), "Set Enter loaded under an overlay");
        }

        // Picker Enter loads the pick (78) onto its deck, and the same Enter never also loads the Set selection (77).
        app.show_shortcuts_hud = false;
        app.preferences.is_open = false;
        app.show_next_track_panel = true;
        app.next_track_picks = vec![(78, 1.0, String::new())];
        app.next_track_target_deck = 2;
        let _ = ctx.run(egui::RawInput { events: vec![key(Key::Enter)], ..Default::default() }, |ctx| app.dispatch_keys(ctx));
        let queued: Vec<usize> = app.library.pending_loads.iter().map(|(_, t)| t.id).collect();
        assert!(!queued.contains(&77), "the Set selection loaded along with the pick");
    }

    #[test]
    fn deck_loading_lasts_until_its_own_result() {
        let mut app = ReTraktorApp::default();
        let t = crate::library_ui::UiTrackEntry { id: 5, title: "gone".into(), ..Default::default() };
        // Two dispatches to deck A (no file path, so the workers fail at once); generation 2 is current.
        app.library.pending_loads.push((1, t.clone()));
        app.library.pending_loads.push((1, t.clone()));
        app.drain_pending_loads();
        assert!(app.deck_loading[0], "loading starts at dispatch");
        // Drain the real worker results, then replay them one at a time in a known order.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let results: Vec<_> = app.load_channel.1.try_iter().collect();
        assert_eq!(results.len(), 2);
        let (stale, current): (Vec<_>, Vec<_>) = results.into_iter().partition(|r| matches!(r, DeckResult::Loaded(_, 1, ..)));
        app.load_channel.0.send(stale.into_iter().next().unwrap()).unwrap();
        app.finish_loads();
        assert!(app.deck_loading[0], "a stale result ended loading");
        app.load_channel.0.send(current.into_iter().next().unwrap()).unwrap();
        app.finish_loads();
        assert!(!app.deck_loading[0], "the matching result did not end loading");
        assert!(app.deck_load_failed[0].is_some(), "the failure is not shown on the deck");
    }

    #[test]
    fn sync_follows_the_master_deck() {
        assert_eq!(sync_leader(None, 0), Some(1));
        assert_eq!(sync_leader(None, 2), Some(0));
        assert_eq!(sync_leader(Some(0), 0), None, "the master never follows another deck");
        assert_eq!(sync_leader(Some(2), 0), Some(2), "a C master leads A");
        assert_eq!(sync_leader(Some(2), 1), Some(2));
    }

    #[test]
    fn deck_keys_ignored_while_help_or_picker_is_open() {
        let ctx = egui::Context::default();
        let mut app = ReTraktorApp::default();
        app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(crate::audio::LoadedTrackAudio {
            title: "t".into(),
            artist: String::new(),
            duration_sec: 10.0,
            bpm: 120.0,
            grid_offset_sec: 0.0,
            key: String::new(),
            sample_rate: 44100,
            waveform_peaks: Vec::new().into(),
            stem_peaks: Vec::new().into(),
            overview: Vec::new().into(),
            pcm_stereo: Arc::default(),
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
        });
        let space = || vec![egui::Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
        let press = |app: &mut ReTraktorApp, events| {
            let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| app.handle_keyboard_shortcuts(ctx));
        };
        let playing = |app: &ReTraktorApp| app.audio.shared.lock().unwrap().is_playing[0];

        app.show_shortcuts_hud = true;
        press(&mut app, space());
        assert!(!playing(&app), "Space played deck A under the help sheet");
        app.show_shortcuts_hud = false;
        app.show_next_track_panel = true;
        press(&mut app, space());
        assert!(!playing(&app), "Space played deck A under the next-track picker");
        app.show_next_track_panel = false;
        press(&mut app, space());
        assert!(playing(&app), "Space no longer plays deck A");

        // Escape closes only the topmost layer.
        app.show_shortcuts_hud = true;
        app.preferences.is_open = false;
        app.show_next_track_panel = true;
        press(&mut app, vec![egui::Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
        assert!(!app.show_shortcuts_hud && app.show_next_track_panel);
    }

    #[test]
    fn real_file_loads_and_recording_retries() {
        let dir = std::env::temp_dir().join(format!("retraktor-load-regression-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("track.wav");
        AudioSystem::write_stereo_wav_file(&wav, &vec![100; 8820], 44100).unwrap();
        let mut app = ReTraktorApp::default();
        app.library = LibraryManager::new(dir.join("collection.nml"));
        app.preferences.lock_playing_decks = true;
        app.toggle_play(1);
        assert!(!app.audio.shared.lock().unwrap().is_playing[0]);
        assert!(!app.locked_decks()[0]);
        let track = crate::library_ui::UiTrackEntry {
            id: 12, title: "Real WAV".into(), file_path: Some(wav.clone()),
            bpm: 128.0, musical_key: "1d".into(), ..Default::default()
        };
        app.deck_hotcues[0] = vec![(1, 42.0)];
        app.library.pending_loads.push((1, track.clone()));
        app.drain_pending_loads();
        // A pad set in the library while the file decodes wins over the queued copy's empty pads.
        let pad6 = retraktor_formats::nml::NmlCue {
            name: "Hotcue 6".into(), displ_order: 0, cue_type: retraktor_core::CueType::Cue,
            start_ms: 3000.0, len_ms: 0.0, repeats: -1, hotcue: 5, grid_bpm: None,
        };
        app.library.tracks.push(crate::library_ui::UiTrackEntry { cues: vec![pad6], ..track.clone() });
        app.library.rebuild_index();
        let result = app.load_channel.1.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(matches!(result, DeckResult::Loaded(_, _, _, Ok(_))));
        app.load_channel.0.send(result).unwrap();
        app.finish_loads();
        assert_eq!(app.deck_track_ids[0], Some(12));
        assert_eq!(app.deck_hotcues[0], [(6, 3.0)]);
        let view = app.deck_view(0).unwrap();
        assert!(Arc::ptr_eq(&view.peaks, &app.audio.shared.lock().unwrap().loaded_tracks[0].as_ref().unwrap().waveform_peaks));

        app.load_generation[0] += 1;
        let stale = LoadedTrackAudio::from_file(&wav, "Stale", "", 128.0, "1d").unwrap();
        app.load_channel.0.send(DeckResult::Loaded(0, 1, track.clone(), Ok(stale))).unwrap();
        app.finish_loads();
        assert_eq!(app.deck_view(0).unwrap().title, "Real WAV");

        let bad = dir.join("bad.wav");
        std::fs::write(&bad, b"not audio").unwrap();
        let bad_track = crate::library_ui::UiTrackEntry { id: 13, file_path: Some(bad), ..track };
        app.library.pending_loads.push((1, bad_track));
        app.drain_pending_loads();
        let result = app.load_channel.1.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        assert!(matches!(result, DeckResult::Loaded(_, _, _, Err(_))));
        app.load_channel.0.send(result).unwrap();
        app.finish_loads();
        assert!(app.library.status_message.contains("Could not load"));
        assert_eq!(app.deck_track_ids[0], Some(12));

        // REC through the real callback: the WAV holds exactly what the speakers got.
        app.audio.shared.lock().unwrap().pattern_enabled = true;
        for rate in [44100u32, 96000] {
            app.start_recording(&dir.join("rec"));
            let wav = app.recording.as_ref().unwrap().wav.clone();
            let mut speakers = Vec::new();
            let mut block = vec![0.0f32; 500 * 2];
            // 500-frame blocks never line up with the recorder's chunks: the last one is partial.
            for _ in 0..rate / 500 + 1 {
                app.audio.render_f32(&mut block, 2, rate);
                speakers.extend_from_slice(&block);
            }
            let frames = speakers.len() / 2;
            assert_eq!(app.recording.as_ref().unwrap().seconds(), frames as f64 / rate as f64);
            app.stop_recording();
            let t0 = std::time::Instant::now();
            while !app.poll_recording() {
                assert!(t0.elapsed() < std::time::Duration::from_secs(10), "recording never finished");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(app.recording.is_none() && app.library.status_message.contains("Saved recording"));
            let bytes = std::fs::read(&wav).unwrap();
            let recorded: Vec<i16> = bytes[80..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            let expected: Vec<i16> = speakers.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0) as i16).collect();
            assert!(expected.iter().any(|&s| s.abs() > 1000), "pattern player should be audible");
            assert_eq!(recorded, expected);
            let decoded = LoadedTrackAudio::from_file(&wav, "", "", 128.0, "1d").unwrap();
            assert_eq!(decoded.sample_rate, rate);
            assert!((decoded.duration_sec - frames as f64 / rate as f64).abs() < 1e-6);
            assert!(wav.with_extension("cue").exists() && wav.with_extension("txt").exists());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    pub(super) fn recording_test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("retraktor-app-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Renders `blocks` 500-frame blocks through the real audio callback.
    fn render_blocks(app: &ReTraktorApp, blocks: usize, rate: u32) {
        let mut block = vec![0.0f32; 500 * 2];
        for _ in 0..blocks {
            app.audio.render_f32(&mut block, 2, rate);
        }
    }

    /// Polls like the GUI until the recording is finished; whether it finished cleanly.
    pub(super) fn poll_until_finished(app: &mut ReTraktorApp) -> bool {
        let t0 = std::time::Instant::now();
        loop {
            let clean = app.poll_recording();
            if app.recording.is_none() {
                return clean;
            }
            assert!(t0.elapsed() < std::time::Duration::from_secs(10), "recording never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn recording_lists_only_tracks_inside_the_saved_audio() {
        let dir = recording_test_dir("cue");
        let mut app = ReTraktorApp::default();
        app.audio.shared.lock().unwrap().pattern_enabled = true;
        app.start_recording(&dir);
        let (cue, txt) = app.recording.as_ref().map(|r| (r.cue.clone(), r.txt.clone())).unwrap();
        render_blocks(&app, 88, 44_100); // 44 000 frames, just under 1 s
        let end = 44_000.0 / 44_100.0;
        // As after a failed write: the clock had run past what reached the disk.
        app.set_state.recorded_audible_tracks = vec![
            (0.5, "A".into(), "Inside".into()),
            (end, "B".into(), "At the end".into()),
            (end + 30.0, "C".into(), "Past the end".into()),
        ];
        // The tracklist cannot be written: its name is taken by a directory.
        std::fs::remove_file(&txt).unwrap();
        std::fs::create_dir(&txt).unwrap();
        app.close_after_recording = true;
        app.stop_recording();
        assert!(!poll_until_finished(&mut app), "a missing tracklist is not a clean save");
        assert!(!app.close_after_recording, "the window stays open so the DJ sees what happened");
        let msg = &app.library.status_message;
        assert!(msg.contains("WAV, CUE to") && msg.contains("tracklist not written") && !msg.contains('✔'), "{msg}");
        let sheet = std::fs::read_to_string(&cue).unwrap();
        assert_eq!(sheet.matches("TRACK ").count(), 1, "{sheet}");
        assert!(sheet.contains("TITLE \"Inside\"") && sheet.contains("INDEX 01 00:00:38"), "{sheet}"); // 0.5 s = 37.5 frames
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rate_change_mid_set_stops_and_keeps_the_first_rate() {
        let dir = recording_test_dir("rate");
        let mut app = ReTraktorApp::default();
        app.audio.shared.lock().unwrap().pattern_enabled = true;
        app.start_recording(&dir);
        let wav = app.recording.as_ref().unwrap().wav.clone();
        render_blocks(&app, 44, 44_100);
        render_blocks(&app, 4, 48_000); // the card came back at another rate
        assert!(!poll_until_finished(&mut app), "an early stop is not a clean save");
        let msg = &app.library.status_message;
        assert!(msg.contains("Recording stopped early: the sound card's sample rate changed"), "{msg}");
        assert!(msg.contains("Saved 0.5s: WAV, CUE, tracklist"), "{msg}");
        assert_eq!(std::fs::metadata(&wav).unwrap().len(), 80 + 22_000 * 4);
        let decoded = LoadedTrackAudio::from_file(&wav, "", "", 128.0, "1d").unwrap();
        assert_eq!(decoded.sample_rate, 44_100);
        assert!((decoded.duration_sec - 22_000.0 / 44_100.0).abs() < 1e-6);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn recording_does_not_start_without_the_audio_engine() {
        let dir = recording_test_dir("engine");
        let mut app = ReTraktorApp::default();
        let slot = Arc::clone(&app.audio.recorder);
        let _ = std::thread::spawn(move || {
            let _tap = slot.lock().unwrap();
            panic!("poisoning the recorder slot on purpose");
        })
        .join();
        app.start_recording(&dir);
        assert!(app.recording.is_none());
        assert!(app.library.status_message.contains("did not start"), "{}", app.library.status_message);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "the reserved files were removed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn hotcue_pads_on_two_decks_share_the_library_track() {
        let dir = std::env::temp_dir().join(format!("retraktor-hotcue-decks-{}", std::process::id()));
        let mut app = ReTraktorApp::default();
        app.library = LibraryManager::new(dir.join("collection.nml"));
        let intro = retraktor_formats::nml::NmlCue {
            name: "Intro".into(), displ_order: 0, cue_type: retraktor_core::CueType::Cue,
            start_ms: 1500.0, len_ms: 0.0, repeats: -1, hotcue: -1, grid_bpm: None,
        };
        app.library.tracks.push(crate::library_ui::UiTrackEntry { id: 40, title: "Shared".into(), cues: vec![intro.clone()], ..Default::default() });
        app.library.rebuild_index();
        // Both decks loaded the track before any pad was set: their load snapshots are empty.
        app.deck_track_ids = [Some(40), Some(40), None, None];
        let set_playhead = |app: &ReTraktorApp, d: usize, sec: f64| app.audio.shared.lock().unwrap().playhead_sec[d] = sec;
        let playhead = |app: &ReTraktorApp, d: usize| app.audio.shared.lock().unwrap().playhead_sec[d];

        set_playhead(&app, 0, 12.0);
        app.jump_to_cue(1, 1);
        set_playhead(&app, 1, 20.0);
        app.jump_to_cue(2, 2);
        app.jump_to_cue(2, 1);
        assert_eq!(playhead(&app, 1), 12.0, "B jumps to A's pad 1 instead of overwriting it");
        assert_eq!(app.deck_hotcue_slots(0), [(1, 12.0), (2, 20.0)]);
        assert_eq!(app.deck_hotcue_slots(1), app.deck_hotcue_slots(0));
        app.jump_to_cue(1, 9);
        assert_eq!(app.library.track(40).unwrap().cues.len(), 3, "pad 9 does not exist");

        app.clear_deck_hotcue(0, 2);
        assert_eq!(app.deck_hotcue_slots(1), [(1, 12.0)]);
        assert_eq!(app.library.track(40).unwrap().cues[0], intro, "the memory cue survives pad edits");

        // Analysis fills pad 4 in the library; deck B shows it on its next draw.
        let mut auto = intro.clone();
        (auto.name, auto.start_ms, auto.hotcue) = ("Auto 4".into(), 45_000.0, 3);
        app.library.tracks[0].cues.push(auto);
        assert_eq!(app.deck_hotcue_slots(1), [(1, 12.0), (4, 45.0)]);

        // Removed from the library while loaded: deck B keeps the pads it last showed, not the load snapshot.
        app.library.tracks.clear();
        app.library.rebuild_index();
        assert_eq!(app.deck_hotcue_slots(1), [(1, 12.0), (4, 45.0)]);
        set_playhead(&app, 1, 30.0);
        app.jump_to_cue(2, 3);
        app.jump_to_cue(2, 1);
        assert_eq!(playhead(&app, 1), 12.0);
        assert_eq!(app.deck_hotcue_slots(1), [(1, 12.0), (4, 45.0), (3, 30.0)]);
        app.clear_deck_hotcue(1, 1);
        assert_eq!(app.deck_hotcue_slots(1), [(4, 45.0), (3, 30.0)]);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Auto gain is read when a deck load lands: switching it on leaves the track on air alone and
    /// matches the next load, with the other deck untouched.
    #[test]
    fn auto_gain_preference_applies_on_the_next_load() {
        let dir = std::env::temp_dir().join(format!("retraktor-autogain-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tone = |name: &str, amp: f32| {
            let path = dir.join(name);
            let pcm: Vec<i16> = (0..44_100).flat_map(|i| {
                let v = (amp * (std::f32::consts::TAU * 1000.0 * i as f32 / 44_100.0).sin() * 32767.0) as i16;
                [v, v]
            }).collect();
            AudioSystem::write_stereo_wav_file(&path, &pcm, 44_100).unwrap();
            crate::library_ui::UiTrackEntry { id: 1, title: name.into(), file_path: Some(path), bpm: 128.0, musical_key: "1d".into(), ..Default::default() }
        };
        let (quiet, loud) = (tone("quiet.wav", 0.1), tone("loud.wav", 0.4));
        let mut app = ReTraktorApp::default();
        app.library = LibraryManager::new(dir.join("collection.nml"));
        let load = |app: &mut ReTraktorApp, deck: usize, track: &crate::library_ui::UiTrackEntry| {
            app.library.pending_loads.push((deck, track.clone()));
            app.drain_pending_loads();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while app.deck_loading[deck - 1] && std::time::Instant::now() < deadline {
                app.finish_loads();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(!app.deck_loading[deck - 1], "deck {deck} load timed out");
        };
        let gains = |app: &ReTraktorApp| app.audio.shared.lock().unwrap().loaded_tracks.each_ref().map(|t| t.as_ref().map(|t| t.auto_gain));

        app.preferences.autogain = false;
        load(&mut app, 1, &quiet);
        assert_eq!(gains(&app)[0], Some(1.0), "off: played as it is");
        assert!(app.audio.shared.lock().unwrap().loaded_tracks[0].as_ref().is_some_and(|t| {
            Arc::ptr_eq(&t.played_peaks, &t.waveform_peaks) && Arc::ptr_eq(&t.played_stem_peaks, &t.stem_peaks)
        }), "off: the waveform shows the track as analysed");
        app.preferences.autogain = true;
        app.sync_audio_state();
        assert_eq!(gains(&app)[0], Some(1.0), "the track on air jumped when auto gain was switched on");
        load(&mut app, 2, &loud);
        let g = gains(&app);
        assert_eq!(g[0], Some(1.0), "loading deck B changed deck A");
        load(&mut app, 1, &quiet);
        let g2 = gains(&app);
        assert_eq!(g2[1], g[1], "reloading deck A changed deck B");
        // Both decks now meet at the same level, 12 dB apart before.
        assert!((0.1 * g2[0].unwrap() / (0.4 * g2[1].unwrap()) - 1.0).abs() < 0.01, "{g2:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn sync_matches_tempo_and_phase() {
        assert!((sync_ratio(91.0, 68.5) - 68.5 / 91.0).abs() < 1e-9);
        assert!((sync_ratio(68.5, 137.0) - 1.0).abs() < 1e-9); // half time, not +100%
        assert!((sync_ratio(140.0, 128.0) * 140.0 - 128.0).abs() < 1e-9);
        assert!((beat_phase_diff(10.25, 3.0, 1.0) - 0.25).abs() < 1e-9);
        assert!((beat_phase_diff(10.0, 3.9, 1.0) - 0.1).abs() < 1e-9);
        assert!((beat_phase_diff(10.0, 3.2, 1.0) + 0.2).abs() < 1e-9);
        assert!((beat_phase_diff(8.0, 5.0, 4.0) + 1.0).abs() < 1e-9, "bar 3 of 4 moves back a beat to the downbeat");
        assert!((beat_phase_diff(8.0, 6.5, 4.0) - 1.5).abs() < 1e-9);
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

#[cfg(test)]
mod sync_audio {
    use super::*;

    fn track(bpm: f64) -> crate::audio::LoadedTrackAudio {
        crate::audio::LoadedTrackAudio {
            title: "t".into(), artist: String::new(), duration_sec: 100.0, bpm, grid_offset_sec: 0.0, key: String::new(),
            sample_rate: 44100, waveform_peaks: Vec::new().into(), stem_peaks: Vec::new().into(), overview: Vec::new().into(), pcm_stereo: Arc::new(vec![0.0; 44100 * 200]),
            stems_ready: false, pcm_drums: None, pcm_bass: None, pcm_other: None, pcm_vocals: None, phrases: Vec::new(),
            auto_gain: 1.0, played_peaks: Vec::new().into(), played_stem_peaks: Vec::new().into(), played_overview: Vec::new().into(),
        }
    }

    /// An accessible node: name, role, rectangle, disabled.
    type Node = (String, egui::accesskit::Role, egui::Rect, bool);

    /// One frame of the workspace as `update` draws it in a `screen`-sized window; returns every named accessible node.
    fn workspace_frame(ctx: &egui::Context, app: &mut ReTraktorApp, screen: egui::Rect, time: f64, events: Vec<egui::Event>) -> Vec<Node> {
        let out = ctx.run(egui::RawInput { time: Some(time), events, screen_rect: Some(screen), ..Default::default() }, |ctx| {
            egui::CentralPanel::default().frame(egui::Frame::none().inner_margin(4.0)).show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
                app.show_top_bar(ui);
                match app.layout {
                    TraktorLayout::FourDeck => app.show_four_deck_layout(ui),
                    TraktorLayout::Timeline => {
                        app.show_mini_decks_bar(ui);
                        app.show_timeline_studio_layout(ui);
                    }
                    _ => app.show_parallel_layout(ui),
                }
                app.show_browser_panel(ui);
            });
        });
        let nodes = out.platform_output.accesskit_update.map(|u| u.nodes).unwrap_or_default();
        nodes
            .into_iter()
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                let r = egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32));
                Some((n.name()?.to_string(), n.role(), r, n.is_disabled()))
            })
            .collect()
    }

    fn key(key: Key, shift: bool) -> egui::Event {
        let modifiers = if shift { egui::Modifiers::SHIFT } else { egui::Modifiers::NONE };
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    fn button(app: &mut ReTraktorApp, control: &str, pressed: bool) {
        app.apply_mx2_surface_event(SurfaceEvent::Button { control: control.into(), pressed });
    }

    /// Library tracks 1..=n on real files, so loads are queued.
    fn loadable_library(app: &mut ReTraktorApp, n: usize) {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
        app.library.tracks = (1..=n)
            .map(|id| crate::library_ui::UiTrackEntry { id, title: format!("Track {id}"), file_path: Some(file.clone()), ..Default::default() })
            .collect();
        app.library.rebuild_index();
    }

    /// SYNC on B makes the playing A the master and jumps B to its tempo; then either pitch fader moves both decks,
    /// sync_audio_state keeps what the fader set, and an unsynced deck's fader moves only itself.
    #[test]
    fn synced_pitch_faders_move_both_decks_together() {
        let mut app = ReTraktorApp::default();
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks = [Some(track(120.0)), Some(track(130.0)), Some(track(100.0)), None];
            s.is_playing = [true, false, false, false];
        }
        let bpm_of = |app: &ReTraktorApp, d: usize, base: f64| base * app.audio.shared.lock().unwrap().pitch_ratio[d];
        app.sync_deck(2);
        assert_eq!(app.tempo_master_deck, Some(0), "the playing deck leads");
        assert!((130.0 * (1.0 + app.deck_pitch_fader[1] as f64 / 100.0) - 120.0).abs() < 1e-3, "B didn't jump to A's tempo at once");
        assert_eq!(app.deck_pitch_fader[0], 0.0, "SYNC on B moved A");

        // B's fader (synced) to 126 BPM: A follows.
        app.set_deck_pitch_fader(1, ((126.0 / 130.0 - 1.0) * 100.0) as f32);
        app.sync_audio_state();
        assert!((bpm_of(&app, 0, 120.0) - 126.0).abs() < 1e-3 && (bpm_of(&app, 1, 130.0) - 126.0).abs() < 1e-3);
        assert!((app.master_tempo - 126.0).abs() < 1e-3);
        // A's fader (the master) back to 0: B follows.
        app.set_deck_pitch_fader(0, 0.0);
        app.sync_audio_state();
        assert!((bpm_of(&app, 0, 120.0) - 120.0).abs() < 1e-3 && (bpm_of(&app, 1, 130.0) - 120.0).abs() < 1e-3);
        // C, unsynced, moves alone.
        app.set_deck_pitch_fader(2, 5.0);
        app.sync_audio_state();
        assert!((bpm_of(&app, 0, 120.0) - 120.0).abs() < 1e-3 && (bpm_of(&app, 2, 100.0) - 105.0).abs() < 1e-3);

        // MASTER on C takes C's pitched tempo; again releases it.
        app.toggle_master(2);
        assert_eq!(app.tempo_master_deck, Some(2));
        assert!((app.master_tempo - 105.0).abs() < 1e-3);
        app.toggle_master(2);
        assert_eq!(app.tempo_master_deck, None);
    }

    /// A new track on a deck starts with every stem on, at full level and dry.
    #[test]
    fn loading_a_track_resets_its_decks_stems() {
        let mut app = ReTraktorApp::default();
        app.deck_stem_muted[0] = [true; 4];
        app.deck_stem_volumes[0] = [0.2; 4];
        app.deck_stem_fx_send[0] = [true; 4];
        app.deck_stem_muted[1] = [true; 4];
        app.load_generation[0] = 1;
        let entry = crate::library_ui::UiTrackEntry { id: 7, title: "New".into(), bpm: 120.0, musical_key: "8A".into(), ..Default::default() };
        app.load_channel.0.send(DeckResult::Loaded(0, 1, entry, Ok(track(120.0)))).unwrap();
        app.finish_loads();
        assert_eq!(app.deck_track_ids[0], Some(7));
        assert_eq!((app.deck_stem_muted[0], app.deck_stem_volumes[0], app.deck_stem_fx_send[0]), ([false; 4], [1.0; 4], [false; 4]));
        assert_eq!(app.deck_stem_muted[1], [true; 4], "the other deck's stems changed");
    }

    /// MX2: SHIFT + hotcue pad deletes the cue, browse push loads the selection onto that side's deck, SHIFT + platter
    /// scrubs fast, and the stem and sync LEDs read 1-indexed bus keys.
    #[test]
    fn mx2_shift_pads_browse_push_shift_jog_and_stem_leds() {
        let mut app = ReTraktorApp::default();
        app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(track(120.0));
        assert!(app.deck_hotcues[0].iter().any(|&(s, _)| s == 1));
        button(&mut app, "surface.left.shift", true);
        button(&mut app, "surface.left.pads.1", true);
        assert!(!app.deck_hotcues[0].iter().any(|&(s, _)| s == 1), "SHIFT + pad 1 kept the cue");
        button(&mut app, "surface.left.shift", false);
        button(&mut app, "surface.left.pads.1", true);
        assert!(app.deck_hotcues[0].iter().any(|&(s, _)| s == 1), "pad 1 didn't set the cue again");

        // Platter on the paused deck: a gentle nudge, with SHIFT a fast scrub (96 ticks = 0.6 s).
        let turn = |app: &mut ReTraktorApp| {
            let before = app.audio.shared.lock().unwrap().playhead_sec[0];
            app.apply_mx2_surface_event(SurfaceEvent::Encoder { control: "surface.left.jogwheel.rotation".into(), delta: 96 });
            app.audio.shared.lock().unwrap().playhead_sec[0] - before
        };
        app.audio.shared.lock().unwrap().playhead_sec[0] = 10.0;
        let nudge = turn(&mut app);
        assert!(nudge > 0.0 && nudge < 0.05, "nudge moved {nudge}");
        button(&mut app, "surface.left.shift", true);
        assert!((turn(&mut app) - 0.6).abs() < 1e-9);
        button(&mut app, "surface.left.shift", false);

        loadable_library(&mut app, 2);
        app.library.selected_track_id = Some(2);
        button(&mut app, "surface.right.browse.encoder.push", true);
        assert!(app.deck_loading[1] && app.load_generation == [0, 1, 0, 0], "right push didn't load deck B");
        button(&mut app, "surface.left.browse.encoder.push", true);
        assert!(app.deck_loading[0] && app.load_generation == [1, 1, 0, 0], "left push didn't load deck A");

        app.deck_stem_muted[0][0] = true;
        app.deck_sync_enabled[1] = true;
        app.sync_audio_state();
        let lit = |app: &ReTraktorApp, key: &str| app.bus.get(key).and_then(|v| v.as_bool()).unwrap();
        assert!(lit(&app, "app.traktor.decks.1.stems.1.muted") && !lit(&app, "app.traktor.decks.1.stems.4.muted"));
        assert!(lit(&app, "app.traktor.decks.2.sync.enabled") && !lit(&app, "app.traktor.decks.1.sync.enabled"));
    }

    /// The picker: arrows and Enter load the highlighted pick, a digit loads that pick, both onto the picker's deck.
    #[test]
    fn next_track_picker_keys_choose_and_load() {
        let ctx = egui::Context::default();
        let mut app = ReTraktorApp::default();
        loadable_library(&mut app, 5);
        let press = |app: &mut ReTraktorApp, events| {
            let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| app.handle_keyboard_shortcuts(ctx));
        };
        let open = |app: &mut ReTraktorApp| {
            app.show_next_track_panel = true;
            app.next_track_picks = (1..=5).map(|id| (id, 1.0, String::new())).collect();
            app.next_track_selected = 0;
            app.next_track_target_deck = 2;
        };
        open(&mut app);
        press(&mut app, vec![key(Key::ArrowDown, false)]);
        press(&mut app, vec![key(Key::ArrowDown, false)]);
        press(&mut app, vec![key(Key::ArrowUp, false)]);
        assert_eq!(app.next_track_selected, 1);
        press(&mut app, vec![key(Key::Enter, false)]);
        assert!(!app.show_next_track_panel && app.deck_loading[1]);
        assert_eq!(app.library.status_message, "Loading 'Track 2' on deck B");

        open(&mut app);
        press(&mut app, vec![key(Key::Num3, false)]);
        assert!(!app.show_next_track_panel && app.load_generation[1] == 2);
        assert_eq!(app.library.status_message, "Loading 'Track 3' on deck B");
        assert!(!app.deck_loading[0], "a picker digit reached deck A's hotcues");
    }

    /// Hide-missing: Preferences and the library toolbar each win when they change.
    #[test]
    fn hide_missing_follows_preferences_and_the_library_toolbar() {
        let mut app = ReTraktorApp::default();
        app.preferences.hide_missing_files = true;
        app.sync_hide_missing();
        assert!(app.library.hide_missing_files);
        app.library.hide_missing_files = false; // "Hiding missing ×"
        app.sync_hide_missing();
        assert!(!app.preferences.hide_missing_files);
        app.sync_hide_missing();
        assert!(!app.library.hide_missing_files && !app.preferences.hide_missing_files, "it flipped back");
    }

    /// The FX units, their deck assigns and the limiter reach the engine, from the screen and the MX2 alike.
    #[test]
    fn fx_units_assigns_and_limiter_reach_the_engine() {
        let mut app = ReTraktorApp::default();
        app.preferences.limiter_enabled = false;
        button(&mut app, "surface.left.fx.buttons.1", true);
        button(&mut app, "surface.left.fx.buttons.2", true);
        button(&mut app, "surface.right.fx.buttons.4", true);
        button(&mut app, "surface.mixer.channels.2.deck_fx_assign.1", true);
        button(&mut app, "surface.mixer.channel_fx.fx4", true);
        app.fx2_param = [0.1, 0.2, 0.3];
        app.sync_audio_state();
        let s = app.audio.shared.lock().unwrap();
        assert_eq!(s.fx_enabled, [true, false]);
        assert_eq!(s.fx_kind, [MixerFxKind::DualDelay, MixerFxKind::DualDelay], "FX 1 didn't step on from Reverb");
        assert_eq!(s.deck_fx_assign[1], [true, false], "deck B: right button 4 left FX 2, mixer assign joined FX 1");
        assert_eq!(s.fx_params[1], [0.1, 0.2, 0.3]);
        assert!(!s.limiter_enabled);
        assert_eq!(s.engine.strips[0].filter.kind, MixerFxKind::TimeGater, "FX4 isn't Preferences slot 4");
    }

    /// Both decks' mixer-FX and FX-unit pills fit the top bar of the smallest window, clear of every other control.
    #[test]
    fn deck_strip_fx_pills_fit_the_smallest_window() {
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let mut app = ReTraktorApp::default();
        app.channel_fx_kind[1] = MixerFxKind::Crush;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1024.0, 640.0));
        let nodes = workspace_frame(&ctx, &mut app, screen, 0.0, Vec::new());
        let top: Vec<&Node> = nodes.iter().filter(|n| n.2.bottom() < 72.0 && n.1 == egui::accesskit::Role::Button).collect();
        for name in ["FLT", "CRU", "REV", "DLY"] {
            assert!(top.iter().any(|n| n.0 == name), "no {name} pill in the top bar: {top:?}");
        }
        for (i, a) in top.iter().enumerate() {
            assert!(screen.contains_rect(a.2), "{} off screen at {:?}", a.0, a.2);
            for b in &top[i + 1..] {
                assert!(!a.2.shrink(0.5).intersects(b.2.shrink(0.5)), "{} overlaps {}", a.0, b.0);
            }
        }
    }

    /// SYNC on B (130) under A (120): the audio callback really plays B at 120/130, with key lock off and on.
    #[test]
    fn synced_deck_plays_at_the_leaders_tempo() {
        let mut app = ReTraktorApp::default();
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks[0] = Some(track(120.0));
            s.loaded_tracks[1] = Some(track(130.0));
            s.is_playing = [true, true, false, false];
        }
        app.sync_deck(2);
        app.sync_audio_state();
        for keylock in [false, true] {
            let shared = app.audio.shared.clone();
            let before = { let mut s = shared.lock().unwrap(); s.keylock = [keylock; 4]; s.playhead_sec };
            let mut buf = vec![0.0f32; 512 * 2];
            for _ in 0..(48_000 * 2 / 512) {
                app.audio.render_f32(&mut buf, 2, 48_000);
            }
            let after = shared.lock().unwrap().playhead_sec;
            let ratio = (after[1] - before[1]) / (after[0] - before[0]);
            assert!((ratio - 120.0 / 130.0).abs() < 1e-3, "key lock {keylock}: B ran at {ratio}");
        }
    }

    /// SYNC and starting a synced deck both land it on the leader's bar, not just its beat.
    #[test]
    fn sync_lines_up_bars_on_press_and_on_play() {
        let bar_gap = |app: &ReTraktorApp| {
            let s = app.audio.shared.lock().unwrap();
            beat_phase_diff(s.playhead_sec[0] * 120.0 / 60.0, s.playhead_sec[1] * 130.0 / 60.0, 4.0)
        };
        let mut app = ReTraktorApp::default();
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks[0] = Some(track(120.0));
            s.loaded_tracks[1] = Some(track(130.0));
            s.is_playing = [true, true, false, false];
            s.playhead_sec[0] = 10.0; // beat 20: a downbeat
            s.playhead_sec[1] = 30.0 + 2.6 * 60.0 / 130.0; // beat 67.6: late in bar 17
        }
        app.sync_audio_state();
        app.sync_deck(2);
        assert!(bar_gap(&app).abs() < 1e-6, "SYNC left the bars {} beats apart", bar_gap(&app));

        // Stopped, moved, started again with SYNC still on: it snaps back onto the bar.
        app.audio.shared.lock().unwrap().is_playing[1] = false;
        app.sync_audio_state();
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.playhead_sec[1] += 1.3 * 60.0 / 130.0;
            s.is_playing[1] = true;
        }
        app.sync_audio_state();
        assert!(bar_gap(&app).abs() < 1e-6, "starting play left the bars {} beats apart", bar_gap(&app));

        // A beat jump on the synced deck moves whole bars: still on A's bar, at the same synced pitch.
        let ratio = app.audio.shared.lock().unwrap().pitch_ratio[1];
        app.deck_jump_beats[1] = 16;
        for dir in [1.0, -1.0] {
            assert!(app.beat_jump(1, dir));
            app.sync_audio_state();
            assert!(bar_gap(&app).abs() < 1e-6, "beat jump left the bars {} beats apart", bar_gap(&app));
            assert!((app.audio.shared.lock().unwrap().pitch_ratio[1] - ratio).abs() < 1e-9);
        }
    }

    fn render(app: &ReTraktorApp, seconds: f64, block: usize, rate: u32) {
        let mut buf = vec![0.0f32; block * 2];
        for _ in 0..(seconds * rate as f64 / block as f64).round() as usize {
            app.audio.render_f32(&mut buf, 2, rate);
        }
    }

    fn playhead(app: &ReTraktorApp, d: usize) -> f64 {
        app.audio.shared.lock().unwrap().playhead_sec[d]
    }

    /// +32 beats at 120 BPM is 16 s of track whatever the pitch, from an off-grid spot; back returns.
    /// The very first block after the jump plays the new region (a tone where the old one was silent).
    /// Playing and paused decks both jump, and nothing on the other deck changes.
    #[test]
    fn beat_jump_moves_by_the_tracks_own_beats() {
        let mut app = ReTraktorApp::default();
        {
            let mut tone = track(120.0); // silent except 26-28 s
            for f in 26 * 44_100..28 * 44_100 {
                let v = (f as f32 * 0.06).sin() * 0.5;
                Arc::make_mut(&mut tone.pcm_stereo)[2 * f] = v;
                Arc::make_mut(&mut tone.pcm_stereo)[2 * f + 1] = v;
            }
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks[0] = Some(tone);
            s.loaded_tracks[1] = Some(track(120.0));
            s.playhead_sec[0] = 10.37; // beat 20.74: between beats
            s.playhead_sec[1] = 3.0;
            s.is_playing[0] = true;
        }
        app.deck_pitch_fader[0] = 6.0;
        app.sync_audio_state();
        let block_peak = |app: &ReTraktorApp| {
            let mut buf = vec![0.0f32; 480 * 2];
            app.audio.render_f32(&mut buf, 2, 48_000);
            buf.iter().fold(0.0f32, |m, v| m.max(v.abs()))
        };
        assert!(block_peak(&app) < 1e-6, "the region before the jump should be silent");
        let p0 = playhead(&app, 0);
        app.deck_jump_beats[0] = 32;
        assert!(app.beat_jump(0, 1.0));
        let p = playhead(&app, 0);
        assert!((p - p0 - 16.0).abs() < 1e-9, "jumped {}", p - p0);
        assert!(((p * 2.0).fract() - (p0 * 2.0).fract()).abs() < 1e-9, "beat phase lost");
        assert!(block_peak(&app) > 0.05, "the first block after the jump didn't play the new region");
        {
            let s = app.audio.shared.lock().unwrap();
            assert!(s.is_playing[0] && !s.is_playing[1]);
            assert_eq!(s.playhead_sec[1], 3.0);
            assert!((s.pitch_ratio[0] - 1.06).abs() < 1e-9);
        }
        assert_eq!(app.deck_jump_beats[1], 4, "decks share a jump size");

        // Plays on at +6% from the new spot, then jumps back 16 s.
        render(&app, 1.0, 480, 48_000);
        let p = playhead(&app, 0);
        assert!((p - (p0 + 16.0 + 1.06 * 1.01)).abs() < 1e-6, "played to {p}");
        assert!(app.beat_jump(0, -1.0));
        assert!((playhead(&app, 0) - (p - 16.0)).abs() < 1e-9);

        // Paused deck B: 4 beats = 2 s, and it stays paused and put.
        assert!(app.beat_jump(1, 1.0));
        render(&app, 0.2, 512, 44_100);
        assert_eq!(playhead(&app, 1), 5.0);
        assert!(!app.audio.shared.lock().unwrap().is_playing[1]);
    }

    /// A jump that would leave [0, duration) is refused and leaves the deck where it was.
    #[test]
    fn beat_jump_refuses_to_leave_the_track() {
        let mut app = ReTraktorApp::default();
        app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(track(120.0)); // 100 s long
        let try_at = |app: &mut ReTraktorApp, at: f64, beats: u32, dir: f64| {
            app.audio.shared.lock().unwrap().playhead_sec[0] = at;
            app.deck_jump_beats[0] = beats;
            (app.beat_jump(0, dir), playhead(app, 0))
        };
        assert_eq!(try_at(&mut app, 10.0, 32, -1.0), (false, 10.0));
        assert_eq!(try_at(&mut app, 2.0, 4, -1.0), (true, 0.0), "landing on the very start is allowed");
        assert_eq!(try_at(&mut app, 84.0, 32, 1.0), (false, 84.0), "the end itself is outside the track");
        let (ok, p) = try_at(&mut app, 83.5, 32, 1.0);
        assert!(ok && (p - 99.5).abs() < 1e-9);

        for (bpm, duration) in [(f64::NAN, 100.0), (0.0, 100.0), (-120.0, 100.0), (f64::INFINITY, 100.0), (120.0, 0.0), (120.0, f64::NAN), (120.0, f64::INFINITY)] {
            assert_eq!(beat_jump_target(10.0, bpm, duration, 4.0, None), None, "bpm {bpm}, duration {duration}");
        }
        let mut bad = track(0.0);
        bad.duration_sec = 100.0;
        app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(bad);
        assert_eq!(try_at(&mut app, 10.0, 4, 1.0), (false, 10.0));
        app.audio.shared.lock().unwrap().loaded_tracks[0] = None;
        assert!(!app.beat_jump(0, 1.0), "an empty deck jumped");
    }

    /// An active loop moves whole with the playhead, and the very next callback plays the new loop,
    /// wrapping across block boundaries at common rates, key lock off and on.
    #[test]
    fn beat_jump_carries_an_active_loop() {
        for (rate, keylock) in [(44_100, false), (48_000, true), (96_000, false)] {
            let mut app = ReTraktorApp::default();
            {
                let mut s = app.audio.shared.lock().unwrap();
                s.loaded_tracks[0] = Some(track(120.0));
                s.playhead_sec[0] = 20.0;
                s.is_playing[0] = true;
                s.keylock[0] = keylock;
            }
            app.toggle_loop(1); // 4 beats = 2 s from 20.0; not yet handed to the audio
            app.audio.shared.lock().unwrap().playhead_sec[0] = 21.995;
            app.deck_jump_beats[0] = 8;
            assert!(app.beat_jump(0, 1.0));
            {
                let s = app.audio.shared.lock().unwrap();
                assert!((s.playhead_sec[0] - 25.995).abs() < 1e-9);
                assert!(s.loop_active[0] && (s.loop_start_sec[0] - 24.0).abs() < 1e-9 && (s.loop_len_sec[0] - 2.0).abs() < 1e-9);
            }
            assert!((app.deck_loop_start_sec[0] - 24.0).abs() < 1e-9, "UI and audio loops differ");
            let mut buf = vec![0.0f32; 512 * 2];
            for _ in 0..(rate as usize / 10 / 512) {
                app.audio.render_f32(&mut buf, 2, rate);
                let p = playhead(&app, 0);
                assert!((24.0..26.0).contains(&p), "{rate} Hz: left the moved loop at {p}");
            }
            assert!(playhead(&app, 0) < 25.0, "{rate} Hz: never wrapped");
        }

        // The whole loop must fit: blocked at either edge, untouched; a loop ending exactly at the end fits.
        let mut app = ReTraktorApp::default();
        app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(track(120.0));
        let setup = |app: &mut ReTraktorApp, start: f64, at: f64| {
            app.deck_loop_active[0] = false;
            app.audio.shared.lock().unwrap().playhead_sec[0] = start;
            app.toggle_loop(1);
            app.audio.shared.lock().unwrap().playhead_sec[0] = at;
        };
        setup(&mut app, 97.5, 98.0);
        assert!(!app.beat_jump(0, 1.0), "the moved loop would run past the end");
        assert_eq!((app.deck_loop_start_sec[0], playhead(&app, 0)), (97.5, 98.0));
        setup(&mut app, 1.0, 2.5);
        assert!(!app.beat_jump(0, -1.0), "the moved loop would start before the track");
        assert_eq!((app.deck_loop_start_sec[0], playhead(&app, 0)), (1.0, 2.5));
        setup(&mut app, 96.0, 97.0);
        assert!(app.beat_jump(0, 1.0));
        assert_eq!((app.deck_loop_start_sec[0], playhead(&app, 0)), (98.0, 99.0));
    }

    /// A playing beat jump from a tone into the same tone at opposite polarity (its left and right are
    /// opposite too), heard at the master output. Unsmoothed, the seam steps by the whole swing. Smoothed,
    /// no sample-to-sample step on either channel may exceed twice the tone's own. That holds at 44.1, 48
    /// and 96 kHz, in 64, 333 and 512-frame blocks, with stems, and for a second jump back that comes
    /// mid-ease. Afterwards the output is the destination audio, exactly as an unsmoothed jump plays
    /// it, and the playhead keeps exact time. Key lock's own crossfade is left alone. A paused jump
    /// doesn't change what plays when the deck starts.
    #[test]
    fn beat_jump_seam_eases_instead_of_stepping() {
        const AMP: f32 = 0.25;
        let tone_track = |stems: bool| {
            let mut t = track(120.0);
            t.duration_sec = 40.0;
            // 441 Hz: exactly 100 samples a period, so the 16 s jump lands on the same phase. From 20 s on, polarity flips.
            t.pcm_stereo = (0..40 * 44_100)
                .flat_map(|i| {
                    let v = AMP * (i as f32 * std::f32::consts::TAU / 100.0).sin() * if i < 20 * 44_100 { 1.0 } else { -1.0 };
                    [v, -v]
                })
                .collect::<Vec<_>>()
                .into();
            if stems {
                let quarter: Vec<f32> = t.pcm_stereo.iter().map(|v| v * 0.25).collect();
                (t.pcm_drums, t.pcm_bass, t.pcm_other, t.pcm_vocals) = (Some(quarter.clone().into()), Some(quarter.clone().into()), Some(quarter.clone().into()), Some(quarter.into()));
            }
            t
        };
        // Largest sample-to-sample step per channel in interleaved stereo.
        let max_step = |out: &[f32]| -> [f32; 2] {
            let mut m = [0.0f32; 2];
            for f in 1..out.len() / 2 {
                for c in 0..2 {
                    m[c] = m[c].max((out[2 * f + c] - out[2 * (f - 1) + c]).abs());
                }
            }
            m
        };
        for (rate, block, stems, keylock) in [(44_100u32, 64usize, false, false), (48_000, 333, true, false), (96_000, 512, false, false), (48_000, 512, false, true)] {
            let case = format!("{rate} Hz, {block}-frame blocks, stems {stems}, key lock {keylock}");
            // `smoothed` jumps with the JUMP button's code; the other sets the playhead directly, as before the fix.
            let run = |smoothed: bool| -> (Vec<f32>, usize, f64) {
                let mut app = ReTraktorApp::default();
                let lead = (0.02 * rate as f64) as usize / block + 1; // blocks before the first jump
                {
                    let mut s = app.audio.shared.lock().unwrap();
                    s.loaded_tracks[0] = Some(tone_track(stems));
                    // Arrive at the jump on a crest (a quarter period), where flipping polarity steps furthest.
                    s.playhead_sec[0] = 10.0 + 25.0 / 44_100.0 - (lead * block) as f64 / rate as f64;
                    s.is_playing[0] = true;
                    s.keylock[0] = keylock;
                }
                app.deck_jump_beats[0] = 32;
                let mut out = Vec::new();
                let mut buf = vec![0.0f32; block * 2];
                let mut play = |app: &ReTraktorApp, blocks: usize, out: &mut Vec<f32>| {
                    for _ in 0..blocks {
                        app.audio.render_f32(&mut buf, 2, rate);
                        out.extend_from_slice(&buf);
                    }
                };
                let jump = |app: &mut ReTraktorApp, dir: f64| {
                    if smoothed {
                        assert!(app.beat_jump(0, dir));
                    } else {
                        app.audio.shared.lock().unwrap().seek_by(0, dir * 16.0);
                    }
                };
                play(&app, lead, &mut out);
                let seam = out.len() / 2;
                jump(&mut app, 1.0);
                play(&app, 1, &mut out);
                jump(&mut app, -1.0); // mid-ease in 64-frame blocks (the ease is 3 ms)
                play(&app, (0.05 * rate as f64) as usize / block + 1, &mut out);
                let end = app.audio.shared.lock().unwrap().playhead_sec[0];
                (out, seam, end)
            };
            let (eased, seam, eased_end) = run(true);
            let (stepped, _, stepped_end) = run(false);
            assert_eq!(eased[..2 * seam], stepped[..2 * seam], "{case}: the audio before the jump changed");
            assert_eq!(eased_end.to_bits(), stepped_end.to_bits(), "{case}: the playhead's timing changed");
            let frames = eased.len() / 2;
            let expected_end = 10.0 + 25.0 / 44_100.0 + (frames - seam) as f64 / rate as f64;
            assert!((eased_end - expected_end).abs() < 1e-9, "{case}: playhead at {eased_end}, expected {expected_end}");

            let tone = max_step(&eased[2 * (seam - (0.01 * rate as f64) as usize)..2 * seam]);
            let window = 2 * (seam - 1)..2 * (seam + (0.01 * rate as f64) as usize);
            let (eased_seam, stepped_seam) = (max_step(&eased[window.clone()]), max_step(&stepped[window]));
            eprintln!(
                "seam {case}: tone step L {:.4} R {:.4} | unsmoothed L {:.4} R {:.4} ({:.1}x) | smoothed L {:.4} R {:.4} ({:.2}x)",
                tone[0], tone[1], stepped_seam[0], stepped_seam[1], stepped_seam[0] / tone[0], eased_seam[0], eased_seam[1], eased_seam[0] / tone[0]
            );
            for c in 0..2 {
                assert!(eased_seam[c] <= 2.0 * tone[c], "{case}: channel {c} steps {} across the seam, the tone itself {}", eased_seam[c], tone[c]);
            }
            if keylock {
                assert_eq!(eased, stepped, "{case}: key lock already crossfades a jump; the ease must stay out of it");
            } else {
                assert!(stepped_seam[0] > 5.0 * tone[0], "{case}: this signal should crack without the ease");
                // 20 ms after the second jump the ease is long over: the destination plays as an unsmoothed jump plays it.
                let settled = 2 * (seam + (0.025 * rate as f64) as usize);
                let off = eased[settled..].iter().zip(&stepped[settled..]).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
                assert!(off < 1e-4, "{case}: still {off} off the destination audio");
            }
        }

        // Paused: a jump moves the playhead only, so starting play sounds as if it had been set directly.
        let paused = |smoothed: bool| {
            let mut app = ReTraktorApp::default();
            app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(tone_track(false));
            app.deck_jump_beats[0] = 32;
            if smoothed {
                assert!(app.beat_jump(0, 1.0));
            } else {
                app.audio.shared.lock().unwrap().playhead_sec[0] = 16.0;
            }
            app.audio.shared.lock().unwrap().is_playing[0] = true;
            let mut buf = vec![0.0f32; 256 * 2];
            app.audio.render_f32(&mut buf, 2, 48_000);
            buf
        };
        assert_eq!(paused(true), paused(false), "a paused jump changed the audio");
    }

    /// A playing jump leaves an ease pending, then with no audio callback in between: stop, two paused
    /// jumps and play again, or a new track and play. The deck must start exactly as if its playhead or
    /// track had simply been set, with no ease from the frame heard before.
    #[test]
    fn stop_paused_jump_and_new_track_drop_a_pending_ease() {
        // L +0.25 and R -0.25 until `flip` seconds, then the opposite.
        let flat = |flip: f64| {
            let mut t = track(120.0);
            t.duration_sec = 40.0;
            t.pcm_stereo = (0..40 * 44_100).flat_map(|i| { let v = if (i as f64) < flip * 44_100.0 { 0.25 } else { -0.25 }; [v, -v] }).collect::<Vec<_>>().into();
            t
        };
        let first_block = |exercise: bool, replace: bool| {
            let mut app = ReTraktorApp::default();
            {
                let mut s = app.audio.shared.lock().unwrap();
                s.loaded_tracks[0] = Some(flat(20.0));
                s.playhead_sec[0] = 10.0;
                s.is_playing[0] = true;
            }
            app.deck_jump_beats[0] = 32;
            let mut buf = vec![0.0f32; 256 * 2];
            app.audio.render_f32(&mut buf, 2, 48_000);
            match (exercise, replace) {
                (true, false) => {
                    assert!(app.beat_jump(0, 1.0)); // to the flipped half; the ease is never rendered
                    app.toggle_play(1);
                    assert!(app.beat_jump(0, -1.0) && app.beat_jump(0, 1.0));
                    app.toggle_play(1);
                }
                (false, false) => app.audio.shared.lock().unwrap().seek_by(0, 16.0),
                (true, true) => {
                    assert!(app.beat_jump(0, 1.0));
                    let entry = crate::library_ui::UiTrackEntry { id: 7, ..Default::default() };
                    app.load_channel.0.send(DeckResult::Loaded(0, app.load_generation[0], entry, Ok(flat(0.0)))).unwrap();
                    app.finish_loads();
                    app.toggle_play(1);
                }
                (false, true) => {
                    app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(flat(0.0));
                    app.audio.shared.lock().unwrap().seek_to(0, 0.0);
                }
            }
            assert!(app.audio.shared.lock().unwrap().is_playing[0]);
            app.audio.render_f32(&mut buf, 2, 48_000);
            buf
        };
        assert_eq!(first_block(true, false), first_block(false, false), "stop and paused jumps left the pending ease");
        assert_eq!(first_block(true, true), first_block(false, true), "a new track played an ease from the old one");
    }

    /// Stems finished for deck A's and B's load go only where that same load still plays: B, reloaded
    /// meanwhile with a same-named track of the same length, keeps its own audio.
    #[test]
    fn stems_install_only_on_the_load_they_were_made_for() {
        let dir = std::env::temp_dir().join(format!("retraktor-stems-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["drums", "bass", "other", "vocals"] {
            AudioSystem::write_stereo_wav_file(&dir.join(format!("{name}.wav")), &[1000i16; 2 * 4410], 44_100).unwrap();
        }
        let load = || crate::audio::LoadedTrackAudio { pcm_stereo: Arc::new(vec![0.0; 2 * 4410]), duration_sec: 0.1, ..track(120.0) };
        let app = ReTraktorApp::default();
        let (a, b) = (load(), load());
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks[0] = Some(a.clone());
            s.loaded_tracks[1] = Some(b.clone());
        }
        app.audio.shared.lock().unwrap().loaded_tracks[1] = Some(load()); // same title, artist and length
        install_stems(app.audio.shared.clone(), vec![(0, a), (1, b)], dir.clone()).join().unwrap();
        let s = app.audio.shared.lock().unwrap();
        let (deck_a, deck_b) = (s.loaded_tracks[0].as_ref().unwrap(), s.loaded_tracks[1].as_ref().unwrap());
        assert!(deck_a.stems_ready && deck_a.pcm_drums.as_ref().is_some_and(|d| d.len() == 2 * 4410), "deck A didn't get its stems");
        assert!(!deck_b.stems_ready && deck_b.pcm_drums.is_none(), "the reloaded deck B got stems made for its old load");
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The UI holds the audio lock for 12 callbacks (32 ms at 48 kHz / 128) while it stops deck B with a
    /// stop-play-stop burst and loads a new track on paused deck D. Meanwhile A (+6 %) and B (-4 %, key lock,
    /// cued to the headphones on 3-4) keep moving and the mix is recorded. Right after, A beat-jumps from the
    /// stale playhead the UI saw; later B starts again; then the output is reopened. Against a run where the
    /// lock is never busy and each action happens at the block the audio picks it up: every block, in f32 and
    /// in i16, plays the same (bitwise before the jump, to float rounding after), every playhead and play
    /// state matches, the streamed WAV is exactly the main output with no block missing (through the hold and
    /// the reopen), and deck D's old audio is freed by the UI thread, not the callback. The tracklist logs A and
    /// B, then B again when it restarts, at the recording's own clock: frames recorded, not wall time.
    #[test]
    fn audio_plays_on_while_the_ui_holds_the_lock() {
        const BLOCK: usize = 128;
        const CH: usize = 4;
        const HOLD: usize = 12;
        let tone = |period: f32, amp: f32, title: &str| {
            let mut t = track(120.0);
            t.title = title.into();
            t.duration_sec = 40.0;
            t.pcm_stereo = Arc::new((0..40 * 44_100).flat_map(|i| { let v = amp * (i as f32 * std::f32::consts::TAU / period).sin(); [v, 0.5 * v] }).collect());
            t
        };
        struct Run { out: Vec<f32>, take: Vec<i16>, playheads: [f64; 4], playing: [bool; 4], busy: u64, jump_block: usize }
        // What `update` logs each UI frame while recording, with the crossfader in the middle.
        let log_frame = |app: &mut ReTraktorApp| {
            let at = app.recording.as_ref().unwrap().seconds();
            let s = app.audio.shared.lock().unwrap();
            for d in 0..2 {
                let title = s.loaded_tracks[d].as_ref().map_or("", |t| t.title.as_str());
                app.set_state.check_audible_track(d, s.is_playing[d], 1.0, 0.5, "", title, at);
            }
        };
        let run = |contended: bool, to_i16: bool| -> Run {
            let mut app = ReTraktorApp::default();
            // The routing this test hears, whatever this machine has saved.
            app.preferences.split_cue = false;
            app.preferences.output_pairs.clear();
            {
                let mut s = app.audio.shared.lock().unwrap();
                s.loaded_tracks[0] = Some(tone(100.0, 0.3, "A"));
                s.loaded_tracks[1] = Some(tone(37.0, 0.2, "B"));
                s.loaded_tracks[3] = Some(tone(53.0, 0.2, "D"));
                s.playhead_sec = [10.0, 5.0, 0.0, 1.0];
                s.is_playing = [true, true, false, false];
            }
            let dir = super::tests::recording_test_dir(&format!("hold-{contended}-{to_i16}"));
            app.start_recording(&dir);
            let (wav, cue, txt) = app.recording.as_ref().map(|r| (r.wav.clone(), r.cue.clone(), r.txt.clone())).unwrap();
            log_frame(&mut app);
            app.deck_pitch_fader = [6.0, -4.0, 0.0, 0.0];
            app.deck_keylock[1] = true;
            app.channel_cue[1] = true;
            app.sync_audio_state();
            let old_d = Arc::downgrade(&app.audio.shared.lock().unwrap().loaded_tracks[3].as_ref().unwrap().pcm_stereo);
            let mut out = Vec::new();
            let play = |app: &ReTraktorApp, blocks: usize, out: &mut Vec<f32>| {
                for _ in 0..blocks {
                    if to_i16 {
                        let mut buf = [0i16; BLOCK * CH];
                        app.audio.render_with(&mut buf, CH, 48_000, crate::audio::pcm_i16);
                        out.extend(buf.iter().map(|&v| v as f32));
                    } else {
                        let mut buf = [0.0f32; BLOCK * CH];
                        app.audio.render_f32(&mut buf, CH, 48_000);
                        out.extend_from_slice(&buf);
                    }
                }
            };
            let ui_actions = |s: &mut crate::audio::AudioSharedState| {
                for playing in [false, true, false] {
                    s.is_playing[1] = playing;
                    s.reset_jump_ease(1);
                }
                s.loaded_tracks[3] = Some(tone(29.0, 0.2, "D2"));
                s.seek_to(3, 0.0);
            };
            play(&app, 20, &mut out);
            if contended {
                let mut held = app.audio.shared.lock().unwrap();
                ui_actions(&mut held);
                play(&app, HOLD, &mut out);
            } else {
                play(&app, HOLD, &mut out);
                ui_actions(&mut app.audio.shared.lock().unwrap());
            }
            let jump_block = out.len() / (BLOCK * CH);
            app.deck_jump_beats[0] = 8;
            assert!(app.beat_jump(0, 1.0));
            play(&app, 1, &mut out);
            assert!(old_d.upgrade().is_some(), "the callback still had deck D's old audio");
            app.sync_audio_state();
            assert!(old_d.upgrade().is_none(), "the UI frame didn't free deck D's old audio");
            log_frame(&mut app); // B is stopped
            play(&app, 30, &mut out);
            app.toggle_play(2);
            play(&app, 30, &mut out);
            let restart = app.recording.as_ref().unwrap().seconds();
            assert_eq!(restart, (out.len() / CH) as f64 / 48_000.0, "the recording clock isn't the frames recorded");
            log_frame(&mut app);

            // Reopen: the new callback state carries on from the published playheads.
            let before = app.audio.shared.lock().unwrap().playhead_sec;
            app.audio.open("", 48_000, BLOCK as u32, app.preferences.output_route());
            play(&app, 1, &mut out);
            let s = app.audio.shared.lock().unwrap();
            for (d, rate) in [(0, 1.06), (1, 0.96)] {
                let moved = s.playhead_sec[d] - before[d];
                assert!((moved - rate * BLOCK as f64 / 48_000.0).abs() < 1e-9, "deck {d} moved {moved} across the reopen");
            }
            drop(s);
            app.stop_recording();
            assert!(super::tests::poll_until_finished(&mut app), "{}", app.library.status_message);
            let bytes = std::fs::read(&wav).unwrap();
            assert_eq!(bytes[..80], crate::recorder::wav_header((bytes.len() as u64 - 80) / 4, 48_000));
            let take = bytes[80..].chunks(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
            let entries = [(0.0, "", "A"), (0.0, "", "B"), (restart, "", "B")];
            let wav_name = wav.file_name().unwrap().to_str().unwrap();
            assert_eq!(std::fs::read_to_string(&cue).unwrap(), crate::setlist::cue_sheet(&entries, wav_name));
            assert_eq!(std::fs::read_to_string(&txt).unwrap(), crate::setlist::plain_tracklist(&entries));
            std::fs::remove_dir_all(&dir).unwrap();
            let s = app.audio.shared.lock().unwrap();
            Run { out, take, playheads: s.playhead_sec, playing: s.is_playing, busy: app.audio.stats.busy_blocks.load(std::sync::atomic::Ordering::Relaxed), jump_block }
        };
        for to_i16 in [false, true] {
            let (busy, calm) = (run(true, to_i16), run(false, to_i16));
            let case = if to_i16 { "i16" } else { "f32" };
            assert_eq!((busy.busy, calm.busy), (HOLD as u64, 0), "{case}: blocks played without the lock");
            assert_eq!(busy.out.len(), calm.out.len());
            let seam = busy.jump_block * BLOCK * CH;
            assert_eq!(busy.out[..seam], calm.out[..seam], "{case}: the held blocks didn't play as uncontended ones");
            let held = &busy.out[20 * BLOCK * CH..seam];
            assert!(held.iter().step_by(CH).any(|v| v.abs() > 0.05) && held.iter().skip(2).step_by(CH).any(|v| v.abs() > 0.05), "{case}: silent while held");
            let tolerance = if to_i16 { 1.0 } else { 1e-4 };
            let off = busy.out.iter().zip(&calm.out).fold(0.0f32, |m, (a, b)| m.max((a - b).abs()));
            assert!(off <= tolerance, "{case}: {off} off the uncontended output after the jump");
            for d in 0..4 {
                assert!((busy.playheads[d] - calm.playheads[d]).abs() < 1e-9, "{case}: deck {d} at {} vs {}", busy.playheads[d], calm.playheads[d]);
            }
            assert_eq!(busy.playing, [true, true, false, false], "{case}: transport");
            assert_eq!(busy.playing, calm.playing);
            for run in [&busy, &calm] {
                let main: Vec<i16> = run.out.chunks(CH).flat_map(|f| if to_i16 { [f[0] as i16, f[1] as i16] } else { [crate::audio::pcm_i16(f[0]), crate::audio::pcm_i16(f[1])] }).collect();
                assert_eq!(run.take, main, "{case}: the recording isn't the main output");
            }
        }
    }

    /// The beat strip keeps one shape whatever phrase it names. Deck A walks through INTRO · 10,
    /// BREAKDOWN · 3, DROP · 30, BREAKDOWN · 88, an exceptional BREAKDOWN · 205 and OUTRO in Mix, 4-Deck and
    /// Timeline, at the default window (1280 x 820, as in the screenshots) and the smallest (1024 x 640).
    /// Deck A's readouts and JUMP row, and deck B's beyond the mixer, never move. The phrase slot keeps its
    /// rectangle inside the deck display, every realistic label fits whole, and the cut-off long one still
    /// gives its full text to screen readers.
    #[test]
    fn phrase_label_never_moves_the_deck() {
        use retraktor_library::{Phrase, PhraseKind::*};
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let mut app = ReTraktorApp::default();
        let mut a = track(120.0); // 2 s bars
        a.duration_sec = 800.0;
        a.phrases = [(0.0, Intro), (20.0, Build), (40.0, Breakdown), (46.0, Drop), (106.0, Breakdown), (282.0, Drop), (290.0, Breakdown), (700.0, Outro)]
            .map(|(start_sec, kind)| Phrase { start_sec, kind })
            .to_vec();
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks[0] = Some(a);
            s.loaded_tracks[1] = Some(track(120.0));
        }
        let stops = [(1.0, "INTRO · 10"), (40.5, "BREAKDOWN · 3"), (47.0, "DROP · 30"), (106.5, "BREAKDOWN · 88"), (290.5, "BREAKDOWN · 205"), (701.0, "OUTRO")];
        let font = crate::theme::dot_font(9.0);
        let text_w = |text: &str| ctx.fonts(|f| f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE).size().x);
        let mut time = 0.0;
        for (w, h) in [(1280.0, 820.0), (1024.0, 640.0)] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(w, h));
            for layout in [TraktorLayout::Parallel, TraktorLayout::FourDeck, TraktorLayout::Timeline] {
                app.layout = layout;
                // Probes: deck A's display readouts and JUMP row, and deck B's beyond the mixer (they move if deck A or the
                // mixer grows). Same-named ones (each deck's BPM) in screen order.
                let probes = |nodes: &[Node]| {
                    let mut p: Vec<(String, egui::Rect)> = nodes
                        .iter()
                        .filter(|n| ["Deck A beat jump", "Deck B beat jump", "BPM ", "KEY ", "IN KEY "].iter().any(|s| n.0.starts_with(s)))
                        .map(|n| (n.0.clone(), n.2))
                        .collect();
                    p.sort_by(|a, b| (&a.0, a.1.min.x, a.1.min.y).partial_cmp(&(&b.0, b.1.min.x, b.1.min.y)).unwrap());
                    p
                };
                let mut first: Option<(Vec<(String, egui::Rect)>, egui::Rect)> = None;
                for (at, text) in stops {
                    let case = format!("{w} x {h} {}, {text}", layout.label());
                    app.audio.shared.lock().unwrap().playhead_sec[0] = at;
                    let mut nodes = Vec::new();
                    for _ in 0..6 {
                        time += 0.1;
                        nodes = workspace_frame(&ctx, &mut app, screen, time, vec![]);
                    }
                    let now = probes(&nodes);
                    let slot = nodes.iter().find(|n| n.0.starts_with("Deck A phrase")).unwrap_or_else(|| panic!("{case}: no phrase node")).clone();
                    let (base, first_slot) = first.get_or_insert_with(|| (now.clone(), slot.2));
                    assert!(now.iter().any(|p| p.0.starts_with("BPM ")) && (layout == TraktorLayout::Timeline || now.iter().any(|p| p.0.starts_with("Deck B beat jump"))), "{case}: probes missing");
                    assert_eq!(&now, base, "{case}: a deck or mixer control moved");
                    assert_eq!(slot.0, format!("Deck A phrase {text}"), "{case}");
                    assert_eq!(slot.2, *first_slot, "{case}: the phrase slot changed size or place");
                    let remain = nodes
                        .iter()
                        .filter(|n| n.0.starts_with("REMAIN") && n.2.top() > slot.2.top() && n.2.x_range().intersects(slot.2.x_range()))
                        .min_by(|p, q| p.2.top().total_cmp(&q.2.top()))
                        .unwrap_or_else(|| panic!("{case}: no REMAIN under the phrase"));
                    assert!(slot.2.right() <= remain.2.right() + 0.5 && screen.contains_rect(slot.2), "{case}: phrase slot {:?} leaves the display (REMAIN ends at {})", slot.2, remain.2.right());
                    // Laid out as `beat_strip` draws it: realistic labels whole, the 3-digit countdown cut with an ellipsis.
                    let mut job = egui::text::LayoutJob::simple_singleline(text.to_string(), font.clone(), Color32::WHITE);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(slot.2.width());
                    let cut = ctx.fonts(|f| f.layout_job(job)).elided;
                    assert_eq!(cut, text.ends_with("205"), "{case}: {text} is {} px in a {} px slot", text_w(text), slot.2.width());
                }
                let (base, slot) = first.unwrap();
                eprintln!("geometry {w} x {h} {}: {} probes fixed across {} phrases, phrase slot {:.1} x {:.1} px", layout.label(), base.len(), stops.len(), slot.width(), slot.height());
            }
        }
    }

    /// A library of six tracks (1: a 0.3 s 44.1 kHz WAV, 2: a 0.2 s 48 kHz WAV, 3: a file that isn't audio,
    /// 4: a missing file, 5: a 3 s 44.1 kHz WAV at 300 Hz then 3 kHz from halfway, 6: a 2 s 48 kHz WAV at 440 Hz),
    /// none analysed, both decks loaded and playing, on a four-channel output with headphones on 3–4.
    fn preview_app(name: &str) -> (PathBuf, ReTraktorApp) {
        let dir = std::env::temp_dir().join(format!("retraktor-preview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let tone = |file: &str, hz: f32, rate: u32, secs: f32| {
            let n = (rate as f32 * secs) as usize;
            let samples: Vec<i16> = (0..n)
                .flat_map(|i| {
                    // Track 5's second half is ten times higher: a passage a listener (and a test) can tell apart.
                    let hz = if file == "five.wav" && i >= n / 2 { hz * 10.0 } else { hz };
                    let v = ((i as f32 * hz * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
                    [v, v]
                })
                .collect();
            AudioSystem::write_stereo_wav_file(&dir.join(file), &samples, rate).unwrap();
            dir.join(file)
        };
        let files = [
            tone("one.wav", 500.0, 44_100, 0.3),
            tone("two.wav", 800.0, 48_000, 0.2),
            dir.join("bad.wav"),
            dir.join("gone.wav"),
            tone("five.wav", 300.0, 44_100, 3.0),
            tone("six.wav", 440.0, 48_000, 2.0),
        ];
        std::fs::write(&files[2], b"not audio").unwrap();
        let mut app = ReTraktorApp::default();
        app.library = LibraryManager::new(dir.join("collection.nml"));
        for (i, path) in files.into_iter().enumerate() {
            let title = format!("Track {}", i + 1);
            app.library.tracks.push(crate::library_ui::UiTrackEntry { id: i + 1, title, file_path: Some(path), ..Default::default() });
        }
        app.library.rebuild_index();
        app.preferences.split_cue = false;
        app.preferences.output_pairs.clear(); // main 1–2, headphones 3–4, whatever this machine has saved
        app.audio.output_channels = 4;
        {
            let mut s = app.audio.shared.lock().unwrap();
            s.loaded_tracks[0] = Some(track(120.0));
            s.loaded_tracks[1] = Some(track(126.0));
            s.is_playing = [true, true, false, false];
        }
        app.deck_track_ids = [Some(90), Some(91), None, None];
        app.sync_audio_state();
        (dir, app)
    }

    /// Latency harness, not a regression test (no timing is asserted). For each file in `RETRAKTOR_LATENCY_DIR`
    /// (`three-min.mp3`, `three-min.wav`), three fresh apps each time: Preview press to the first preview samples in
    /// the headphones, the same press again in that app, a cold start three quarters in through the overview's click
    /// path (asserting that the fixture's 1 kHz lead, from 2:00, is what is heard), and a queued deck load until the deck
    /// is playable: after that seeked preview (not exact, so the file is decoded), after ordinary browsing (the straight
    /// overview decode reused), with BPM/key missing, tagged, and the tagged track reloaded; plus when the missing BPM
    /// arrives. Prints medians.
    /// `RETRAKTOR_LATENCY_DIR=/tmp/retraktor-wave-fixtures cargo test -p retraktor-gui --locked latency_harness -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn latency_harness() {
        use std::time::{Duration, Instant};
        let Ok(dir) = std::env::var("RETRAKTOR_LATENCY_DIR") else { return };
        let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
        // One real block every 5 ms, as a 256-frame callback would; a tight loop would starve the decoder.
        let heard_where = |app: &mut ReTraktorApp, t: Instant| -> (f64, f32) {
            let mut buf = vec![0.0f32; 256 * 4];
            loop {
                app.poll_preview();
                app.audio.render_with(&mut buf, 4, 48_000, |x| x);
                if let Some(from) = buf.chunks(4).position(|f| f[2].abs() > 1e-4) {
                    let left: Vec<f32> = buf.chunks(4).skip(from + 1).map(|f| f[2]).collect();
                    let slope: f32 = left.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
                    return (ms(t), slope / left.iter().map(|v| v * v).sum::<f32>().max(1e-9));
                }
                assert!(t.elapsed() < Duration::from_secs(300) && app.preview.is_some(), "the preview never played: {}", app.library.status_message);
                std::thread::sleep(Duration::from_millis(5));
            }
        };
        let heard = |app: &mut ReTraktorApp, t: Instant| heard_where(app, t).0;
        for file in ["three-min.mp3", "three-min.wav"] {
            let path = PathBuf::from(&dir).join(file);
            assert!(path.is_file(), "no fixture at {}", path.display());
            let mut rows: Vec<(&str, Vec<f64>)> = Vec::new();
            let note = |rows: &mut Vec<(&str, Vec<f64>)>, what: &'static str, v: f64| match rows.iter_mut().find(|r| r.0 == what) {
                Some(r) => r.1.push(v),
                None => rows.push((what, vec![v])),
            };
            for _ in 0..3 {
                let mut app = ReTraktorApp::default();
                app.library = LibraryManager::new(std::env::temp_dir().join(format!("retraktor-latency-{}/c.nml", std::process::id())));
                for (id, bpm, key) in [(1, 0.0, ""), (2, 124.0, "8m")] {
                    let title = format!("Latency {id}");
                    app.library.tracks.push(crate::library_ui::UiTrackEntry { id, title, file_path: Some(path.clone()), bpm, musical_key: key.into(), ..Default::default() });
                }
                app.library.rebuild_index();
                app.preferences.split_cue = false;
                app.audio.output_channels = 4;
                app.sync_audio_state();

                let t = Instant::now();
                app.press_preview(Some(1));
                let (when, bright_start) = heard_where(&mut app, t);
                note(&mut rows, "preview: press -> first samples", when);
                note(&mut rows, "preview: 0% brightness x1000, not ms (reference, no lead)", bright_start as f64 * 1000.0);
                app.press_preview(None);
                let t = Instant::now();
                app.press_preview(Some(1));
                note(&mut rows, "preview: same track again", heard(&mut app, t));
                app.press_preview(None);
                app.library.selected_track_id = Some(2); // selected, as when clicking its overview: its decode goes on after a stop
                let t = Instant::now();
                app.click_overview(2, 0.75); // track 2: the same file, not decoded yet for it
                let (when, bright) = heard_where(&mut app, t);
                note(&mut rows, "preview: cold start at 75% -> first samples", when);
                note(&mut rows, "preview: 75% brightness x1000, not ms (1 kHz lead ~3, start ~0)", bright as f64 * 1000.0);
                assert!(bright > 1e-3 && bright > 3.0 * bright_start, "{file}: the cold 75% start isn't the 1 kHz passage ({bright} against {bright_start} at the start)");
                app.press_preview(None);

                // Each deck load waits for what it is measured after, with nothing else decoding meanwhile.
                let complete = |app: &mut ReTraktorApp, what: &str| {
                    let t = Instant::now();
                    while !app.prepared_for(2).is_some_and(|p| p.complete()) {
                        assert!(t.elapsed() < Duration::from_secs(300), "{what} never completed");
                        std::thread::sleep(Duration::from_millis(5));
                        app.poll_preview();
                    }
                    ms(t)
                };
                note(&mut rows, "preview: rest of the file decoded (after the 75% start)", complete(&mut app, "the 75% decode"));
                assert!(!app.prepared_for(2).unwrap().exact(), "a seeked decode counted as exact");
                let loads = [
                    ("deck: tagged after a 75% preview (seeked, so decodes the file) -> playable", 2, false),
                    ("deck: tagged after ordinary browsing (straight overview decode reused) -> playable", 2, true),
                    ("deck: BPM/key missing -> playable", 1, false),
                    ("deck: tagged -> playable", 2, false),
                    ("deck: tagged reload -> playable", 2, false),
                ];
                for (n, (what, id, browsed)) in loads.into_iter().enumerate() {
                    if browsed {
                        app.prepared.clear(); // as if track 2 were selected afresh and had rested
                        app.browse_seen = (Some(2), Instant::now() - BROWSE_SETTLE);
                        app.browse_looked = None;
                        note(&mut rows, "overview: selected track decoded straight through", complete(&mut app, "the overview decode"));
                        assert!(app.prepared_for(2).unwrap().exact(), "a straight overview decode isn't exact");
                    } else if n > 0 {
                        app.library.selected_track_id = None;
                        app.prepared.clear(); // decode from the file, as before
                        app.poll_preview();
                    }
                    let track = app.library.track(id).cloned().unwrap();
                    let t = Instant::now();
                    app.library.pending_loads.push((1, track));
                    app.drain_pending_loads();
                    while app.deck_loading[0] {
                        assert!(t.elapsed() < Duration::from_secs(300), "the deck never loaded");
                        std::thread::sleep(Duration::from_millis(2));
                        app.finish_loads();
                    }
                    assert!(app.deck_load_failed[0].is_none(), "{}", app.library.status_message);
                    note(&mut rows, what, ms(t));
                    if id == 1 {
                        while app.audio.shared.lock().unwrap().loaded_tracks[0].as_ref().map_or(true, |l| l.bpm <= 0.0) {
                            assert!(t.elapsed() < Duration::from_secs(300), "the BPM never arrived");
                            std::thread::sleep(Duration::from_millis(2));
                            app.finish_loads();
                        }
                        note(&mut rows, "deck: BPM/key missing -> BPM known", ms(t));
                    }
                }
            }
            for (what, mut v) in rows {
                v.sort_by(f64::total_cmp);
                println!("LATENCY {file} | {what} | median {:.0} ms | runs {:?}", v[v.len() / 2], v.iter().map(|x| x.round() as i64).collect::<Vec<_>>());
            }
        }
    }

    /// Polls like the GUI until the preview's file is decoded (or it failed and was stopped).
    fn settle_preview(app: &mut ReTraktorApp) {
        let t0 = std::time::Instant::now();
        while app.preview.as_ref().is_some_and(|p| !p.pcm.complete()) {
            assert!(t0.elapsed() < std::time::Duration::from_secs(10), "the preview never finished loading");
            std::thread::sleep(std::time::Duration::from_millis(5));
            app.poll_preview();
        }
    }

    fn previewing(app: &ReTraktorApp) -> Option<usize> {
        app.preview.as_ref().map(|p| p.track)
    }

    /// Peaks of the main (1–2) and headphone (3–4) outputs over `blocks` 256-frame 48 kHz blocks of the real callback.
    fn output_peaks(app: &ReTraktorApp, blocks: usize) -> (f32, f32) {
        let (mut main, mut phones, mut buf) = (0.0f32, 0.0f32, vec![0.0f32; 256 * 4]);
        for _ in 0..blocks {
            app.audio.render_with(&mut buf, 4, 48_000, |x| x);
            for f in buf.chunks(4) {
                main = main.max(f[0].abs()).max(f[1].abs());
                phones = phones.max(f[2].abs()).max(f[3].abs());
            }
        }
        (main, phones)
    }

    /// Whatever the preview did, the decks, the history and the set are as they were.
    fn decks_untouched(app: &ReTraktorApp, pcm: &[Arc<Vec<f32>>; 2], set: &[usize]) {
        let s = app.audio.shared.lock().unwrap();
        for d in 0..2 {
            assert!(Arc::ptr_eq(&s.loaded_tracks[d].as_ref().unwrap().pcm_stereo, &pcm[d]), "deck {d} changed track");
        }
        assert_eq!(s.is_playing, [true, true, false, false]);
        drop(s);
        assert_eq!(app.deck_track_ids, [Some(90), Some(91), None, None]);
        assert!(app.library.history.is_empty() && app.library.pending_loads.is_empty() && !app.deck_loading.iter().any(|&l| l));
        assert_eq!(app.set_state.set_list.track_ids, set);
    }

    /// The controller's Preview button (left and right, presses only) and the shared press logic, with both
    /// decks playing: it plays the selected WAV in the headphones and nowhere else; another selection replaces
    /// it at once; the same press stops it. A decode that finishes after a replacement or a stop is never
    /// heard. A preview that runs out ends, so the same track starts again. A file that isn't audio, a missing
    /// file and an output with no headphone route are refused with a message, and losing the route stops it.
    #[test]
    fn library_preview_presses_replace_stop_and_end() {
        let (dir, mut app) = preview_app("presses");
        let pcm = { let s = app.audio.shared.lock().unwrap(); [0, 1].map(|d| Arc::clone(&s.loaded_tracks[d].as_ref().unwrap().pcm_stereo)) };
        let set = app.set_state.set_list.track_ids.clone();
        let press = |app: &mut ReTraktorApp, side: &str, pressed: bool| {
            app.apply_mx2_surface_event(SurfaceEvent::Button { control: format!("surface.{side}.browse.preview"), pressed });
        };
        let playing = previewing;

        // Left Preview on track 1: in the headphones only. A release does nothing.
        app.library.selected_track_id = Some(1);
        press(&mut app, "left", true);
        press(&mut app, "left", false);
        assert_eq!(previewing(&app), Some(1));
        settle_preview(&mut app);
        assert_eq!(playing(&app), Some(1), "{}", app.library.status_message);
        let (main, phones) = output_peaks(&app, 4);
        assert!(phones > 0.1 && main == 0.0, "headphones {phones}, main {main}");

        // Right Preview on track 2 replaces it at once: track 1 is never heard again, 2 plays.
        app.library.selected_track_id = Some(2);
        let first = Arc::clone(&app.preview.as_ref().unwrap().pcm);
        press(&mut app, "right", true);
        assert_eq!(previewing(&app), Some(2));
        let (_, phones) = output_peaks(&app, 1);
        let p = app.audio.shared.lock().unwrap().preview_progress;
        assert!(p.serial == app.preview.as_ref().unwrap().serial && !Arc::ptr_eq(&first, &app.preview.as_ref().unwrap().pcm), "track 1 played on");
        assert!(phones == 0.0 || p.started, "something played before track 2 had started");
        settle_preview(&mut app);
        assert_eq!(playing(&app), Some(2));
        assert!(output_peaks(&app, 2).1 > 0.1);

        // The same press stops it.
        press(&mut app, "right", true);
        assert!(app.preview.is_none() && app.library.status_message.contains("Stopped"), "{}", app.library.status_message);
        assert_eq!(output_peaks(&app, 1).1, 0.0);

        // Replaced while still decoding, and stopped while decoding: whatever lands later is never heard.
        app.press_preview(Some(5));
        app.press_preview(Some(2));
        settle_preview(&mut app);
        assert_eq!(playing(&app), Some(2));
        app.press_preview(Some(5));
        app.press_preview(None);
        std::thread::sleep(std::time::Duration::from_millis(300)); // the worker, if it got that far
        app.poll_preview();
        assert!(app.preview.is_none(), "{:?}", previewing(&app));
        assert_eq!(output_peaks(&app, 2).1, 0.0, "a decode finished after the stop was heard");

        // Track 1 (0.3 s) runs out: the preview ends and the same track starts again from its top.
        app.press_preview(Some(1));
        settle_preview(&mut app);
        output_peaks(&app, 70); // 0.37 s
        app.poll_preview();
        assert!(app.preview.is_none() && app.library.status_message.contains("finished"), "{}", app.library.status_message);
        assert_eq!(output_peaks(&app, 1).1, 0.0);
        app.press_preview(Some(1));
        settle_preview(&mut app);
        assert_eq!(playing(&app), Some(1));
        assert!(output_peaks(&app, 2).1 > 0.1, "the restarted preview is silent");

        // A missing file or a track gone from the collection never replaces it: track 1 plays on, and says so.
        for (id, why) in [(4, "File not found for 'Track 4'"), (99, "no longer in the collection")] {
            app.press_preview(Some(id));
            let msg = &app.library.status_message;
            assert!(msg.contains(why) && msg.contains("still previewing 'Track 1'"), "{msg}");
            assert_eq!(playing(&app), Some(1));
        }
        assert!(output_peaks(&app, 1).1 > 0.1, "a refused press silenced the preview");
        app.press_preview(None);

        // Not audio, and missing: refused with a message.
        app.press_preview(Some(3));
        settle_preview(&mut app);
        assert!(app.preview.is_none() && app.library.status_message.contains("Could not preview 'Track 3'"), "{}", app.library.status_message);
        app.press_preview(Some(4));
        assert!(app.preview.is_none() && app.library.status_message.contains("File not found"), "{}", app.library.status_message);

        // A stereo card with headphones on 3–4: refused. Split cue: the left ear. Back to 3–4 on stereo: stopped.
        app.audio.output_channels = 2;
        app.press_preview(Some(1));
        assert!(app.preview.is_none() && app.library.status_message.contains("no free channels 3–4"), "{}", app.library.status_message);
        app.preferences.split_cue = true;
        app.apply_audio_output();
        app.press_preview(Some(1));
        settle_preview(&mut app);
        let mut stereo = vec![0.0f32; 256 * 2];
        app.audio.render_with(&mut stereo, 2, 48_000, |x| x);
        assert!(stereo.chunks(2).any(|f| f[0].abs() > 0.05) && stereo.chunks(2).all(|f| f[1] == 0.0), "split: the preview belongs in the left ear only");
        app.preferences.split_cue = false;
        app.apply_audio_output();
        app.poll_preview();
        assert!(app.preview.is_none() && app.library.status_message.contains("Stopped the preview"), "{}", app.library.status_message);
        app.audio.render_with(&mut stereo, 2, 48_000, |x| x);
        assert!(stereo.iter().all(|&x| x == 0.0), "the preview reached the main output");

        // A route lost while the preview is still decoding: stopped with a message, never played.
        app.audio.output_channels = 4;
        app.press_preview(Some(2));
        app.audio.output_channels = 2;
        app.poll_preview();
        assert!(app.preview.is_none() && app.library.status_message.contains("Stopped the preview of 'Track 2'"), "{}", app.library.status_message);
        assert_eq!(output_peaks(&app, 2).1, 0.0, "the late decode was heard");

        decks_untouched(&app, &pcm, &set);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A routing change goes through the same step `update` runs, and the very first block after it is already
    /// mapped the new way, before any later UI sync, with deck A audible on the main output and a preview live.
    /// Split off on a stereo card reopens the output: the preview never lands in the left speaker (both
    /// speakers carry the same main mix), then it stops with a message. Swapping main and headphone pairs on a
    /// four-channel card, then split on, keep the stream: the preview carries on from where it had got to.
    #[test]
    fn reopened_output_maps_the_preview_on_its_very_first_block() {
        let (dir, mut app) = preview_app("reroute");
        {
            let mut deck = track(120.0);
            deck.duration_sec = 4.0;
            deck.pcm_stereo = Arc::new(
                (0..44_100 * 4)
                    .flat_map(|i| {
                        let v = (i as f32 * 300.0 * std::f32::consts::TAU / 44_100.0).sin() * 0.8;
                        [v, v]
                    })
                    .collect(),
            );
            app.audio.shared.lock().unwrap().loaded_tracks[0] = Some(deck);
        }
        let first_block = |app: &ReTraktorApp, channels: usize| {
            let mut buf = vec![0.0f32; 256 * channels];
            app.audio.render_with(&mut buf, channels, 48_000, |x| x);
            buf
        };
        let progress = |app: &ReTraktorApp| app.audio.shared.lock().unwrap().preview_progress;

        // Split cue on a stereo card: the preview in the left ear, the main mix in the right.
        app.preferences.split_cue = true;
        app.audio.output_channels = 2;
        app.apply_audio_output();
        app.sync_audio_state();
        app.press_preview(Some(1));
        settle_preview(&mut app);
        let split = first_block(&app, 2);
        assert!(split.chunks(2).any(|f| (f[0] - f[1]).abs() > 0.05), "split: the preview isn't in the left ear");

        // Split off, same stereo card: the reopened stream's first block is the main mix on both speakers.
        app.preferences.split_cue = false;
        app.apply_audio_output();
        let first = first_block(&app, 2);
        assert!(first.iter().any(|x| x.abs() > 0.05), "the main mix is silent");
        assert!(first.chunks(2).all(|f| f[0] == f[1]), "the preview reached the left speaker in the new stream's first block");
        app.poll_preview();
        assert!(app.preview.is_none() && app.library.status_message.contains("no free channels 3–4"), "{}", app.library.status_message);

        // 3–4 on a four-channel card, then main and headphones swapped as on a Xone:K2 (main on its rear 3–4,
        // headphones in the front 1–2). The same stream plays on, and its very next block has the preview on
        // 1–2 and deck A on 3–4 only, with the preview carrying on.
        app.audio.output_channels = 4;
        app.press_preview(Some(1));
        settle_preview(&mut app);
        let on_34 = first_block(&app, 4);
        assert!(on_34.chunks(4).any(|f| f[2].abs() > 0.05), "the preview isn't on 3–4");
        let (before, opens) = (progress(&app), app.audio.opens);
        let device = app.preferences.audio_device_name.clone();
        app.preferences.output_pairs.insert(device, [1, 0]);
        app.apply_audio_output();
        let swapped = first_block(&app, 4);
        assert_eq!(app.audio.opens, opens, "swapping the pairs reopened the output");
        // Deck A (main) and the preview (headphones) peak at different levels: the pairs traded places whole.
        let peaks = |block: &[f32]| block.chunks(4).fold([0.0f32; 4], |p, f| std::array::from_fn(|c| p[c].max(f[c].abs())));
        let (was, now) = (peaks(&on_34), peaks(&swapped));
        assert!((was[0] - was[2]).abs() > 0.05 && was[0] == was[1] && was[2] == was[3], "deck A and the preview can't be told apart: {was:?}");
        assert!((now[0] - was[2]).abs() < 0.01 && (now[2] - was[0]).abs() < 0.01 && now[0] == now[1] && now[2] == now[3], "the pairs didn't swap: {was:?} then {now:?}");
        let after = progress(&app);
        assert_eq!((after.serial, after.ended), (before.serial, false));
        assert!((after.frame - before.frame - 256.0 * 44_100.0 / 48_000.0).abs() < 1e-6, "the preview didn't carry on: {before:?} then {after:?}");

        // Then split cue on: 1–2 on the same stream, the preview going on in the left ear; stereo works too.
        let before = progress(&app);
        app.preferences.split_cue = true;
        app.apply_audio_output();
        app.audio.output_channels = 2; // as on a stereo stream
        let first = first_block(&app, 2);
        assert!(first.chunks(2).any(|f| (f[0] - f[1]).abs() > 0.05), "the preview dropped out of the new stream's first block");
        let after = progress(&app);
        assert_eq!((after.serial, after.ended), (before.serial, false));
        assert!((after.frame - before.frame - 256.0 * 44_100.0 / 48_000.0).abs() < 1e-6, "the preview didn't carry on: {before:?} then {after:?}");
        app.poll_preview();
        assert_eq!(previewing(&app), Some(1));

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The output reopens only when it must. A routing the open stream has the channels for, or whose width was
    /// already asked of these settings, plays on the stream as it is: a stereo card or a failed open isn't
    /// retried every frame, Split cue on and off included. A new width the stream lacks reopens once, also back
    /// to a narrower viable one; an eight-channel stream asked for four takes 5–6 and 7–8 live. Rescan tries
    /// again once while nothing plays, and leaves a working output alone.
    #[test]
    fn output_reopens_only_when_a_route_needs_it() {
        let mut app = ReTraktorApp::default();
        app.preferences.split_cue = false;
        app.preferences.output_pairs.clear();
        app.preferences.audio_device_name = "pw:k2".into();
        let set = |app: &mut ReTraktorApp, pairs: Option<[u8; 2]>, split: bool| {
            app.preferences.output_pairs.extend(pairs.map(|p| ("pw:k2".to_string(), p)));
            app.preferences.split_cue = split;
            app.apply_audio_output();
            app.apply_audio_output(); // the next frame
            app.audio.opens
        };
        let first = set(&mut app, None, false);
        app.audio.output_channels = 2; // what a stereo card gives for 1–2 and 3–4
        assert_eq!([set(&mut app, None, true), set(&mut app, None, false)], [first; 2], "a stereo card reopened");
        assert_eq!(set(&mut app, Some([2, 0]), false), first + 1, "main 5–6 is a new width");
        assert_eq!(set(&mut app, Some([1, 0]), false), first + 2, "back to four channels");
        app.audio.output_channels = 8;
        for pairs in [[2, 0], [3, 2], [1, 3]] {
            assert_eq!(set(&mut app, Some(pairs), false), first + 2, "{pairs:?} reopened an eight-channel stream");
        }
        assert_eq!(app.audio.output_route, crate::audio::OutputRoute::Pairs { main: 1, headphones: Some(3) });
        assert_eq!(set(&mut app, Some([1, 0]), false), first + 2);

        app.preferences.audio_rescanned = true;
        assert_eq!(set(&mut app, None, false), first + 2, "Rescan reopened a working output");
        assert!(!app.preferences.audio_rescanned);
        app.audio.output_channels = 0; // the chosen card was missing and nothing opened
        assert_eq!(set(&mut app, None, false), first + 2, "a failed open is retried every frame");
        app.preferences.audio_rescanned = true;
        assert_eq!(set(&mut app, None, false), first + 3, "Rescan didn't try the chosen card again");
    }

    /// The browser in real frames, 1024 px wide: Preview in the toolbar for the selected track (greyed out with
    /// none), lit while that track previews. One Stop preview, in the status bar, from the press on, after the
    /// selection moves to another track, in the Explorer, under a message far wider than the window, and
    /// with the collection emptied; each click on it stops the preview.
    #[test]
    fn preview_toggle_in_the_toolbar_and_one_stop_in_every_browser_view() {
        use egui::accesskit::Role;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1024.0, 1000.0));
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let (dir, mut app) = preview_app("toolbar");
        let pcm = { let s = app.audio.shared.lock().unwrap(); [0, 1].map(|d| Arc::clone(&s.loaded_tracks[d].as_ref().unwrap().pcm_stereo)) };
        let set = app.set_state.set_list.track_ids.clone();
        app.layout = TraktorLayout::Browser;
        let mut time = 0.0;
        let mut frame = |app: &mut ReTraktorApp, events: Vec<egui::Event>| -> Vec<Node> {
            time += 0.1;
            workspace_frame(&ctx, app, screen, time, events)
        };
        let button = |nodes: &[Node], name: &str| nodes.iter().find(|n| n.0 == name && n.1 == Role::Button).cloned();
        // The only Stop preview, and on screen.
        let stop = |nodes: &[Node], when: &str| {
            let stops: Vec<&Node> = nodes.iter().filter(|n| n.0 == "Stop preview" && n.1 == Role::Button).collect();
            assert!(stops.len() == 1 && screen.contains_rect(stops[0].2) && !stops[0].3, "{when}: Stop preview {stops:?}");
            stops[0].2.center()
        };
        let settle = |app: &mut ReTraktorApp, frame: &mut dyn FnMut(&mut ReTraktorApp, Vec<egui::Event>) -> Vec<Node>| {
            (0..4).map(|_| frame(app, vec![])).last().unwrap()
        };
        let click = |app: &mut ReTraktorApp, frame: &mut dyn FnMut(&mut ReTraktorApp, Vec<egui::Event>) -> Vec<Node>, at: egui::Pos2| {
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(app, vec![egui::Event::PointerMoved(at)]);
            frame(app, vec![press(true)]);
            frame(app, vec![press(false)]);
            frame(app, vec![])
        };

        let nodes = settle(&mut app, &mut frame);
        assert!(button(&nodes, "Preview").expect("no Preview button").3, "Preview is enabled with nothing selected");
        assert!(button(&nodes, "Stop preview").is_none(), "Stop preview with nothing previewing");
        app.library.selected_track_id = Some(1);
        let nodes = settle(&mut app, &mut frame);
        let preview = button(&nodes, "Preview").expect("no Preview button");
        assert!(!preview.3 && screen.contains_rect(preview.2), "Preview is greyed out or off screen: {preview:?}");
        let nodes = click(&mut app, &mut frame, preview.2.center());
        assert_eq!(previewing(&app), Some(1));
        stop(&nodes, "loading");
        settle_preview(&mut app);
        stop(&frame(&mut app, vec![]), "playing");

        // Another selection: Preview offers it, and the one Stop preview stops track 1.
        app.library.selected_track_id = Some(2);
        let nodes = frame(&mut app, vec![]);
        assert!(button(&nodes, "Preview").is_some_and(|b| !b.3));
        click(&mut app, &mut frame, stop(&nodes, "after reselecting"));
        assert!(app.preview.is_none(), "{:?}", previewing(&app));
        assert_eq!(output_peaks(&app, 1).1, 0.0);

        // The Explorer, under a message far wider than the window: still one Stop preview, on screen.
        app.press_preview(Some(2));
        settle_preview(&mut app);
        app.library.selected_nav = crate::library_ui::NavItem::Explorer;
        app.library.status_message = "Previewing a track whose title goes on and on ".repeat(12);
        let nodes = settle(&mut app, &mut frame);
        click(&mut app, &mut frame, stop(&nodes, "Explorer, long message"));
        assert!(app.preview.is_none(), "{:?}", previewing(&app));

        // An emptied collection.
        app.library.selected_nav = crate::library_ui::NavItem::Collection;
        app.press_preview(Some(1));
        settle_preview(&mut app);
        app.library.tracks.clear();
        app.library.rebuild_index();
        let nodes = settle(&mut app, &mut frame);
        click(&mut app, &mut frame, stop(&nodes, "empty collection"));
        assert!(app.preview.is_none(), "{:?}", previewing(&app));
        assert_eq!(output_peaks(&app, 1).1, 0.0);

        decks_untouched(&app, &pcm, &set);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// How much of `x` is high frequency (squared slope over energy): about 0.0015 for 300 Hz at 48 kHz, 0.15 for 3 kHz.
    fn brightness(x: &[f32]) -> f32 {
        let slope: f32 = x.windows(2).map(|w| (w[1] - w[0]).powi(2)).sum();
        slope / x.iter().map(|v| v * v).sum::<f32>().max(1e-9)
    }

    /// Renders 256-frame blocks (polling like the GUI) until the preview is heard on 3–4. Returns the left
    /// headphone samples from where they start, and the preview's progress after that block.
    fn first_heard(app: &mut ReTraktorApp) -> (Vec<f32>, crate::audio::PreviewProgress) {
        let t0 = std::time::Instant::now();
        let mut buf = vec![0.0f32; 256 * 4];
        loop {
            app.poll_preview();
            app.audio.render_with(&mut buf, 4, 48_000, |x| x);
            let left: Vec<f32> = buf.iter().skip(2).step_by(4).copied().collect();
            if let Some(from) = left.iter().position(|x| x.abs() > 1e-4) {
                return (left[from + 1..].to_vec(), app.audio.shared.lock().unwrap().preview_progress);
            }
            assert!(t0.elapsed() < std::time::Duration::from_secs(10) && app.preview.is_some(), "the preview was never heard: {}", app.library.status_message);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// The Preview column in real frames, 1024 px wide: every row's waveform sits just left of its title, about
    /// 150 by 18 px. A plain click on track 5's waveform, never selected and with nothing decoded for it yet,
    /// selects it and starts its preview three quarters in (its 3 kHz half); another, a quarter along, moves that
    /// same preview (the 300 Hz half). Track 6, never selected, gets its real waveform from a peaks-only decode, and
    /// files that can't be read are marked once and not tried again. A click halfway along track 6 replaces the
    /// preview there. Double-clicking a waveform never loads a deck; double-clicking a title still does.
    #[test]
    fn row_waveforms_click_to_start_and_move_the_preview() {
        use egui::accesskit::Role;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1024.0, 1000.0));
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let (dir, mut app) = preview_app("rows");
        let pcm = { let s = app.audio.shared.lock().unwrap(); [0, 1].map(|d| Arc::clone(&s.loaded_tracks[d].as_ref().unwrap().pcm_stereo)) };
        let set = app.set_state.set_list.track_ids.clone();
        app.layout = TraktorLayout::Browser;
        let mut time = 0.0;
        let mut frame = |app: &mut ReTraktorApp, events: Vec<egui::Event>| -> Vec<Node> {
            time += 0.1;
            workspace_frame(&ctx, app, screen, time, events)
        };
        // `clicks` plain clicks at one spot, a frame apart (two make a double-click), then a pause.
        let click = |app: &mut ReTraktorApp, frame: &mut dyn FnMut(&mut ReTraktorApp, Vec<egui::Event>) -> Vec<Node>, at: egui::Pos2, clicks: usize| {
            let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(app, vec![egui::Event::PointerMoved(at)]);
            for _ in 0..clicks {
                frame(app, vec![press(true)]);
                frame(app, vec![press(false)]);
            }
            frame(app, vec![]);
            frame(app, vec![]);
        };
        // Track `id`'s waveform and title, on screen and in one row.
        let row = |nodes: &[Node], id: usize| {
            let wave = nodes.iter().find(|n| n.0.starts_with(&format!("Waveform of 'Track {id}'")) && n.1 == Role::Button).unwrap_or_else(|| panic!("no waveform for track {id}"));
            let title = nodes.iter().find(|n| n.0 == format!("Track {id}") && n.1 == Role::Label).unwrap_or_else(|| panic!("no title for track {id}")).2;
            assert!(screen.contains_rect(wave.2) && screen.contains_rect(title), "track {id} is off screen: {:?} {title:?}", wave.2);
            assert!(wave.2.right() <= title.left() + 1.0 && (wave.2.center().y - title.center().y).abs() < 2.0, "track {id}'s waveform isn't just left of its title: {:?} {title:?}", wave.2);
            assert!((140.0..=151.0).contains(&wave.2.width()) && (wave.2.height() - 18.0).abs() < 0.5, "track {id}'s waveform is {:?}", wave.2.size());
            (wave.0.clone(), wave.2)
        };
        let along = |r: egui::Rect, f: f32| egui::pos2(r.left() + f * r.width(), r.center().y);

        let nodes = (0..3).map(|_| frame(&mut app, vec![])).last().unwrap();
        let ((label5, wave5), (_, wave6)) = (row(&nodes, 5), row(&nodes, 6));
        assert!(wave6.top() >= wave5.bottom(), "track 6's row isn't below track 5's");
        assert!(label5.ends_with("(preparing)") && app.library.row_waveforms.is_empty() && app.library.selected_track_id.is_none(), "{label5}");

        click(&mut app, &mut frame, along(wave5, 0.75), 1);
        assert_eq!((app.library.selected_track_id, previewing(&app)), (Some(5), Some(5)));
        let serial = app.preview.as_ref().unwrap().serial;
        let (heard, p) = first_heard(&mut app);
        let frames = 3.0 * 44_100.0;
        assert!(p.started && (p.frame / frames - 0.75).abs() < 0.01, "{p:?}");
        assert!(brightness(&heard) > 0.05, "three quarters in isn't the 3 kHz half: {}", brightness(&heard));

        click(&mut app, &mut frame, along(wave5, 0.25), 1);
        let (heard, p) = first_heard(&mut app);
        assert!(p.serial == serial && (p.frame / frames - 0.25).abs() < 0.01, "the click restarted the preview or missed: {p:?}");
        assert!(brightness(&heard) < 0.02, "a quarter in isn't the 300 Hz half: {}", brightness(&heard));
        frame(&mut app, vec![]);
        let at = app.library.preview_position.unwrap();
        assert!((at - 0.25).abs() < 0.01, "the row shows the preview at {at}");

        // The rows in view get their waveforms one at a time, unselected ones included.
        let t0 = std::time::Instant::now();
        let finished = |app: &ReTraktorApp, id: usize| app.library.row_waveforms.get(&id).is_some_and(|w| w.made_from.is_none());
        while !(1..=6).all(|id| finished(&app, id)) {
            assert!(t0.elapsed() < std::time::Duration::from_secs(60), "rows never finished: {:?}", app.library.row_waveforms.keys().collect::<Vec<_>>());
            app.poll_preview();
            frame(&mut app, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let six = &app.library.row_waveforms[&6];
        let (peaks, length) = six.peaks.as_ref().unwrap();
        assert!(peaks.len() == crate::overview::OVERVIEW_COLUMNS && peaks.iter().all(|c| c.total > 0) && *length == Some(2.0), "track 6: {} columns, {length:?}", peaks.len());
        assert!(app.library.row_waveforms[&3].peaks.is_err() && app.library.row_waveforms[&4].peaks.is_err(), "unreadable files have waveforms");
        assert!(!app.prepared.iter().any(|(t, _)| *t == 6) && previewing(&app) == Some(5), "track 6 was decoded as a preview");
        for _ in 0..3 {
            app.poll_preview();
            frame(&mut app, vec![]);
        }
        assert!(app.row_job.is_none(), "a finished or unreadable row is decoded again");
        let nodes = frame(&mut app, vec![]);
        assert!(row(&nodes, 6).0.ends_with("from there") && row(&nodes, 3).0.contains("Can't read this file"), "{:?}", row(&nodes, 3).0);

        // A click on the unreadable row selects it and says why; track 5 plays on.
        click(&mut app, &mut frame, along(row(&nodes, 3).1, 0.5), 1);
        assert_eq!((app.library.selected_track_id, previewing(&app), app.preview.as_ref().unwrap().serial), (Some(3), Some(5), serial));
        let msg = &app.library.status_message;
        assert!(msg.contains("Can't preview 'Track 3'") && msg.contains("still previewing 'Track 5'"), "{msg}");
        // Once the file is repaired, the selection's own decode of it replaces the error.
        std::fs::copy(dir.join("six.wav"), dir.join("bad.wav")).unwrap();
        let t0 = std::time::Instant::now();
        while !app.library.row_waveforms[&3].peaks.as_ref().is_ok_and(|(_, length)| *length == Some(2.0)) {
            assert!(t0.elapsed() < std::time::Duration::from_secs(20), "the repaired row still shows {:?}", app.library.row_waveforms[&3].peaks.as_ref().err());
            app.poll_preview();
            frame(&mut app, vec![]);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(previewing(&app), Some(5));

        click(&mut app, &mut frame, along(wave6, 0.5), 1);
        assert_eq!((app.library.selected_track_id, previewing(&app)), (Some(6), Some(6)));
        let (_, p) = first_heard(&mut app);
        assert!((p.frame / 96_000.0 - 0.5).abs() < 0.01, "{p:?}");

        // A double-click on a waveform previews, and never loads a deck, even with the decks unlocked.
        app.preferences.lock_playing_decks = false;
        let (_, wave2) = row(&nodes, 2);
        click(&mut app, &mut frame, along(wave2, 0.5), 2);
        assert_eq!((app.library.selected_track_id, previewing(&app)), (Some(2), Some(2)));
        decks_untouched(&app, &pcm, &set);
        let title = nodes.iter().find(|n| n.0 == "Track 2" && n.1 == Role::Label).unwrap().2;
        click(&mut app, &mut frame, title.center(), 2);
        assert!(app.deck_loading.iter().any(|&l| l) && app.library.status_message.contains("Loading 'Track 2'"), "{}", app.library.status_message);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A row's waveform follows its track's file: relinked mid-decode, the old decode is dropped at once and the new
    /// file decoded instead; a file replaced at the same path replaces even a finished waveform once a decode of the
    /// new file (here the selection's) has been made.
    #[test]
    fn row_waveforms_follow_relinks_and_replaced_files() {
        let (dir, mut app) = preview_app("relink");
        let job = |app: &ReTraktorApp| app.row_job.as_ref().map(|(id, p)| (*id, p.path.clone()));
        let length = |app: &ReTraktorApp| app.library.row_waveforms.get(&6).and_then(|w| w.peaks.as_ref().ok()?.1.filter(|_| w.made_from.is_none()));
        let poll_until = |app: &mut ReTraktorApp, what: &str, done: &dyn Fn(&ReTraktorApp) -> bool| {
            let t0 = std::time::Instant::now();
            while !done(app) {
                assert!(t0.elapsed() < std::time::Duration::from_secs(20), "{what}");
                app.library.visible_rows = vec![6];
                app.poll_preview();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        app.library.visible_rows = vec![6];
        app.poll_preview();
        assert_eq!(job(&app), Some((6, dir.join("six.wav"))));
        app.library.tracks.iter_mut().find(|t| t.id == 6).unwrap().file_path = Some(dir.join("five.wav"));
        app.library.visible_rows = vec![6];
        app.poll_preview();
        assert_eq!(job(&app), Some((6, dir.join("five.wav"))), "the decode of the old path went on");
        poll_until(&mut app, "the relinked row never finished", &|app| length(app) == Some(3.0));
        assert_eq!(app.library.row_waveforms[&6].path, dir.join("five.wav"));

        // five.wav replaced by a 2 s file: the selection's decode of it, finished before the app looks again, replaces
        // the old finished waveform in one poll.
        std::fs::copy(dir.join("six.wav"), dir.join("five.wav")).unwrap();
        app.library.selected_track_id = Some(6);
        let fresh = crate::audio::PreviewPcm::new(dir.join("five.wav"), 0.0);
        app.remember_prepared(6, &fresh); // as `prepare_selection` does once the selection rests
        app.update_preview_jobs();
        let t0 = std::time::Instant::now();
        while !(fresh.complete() && fresh.cached().is_some()) {
            assert!(t0.elapsed() < std::time::Duration::from_secs(60), "the selection's decode never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(length(&app), Some(3.0), "the old waveform changed before the app looked");
        app.library.visible_rows = vec![6];
        app.poll_preview();
        assert_eq!(length(&app), Some(2.0), "the replaced file's finished decode didn't replace the old waveform");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// With a stripe cache: a row decoded once is kept (cold). Seen again, it draws the very same overview from the
    /// cache without decoding: the file's audio is now unreadable, with the same time and size, so a decode would
    /// fail. Once the file really changes, its old stripe is no longer used. Rows and decks draw one overview per
    /// track: a deck loading a track its row kept draws that at once, and a deck loading a track first keeps it (after
    /// it is playable) for the row. A selected track whose stripe is kept shows it while its audio is still decoded
    /// in full, for a preview or a deck.
    #[test]
    fn rows_and_deck_loads_share_the_stripe_cache() {
        let (dir, mut app) = preview_app("stripes");
        app.stripes = crate::overview::StripeCache::at(dir.join("stripes"));
        app.preview_loader = crate::audio::PreviewLoader::new(app.stripes.clone());
        let wait = std::time::Duration::from_secs(20);
        let row = |app: &ReTraktorApp, id: usize| app.library.row_waveforms.get(&id).filter(|w| w.made_from.is_none()).map(|w| w.peaks.clone());
        let fresh_row = |app: &mut ReTraktorApp, id: usize, what: &str| {
            app.library.row_waveforms.clear();
            app.row_waveform_order.clear();
            let t0 = std::time::Instant::now();
            while row(app, id).is_none() {
                assert!(t0.elapsed() < wait, "{what}");
                app.library.visible_rows = vec![id];
                app.poll_preview();
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            row(app, id).unwrap()
        };
        // Loads track `id` on deck `d` and returns its overview once `drawn` holds (or fails after a while).
        let deck_overview = |app: &mut ReTraktorApp, d: usize, id: usize, drawn: &dyn Fn(&crate::overview::Overview) -> bool| {
            app.library.pending_loads.push((d + 1, app.library.track(id).unwrap().clone()));
            app.drain_pending_loads();
            let t0 = std::time::Instant::now();
            loop {
                app.finish_loads();
                let overview = app.audio.shared.lock().unwrap().loaded_tracks[d].as_ref().filter(|_| !app.deck_loading[d]).map(|t| t.overview.clone());
                if let Some(overview) = overview.filter(|o| drawn(o)) {
                    return overview;
                }
                assert!(t0.elapsed() < wait, "deck {d} never drew track {id}'s overview");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let six = dir.join("six.wav");
        let identity = crate::audio::file_identity(&six);
        assert!(app.stripes.load(&six, identity).is_none());
        let (cold, length) = fresh_row(&mut app, 6, "the cold row never finished").unwrap();
        assert_eq!(length, Some(2.0));
        let (kept, kept_length) = app.stripes.load(&six, identity).expect("the finished row wasn't kept");
        assert!(kept == cold && (kept_length - 2.0).abs() < 0.03, "kept {kept_length} s, not the row's overview");

        // Row first, then a deck, for track 5 (a low tone, then a high one): the deck draws the row's from the start.
        let (row_five, _) = fresh_row(&mut app, 5, "track 5's row never finished").unwrap();
        let deck_five = deck_overview(&mut app, 2, 5, &|_| true);
        assert!(deck_five.len() == crate::overview::OVERVIEW_COLUMNS && deck_five == row_five, "the deck doesn't draw track 5's row");

        // A deck first, for track 2: once playable it keeps the overview it then draws, and the row finds it.
        let (two, stripes) = (dir.join("two.wav"), app.stripes.clone());
        let deck_two = deck_overview(&mut app, 3, 2, &|o| stripes.load(&two, crate::audio::file_identity(&two)).is_some_and(|(kept, _)| kept == *o));
        assert_eq!(fresh_row(&mut app, 2, "track 2's row never finished").unwrap().0, deck_two, "the row doesn't draw deck D's");

        // Selected with a stripe kept (made up, so it can't come from the audio): shown at once, audio decoded in full.
        let one = dir.join("one.wav");
        let made_up: Vec<[f32; 3]> = (0..18).map(|j| [0.1 + j as f32 / 20.0, 0.3, 0.2]).collect();
        let made_up_overview = app.stripes.store(&one, crate::audio::file_identity(&one), &made_up, 0.3).unwrap();
        app.library.row_waveforms.clear();
        app.library.selected_track_id = Some(1);
        app.poll_preview();
        std::thread::sleep(BROWSE_SETTLE + std::time::Duration::from_millis(20));
        let t0 = std::time::Instant::now();
        while !app.prepared_for(1).is_some_and(|p| p.complete()) || row(&app, 1).is_none() {
            assert!(t0.elapsed() < wait, "track 1's selection never finished");
            app.poll_preview();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(row(&app, 1).unwrap().unwrap().0, made_up_overview, "the selected row didn't draw the kept stripe");
        let selected = Arc::clone(app.prepared_for(1).unwrap());
        assert!(selected.failed().is_none() && selected.to_vec().is_some_and(|pcm| pcm.len() == 2 * 13_230), "the selection's audio wasn't decoded in full");

        let (modified, size) = identity.unwrap();
        std::fs::write(&six, vec![0u8; size as usize]).unwrap();
        std::fs::File::options().write(true).open(&six).unwrap().set_modified(modified).unwrap();
        assert_eq!(crate::audio::file_identity(&six), identity);
        let (warm, warm_length) = fresh_row(&mut app, 6, "the warm row never finished").expect("the warm row was decoded");
        assert!(warm == cold && warm_length == Some(kept_length));

        std::fs::write(&six, vec![0u8; size as usize]).unwrap(); // a new modified time
        assert!(fresh_row(&mut app, 6, "the changed file's row never finished").is_err(), "a changed file kept its old stripe");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Browsing quickly decodes nothing until the selection rests, then only the track it rests on; at most three
    /// decodes are kept. A click on an overview before that track's decode has even begun lands where it pointed.
    #[test]
    fn browsing_waits_for_a_rest_and_early_clicks_land_where_they_point() {
        let (dir, mut app) = preview_app("browse");
        for id in [1, 2, 5, 6, 5, 1, 6] {
            app.library.selected_track_id = Some(id);
            app.poll_preview();
        }
        assert!(app.prepared.is_empty(), "browsing decoded before the selection rested");
        std::thread::sleep(BROWSE_SETTLE + std::time::Duration::from_millis(50));
        app.poll_preview();
        assert_eq!(app.prepared.iter().map(|(t, _)| *t).collect::<Vec<_>>(), [6]);
        assert!(app.preview.is_none(), "browsing started a preview");

        app.library.selected_track_id = Some(5);
        app.click_overview(5, 0.75);
        let (heard, p) = first_heard(&mut app);
        assert!((p.frame / (3.0 * 44_100.0) - 0.75).abs() < 0.005, "the early click landed at {p:?}");
        assert!(brightness(&heard) > 0.05, "not the 3 kHz half: {}", brightness(&heard));
        for id in [1, 2] {
            app.library.selected_track_id = Some(id);
            app.poll_preview();
            std::thread::sleep(BROWSE_SETTLE + std::time::Duration::from_millis(20));
            app.poll_preview();
        }
        assert!(app.prepared.len() <= 3 && app.prepared.iter().any(|(t, _)| *t == 5), "{:?}", app.prepared.iter().map(|(t, _)| *t).collect::<Vec<_>>());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Loads unanalysed track `id` on deck `d` (0-based), holding back its analysis: the load's result and then its
    /// analysis come out of the channels, the load is applied, and the analysis is returned to apply later.
    fn load_holding_analysis(app: &mut ReTraktorApp, d: usize, id: usize) -> Option<DeckResult> {
        let wait = std::time::Duration::from_secs(30);
        let track = app.library.track(id).cloned().unwrap();
        app.library.pending_loads.push((d + 1, track));
        app.drain_pending_loads();
        let loaded = app.load_channel.1.recv_timeout(wait).unwrap();
        // Past the load's overview, which comes first.
        let analysis = std::iter::from_fn(|| app.load_channel.1.recv_timeout(wait).ok()).find(|r| !matches!(r, DeckResult::Overview(..)));
        app.load_channel.0.send(loaded).unwrap();
        app.finish_loads();
        analysis
    }

    /// A deck load of a track with no BPM or key is playable before its analysis, which shows as pending. Meanwhile
    /// sync, grid edits and loops wait (no loop sized by a made-up tempo), and play, a moved playhead and hot cues
    /// the DJ set and then cleared all stay as they are when the analysis lands. It fills in BPM and key but keeps
    /// the track's grid cue (at 0 s) and its phrases from the library. Analyses from an earlier load, or for other
    /// audio, leave the deck alone. Auto hot cues follow the track's hand edits, whichever deck made them: a deck
    /// replaced before its analysis lands still gives an unedited track its auto cues, while an edit on another deck
    /// holding the same track keeps them out; the saved collection agrees after a reload. A tagged BPM shows at once
    /// while only the key is pending, and stays when it lands.
    #[test]
    fn deck_plays_before_its_analysis_and_keeps_what_was_done_meanwhile() {
        let (dir, mut app) = preview_app("analysis");
        let d = 2;
        let phrases = vec![retraktor_library::Phrase { start_sec: 0.5, kind: retraktor_library::PhraseKind::Build }];
        if let Some(t) = app.library.tracks.iter_mut().find(|t| t.id == 5) {
            let grid = retraktor_formats::nml::NmlCue { name: "Grid".into(), displ_order: 0, cue_type: retraktor_core::CueType::Grid, start_ms: 0.0, len_ms: 0.0, repeats: -1, hotcue: -1, grid_bpm: None };
            t.cues.push(grid);
            t.phrases = phrases.clone();
        }
        let Some(DeckResult::Analysis(_, generation, id, analysis, audio, kept)) = load_holding_analysis(&mut app, d, 5) else { panic!("no analysis") };
        assert!(app.deck_analysing[d] && !app.deck_loading[d]);
        assert_eq!((bpm_text(0.0, 0.0, true), key_text("", true), bpm_text(0.0, 0.0, false)), ("…".to_string(), "…", "—".to_string()));

        app.toggle_play(d + 1);
        app.audio.shared.lock().unwrap().seek_to(d, 1.0);
        app.jump_to_cue(d + 1, 3); // sets hot cue 3 here (deck and library)...
        app.clear_deck_hotcue(d, 3); // ...and the DJ clears it again, deck and library, as a right-click does
        app.deck_sync_enabled[d] = true;
        app.adjust_deck_grid(d, 0.1, 2.0);
        app.toggle_loop(d + 1);
        assert!(!app.deck_loop_active[d] && app.library.status_message.contains("wait for its BPM"), "{}", app.library.status_message);
        app.sync_audio_state();
        output_peaks(&app, 10);
        let (bpm, ratio, at, looping) = {
            let s = app.audio.shared.lock().unwrap();
            (s.loaded_tracks[d].as_ref().unwrap().bpm, s.pitch_ratio[d], s.playhead_sec[d], s.loop_active[d])
        };
        assert_eq!((bpm, ratio, looping), (0.0, 1.0, false), "a grid edit, sync or loop acted on a deck with no grid");
        assert!(at > 1.0, "the deck isn't playing on from 1 s: {at}");

        let mut late = analysis.clone();
        late.beatgrid_offset_sec = 1.23; // so a replaced grid would show
        for stale in [DeckResult::Analysis(d, generation - 1, id, late.clone(), Arc::clone(&audio), kept), DeckResult::Analysis(d, generation, id, late.clone(), Arc::new(vec![0.0; 4]), kept)] {
            app.load_channel.0.send(stale).unwrap();
        }
        app.finish_loads();
        assert!(app.deck_analysing[d] && app.audio.shared.lock().unwrap().loaded_tracks[d].as_ref().unwrap().bpm == 0.0, "a stale analysis landed");

        app.load_channel.0.send(DeckResult::Analysis(d, generation, id, late.clone(), audio, kept)).unwrap();
        app.finish_loads();
        let s = app.audio.shared.lock().unwrap();
        let t = s.loaded_tracks[d].as_ref().unwrap();
        assert!(t.bpm == late.bpm && t.bpm > 0.0 && t.key == late.key_open, "BPM {} key {}", t.bpm, t.key);
        assert_eq!((t.grid_offset_sec, &t.phrases), (0.0, &phrases), "the library's grid cue or phrases were replaced");
        assert!(s.is_playing[d] && s.playhead_sec[d] >= at, "the analysis moved or stopped the deck");
        drop(s);
        assert!(!app.deck_analysing[d]);
        assert!(app.deck_hotcues[d].is_empty(), "hot cues the DJ cleared came back: {:?}", app.deck_hotcues[d]);
        // Deck A loads track 2, then track 1 before track 2's analysis lands: no one edited track 2's cues, so its
        // entry still gets the auto cues. Deck B loads track 1 too; a cue added and cleared there means deck A's
        // pending analysis of track 1 must not bring any back.
        let Some(DeckResult::Analysis(_, superseded, two, mut late_two, audio_two, kept_two)) = load_holding_analysis(&mut app, 0, 2) else { panic!("no analysis for track 2") };
        late_two.cue_points_sec = vec![0.05, 0.1];
        let Some(DeckResult::Analysis(_, on_a, one, mut late_one, audio_one, kept_one)) = load_holding_analysis(&mut app, 0, 1) else { panic!("no analysis for track 1 on A") };
        late_one.cue_points_sec = vec![0.05, 0.1];
        let _on_b = load_holding_analysis(&mut app, 1, 1);
        app.jump_to_cue(2, 4);
        app.clear_deck_hotcue(1, 4);
        app.load_channel.0.send(DeckResult::Analysis(0, superseded, two, late_two, audio_two, kept_two)).unwrap();
        app.load_channel.0.send(DeckResult::Analysis(0, on_a, one, late_one, audio_one, kept_one)).unwrap();
        app.finish_loads();
        assert_eq!(deck_hotcues_of(&app.library.track(2).unwrap().cues).len(), 2, "the superseded, unedited track lost its auto cues");
        assert!(deck_hotcues_of(&app.library.track(1).unwrap().cues).is_empty() && app.deck_hotcues[0].is_empty(), "deck A's analysis restored cues cleared on deck B");

        // The library entries too, through normal polling, a save and a reload of the collection.
        app.library.poll();
        app.library.save_collection_nml(&dir.join("collection.nml")).unwrap();
        let reloaded = LibraryManager::new(dir.join("collection.nml"));
        let five = reloaded.tracks.iter().find(|t| t.file_path.as_ref().is_some_and(|p| p.ends_with("five.wav"))).expect("track 5 wasn't saved");
        assert!(!five.cues.iter().any(|c| c.hotcue >= 0), "cleared hot cues came back in the library: {:?}", five.cues);
        assert!(five.bpm == late.bpm && five.cues.iter().any(|c| c.cue_type == retraktor_core::CueType::Grid && c.start_ms == 0.0), "the analysis or the grid cue wasn't saved");
        let saved = |file: &str| reloaded.tracks.iter().find(|t| t.file_path.as_ref().is_some_and(|p| p.ends_with(file))).map(|t| deck_hotcues_of(&t.cues).len());
        assert_eq!((saved("one.wav"), saved("two.wav")), (Some(0), Some(2)), "saved hot cues of tracks 1 and 2");
        app.toggle_loop(d + 1);
        assert!(app.deck_loop_active[d], "no loop once the BPM is known");

        // Track 6 with a tagged BPM and no key: the BPM shows (and loops work) at once; the key fills in later.
        if let Some(t) = app.library.tracks.iter_mut().find(|t| t.id == 6) {
            t.bpm = 124.0;
        }
        let Some(update) = load_holding_analysis(&mut app, 3, 6) else { panic!("no key analysis") };
        let DeckResult::Analysis(_, _, _, ref key_analysis, _, _) = update else { panic!("not an analysis") };
        let auto: Vec<(u8, f64)> = key_analysis.cue_points_sec.iter().take(8).enumerate().map(|(i, &t)| (i as u8 + 1, t)).collect();
        assert_eq!(app.audio.shared.lock().unwrap().loaded_tracks[3].as_ref().unwrap().bpm, 124.0);
        app.toggle_loop(4);
        assert!(app.deck_loop_active[3], "a tagged BPM was treated as missing");
        app.load_channel.0.send(update).unwrap();
        app.finish_loads();
        let s = app.audio.shared.lock().unwrap();
        let t = s.loaded_tracks[3].as_ref().unwrap();
        assert!(t.bpm == 124.0 && !t.key.is_empty(), "BPM {} key {:?}", t.bpm, t.key);
        drop(s);
        let same = |a: &[(u8, f64)], b: &[(u8, f64)]| a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.0 == y.0 && (x.1 - y.1).abs() < 1e-9);
        assert!(same(&app.deck_hotcues[3], &auto), "an untouched, cue-less deck didn't get the auto cues: {:?}", app.deck_hotcues[3]);
        assert!(same(&deck_hotcues_of(&app.library.track(6).unwrap().cues), &auto), "the library didn't get the same auto cues");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A deck reuses a preview's audio only when it is exactly what a fresh decode of the file as it is now would give:
    /// after ordinary browsing (the overview decoded straight through) it is offered and is the file. A file replaced in
    /// place (another take, same path) is never played from the old audio, on a deck or in a preview. A decode that
    /// started three quarters in (seeked) isn't offered; the deck decodes the file itself.
    #[test]
    fn deck_reuses_only_an_exact_decode_of_the_file_as_it_is() {
        let (dir, mut app) = preview_app("reuse");
        let wait = std::time::Duration::from_secs(30);
        let load = |app: &mut ReTraktorApp, d: usize, id: usize| -> LoadedTrackAudio {
            app.library.pending_loads.push((d + 1, app.library.track(id).cloned().unwrap()));
            app.drain_pending_loads();
            loop {
                // Skips what earlier loads send after theirs (their analysis).
                if let DeckResult::Loaded(_, generation, _, loaded) = app.load_channel.1.recv_timeout(wait).unwrap() {
                    if generation == app.load_generation[d] {
                        return loaded.unwrap();
                    }
                }
            }
        };
        let rest_on = |app: &mut ReTraktorApp, id: usize| -> Arc<crate::audio::PreviewPcm> {
            app.library.selected_track_id = Some(id);
            let t0 = std::time::Instant::now();
            loop {
                app.poll_preview();
                if let Some(p) = app.prepared_for(id).filter(|p| p.complete()) {
                    return Arc::clone(p);
                }
                assert!(t0.elapsed() < wait, "track {id}'s overview never completed");
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        };
        let six = app.library.track(6).unwrap().file_path.clone().unwrap();

        let browsed = rest_on(&mut app, 6);
        assert!(browsed.exact() && browsed.to_vec().is_some(), "a straight overview decode isn't offered to a deck");
        let (expected, _) = LoadedTrackAudio::decode_file_to_pcm(&six).unwrap();
        assert!(*load(&mut app, 3, 6).pcm_stereo == expected, "the deck doesn't play the file");

        // Another take at the same path (3 s at 660 Hz instead of 2 s at 440 Hz).
        let take: Vec<i16> = (0..3 * 48_000).flat_map(|i| {
            let v = ((i as f32 * 660.0 * std::f32::consts::TAU / 48_000.0).sin() * 9_000.0) as i16;
            [v, v]
        }).collect();
        AudioSystem::write_stereo_wav_file(&six, &take, 48_000).unwrap();
        assert!(!browsed.same_file() && browsed.to_vec().is_none(), "the old take still counts as this file");
        let (expected, _) = LoadedTrackAudio::decode_file_to_pcm(&six).unwrap();
        assert!(*load(&mut app, 3, 6).pcm_stereo == expected, "the deck played the old take");
        app.press_preview(Some(6));
        let preview = Arc::clone(&app.preview.as_ref().unwrap().pcm);
        assert!(!Arc::ptr_eq(&preview, &browsed), "the preview reused the old take");
        settle_preview(&mut app);
        assert_eq!(preview.playable_frames(), Some(3 * 48_000));
        app.press_preview(None);
        // Selected again, its overview is the new take too.
        app.library.selected_track_id = Some(5);
        app.poll_preview();
        let overview = rest_on(&mut app, 6);
        assert!(!Arc::ptr_eq(&overview, &browsed) && overview.playable_frames() == Some(3 * 48_000), "the overview is still the old take");

        // Track 5, previewed from three quarters in: not offered; the deck decodes the file.
        app.library.selected_track_id = Some(5);
        app.click_overview(5, 0.75);
        let seeked = Arc::clone(&app.preview.as_ref().unwrap().pcm);
        settle_preview(&mut app);
        assert!(seeked.complete() && !seeked.exact() && seeked.to_vec().is_none(), "a seeked decode was offered");
        let five = app.library.track(5).unwrap().file_path.clone().unwrap();
        assert!(*load(&mut app, 2, 5).pcm_stereo == LoadedTrackAudio::decode_file_to_pcm(&five).unwrap().0);
        assert!(app.prepared.len() <= 2, "{} decodes kept", app.prepared.len());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The real Mix and 4-Deck layouts in the smallest window (1024 x 640) with Tools open on every deck:
    /// each loaded deck's JUMP arrows and size picker are on screen (C and D once the 4-Deck view scrolls),
    /// clicks jump, the picker changes the size, and an edge greys the arrow out.
    #[test]
    fn beat_jump_controls_reachable_in_the_real_layouts() {
        use egui::accesskit::Role;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(1024.0, 640.0));
        let ctx = egui::Context::default();
        crate::theme::install_fonts(&ctx);
        ctx.enable_accesskit();
        let mut app = ReTraktorApp::default();
        {
            let mut s = app.audio.shared.lock().unwrap();
            for d in 0..4 {
                s.loaded_tracks[d] = Some(track(120.0));
                s.playhead_sec[d] = 50.0;
            }
        }
        app.deck_expanded = [true; 4];
        let mut time = 0.0;
        let mut frame = |app: &mut ReTraktorApp, events: Vec<egui::Event>| -> Vec<Node> {
            time += 0.1;
            workspace_frame(&ctx, app, screen, time, events)
        };
        let node = |nodes: &[Node], name: &str| nodes.iter().find(|n| n.0 == name).cloned().unwrap_or_else(|| panic!("no {name}"));
        let settle = |app: &mut ReTraktorApp, frame: &mut dyn FnMut(&mut ReTraktorApp, Vec<egui::Event>) -> Vec<Node>, events: Vec<egui::Event>| {
            frame(app, events);
            (0..12).map(|_| frame(app, vec![])).last().unwrap()
        };
        let click = |app: &mut ReTraktorApp, frame: &mut dyn FnMut(&mut ReTraktorApp, Vec<egui::Event>) -> Vec<Node>, at: egui::Pos2| {
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            frame(app, vec![egui::Event::PointerMoved(at)]);
            frame(app, vec![button(true)]);
            frame(app, vec![button(false)]);
        };
        let controls = |letter: char| ["back 4 beats", "forward 4 beats", "size 4 beats"].map(|c| format!("Deck {letter} beat jump {c}"));
        let assert_on_screen = |nodes: &[Node], letters: &str, what: &str| {
            for letter in letters.chars() {
                for name in controls(letter) {
                    let r = node(nodes, &name).2;
                    assert!(screen.contains_rect(r), "{what}: {name} at {r:?} is off the 1024 x 640 window");
                }
            }
        };

        app.layout = TraktorLayout::Parallel;
        let nodes = settle(&mut app, &mut frame, vec![]);
        assert_on_screen(&nodes, "AB", "Mix, Tools open");

        app.layout = TraktorLayout::FourDeck;
        let nodes = settle(&mut app, &mut frame, vec![]);
        assert_on_screen(&nodes, "AB", "4-Deck top");
        // C and D sit below the window until the deck area scrolls.
        let wheel = egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: Vec2::new(0.0, -2000.0), modifiers: egui::Modifiers::NONE };
        frame(&mut app, vec![egui::Event::PointerMoved(egui::pos2(500.0, 400.0))]);
        let nodes = settle(&mut app, &mut frame, vec![wheel]);
        assert_on_screen(&nodes, "CD", "4-Deck scrolled");

        // Deck D (bottom right): forward, back, then 16 beats from the size menu.
        click(&mut app, &mut frame, node(&nodes, "Deck D beat jump forward 4 beats").2.center());
        assert_eq!(playhead(&app, 3), 52.0);
        click(&mut app, &mut frame, node(&nodes, "Deck D beat jump back 4 beats").2.center());
        assert_eq!(playhead(&app, 3), 50.0);
        click(&mut app, &mut frame, node(&nodes, "Deck D beat jump size 4 beats").2.center());
        let menu = frame(&mut app, vec![]);
        let item = menu.iter().find(|n| n.0 == "16 beats" && n.1 != Role::ComboBox).expect("no 16 beats item");
        click(&mut app, &mut frame, item.2.center());
        assert_eq!(app.deck_jump_beats, [4, 4, 4, 16], "only deck D's size changes");
        let nodes = frame(&mut app, vec![]);
        click(&mut app, &mut frame, node(&nodes, "Deck D beat jump forward 16 beats").2.center());
        assert_eq!(playhead(&app, 3), 58.0);

        // 8 s from the end, 16 beats (8 s) forward lands on the end: greyed out, and a click does nothing.
        app.audio.shared.lock().unwrap().playhead_sec[3] = 92.0;
        let nodes = settle(&mut app, &mut frame, vec![]);
        let fwd = node(&nodes, "Deck D beat jump forward 16 beats");
        assert!(fwd.3 && !node(&nodes, "Deck D beat jump back 16 beats").3, "forward should be disabled, back enabled");
        click(&mut app, &mut frame, fwd.2.center());
        assert_eq!(playhead(&app, 3), 92.0);
    }

    #[test]
    fn failed_load_does_not_strand_current_decks_pending_analysis() {
        let mut app = ReTraktorApp::default();
        let entry = crate::library_ui::UiTrackEntry { id: 7, title: "Analysing".into(), bpm: 0.0, ..Default::default() };
        app.library.tracks.push(entry.clone());
        app.library.rebuild_index();
        app.load_generation[0] = 1;
        let loaded = track(0.0);
        let audio = Arc::clone(&loaded.pcm_stereo);
        app.load_channel.0.send(DeckResult::Loaded(0, 1, entry.clone(), Ok(loaded))).unwrap();
        app.finish_loads();
        assert!(app.deck_analysing[0]);
        assert_eq!(app.deck_loaded_generation[0], 1);

        // A second load attempt bumps load_generation to 2 and fails; track 7 stays on Deck A.
        app.load_generation[0] = 2;
        app.load_channel.0.send(DeckResult::Loaded(0, 2, entry, Err("corrupt".into()))).unwrap();
        app.finish_loads();
        assert_eq!(app.deck_loaded_generation[0], 1);

        // Track 7's pending analysis (generation 1) still lands and clears `deck_analysing`.
        let analysis = retraktor_library::TrackAnalysis {
            bpm: 126.0,
            beatgrid_offset_sec: 0.0,
            key_value: 22,
            key_open: "8m".into(),
            energy: 6,
            loudness_db: -10.0,
            peak_db: -1.0,
            cue_points_sec: Vec::new(),
            phrases: Vec::new(),
        };
        let kept = Kept { bpm: false, grid: false, phrases: false, cue_revision: 0 };
        app.load_channel.0.send(DeckResult::Analysis(0, 1, 7, analysis, audio, kept)).unwrap();
        app.finish_loads();
        assert!(!app.deck_analysing[0]);
        assert_eq!(app.audio.shared.lock().unwrap().loaded_tracks[0].as_ref().unwrap().bpm, 126.0);
    }
}
