use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefsTab {
    AudioSetup,
    OutputRouting,
    InputRouting,
    ExternalSync,
    Mixer,
    Decks,
    TrackDecks,
    Effects,
    StemsAndAi,
    Recording,
    BrowserColumns,
    FileManagement,
    AnalyzeOptions,
    Controllers,
    OtherControllers,
    Look,
    Ai,
    About,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LibraryColumnsConfig {
    pub album: bool,
    pub bpm: bool,
    pub key: bool,
    pub stems: bool,
    pub duration: bool,
    pub genre: bool,
    pub rating: bool,
    pub comment: bool,
    pub remixer: bool,
    pub label: bool,
    pub year: bool,
    pub bitrate: bool,
    pub format: bool,
    pub track_number: bool,
    pub energy: bool,
}

impl Default for LibraryColumnsConfig {
    fn default() -> Self {
        Self {
            album: true,
            bpm: true,
            key: true,
            stems: true,
            duration: true,
            genre: true,
            rating: true,
            comment: false,
            remixer: false,
            label: false,
            year: false,
            bitrate: false,
            format: false,
            track_number: false,
            energy: true,
        }
    }
}

impl LibraryColumnsConfig {
    pub fn set_all(&mut self, enabled: bool) {
        self.album = enabled;
        self.bpm = enabled;
        self.key = enabled;
        self.stems = enabled;
        self.duration = enabled;
        self.genre = enabled;
        self.rating = enabled;
        self.comment = enabled;
        self.remixer = enabled;
        self.label = enabled;
        self.year = enabled;
        self.bitrate = enabled;
        self.format = enabled;
        self.track_number = enabled;
        self.energy = enabled;
    }
}

pub struct PreferencesDialog {
    /// Set when the user asks for the folder chooser; the app opens it via the library.
    pub request_folder_pick: bool,
    pub is_open: bool,
    pub current_tab: PrefsTab,
    pub search_query: String,

    // Audio Setup
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub audio_device_name: String,

    // Output & Input Routing
    pub mixing_mode: usize, // 0: Internal (Software), 1: External (Hardware Mixer)
    pub monitor_out_l: String,
    pub monitor_out_r: String,
    pub master_out_l: String,
    pub master_out_r: String,
    pub booth_out_l: String,
    pub booth_out_r: String,
    pub split_cue: bool,
    pub deck_a_in_l: String,
    pub deck_a_in_r: String,
    pub deck_b_in_l: String,
    pub deck_b_in_r: String,
    pub aux_in_l: String,
    pub aux_in_r: String,

    // External Sync & MIDI Clock
    pub clock_mode: usize, // 0: Auto Master, 1: Manual
    pub send_midi_clock: bool,
    pub midi_clock_offset_ms: f32,
    pub ableton_link_enabled: bool,

    // Mixer & EQ
    pub eq_model_idx: usize,
    pub filter_model_idx: usize,
    pub filter_resonance: f32,
    pub xfader_curve_idx: usize,
    pub autogain: bool,
    pub limiter_enabled: bool,
    pub limiter_headroom: usize, // 0: None (0.0 dB), 1: -3.0 dB, 2: -6.0 dB
    pub neural_mix_eq_mode: bool,

    // Decks & Waveforms
    pub four_deck_mode: bool,
    pub waveform_color_mode: usize, // 0: Spectrum 4-Band, 1: Ultraviolet, 2: High-Contrast RGB
    pub show_minute_markers: bool,
    pub show_beatgrid_ticks: bool,
    pub stripe_fit_full: bool,

    // Track Decks & Scratch
    pub pitch_range_idx: usize, // 0: ±8%, 1: ±16%, 2: ±32%, 3: ±50%, 4: ±100%
    pub keylock_quality: usize, // 0: HiQ (Scratch-Preserving), 1: ECO
    pub play_marker_pos: usize, // 0: Centered 50%, 1: 30% Left-Weighted
    pub auto_cue_on_load: bool,
    pub jog_scratch_sensitivity: f32,
    pub jog_mode_turntable: bool,
    pub lock_playing_decks: bool,

    // Effects & Mixer FX
    pub fx_units_count: usize, // 0: 2 FX Units, 1: 4 FX Units
    pub fx_panel_mode: usize,  // 0: Group (3-FX), 1: Single (Deep)
    pub mixer_fx_slots: [usize; 4],

    // Stems & AI
    pub stems_engine_mode: usize, // 0: Hybrid (Real-time DSP + Demucs AI Cache), 1: Real-Time DSP, 2: Offline AI
    pub demucs_install_state: crate::demucs::DemucsInstallState,
    pub trigger_demucs_install: bool,
    pub demucs_active_separation: Option<String>,

    // Recording & Broadcast
    pub recording_dir: std::path::PathBuf,
    pub recording_format: usize, // 0: 16-bit 48kHz WAV, 1: 24-bit 48kHz WAV
    pub broadcast_host: String,
    pub broadcast_port: u16,
    pub broadcast_mount: String,
    pub broadcast_password: String,
    pub broadcast_connected: bool,

    // File Management & Folders
    pub music_folders: Vec<std::path::PathBuf>,
    pub scan_on_startup: bool,
    pub analyze_new_tracks: bool,
    pub trigger_music_rescan: bool,
    pub file_management_status: String,
    pub library_columns: LibraryColumnsConfig,
    pub collection_save_format: usize, // 0: Traktor .nml (v20), 1: Extended .m3u8, 2: Both (.nml + .m3u8)
    pub prompt_save_on_exit: bool,

    // Analyze Options
    pub bpm_detection_range: usize, // 0: 70..140 BPM, 1: 60..120 BPM, 2: 90..180 BPM, 3: 110..220 BPM
    pub key_notation_idx: usize,    // 0: Open Key (4m), 1: Camelot (8A), 2: Musical (Am)

    // Controller Manager & Surface Remapper
    pub tsi_path_input: String,
    pub loaded_tsi_summary: Option<String>,
    pub mx2_bottom_led_brightness: f32,
    pub mx2_connected_node: Option<String>,
    pub mx2_access_status: Option<String>,
    pub mx2_packets_count: u64,
    pub mx2_last_report_id: Option<u8>,
    pub mx2_last_hex: Option<String>,
    pub mx2_recent_events: Vec<String>,
    pub discovered_hardware: Vec<retraktor_hid::DiscoveredHardwareItem>,
    pub self_test_report: Option<retraktor_hid::Mx2SelfTestReport>,
    pub trigger_rescan: bool,
    pub trigger_connect_node: Option<String>,
    pub trigger_led_test: bool,
    pub trigger_self_test: bool,
    pub udev_command_copied: bool,
    /// User MX2 mapping, saved to ~/.config/retraktor/mx2-mapping.tsv.
    pub mapping: crate::controller_map::Mapping,
    pub mapper: crate::controller_map::MapperState,
    /// Generic MIDI and NI HID controller manager and mapper.
    pub generic_manager: crate::midi::GenericControllerManager,
    /// Last control touched on the hardware (for "press to select").
    pub last_surface_control: Option<String>,
    pub ai_settings: crate::ai::AiSettings,
    pub ai_test_runner: Option<crate::ai::AiTestRunner>,
    pub ai_test_status: Option<Result<String, String>>,
}

impl Default for PreferencesDialog {
    fn default() -> Self {
        let home = std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("/"));
        // Only default to ~/Music when it exists; never scan the whole home folder.
        let default_music = Some(home.join("Music")).filter(|m| m.is_dir());

        Self {
            is_open: false,
            current_tab: PrefsTab::Controllers,
            search_query: String::new(),
            ai_settings: crate::ai::AiSettings::load(),
            ai_test_runner: None,
            ai_test_status: None,
            sample_rate: 48000,
            buffer_size: 128,
            audio_device_name: "PipeWire pro-audio / ALSA Default".to_string(),
            mixing_mode: 0,
            monitor_out_l: "Channel 3 (Headphones L)".to_string(),
            monitor_out_r: "Channel 4 (Headphones R)".to_string(),
            master_out_l: "Channel 1 (Master Out L)".to_string(),
            master_out_r: "Channel 2 (Master Out R)".to_string(),
            booth_out_l: "Channel 5 (Booth Out L)".to_string(),
            booth_out_r: "Channel 6 (Booth Out R)".to_string(),
            split_cue: false,
            deck_a_in_l: "In 1 (Line / Phono A-L)".to_string(),
            deck_a_in_r: "In 2 (Line / Phono A-R)".to_string(),
            deck_b_in_l: "In 3 (Line / Phono B-L)".to_string(),
            deck_b_in_r: "In 4 (Line / Phono B-R)".to_string(),
            aux_in_l: "In 5 (Mic / Aux L)".to_string(),
            aux_in_r: "In 6 (Mic / Aux R)".to_string(),
            clock_mode: 0,
            send_midi_clock: false,
            midi_clock_offset_ms: 0.0,
            ableton_link_enabled: false,
            eq_model_idx: 0,     // Z ISO
            filter_model_idx: 0, // Ladder
            filter_resonance: 0.3,
            xfader_curve_idx: 0, // Smooth
            autogain: true,
            limiter_enabled: true,
            limiter_headroom: 1, // -3dB
            four_deck_mode: false,
            waveform_color_mode: 0,
            show_minute_markers: true,
            show_beatgrid_ticks: true,
            stripe_fit_full: true,
            pitch_range_idx: 0, // ±8%
            keylock_quality: 0, // HiQ
            play_marker_pos: 0, // Centered 50%
            auto_cue_on_load: true,
            jog_scratch_sensitivity: 1.0,
            jog_mode_turntable: true,
            lock_playing_decks: true,
            fx_units_count: 0, // 2 FX units
            fx_panel_mode: 0,  // Group
            mixer_fx_slots: [0, 1, 2, 3], // Filter, Reverb, Delay, Noise
            neural_mix_eq_mode: false,
            stems_engine_mode: 0,
            demucs_install_state: crate::demucs::DemucsInstallState::NotInstalled,
            trigger_demucs_install: false,
            demucs_active_separation: None,
            recording_dir: home.join("Music"),
            recording_format: 0,
            broadcast_host: "localhost".to_string(),
            broadcast_port: 8000,
            broadcast_mount: "/retraktor.mp3".to_string(),
            broadcast_password: "hackme".to_string(),
            broadcast_connected: false,
            music_folders: default_music.into_iter().collect(),
            request_folder_pick: false,
            scan_on_startup: true,
            analyze_new_tracks: true,
            trigger_music_rescan: false,
            file_management_status: "Ready".to_string(),
            library_columns: LibraryColumnsConfig::default(),
            collection_save_format: 0, // Traktor .nml (v20)
            prompt_save_on_exit: true,
            bpm_detection_range: 0, // 70..140
            key_notation_idx: 0,    // Open Key
            tsi_path_input: String::new(),
            loaded_tsi_summary: None,
            mx2_bottom_led_brightness: 1.0,
            mx2_connected_node: None,
            mx2_access_status: None,
            mx2_packets_count: 0,
            mx2_last_report_id: None,
            mx2_last_hex: None,
            mx2_recent_events: Vec::new(),
            discovered_hardware: Vec::new(),
            self_test_report: None,
            trigger_rescan: true,
            trigger_connect_node: None,
            trigger_led_test: false,
            trigger_self_test: false,
            udev_command_copied: false,
            mapping: crate::controller_map::Mapping::load(),
            mapper: Default::default(),
            generic_manager: Default::default(),
            last_surface_control: None,
        }
    }
}

impl PreferencesDialog {
    /// Returns the standard path for user preferences (`~/.config/retraktor/preferences.json`).
    pub fn config_path() -> std::path::PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        std::path::PathBuf::from(home).join(".config/retraktor/preferences.json")
    }

    /// Loads persisted user preferences from disk, falling back to defaults if not found.
    pub fn load() -> Self {
        let mut prefs = Self::default();
        let path = Self::config_path();
        if let Ok(content) = std::fs::read_to_string(&path) {
            prefs.apply_json_str(&content);
        }
        prefs
    }

    /// Saves user preferences to disk.
    pub fn save(&self) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = self.to_json_str();
        let _ = std::fs::write(&path, json);
    }

    pub fn to_json_str(&self) -> String {
        let folders_json = self
            .music_folders
            .iter()
            .map(|f| format!("\"{}\"", f.display()))
            .collect::<Vec<_>>()
            .join(", ");
        let c = &self.library_columns;
        format!(
            "{{\n  \"sample_rate\": {},\n  \"buffer_size\": {},\n  \"audio_device_name\": \"{}\",\n  \"four_deck_mode\": {},\n  \"waveform_color_mode\": {},\n  \"eq_model_idx\": {},\n  \"filter_model_idx\": {},\n  \"filter_resonance\": {},\n  \"xfader_curve_idx\": {},\n  \"autogain\": {},\n  \"limiter_enabled\": {},\n  \"lock_playing_decks\": {},\n  \"scan_on_startup\": {},\n  \"analyze_new_tracks\": {},\n  \"key_notation_idx\": {},\n  \"collection_save_format\": {},\n  \"prompt_save_on_exit\": {},\n  \"col_album\": {},\n  \"col_bpm\": {},\n  \"col_key\": {},\n  \"col_stems\": {},\n  \"col_duration\": {},\n  \"col_genre\": {},\n  \"col_rating\": {},\n  \"col_comment\": {},\n  \"col_remixer\": {},\n  \"col_label\": {},\n  \"col_year\": {},\n  \"col_bitrate\": {},\n  \"col_format\": {},\n  \"col_track_number\": {},\n  \"col_energy\": {},\n  \"music_folders\": [{}]\n}}",
            self.sample_rate,
            self.buffer_size,
            self.audio_device_name.replace('"', "\\\""),
            self.four_deck_mode,
            self.waveform_color_mode,
            self.eq_model_idx,
            self.filter_model_idx,
            self.filter_resonance,
            self.xfader_curve_idx,
            self.autogain,
            self.limiter_enabled,
            self.lock_playing_decks,
            self.scan_on_startup,
            self.analyze_new_tracks,
            self.key_notation_idx,
            self.collection_save_format,
            self.prompt_save_on_exit,
            c.album,
            c.bpm,
            c.key,
            c.stems,
            c.duration,
            c.genre,
            c.rating,
            c.comment,
            c.remixer,
            c.label,
            c.year,
            c.bitrate,
            c.format,
            c.track_number,
            c.energy,
            folders_json,
        )
    }

    pub fn apply_json_str(&mut self, text: &str) {
        for line in text.lines() {
            let trimmed = line.trim().trim_end_matches(',');
            if let Some((k, v)) = trimmed.split_once(':') {
                let key = k.trim().trim_matches('"');
                let val = v.trim().trim_matches('"');
                match key {
                    "sample_rate" => if let Ok(n) = val.parse() { self.sample_rate = n; },
                    "buffer_size" => if let Ok(n) = val.parse() { self.buffer_size = n; },
                    "audio_device_name" => self.audio_device_name = val.to_string(),
                    "four_deck_mode" => self.four_deck_mode = val == "true",
                    "waveform_color_mode" => if let Ok(n) = val.parse() { self.waveform_color_mode = n; },
                    "eq_model_idx" => if let Ok(n) = val.parse() { self.eq_model_idx = n; },
                    "filter_model_idx" => if let Ok(n) = val.parse() { self.filter_model_idx = n; },
                    "filter_resonance" => if let Ok(n) = val.parse() { self.filter_resonance = n; },
                    "xfader_curve_idx" => if let Ok(n) = val.parse() { self.xfader_curve_idx = n; },
                    "autogain" => self.autogain = val == "true",
                    "limiter_enabled" => self.limiter_enabled = val == "true",
                    "lock_playing_decks" => self.lock_playing_decks = val == "true",
                    "scan_on_startup" => self.scan_on_startup = val == "true",
                    "analyze_new_tracks" => self.analyze_new_tracks = val == "true",
                    "key_notation_idx" => if let Ok(n) = val.parse() { self.key_notation_idx = n; },
                    "collection_save_format" => if let Ok(n) = val.parse() { self.collection_save_format = n; },
                    "prompt_save_on_exit" => self.prompt_save_on_exit = val == "true",
                    "col_album" => self.library_columns.album = val == "true",
                    "col_bpm" => self.library_columns.bpm = val == "true",
                    "col_key" => self.library_columns.key = val == "true",
                    "col_stems" => self.library_columns.stems = val == "true",
                    "col_duration" => self.library_columns.duration = val == "true",
                    "col_genre" => self.library_columns.genre = val == "true",
                    "col_rating" => self.library_columns.rating = val == "true",
                    "col_comment" => self.library_columns.comment = val == "true",
                    "col_remixer" => self.library_columns.remixer = val == "true",
                    "col_label" => self.library_columns.label = val == "true",
                    "col_year" => self.library_columns.year = val == "true",
                    "col_bitrate" => self.library_columns.bitrate = val == "true",
                    "col_format" => self.library_columns.format = val == "true",
                    "col_track_number" => self.library_columns.track_number = val == "true",
                    "music_folders" => {
                        self.music_folders = v
                            .trim()
                            .trim_start_matches('[')
                            .trim_end_matches(']')
                            .split("\",")
                            .map(|f| f.trim().trim_matches('"'))
                            .filter(|f| !f.is_empty())
                            .map(std::path::PathBuf::from)
                            .collect();
                    }
                    "col_energy" => self.library_columns.energy = val == "true",
                    _ => {}
                }
            }
        }
    }

    /// Renders an accessible, high-contrast settings card container with consistent border and padding.
    pub fn render_card<R>(
        ui: &mut Ui,
        title: &str,
        description: &str,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL_ALT())
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()))
            .rounding(4.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong().color(TraktorTheme::TEXT_PRIMARY()));
                        if !description.is_empty() {
                            ui.label(RichText::new(format!("— {description}")).size(12.0).color(TraktorTheme::TEXT_MUTED()));
                        }
                    });
                    ui.add_space(8.0);
                    content(ui)
                })
                .inner
            })
            .inner
    }

    /// Renders a single option row in a vertical list-type view: Label + Description on the left, Control on the right.
    pub fn list_row<R>(
        ui: &mut Ui,
        label: &str,
        description: &str,
        control: impl FnOnce(&mut Ui) -> R,
    ) -> R {
        let out = egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(4.0, 7.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.set_min_height(28.0);
                ui.horizontal(|ui| {
                    let label_col_w = (ui.available_width() * 0.52).clamp(340.0, 480.0);
                    ui.allocate_ui_with_layout(Vec2::new(label_col_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            RichText::new(label)
                                .color(TraktorTheme::TEXT_PRIMARY())
                                .font(egui::FontId::proportional(13.0))
                                .strong(),
                        );
                        if !description.is_empty() {
                            ui.label(
                                RichText::new(description)
                                    .color(TraktorTheme::TEXT_SECONDARY())
                                    .font(egui::FontId::proportional(11.0)),
                            );
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        control(ui)
                    })
                    .inner
                })
                .inner
            });
        let r = out.response.rect;
        ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0_f32, TraktorTheme::BG_PANEL()));
        out.inner
    }

    /// Section heading in dot font with a short dot rule.
    pub fn list_section_header(ui: &mut Ui, title: &str) {
        ui.add_space(18.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.label(
                RichText::new(title.to_uppercase())
                    .font(crate::theme::dot_font(13.0))
                    .color(TraktorTheme::TEXT_PRIMARY()),
            );
            let (rect, _) = ui.allocate_exact_size(Vec2::new(32.0, 14.0), Sense::hover());
            let painter = ui.painter();
            let cy = rect.center().y;
            crate::dot::paint_dot_rule(painter, Pos2::new(rect.left() + 2.0, cy), 6, 5.0, 1.2, TraktorTheme::BORDER_ACCENT());
        });
        ui.add_space(3.0);
    }

    pub fn get_tab_breadcrumb(&self, tab: PrefsTab) -> (&'static str, &'static str) {
        match tab {
            PrefsTab::AudioSetup => ("AUDIO & ROUTING", "Audio Setup"),
            PrefsTab::OutputRouting => ("AUDIO & ROUTING", "Output Routing"),
            PrefsTab::InputRouting => ("AUDIO & ROUTING", "Input Routing"),
            PrefsTab::ExternalSync => ("AUDIO & ROUTING", "External Sync & Clock"),
            PrefsTab::Mixer => ("DECKS & MIXER", "Mixer & EQ"),
            PrefsTab::Decks => ("DECKS & MIXER", "Decks Layout & Waveforms"),
            PrefsTab::TrackDecks => ("DECKS & MIXER", "Track Decks & Vinyl Scratch"),
            PrefsTab::Effects => ("DECKS & MIXER", "Effects & Mixer FX"),
            PrefsTab::StemsAndAi => ("STEMS & RECORDING", "Stems & Demucs AI"),
            PrefsTab::Recording => ("STEMS & RECORDING", "Recording & Broadcast"),
            PrefsTab::BrowserColumns => ("COLLECTION & HARDWARE", "Browser & Metadata Columns"),
            PrefsTab::FileManagement => ("COLLECTION & HARDWARE", "File Management & Folders"),
            PrefsTab::AnalyzeOptions => ("COLLECTION & HARDWARE", "Analyze & Musical Key"),
            PrefsTab::Controllers => ("COLLECTION & HARDWARE", "Controller Manager & Remapper"),
            PrefsTab::OtherControllers => ("COLLECTION & HARDWARE", "Other Controllers"),
            PrefsTab::Look => ("LOOK", "Theme & colours"),
            PrefsTab::Ai => ("AI ASSISTANT", "API Key & Assistant Setup"),
            PrefsTab::About => ("COLLECTION & HARDWARE", "About & Diagnostics"),
        }
    }

    /// Settings pages grouped into the top-level tabs.
    pub const GROUPS: [(&'static str, &'static [PrefsTab]); 8] = [
        ("Audio", &[PrefsTab::AudioSetup, PrefsTab::OutputRouting, PrefsTab::InputRouting, PrefsTab::ExternalSync]),
        ("Decks & Mixer", &[PrefsTab::Decks, PrefsTab::TrackDecks, PrefsTab::Mixer, PrefsTab::Effects]),
        ("Library", &[PrefsTab::FileManagement, PrefsTab::BrowserColumns, PrefsTab::AnalyzeOptions]),
        ("Stems & Recording", &[PrefsTab::StemsAndAi, PrefsTab::Recording]),
        ("Controllers", &[PrefsTab::Controllers, PrefsTab::OtherControllers]),
        ("Look", &[PrefsTab::Look]),
        ("AI", &[PrefsTab::Ai]),
        ("About", &[PrefsTab::About]),
    ];

    fn current_group(&self) -> &'static [PrefsTab] {
        Self::GROUPS.iter().map(|g| g.1).find(|g| g.contains(&self.current_tab)).unwrap_or(&[PrefsTab::AudioSetup])
    }

    /// Renders the top-level tab strip styled like the top bar layout tabs.
    pub fn render_top_section_tabs(&mut self, ui: &mut Ui) {
        let query = self.search_query.trim().to_lowercase();
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(3.0, 4.0);
            ui.horizontal(|ui| {
                for (idx, (label, tabs)) in Self::GROUPS.iter().enumerate() {
                    let matches = query.is_empty()
                        || label.to_lowercase().contains(&query)
                        || tabs.iter().any(|t| self.get_tab_breadcrumb(*t).1.to_lowercase().contains(&query));
                    if !matches {
                        continue;
                    }
                    let selected = tabs.contains(&self.current_tab);
                    let shortcut_tip = format!("{} (Ctrl+{})", label, idx + 1);
                    let t = crate::theme::fade(ui.ctx(), ui.id().with(("pref_group", idx)), selected);
                    let idle = TraktorTheme::CONTROL();
                    let fill = crate::theme::mix(idle, TraktorTheme::ROW_SELECTED(), t);
                    let fg = if t > 0.5 { on_selected_text() } else { TraktorTheme::TEXT_PRIMARY() };
                    let button = egui::Button::new(RichText::new(*label).size(11.5).color(fg))
                        .min_size(Vec2::new(60.0, 24.0));
                    let mut resp = if t > 0.0 {
                        ui.add(button.fill(fill))
                    } else {
                        ui.add(button)
                    };
                    if resp.hovered() || resp.has_focus() {
                        ui.painter().rect_stroke(resp.rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::TEXT_SECONDARY()));
                    }
                    resp = resp.on_hover_text(shortcut_tip);
                    if resp.clicked() {
                        self.current_tab = tabs[0];
                    }
                }
            });
        });
    }

    fn show_section(&mut self, ui: &mut Ui, tab: PrefsTab) {
        match tab {
            PrefsTab::AudioSetup => self.show_audio_setup(ui),
            PrefsTab::OutputRouting => self.show_output_routing(ui),
            PrefsTab::InputRouting => self.show_input_routing(ui),
            PrefsTab::ExternalSync => self.show_external_sync(ui),
            PrefsTab::Mixer => self.show_mixer_prefs(ui),
            PrefsTab::Decks => self.show_deck_prefs(ui),
            PrefsTab::TrackDecks => self.show_track_decks_prefs(ui),
            PrefsTab::Effects => self.show_effects_prefs(ui),
            PrefsTab::StemsAndAi => self.show_stems_prefs(ui),
            PrefsTab::Recording => self.show_recording_prefs(ui),
            PrefsTab::BrowserColumns => self.show_browser_columns_prefs(ui),
            PrefsTab::FileManagement => self.show_file_management(ui),
            PrefsTab::AnalyzeOptions => self.show_analyze_prefs(ui),
            PrefsTab::Controllers => self.show_controller_prefs(ui),
            PrefsTab::OtherControllers => self.show_other_controllers_prefs(ui),
            PrefsTab::Look => crate::theme::editor(ui),
            PrefsTab::Ai => self.show_ai_prefs(ui),
            PrefsTab::About => self.show_about(ui),
        }
    }

    /// Renders Preferences as a full-page view with top section tabs and a vertical list-type options view.
    pub fn show_full_page(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);

            // 1. Top Header Bar
            egui::Frame::none()
                .fill(TraktorTheme::BG_WINDOW())
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Preferences")
                                .color(TraktorTheme::TEXT_PRIMARY())
                                .size(20.0)
                                .strong(),
                        );
                        ui.add_space(8.0);


                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Close (Esc)")
                                            .color(on_selected_text())
                                            .font(egui::FontId::proportional(12.0))
                                            .strong(),
                                    )
                                    .fill(TraktorTheme::ROW_SELECTED())
                                    .min_size(Vec2::new(110.0, 28.0)),
                                )
                                .on_hover_text("Save preferences and return to Decks (Esc)")
                                .clicked()
                            {
                                self.save();
                                self.is_open = false;
                            }

                            ui.add_space(12.0);

                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.search_query)
                                        .hint_text("Search settings…")
                                        .desired_width(220.0),
                                );
                                if !self.search_query.is_empty() && ui.small_button("Clear").clicked() {
                                    self.search_query.clear();
                                }
                            });
                        });
                    });
                });

            // 2. Top Horizontal Section Tabs Strip
            egui::Frame::none()
                .fill(TraktorTheme::BG_WINDOW())
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(16.0, 8.0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.render_top_section_tabs(ui);
                });

            // 3. Full-Width Scrollable List-Type Options View
            egui::Frame::none()
                .fill(TraktorTheme::BG_WINDOW())
                .inner_margin(egui::Margin::symmetric(20.0, 12.0))
                .show(ui, |ui| {
                    let avail_w = ui.available_width();
                    let avail_h = ui.available_height();
                    ui.set_min_size(Vec2::new(avail_w, avail_h));

                    egui::ScrollArea::vertical()
                        .id_salt("prefs_main_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.vertical(|ui| {
                                ui.set_min_width(avail_w - 16.0);
                                ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);

                                for tab in self.current_group() {
                                    if self.current_group() == &[PrefsTab::Controllers, PrefsTab::OtherControllers] && *tab != self.current_tab {
                                        continue;
                                    }
                                    self.show_section(ui, *tab);
                                    ui.add_space(12.0);
                                }
                            });
                        });
                });
        });
    }

    /// Backwards compatibility shim
    pub fn show(&mut self, _ctx: &egui::Context) {}

    fn show_audio_setup(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Sound card");

        let device_label = match self.audio_device_name.as_str() {
            "PipeWire pro-audio / ALSA Default" => "System default (PipeWire / ALSA)",
            "Traktor Kontrol / MX2 USB Audio (hw:MX2,0)" => "Traktor MX2 USB audio",
            "USB Audio Class 2.0 Interface (hw:UAC2,0)" => "USB audio interface",
            other => other,
        }
        .to_string();
        Self::list_row(
            ui,
            "Audio device",
            "Which sound card or audio output ReTraktor plays through.",
            |ui| {
                egui::ComboBox::from_id_salt("audio_device_combo")
                    .selected_text(device_label)
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.audio_device_name,
                            "PipeWire pro-audio / ALSA Default".to_string(),
                            "System default (PipeWire / ALSA)",
                        );
                        ui.selectable_value(
                            &mut self.audio_device_name,
                            "Traktor Kontrol / MX2 USB Audio (hw:MX2,0)".to_string(),
                            "Traktor MX2 USB audio",
                        );
                        ui.selectable_value(
                            &mut self.audio_device_name,
                            "USB Audio Class 2.0 Interface (hw:UAC2,0)".to_string(),
                            "USB audio interface",
                        );
                    });
            },
        );

        Self::list_row(
            ui,
            "Sample rate",
            "Audio rate in Hz; 48000 Hz matches most DJ controllers.",
            |ui| {
                egui::ComboBox::from_id_salt("sr_combo")
                    .selected_text(format!("{} Hz", self.sample_rate))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.sample_rate, 44100, "44100 Hz (CD)");
                        ui.selectable_value(&mut self.sample_rate, 48000, "48000 Hz (standard)");
                        ui.selectable_value(&mut self.sample_rate, 88200, "88200 Hz");
                        ui.selectable_value(&mut self.sample_rate, 96000, "96000 Hz (high)");
                    });
            },
        );

        let latency_ms = (self.buffer_size as f64 / self.sample_rate as f64) * 1000.0;
        Self::list_row(
            ui,
            "Buffer size",
            "Smaller values feel more responsive when scratching; larger values prevent crackles.",
            |ui| {
                egui::ComboBox::from_id_salt("buf_combo")
                    .selected_text(format!("{} samples ({:.1} ms)", self.buffer_size, latency_ms))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.buffer_size, 64, "64 samples (1.3 ms, fast)");
                        ui.selectable_value(&mut self.buffer_size, 128, "128 samples (2.7 ms, balanced)");
                        ui.selectable_value(&mut self.buffer_size, 256, "256 samples (5.3 ms, safe)");
                        ui.selectable_value(&mut self.buffer_size, 512, "512 samples (10.7 ms, relaxed)");
                    });
            },
        );

        Self::list_row(
            ui,
            "Audio status",
            "Live round-trip latency and audio dropouts.",
            |ui| {
                ui.label(
                    RichText::new(format!("Running · {:.1} ms latency · 0 dropouts", latency_ms))
                        .color(TraktorTheme::PLAY_ACTIVE())
                        .font(egui::FontId::monospace(11.5))
                        .strong(),
                );
            },
        );
    }

    fn show_output_routing(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Outputs");

        Self::list_row(
            ui,
            "Mixing mode",
            "Mix inside ReTraktor or send each deck to an external hardware mixer.",
            |ui| {
                let modes = [
                    "Internal mixer",
                    "External mixer",
                ];
                egui::ComboBox::from_id_salt("mixing_mode_combo")
                    .selected_text(modes[self.mixing_mode.min(1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mixing_mode, 0, modes[0]);
                        ui.selectable_value(&mut self.mixing_mode, 1, modes[1]);
                    });
            },
        );

        Self::list_row(
            ui,
            "Main output",
            "Stereo channels sent to your main speakers.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.master_out_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.master_out_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Headphones",
            "Stereo channels used for headphone cueing.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.monitor_out_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.monitor_out_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Booth output",
            "Second stereo output for booth speakers with its own volume.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.booth_out_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.booth_out_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Split cue",
            "Put the cued deck in the left headphone ear and the main mix in the right.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.split_cue, TraktorTheme::PLAY_ACTIVE(), "Split cue");
            },
        );
    }

    fn show_input_routing(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Inputs");

        Self::list_row(
            ui,
            "Deck A input",
            "Stereo input channels for a turntable or CDJ on Deck A.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.deck_a_in_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.deck_a_in_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Deck B input",
            "Stereo input channels for a turntable or CDJ on Deck B.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.deck_b_in_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.deck_b_in_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Aux or microphone",
            "Extra stereo or mic input sent straight to the main mix.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.aux_in_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.aux_in_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Timecode vinyl",
            "Control playback speed and position with timecode records or CDs.",
            |ui| {
                ui.label(
                    RichText::new("Ready")
                        .color(TraktorTheme::DECK_A())
                        .font(egui::FontId::monospace(11.0))
                        .strong(),
                );
            },
        );
    }

    fn show_external_sync(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Sync");

        Self::list_row(
            ui,
            "Tempo leader",
            "Follow the playing deck automatically or hold a fixed master tempo.",
            |ui| {
                let modes = ["Follow playing deck", "Manual tempo"];
                egui::ComboBox::from_id_salt("clock_mode_combo")
                    .selected_text(modes[self.clock_mode.min(1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.clock_mode, 0, modes[0]);
                        ui.selectable_value(&mut self.clock_mode, 1, modes[1]);
                    });
            },
        );

        Self::list_row(
            ui,
            "Send MIDI clock",
            "Keep external drum machines and synths in time over MIDI.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.send_midi_clock, TraktorTheme::PLAY_ACTIVE(), "Send MIDI clock");
            },
        );

        Self::list_row(
            ui,
            "MIDI clock offset",
            "Nudge the MIDI clock early or late to line up external gear.",
            |ui| {
                let txt = format!("{:.1} ms", self.midi_clock_offset_ms);
                crate::dot::dot_slider(ui, &mut self.midi_clock_offset_ms, -50.0, 50.0, 0.0, &txt, TraktorTheme::ACCENT())
                    .on_hover_text("Compensate for hardware or USB latency in milliseconds");
            },
        );

        Self::list_row(
            ui,
            "Ableton Link",
            "Share tempo and beat phase with other music apps on your network.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.ableton_link_enabled, TraktorTheme::PLAY_ACTIVE(), "Ableton Link");
            },
        );
    }

    fn show_mixer_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Mixer");

        let models = [
            "Z ISO (full kill)",
            "Classic",
            "P600 (club)",
            "Nuo (warm lows)",
            "Xone:92 (4-band)",
        ];
        Self::list_row(
            ui,
            "EQ style",
            "How the high, mid, and low knobs shape and cut each frequency band.",
            |ui| {
                egui::ComboBox::from_id_salt("eq_model_combo")
                    .selected_text(models[self.eq_model_idx.min(models.len() - 1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, m) in models.iter().enumerate() {
                            ui.selectable_value(&mut self.eq_model_idx, i, *m);
                        }
                    });
            },
        );

        let filters = [
            "Ladder (warm, resonant)",
            "Clean",
            "Synth (Pro-53)",
        ];
        Self::list_row(
            ui,
            "Filter style",
            "Character of the channel filter knob.",
            |ui| {
                egui::ComboBox::from_id_salt("filter_model_combo")
                    .selected_text(filters[self.filter_model_idx.min(filters.len() - 1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, f) in filters.iter().enumerate() {
                            ui.selectable_value(&mut self.filter_model_idx, i, *f);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Filter resonance",
            "How sharp the filter sounds around its cutoff point.",
            |ui| {
                let txt = format!("{:.2}", self.filter_resonance);
                crate::dot::dot_slider(ui, &mut self.filter_resonance, 0.0, 1.0, 0.5, &txt, TraktorTheme::ACCENT())
                    .on_hover_text("How sharp the filter sounds around its cutoff point.");
            },
        );

        let curves = [
            "Smooth blend",
            "Steep blend",
            "Sharp cut (scratch)",
        ];
        Self::list_row(
            ui,
            "Crossfader curve",
            "Blend smoothly across the middle or cut in sharply at the edges for scratching.",
            |ui| {
                egui::ComboBox::from_id_salt("xf_curve_combo")
                    .selected_text(curves[self.xfader_curve_idx.min(curves.len() - 1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, c) in curves.iter().enumerate() {
                            ui.selectable_value(&mut self.xfader_curve_idx, i, *c);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Auto gain",
            "Match volume across quiet and loud tracks automatically.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.autogain, TraktorTheme::PLAY_ACTIVE(), "Auto gain");
            },
        );

        Self::list_row(
            ui,
            "Master limiter",
            "Keep loud peaks on the main output from clipping.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.limiter_enabled, TraktorTheme::PLAY_ACTIVE(), "Master limiter");
            },
        );

        if self.limiter_enabled {
            let headrooms = [
                "None (0 dB)",
                "-3 dB (recommended)",
                "-6 dB (extra headroom)",
            ];
            Self::list_row(
                ui,
                "Headroom",
                "Leave breathing room before the limiter kicks in.",
                |ui| {
                    egui::ComboBox::from_id_salt("limiter_headroom_combo")
                        .selected_text(headrooms[self.limiter_headroom.min(2)])
                        .width(220.0)
                        .show_ui(ui, |ui| {
                            for (i, h) in headrooms.iter().enumerate() {
                                ui.selectable_value(&mut self.limiter_headroom, i, *h);
                            }
                        });
                },
            );
        }

        Self::list_row(
            ui,
            "EQ controls stems",
            "Use the high, mid, and low EQ knobs to adjust vocals, instruments, and drums.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.neural_mix_eq_mode, TraktorTheme::PLAY_ACTIVE(), "EQ controls stems");
            },
        );
    }

    fn show_stems_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Stems");

        Self::list_row(
            ui,
            "Stem separation (Demucs)",
            "Splits tracks into drums, bass, instruments, and vocals in the background.",
            |ui| match &self.demucs_install_state {
                crate::demucs::DemucsInstallState::Ready { version, .. } => {
                    ui.label(
                        RichText::new(format!("{version} ready"))
                            .color(TraktorTheme::PLAY_ACTIVE())
                            .strong(),
                    );
                }
                crate::demucs::DemucsInstallState::Installing { step, percent } => {
                    ui.add(egui::ProgressBar::new(*percent).desired_width(140.0).show_percentage());
                    ui.label(RichText::new(step).color(TraktorTheme::METER_YELLOW()));
                }
                crate::demucs::DemucsInstallState::NotInstalled => {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Install Demucs (~1.2 GB)")
                                    .color(on_selected_text())
                                    .strong(),
                            )
                            .fill(TraktorTheme::ROW_SELECTED())
                            .min_size(Vec2::new(200.0, 26.0)),
                        )
                        .clicked()
                    {
                        self.trigger_demucs_install = true;
                    }
                }
                crate::demucs::DemucsInstallState::Failed { error } => {
                    if ui.button("Retry install").clicked() {
                        self.trigger_demucs_install = true;
                    }
                    ui.label(RichText::new(error_summary(error)).color(TraktorTheme::METER_RED()));
                }
            },
        );
        // Full log stays out of the row; available on request, selectable and copyable.
        if let crate::demucs::DemucsInstallState::Failed { error } = &self.demucs_install_state {
            egui::CollapsingHeader::new(RichText::new("Details").size(11.5).color(TraktorTheme::TEXT_SECONDARY()))
                .id_salt("demucs_error_details")
                .show(ui, |ui| {
                    if ui.small_button("Copy").clicked() {
                        ui.ctx().copy_text(error.clone());
                    }
                    egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                        let mut text = error.as_str();
                        ui.add(egui::TextEdit::multiline(&mut text).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                    });
                });
        }

        let stem_modes = [
            "Automatic (Demucs + live fallback)",
            "Live frequency split only",
            "Saved stem files only",
        ];
        Self::list_row(
            ui,
            "Separation mode",
            "Use Demucs when available, live frequency splitting, or saved stem files.",
            |ui| {
                egui::ComboBox::from_id_salt("stem_engine_mode_combo")
                    .selected_text(stem_modes[self.stems_engine_mode.min(2)])
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for (i, m) in stem_modes.iter().enumerate() {
                            ui.selectable_value(&mut self.stems_engine_mode, i, *m);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "EQ controls stems",
            "Use the high, mid, and low EQ knobs to adjust vocals, instruments, and drums.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.neural_mix_eq_mode, TraktorTheme::PLAY_ACTIVE(), "EQ controls stems");
            },
        );
    }

    fn show_deck_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Waveforms");

        Self::list_row(
            ui,
            "Four decks",
            "Show Decks C and D alongside Decks A and B.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.four_deck_mode, TraktorTheme::PLAY_ACTIVE(), "Four decks");
            },
        );

        let palettes = [
            "Spectrum",
            "Ultraviolet",
            "High contrast",
        ];
        Self::list_row(
            ui,
            "Waveform colours",
            "Colour scheme used to draw bass, mids, and highs on waveforms.",
            |ui| {
                egui::ComboBox::from_id_salt("wf_palette_combo")
                    .selected_text(palettes[self.waveform_color_mode.min(2)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, p) in palettes.iter().enumerate() {
                            ui.selectable_value(&mut self.waveform_color_mode, i, *p);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Minute markers",
            "Draw a tick every minute along the full-track overview bar.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.show_minute_markers, TraktorTheme::PLAY_ACTIVE(), "Minute markers");
            },
        );

        Self::list_row(
            ui,
            "Beat markers",
            "Draw beat and bar lines on the scrolling waveform.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.show_beatgrid_ticks, TraktorTheme::PLAY_ACTIVE(), "Beat markers");
            },
        );

        Self::list_row(
            ui,
            "Fit full track in overview",
            "Always fit the entire song into the small overview stripe.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.stripe_fit_full, TraktorTheme::PLAY_ACTIVE(), "Fit full track in overview");
            },
        );
    }

    fn show_track_decks_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Decks");

        Self::list_row(
            ui,
            "Lock playing decks",
            "Stop a new track from loading onto a deck that is currently playing.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.lock_playing_decks, TraktorTheme::PLAY_ACTIVE(), "Lock playing decks");
            },
        );

        let ranges = [
            "±8%",
            "±16%",
            "±32%",
            "±50%",
            "±100%",
        ];
        Self::list_row(
            ui,
            "Tempo fader range",
            "How far the tempo slider can speed up or slow down a track.",
            |ui| {
                egui::ComboBox::from_id_salt("pitch_range_combo")
                    .selected_text(ranges[self.pitch_range_idx.min(4)])
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for (i, r) in ranges.iter().enumerate() {
                            ui.selectable_value(&mut self.pitch_range_idx, i, *r);
                        }
                    });
            },
        );

        let modes = ["High quality", "Eco (lower CPU)"];
        Self::list_row(
            ui,
            "Key lock quality",
            "Sound quality vs CPU use when keeping musical key fixed across tempo changes.",
            |ui| {
                egui::ComboBox::from_id_salt("keylock_mode_combo")
                    .selected_text(modes[self.keylock_quality.min(1)])
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (i, m) in modes.iter().enumerate() {
                            ui.selectable_value(&mut self.keylock_quality, i, *m);
                        }
                    });
            },
        );

        let positions = ["Centre", "Left of centre"];
        Self::list_row(
            ui,
            "Playhead position",
            "Where the playhead sits on the scrolling waveform.",
            |ui| {
                egui::ComboBox::from_id_salt("play_marker_combo")
                    .selected_text(positions[self.play_marker_pos.min(1)])
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for (i, p) in positions.iter().enumerate() {
                            ui.selectable_value(&mut self.play_marker_pos, i, *p);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Auto cue on load",
            "Park newly loaded tracks at their first cue point.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.auto_cue_on_load, TraktorTheme::PLAY_ACTIVE(), "Jump to cue on load");
            },
        );

        Self::list_row(
            ui,
            "Jog wheel sensitivity",
            "How far the track moves when you spin the jog wheel.",
            |ui| {
                let txt = format!("{:.1}x", self.jog_scratch_sensitivity);
                crate::dot::dot_slider(ui, &mut self.jog_scratch_sensitivity, 0.2, 3.0, 1.0, &txt, TraktorTheme::ACCENT())
                    .on_hover_text("Jog wheel sensitivity when scratching");
            },
        );

        Self::list_row(
            ui,
            "Vinyl mode",
            "Touch the top of the platter to scratch, or use the outer rim to nudge tempo.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.jog_mode_turntable, TraktorTheme::PLAY_ACTIVE(), "Vinyl mode");
            },
        );
    }

    fn show_effects_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Effects");

        let unit_counts = ["2 units", "4 units"];
        Self::list_row(
            ui,
            "FX units",
            "How many effect units appear at the top of the screen.",
            |ui| {
                egui::ComboBox::from_id_salt("fx_units_count_combo")
                    .selected_text(unit_counts[self.fx_units_count.min(1)])
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.fx_units_count, 0, unit_counts[0]);
                        ui.selectable_value(&mut self.fx_units_count, 1, unit_counts[1]);
                    });
            },
        );

        let panel_modes = [
            "Group (3 effects)",
            "Single (detailed)",
        ];
        Self::list_row(
            ui,
            "FX unit mode",
            "Chain three simple effects together or control one effect in detail.",
            |ui| {
                egui::ComboBox::from_id_salt("fx_panel_mode_combo")
                    .selected_text(panel_modes[self.fx_panel_mode.min(1)])
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.fx_panel_mode, 0, panel_modes[0]);
                        ui.selectable_value(&mut self.fx_panel_mode, 1, panel_modes[1]);
                    });
            },
        );

        Self::list_section_header(ui, "Channel FX");
        let fx_options = [
            "Filter",
            "Reverb",
            "Dual delay",
            "Noise sweep",
            "Time gater",
            "Barber pole phaser",
            "Flanger",
            "Dotted eighth delay",
            "Bit crush",
        ];

        for idx in 0..4 {
            let mut slot_val = self.mixer_fx_slots[idx].min(fx_options.len() - 1);
            Self::list_row(
                ui,
                &format!("Slot {}", idx + 1),
                "One-knob effect available on each mixer channel.",
                |ui| {
                    egui::ComboBox::from_id_salt(format!("mfx_slot_{idx}"))
                        .selected_text(fx_options[slot_val])
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (opt_idx, name) in fx_options.iter().enumerate() {
                                ui.selectable_value(&mut slot_val, opt_idx, *name);
                            }
                        });
                },
            );
            self.mixer_fx_slots[idx] = slot_val;
        }
    }

    fn show_recording_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Recording");

        let path_str = self.recording_dir.to_string_lossy().to_string();
        Self::list_row(
            ui,
            "Recordings folder",
            "Where recorded mixes are saved as WAV files.",
            |ui| {
                // Long paths shorten to ~ and truncate instead of running over the label.
                let home = std::env::var("HOME").unwrap_or_default();
                let shown = if home.is_empty() { path_str.to_string() } else { path_str.replacen(&home, "~", 1) };
                ui.add(egui::Label::new(RichText::new(shown).monospace().color(TraktorTheme::TEXT_PRIMARY())).truncate()).on_hover_text(path_str);
            },
        );

        let formats = [
            "16-bit WAV (CD quality)",
            "24-bit WAV (studio)",
        ];
        Self::list_row(
            ui,
            "Recording quality",
            "Bit depth for recorded WAV files.",
            |ui| {
                egui::ComboBox::from_id_salt("rec_format_combo")
                    .selected_text(formats[self.recording_format.min(1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, f) in formats.iter().enumerate() {
                            ui.selectable_value(&mut self.recording_format, i, *f);
                        }
                    });
            },
        );

        Self::list_section_header(ui, "Live stream");

        Self::list_row(
            ui,
            "Stream server",
            "Address of your Icecast or Shoutcast server.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.broadcast_host).desired_width(200.0));
            },
        );

        Self::list_row(
            ui,
            "Port",
            "Server port (usually 8000).",
            |ui| {
                let mut port_str = self.broadcast_port.to_string();
                if ui
                    .add(egui::TextEdit::singleline(&mut port_str).desired_width(100.0))
                    .changed()
                {
                    if let Ok(p) = port_str.parse::<u16>() {
                        self.broadcast_port = p;
                    }
                }
            },
        );

        Self::list_row(
            ui,
            "Mount path",
            "Stream path on the server, such as /retraktor.mp3.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.broadcast_mount).desired_width(200.0));
            },
        );

        Self::list_row(
            ui,
            "Password",
            "Source password for your streaming server.",
            |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.broadcast_password)
                        .password(true)
                        .desired_width(200.0),
                );
            },
        );
    }

    fn show_browser_columns_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Columns");

        Self::list_row(
            ui,
            "Column preset",
            "Pick a starting set of columns for the track list.",
            |ui| {
                if ui.button("Minimal").clicked() {
                    self.library_columns.set_all(false);
                    self.library_columns.bpm = true;
                    self.library_columns.key = true;
                    self.library_columns.stems = true;
                    self.library_columns.duration = true;
                }
                if ui.button("All").clicked() {
                    self.library_columns.set_all(true);
                }
                if ui.button("Default").clicked() {
                    self.library_columns = LibraryColumnsConfig::default();
                }
            },
        );

        Self::list_row(
            ui,
            "Album",
            "",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.album, TraktorTheme::PLAY_ACTIVE(), "Album");
            },
        );
        Self::list_row(
            ui,
            "BPM",
            "Tempo in beats per minute.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.bpm, TraktorTheme::PLAY_ACTIVE(), "BPM");
            },
        );
        Self::list_row(
            ui,
            "Key",
            "Musical key for harmonic mixing.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.key, TraktorTheme::PLAY_ACTIVE(), "Key");
            },
        );
        Self::list_row(
            ui,
            "Stems",
            "Shows whether stems are ready and lets you split a track.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.stems, TraktorTheme::PLAY_ACTIVE(), "Stems");
            },
        );
        Self::list_row(
            ui,
            "Duration",
            "Track length in minutes and seconds.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.duration, TraktorTheme::PLAY_ACTIVE(), "Duration");
            },
        );
        Self::list_row(
            ui,
            "Genre",
            "",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.genre, TraktorTheme::PLAY_ACTIVE(), "Genre");
            },
        );
        Self::list_row(
            ui,
            "Rating",
            "Star rating from 1 to 5.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.rating, TraktorTheme::PLAY_ACTIVE(), "Rating");
            },
        );
        Self::list_row(
            ui,
            "Comment",
            "Your notes on the track.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.comment, TraktorTheme::PLAY_ACTIVE(), "Comment");
            },
        );
        Self::list_row(
            ui,
            "Remixer",
            "",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.remixer, TraktorTheme::PLAY_ACTIVE(), "Remixer");
            },
        );
        Self::list_row(
            ui,
            "Label",
            "Record label.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.label, TraktorTheme::PLAY_ACTIVE(), "Label");
            },
        );
        Self::list_row(
            ui,
            "Year",
            "",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.year, TraktorTheme::PLAY_ACTIVE(), "Year");
            },
        );
        Self::list_row(
            ui,
            "Bitrate",
            "Audio quality in kbps.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.bitrate, TraktorTheme::PLAY_ACTIVE(), "Bitrate");
            },
        );
        Self::list_row(
            ui,
            "Format",
            "File type such as FLAC, MP3, or WAV.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.format, TraktorTheme::PLAY_ACTIVE(), "Format");
            },
        );
        Self::list_row(
            ui,
            "Track number",
            "",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.track_number, TraktorTheme::PLAY_ACTIVE(), "Track number");
            },
        );
        Self::list_row(
            ui,
            "Energy",
            "Estimated dancefloor intensity from 1 to 10.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.library_columns.energy, TraktorTheme::PLAY_ACTIVE(), "Energy");
            },
        );
    }

    fn show_analyze_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Analysis");

        let ranges = [
            "70–140 BPM (house, techno, pop)",
            "60–120 BPM (hip-hop, downtempo)",
            "90–180 BPM (drum & bass, jungle)",
            "110–220 BPM (hardcore, fast)",
        ];
        Self::list_row(
            ui,
            "BPM range",
            "Expected tempo range so half-time and double-time tracks read accurately.",
            |ui| {
                egui::ComboBox::from_id_salt("bpm_range_combo")
                    .selected_text(ranges[self.bpm_detection_range.min(3)])
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        for (i, r) in ranges.iter().enumerate() {
                            ui.selectable_value(&mut self.bpm_detection_range, i, *r);
                        }
                    });
            },
        );

        let keys = [
            "Open Key (1d / 1m)",
            "Camelot (8A / 8B)",
            "Musical (Am, C)",
        ];
        Self::list_row(
            ui,
            "Key format",
            "How musical keys are written in the library and on the decks.",
            |ui| {
                egui::ComboBox::from_id_salt("key_notation_combo")
                    .selected_text(keys[self.key_notation_idx.min(2)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, k) in keys.iter().enumerate() {
                            ui.selectable_value(&mut self.key_notation_idx, i, *k);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Analyse on import",
            "Find BPM, beatgrid, and key automatically when you add files.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.analyze_new_tracks, TraktorTheme::PLAY_ACTIVE(), "Analyse on import");
            },
        );
    }

    fn show_controllers_subtabs(&mut self, ui: &mut Ui) {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(3.0, 4.0);
            ui.horizontal(|ui| {
                for (tab, label) in [
                    (PrefsTab::Controllers, "Traktor MX2"),
                    (PrefsTab::OtherControllers, "Other controllers"),
                ] {
                    let selected = self.current_tab == tab;
                    let t = crate::theme::fade(ui.ctx(), ui.id().with(("ctl_subtab", label)), selected);
                    let idle = TraktorTheme::CONTROL();
                    let fill = crate::theme::mix(idle, TraktorTheme::ROW_SELECTED(), t);
                    let fg = if t > 0.5 { on_selected_text() } else { TraktorTheme::TEXT_PRIMARY() };
                    let button = egui::Button::new(RichText::new(label).size(11.5).color(fg))
                        .min_size(Vec2::new(90.0, 24.0));
                    let resp = if t > 0.0 {
                        ui.add(button.fill(fill))
                    } else {
                        ui.add(button)
                    };
                    if resp.hovered() || resp.has_focus() {
                        ui.painter().rect_stroke(resp.rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::TEXT_SECONDARY()));
                    }
                    if resp.clicked() {
                        self.current_tab = tab;
                    }
                }
            });
        });
        ui.add_space(6.0);
    }

    fn show_controller_prefs(&mut self, ui: &mut Ui) {
        self.show_controllers_subtabs(ui);
        // 6. Line-drawn replica of the MX2: see and change what every control does.
        ui.add_space(8.0);
        let connected = self.mx2_connected_node.is_some();
        let touched = self.last_surface_control.clone();
        if crate::controller_map::show(ui, &mut self.mapping, &mut self.mapper, touched.as_deref(), connected) {
            if let Err(e) = self.mapping.save() {
                eprintln!("retraktor: could not save MX2 mapping: {e}");
            }
            self.trigger_led_test = true;
        }
        ui.add_space(14.0);
        egui::CollapsingHeader::new(RichText::new("Connection, lights test & troubleshooting").size(14.0))
            .id_salt("controller_troubleshooting")
            .default_open(self.mx2_connected_node.is_none())
            .show(ui, |ui| self.show_controller_troubleshooting(ui));
    }

    fn show_other_controllers_prefs(&mut self, ui: &mut Ui) {
        self.show_controllers_subtabs(ui);
        crate::midi::show_other_controllers_page(ui, &mut self.generic_manager, self.last_surface_control.as_deref());
    }

    fn show_controller_troubleshooting(&mut self, ui: &mut Ui) {

        // 1. Live Connection Status Banner
        let is_connected = self.mx2_connected_node.is_some();
        let (banner_bg, stroke_col, title_text, subtitle_text) = if is_connected {
            (
                TraktorTheme::BG_PANEL(),
                TraktorTheme::PLAY_ACTIVE(),
                format!("Connected: Traktor MX2 ({})", self.mx2_connected_node.as_deref().unwrap_or("")),
                format!(
                    "{} · {} packets received",
                    self.mx2_access_status.as_deref().unwrap_or("Read/write ready"),
                    self.mx2_packets_count
                ),
            )
        } else if let Some(ref status) = self.mx2_access_status {
            (
                TraktorTheme::BG_PANEL(),
                TraktorTheme::METER_YELLOW(),
                "Controller detected — permission needed".to_string(),
                status.clone(),
            )
        } else {
            (
                TraktorTheme::BG_PANEL(),
                TraktorTheme::BORDER_ACCENT(),
                "No Traktor MX2 connected".to_string(),
                "Plug in the 15V power adapter and connect the controller over USB-C.".to_string(),
            )
        };

        egui::Frame::none()
            .fill(banner_bg)
            .stroke(Stroke::new(1.0_f32, stroke_col))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title_text).color(TraktorTheme::TEXT_PRIMARY()).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Rescan").clicked() {
                            self.trigger_rescan = true;
                        }
                    });
                });
                ui.label(RichText::new(subtitle_text).color(Color32::LIGHT_GRAY).font(egui::FontId::monospace(9.5)));
            });

        ui.add_space(6.0);

        // 2. Hardware Checklist & Troubleshooting Card
        egui::CollapsingHeader::new(RichText::new("Setup and permissions").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.label("Power: Plug the 15V DC adapter into the back of the MX2. USB-C alone cannot power the sound card, platters, and lights.");
                ui.label("HID mode: If the controller is in MIDI mode, hold Left SHIFT and Right SHIFT together to switch back.");
                ui.label("Linux permissions: Install the udev rule so ReTraktor can access the controller without root.");
                ui.horizontal(|ui| {
                    if ui.button("Copy udev setup command").clicked() {
                        let cmd = "sudo cp assets/99-retraktor-ni.rules /etc/udev/rules.d/60-retraktor-controllers.rules && sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hidraw";
                        ui.output_mut(|o| o.copied_text = cmd.to_string());
                        self.udev_command_copied = true;
                    }
                    if self.udev_command_copied {
                        ui.label(RichText::new("Copied to clipboard").color(TraktorTheme::PLAY_ACTIVE()));
                    }
                });
            });

        ui.add_space(6.0);

        // 3. Hardware Test & Verification Actions
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Test lights").strong())).clicked() {
                self.trigger_led_test = true;
            }
            if ui.add(egui::Button::new(RichText::new("Run self-test").strong())).clicked() {
                self.trigger_self_test = true;
            }
        });

        if let Some(ref report) = self.self_test_report {
            ui.add_space(4.0);
            egui::Frame::none()
                .fill(TraktorTheme::BG_PANEL())
                .stroke(Stroke::new(1.0_f32, TraktorTheme::PLAY_ACTIVE()))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("Self-test passed").color(TraktorTheme::PLAY_ACTIVE()).strong());
                    ui.label(format!(
                        "Buttons: {} ({}), encoders: {} ({}), knobs and faders: {} ({}), jog wheels: {} ({}), lights: {} ({})",
                        report.buttons_tested, report.buttons_passed,
                        report.encoders_tested, report.encoders_passed,
                        report.pots_faders_tested, report.pots_faders_passed,
                        report.jogwheels_tested, report.jogwheels_passed,
                        report.leds_tested, report.leds_passed
                    ));
                });
        }

        ui.add_space(6.0);

        // 4. Live HID Packet & Control Inspector
        egui::CollapsingHeader::new(RichText::new("Live input monitor").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let r_id = self.mx2_last_report_id.map(|id| format!("0x{:02X}", id)).unwrap_or_else(|| "None".to_string());
                    ui.label(RichText::new(format!("Last report: {}", r_id)).monospace().color(TraktorTheme::DECK_A()));
                    ui.label(RichText::new(format!("Packets: {}", self.mx2_packets_count)).monospace());
                });
                if let Some(ref hex) = self.mx2_last_hex {
                    ui.label(RichText::new(format!("Raw data: {}", hex)).monospace().color(Color32::LIGHT_GRAY).font(egui::FontId::monospace(9.0)));
                }
                if !self.mx2_recent_events.is_empty() {
                    ui.label(RichText::new("Recent events:").strong());
                    for ev in self.mx2_recent_events.iter().rev().take(4) {
                        ui.label(RichText::new(format!("  {}", ev)).monospace().color(TraktorTheme::PLAY_ACTIVE()).font(egui::FontId::monospace(9.5)));
                    }
                } else {
                    ui.label(RichText::new("Move any button, knob, fader, or jog wheel on the controller to see input here.").italics());
                }
            });

        ui.add_space(6.0);

        // 5. Detected Hardware Devices Table
        ui.label(RichText::new("Connected audio and USB devices").strong());
        egui::ScrollArea::vertical()
            .id_salt("prefs_discovered_hw_scroll")
            .max_height(110.0)
            .show(ui, |ui| {
                egui::Grid::new("prefs_discovered_hw_grid")
                    .striped(true)
                    .spacing(Vec2::new(10.0, 3.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Type").color(TraktorTheme::TEXT_SECONDARY()).strong());
                        ui.label(RichText::new("Path").color(TraktorTheme::TEXT_SECONDARY()).strong());
                        ui.label(RichText::new("Device").color(TraktorTheme::TEXT_SECONDARY()).strong());
                        ui.label(RichText::new("ID").color(TraktorTheme::TEXT_SECONDARY()).strong());
                        ui.label(RichText::new("Status").color(TraktorTheme::TEXT_SECONDARY()).strong());
                        ui.end_row();

                        for item in &self.discovered_hardware {
                            ui.label(RichText::new(item.category).monospace());
                            ui.label(RichText::new(&item.identifier).monospace().color(TraktorTheme::DECK_A()));
                            ui.label(RichText::new(&item.name).strong());
                            ui.label(
                                if let Some((v, p)) = item.vid_pid {
                                    RichText::new(format!("{:04X}:{:04X}", v, p)).monospace()
                                } else {
                                    RichText::new("-").monospace()
                                }
                            );
                            if item.category == "HIDRAW" {
                                match item.access {
                                    retraktor_hid::DeviceAccessLevel::ReadWriteOk => {
                                        let is_cur = self.mx2_connected_node.as_ref() == Some(&item.identifier);
                                        if is_cur {
                                            ui.label(RichText::new("Connected").color(TraktorTheme::PLAY_ACTIVE()).strong());
                                        } else if ui.button("Connect").clicked() {
                                            self.trigger_connect_node = Some(item.identifier.clone());
                                        }
                                    }
                                    retraktor_hid::DeviceAccessLevel::ReadOnlyOk => {
                                        ui.label(RichText::new("Read-only").color(TraktorTheme::METER_YELLOW()));
                                    }
                                    retraktor_hid::DeviceAccessLevel::PermissionDenied => {
                                        ui.label(RichText::new("Permission denied").color(TraktorTheme::METER_RED()));
                                    }
                                    _ => {
                                        ui.label("-");
                                    }
                                }
                            } else {
                                ui.label("-");
                            }
                            ui.end_row();
                        }
                    });
            });

        Self::list_row(
            ui,
            "Under-glow brightness",
            "Adjust base LED intensity on Traktor MX2.",
            |ui| {
                let disp = format!("{:.0}%", self.mx2_bottom_led_brightness * 100.0);
                crate::dot::dot_slider(
                    ui,
                    &mut self.mx2_bottom_led_brightness,
                    0.1,
                    1.0,
                    0.5,
                    &disp,
                    TraktorTheme::ACCENT(),
                );
            },
        );
    }

    fn show_file_management(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Collection");
        Self::list_row(
            ui,
            "Autosave",
            "Your collection saves automatically to ~/.local/share/retraktor/collection.nml.",
            |ui| {
                let mut m3u = self.collection_save_format == 2;
                if crate::dot::dot_toggle(ui, &mut m3u, TraktorTheme::PLAY_ACTIVE(), "Export m3u8 on exit").changed() {
                    self.collection_save_format = if m3u { 2 } else { 0 };
                }
                ui.label(RichText::new("Also export collection.m3u8 on exit").size(11.0).color(TraktorTheme::TEXT_SECONDARY()));
            },
        );

        Self::list_section_header(ui, "Music folders");

        let mut to_remove = None;
        for (idx, folder) in self.music_folders.iter().enumerate() {
            let folder_str = folder.to_string_lossy().to_string();
            Self::list_row(
                ui,
                &folder_str,
                "",
                |ui| {
                    if ui
                        .button(RichText::new("Remove").color(TraktorTheme::METER_RED()))
                        .clicked()
                    {
                        to_remove = Some(idx);
                    }
                },
            );
        }
        if let Some(idx) = to_remove {
            self.music_folders.remove(idx);
        }

        Self::list_row(
            ui,
            "Add folder",
            "Choose a folder of music files to include in your library.",
            |ui| {
                if ui.button("Choose folder…").clicked() {
                    self.request_folder_pick = true;
                }
            },
        );

        Self::list_section_header(ui, "Scanning");

        Self::list_row(
            ui,
            "Scan on startup",
            "Check your music folders for new tracks when ReTraktor opens.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.scan_on_startup, TraktorTheme::PLAY_ACTIVE(), "Scan on startup");
            },
        );

        Self::list_row(
            ui,
            "Analyse new tracks",
            "Detect BPM, musical key, and energy for newly added tracks.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.analyze_new_tracks, TraktorTheme::PLAY_ACTIVE(), "Analyse new tracks");
            },
        );

        Self::list_row(
            ui,
            "Rescan now",
            &self.file_management_status.clone(),
            |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Rescan folders")
                                .color(on_selected_text())
                                .strong(),
                        )
                        .fill(TraktorTheme::ROW_SELECTED())
                        .min_size(Vec2::new(150.0, 26.0)),
                    )
                    .clicked()
                {
                    self.trigger_music_rescan = true;
                }
            },
        );
    }

    fn show_about(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "About");

        Self::list_row(
            ui,
            "Version",
            "Personal DJ app for Linux, compatible with Traktor libraries.",
            |ui| {
                ui.label(
                    RichText::new(concat!("ReTraktor ", env!("CARGO_PKG_VERSION")))
                        .color(TraktorTheme::DECK_A())
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                );
            },
        );

        Self::list_row(
            ui,
            "Audio engine",
            "32-bit float audio with live frequency splitting and optional Demucs stems.",
            |ui| {
                ui.label(RichText::new("Rust · 32-bit float").monospace());
            },
        );

        Self::list_row(
            ui,
            "Library formats",
            "Reads and writes Traktor NML, M3U8 playlists, rekordbox XML, and standard audio tags.",
            |ui| {
                ui.label(RichText::new("NML · M3U8 · rekordbox XML").monospace());
            },
        );

        Self::list_row(
            ui,
            "Platform",
            "Linux on Wayland or X11 with PipeWire or ALSA audio.",
            |ui| {
                ui.label(RichText::new(format!("Linux {}", std::env::consts::ARCH)).monospace());
            },
        );
    }

    fn show_ai_prefs(&mut self, ui: &mut Ui) {
        if let Some(runner) = &self.ai_test_runner {
            if let Some(res) = runner.poll() {
                self.ai_test_status = Some(res);
                self.ai_test_runner = None;
            }
        }

        Self::list_section_header(ui, "AI Assistant");

        // Provider picker
        Self::list_row(
            ui,
            "AI Provider",
            "Choose your model provider. ReTraktor communicates directly from your machine.",
            |ui| {
                let mut changed = false;
                ui.horizontal(|ui| {
                    for p in crate::ai::AiProvider::ALL {
                        let sel = self.ai_settings.provider == p;
                        let btn = egui::Button::new(
                            RichText::new(p.label())
                                .size(12.0)
                                .color(if sel { on_selected_text() } else { TraktorTheme::TEXT_PRIMARY() }),
                        )
                        .fill(if sel { TraktorTheme::ROW_SELECTED() } else { TraktorTheme::CONTROL() })
                        .rounding(4.0)
                        .min_size(Vec2::new(70.0, 24.0));
                        if ui.add(btn).clicked() && !sel {
                            self.ai_settings.provider = p;
                            self.ai_settings.model = p.default_model().to_string();
                            if p == crate::ai::AiProvider::OpenAiCompatible && self.ai_settings.base_url.is_empty() {
                                self.ai_settings.base_url = p.default_base_url().to_string();
                            }
                            changed = true;
                        }
                    }
                });
                if changed {
                    let _ = self.ai_settings.save();
                }
            },
        );

        // API Key (password field)
        let key_desc = match self.ai_settings.provider {
            crate::ai::AiProvider::OpenAiCompatible => "API key (optional for local Ollama). Never logged or printed.",
            _ => "Your secret API key. Never logged or printed.",
        };
        Self::list_row(ui, "API Key", key_desc, |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.ai_settings.api_key)
                    .password(true)
                    .desired_width(340.0)
                    .hint_text("Enter API key…"),
            );
            if r.changed() {
                let _ = self.ai_settings.save();
            }
        });

        // Model
        Self::list_row(
            ui,
            "Model",
            "The model identifier sent in requests.",
            |ui| {
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.ai_settings.model)
                        .desired_width(240.0),
                );
                if r.changed() {
                    let _ = self.ai_settings.save();
                }
            },
        );

        // Base URL (only for OpenAI-compatible)
        if self.ai_settings.provider == crate::ai::AiProvider::OpenAiCompatible {
            Self::list_row(
                ui,
                "Base URL",
                "Custom OpenAI-compatible endpoint (e.g. http://localhost:11434/v1 for Ollama).",
                |ui| {
                    let r = ui.add(
                        egui::TextEdit::singleline(&mut self.ai_settings.base_url)
                            .desired_width(340.0)
                            .hint_text("http://localhost:11434/v1"),
                    );
                    if r.changed() {
                        let _ = self.ai_settings.save();
                    }
                },
            );
        }

        // Connection Test
        Self::list_row(
            ui,
            "Connection Test",
            "Sends a one-word verification request to confirm connectivity.",
            |ui| {
                ui.horizontal(|ui| {
                    let is_testing = self.ai_test_runner.is_some();
                    let btn = egui::Button::new(
                        RichText::new(if is_testing { "Testing…" } else { "TEST" })
                            .color(on_selected_text())
                            .strong(),
                    )
                    .fill(TraktorTheme::ROW_SELECTED())
                    .rounding(4.0)
                    .min_size(Vec2::new(80.0, 24.0));

                    if ui.add_enabled(!is_testing, btn).clicked() {
                        self.ai_test_status = None;
                        self.ai_test_runner = Some(crate::ai::AiTestRunner::start(self.ai_settings.clone()));
                    }

                    if let Some(status) = &self.ai_test_status {
                        match status {
                            Ok(reply) => {
                                ui.label(RichText::new(format!("✔ OK ({})", reply.trim())).color(TraktorTheme::PLAY_ACTIVE()).strong());
                            }
                            Err(err) => {
                                ui.label(RichText::new(format!("⚠ {err}")).color(TraktorTheme::CUE_ACTIVE()));
                            }
                        }
                    }
                });
            },
        );

        // Privacy notice
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Only track titles, artists, BPM, key, genre and energy are sent. Never your audio or file paths.")
                    .color(TraktorTheme::TEXT_MUTED())
                    .font(egui::FontId::proportional(11.5)),
            );
        });
    }
}

/// High-contrast light text on `ROW_SELECTED` across dark and light presets.
fn on_selected_text() -> Color32 {
    if crate::theme::palette().is_dark() {
        TraktorTheme::TEXT_HIGHLIGHT()
    } else {
        TraktorTheme::BG_INPUT()
    }
}

/// One readable line from a long installer error: the last line, e.g. "ModuleNotFoundError: No module named 'numpy'".
fn error_summary(error: &str) -> String {
    let last = error.lines().map(str::trim).filter(|l| !l.is_empty()).last().unwrap_or("Install failed");
    let mut s: String = last.chars().take(110).collect();
    if last.chars().count() > 110 {
        s.push('…');
    }
    s
}

/// "SCAN & IMPORT OPTIONS" -> "Scan & import options", keeping acronyms upper-case.
fn sentence_case(title: &str) -> String {
    const KEEP: [&str; 12] = ["MX2", "AI", "EQ", "FX", "MIDI", "HID", "USB", "BPM", "NML", "ALSA", "DJ", "LED"];
    title
        .split(' ')
        .enumerate()
        .map(|(i, w)| {
            if KEEP.iter().any(|k| w.trim_matches(|c: char| !c.is_alphanumeric()).eq_ignore_ascii_case(k)) {
                w.to_uppercase()
            } else if i == 0 {
                let lower = w.to_lowercase();
                let mut c = lower.chars();
                c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
            } else {
                w.to_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    #[test]
    fn error_summary_takes_last_line() {
        let e = "Demucs verification failed: Traceback (most recent call last):\n  File x\nModuleNotFoundError: No module named 'numpy'\n";
        assert_eq!(super::error_summary(e), "ModuleNotFoundError: No module named 'numpy'");
    }

    #[test]
    fn sentence_case_keeps_acronyms() {
        assert_eq!(super::sentence_case("SCAN & IMPORT OPTIONS"), "Scan & import options");
        assert_eq!(super::sentence_case("MX2 MAIN EQ (HI)"), "MX2 main EQ (hi)");
    }

    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_file_management_preferences_and_folders() {
        let mut prefs = PreferencesDialog::default();
        assert!(!prefs.music_folders.is_empty(), "Should initialize with default music folder");

        // Add a new music folder
        let custom_dir = PathBuf::from("/media/dj_usb/lossless");
        prefs.music_folders.push(custom_dir.clone());
        assert!(prefs.music_folders.contains(&custom_dir));

        // Remove folder
        if let Some(pos) = prefs.music_folders.iter().position(|p| p == &custom_dir) {
            prefs.music_folders.remove(pos);
        }
        assert!(!prefs.music_folders.contains(&custom_dir));

        // Switch to FileManagement tab
        prefs.current_tab = PrefsTab::FileManagement;
        assert_eq!(prefs.current_tab, PrefsTab::FileManagement);
    }

    #[test]
    fn test_preferences_tabs_and_breadcrumbs() {
        let prefs = PreferencesDialog::default();
        let (cat, sec) = prefs.get_tab_breadcrumb(PrefsTab::AudioSetup);
        assert_eq!(cat, "AUDIO & ROUTING");
        assert_eq!(sec, "Audio Setup");

        let (cat, sec) = prefs.get_tab_breadcrumb(PrefsTab::StemsAndAi);
        assert_eq!(cat, "STEMS & RECORDING");
        assert_eq!(sec, "Stems & Demucs AI");

        let (cat, sec) = prefs.get_tab_breadcrumb(PrefsTab::BrowserColumns);
        assert_eq!(cat, "COLLECTION & HARDWARE");
        assert_eq!(sec, "Browser & Metadata Columns");

        let (cat, sec) = prefs.get_tab_breadcrumb(PrefsTab::Controllers);
        assert_eq!(cat, "COLLECTION & HARDWARE");
        assert_eq!(sec, "Controller Manager & Remapper");

        let (cat, sec) = prefs.get_tab_breadcrumb(PrefsTab::OtherControllers);
        assert_eq!(cat, "COLLECTION & HARDWARE");
        assert_eq!(sec, "Other Controllers");

        assert_eq!(prefs.buffer_size, 128);
        assert_eq!(prefs.sample_rate, 48000);
        assert_eq!(prefs.mixing_mode, 0);
        assert_eq!(prefs.limiter_headroom, 1);
    }

    #[test]
    fn test_preferences_persistence_roundtrip() {
        let mut original = PreferencesDialog::default();
        original.sample_rate = 96000;
        original.buffer_size = 256;
        original.four_deck_mode = true;
        original.lock_playing_decks = false;
        original.audio_device_name = "Focusrite Scarlett 4i4".to_string();
        original.library_columns.comment = true;
        original.library_columns.remixer = true;
        original.library_columns.bitrate = true;
        original.collection_save_format = 2;
        original.music_folders = vec!["/DATA/Media/Music".into(), "/home/dj/My Music, Old".into()];

        let json = original.to_json_str();
        let mut restored = PreferencesDialog::default();
        restored.apply_json_str(&json);

        assert_eq!(restored.sample_rate, 96000);
        assert_eq!(restored.buffer_size, 256);
        assert!(restored.four_deck_mode);
        assert!(!restored.lock_playing_decks);
        assert_eq!(restored.audio_device_name, "Focusrite Scarlett 4i4");
        assert!(restored.library_columns.comment);
        assert!(restored.library_columns.remixer);
        assert!(restored.library_columns.bitrate);
        assert_eq!(restored.collection_save_format, 2);
        assert_eq!(restored.music_folders, original.music_folders);
    }
}
