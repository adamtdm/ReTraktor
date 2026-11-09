use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2};

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
    pub selected_visual_control: Option<String>,
    pub custom_control_action: std::collections::HashMap<String, String>,
    pub custom_control_color: std::collections::HashMap<String, u8>,
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
            selected_visual_control: None,
            custom_control_action: std::collections::HashMap::new(),
            custom_control_color: std::collections::HashMap::new(),
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
            .fill(TraktorTheme::BG_PANEL_ALT)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .rounding(4.0)
            .inner_margin(12.0)
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(title).strong().color(TraktorTheme::TEXT_PRIMARY));
                        if !description.is_empty() {
                            ui.label(RichText::new(format!("— {description}")).size(12.0).color(TraktorTheme::TEXT_MUTED));
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
        // Plain aligned row with a hairline under it; no card per setting.
        let out = egui::Frame::none()
            .inner_margin(egui::Margin::symmetric(4.0, 8.0))
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(
                            RichText::new(label)
                                .color(Color32::WHITE)
                                .font(egui::FontId::proportional(13.0))
                                .strong(),
                        );
                        if !description.is_empty() {
                            ui.label(
                                RichText::new(description)
                                    .color(TraktorTheme::TEXT_SECONDARY)
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
        ui.painter().hline(r.x_range(), r.bottom(), Stroke::new(1.0_f32, TraktorTheme::BG_PANEL));
        out.inner
    }

    /// Section heading: sentence case, more space above than below.
    pub fn list_section_header(ui: &mut Ui, title: &str) {
        ui.add_space(18.0);
        ui.label(RichText::new(sentence_case(title)).size(15.0).strong().color(TraktorTheme::TEXT_PRIMARY));
        ui.add_space(2.0);
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
            PrefsTab::About => ("COLLECTION & HARDWARE", "About & Diagnostics"),
        }
    }

    /// Settings pages grouped into the six top-level tabs.
    pub const GROUPS: [(&'static str, &'static [PrefsTab]); 6] = [
        ("Audio", &[PrefsTab::AudioSetup, PrefsTab::OutputRouting, PrefsTab::InputRouting, PrefsTab::ExternalSync]),
        ("Decks & Mixer", &[PrefsTab::Decks, PrefsTab::TrackDecks, PrefsTab::Mixer, PrefsTab::Effects]),
        ("Library", &[PrefsTab::FileManagement, PrefsTab::BrowserColumns, PrefsTab::AnalyzeOptions]),
        ("Stems & Recording", &[PrefsTab::StemsAndAi, PrefsTab::Recording]),
        ("Controllers", &[PrefsTab::Controllers]),
        ("About", &[PrefsTab::About]),
    ];

    fn current_group(&self) -> &'static [PrefsTab] {
        Self::GROUPS.iter().map(|g| g.1).find(|g| g.contains(&self.current_tab)).unwrap_or(&[PrefsTab::AudioSetup])
    }

    /// Renders the top-level tab strip; the search box filters tabs by any section they contain.
    pub fn render_top_section_tabs(&mut self, ui: &mut Ui) {
        let query = self.search_query.trim().to_lowercase();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
            for (label, tabs) in Self::GROUPS {
                let matches = query.is_empty()
                    || label.to_lowercase().contains(&query)
                    || tabs.iter().any(|t| self.get_tab_breadcrumb(*t).1.to_lowercase().contains(&query));
                if !matches {
                    continue;
                }
                let selected = tabs.contains(&self.current_tab);
                let button = egui::Button::new(RichText::new(label).size(13.0).color(if selected { Color32::WHITE } else { TraktorTheme::TEXT_PRIMARY }))
                    .fill(if selected { TraktorTheme::ROW_SELECTED } else { TraktorTheme::BG_PANEL })
                    .rounding(4.0)
                    .min_size(Vec2::new(110.0, 30.0));
                if ui.add(button).clicked() {
                    self.current_tab = tabs[0];
                }
            }
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
            PrefsTab::About => self.show_about(ui),
        }
    }

    /// Renders Preferences as a full-page view with top section tabs and a vertical list-type options view.
    pub fn show_full_page(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);

            // 1. Top Header Bar
            egui::Frame::none()
                .fill(TraktorTheme::BG_WINDOW)
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Preferences")
                                .color(Color32::WHITE)
                                .size(20.0)
                                .strong(),
                        );
                        ui.add_space(8.0);


                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Close (Esc)")
                                            .color(Color32::WHITE)
                                            .font(egui::FontId::proportional(12.0))
                                            .strong(),
                                    )
                                    .fill(TraktorTheme::ROW_SELECTED)
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
                .fill(TraktorTheme::BG_WINDOW)
                .stroke(Stroke::NONE)
                .inner_margin(egui::Margin::symmetric(16.0, 8.0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    self.render_top_section_tabs(ui);
                });

            // 3. Full-Width Scrollable List-Type Options View
            egui::Frame::none()
                .fill(TraktorTheme::BG_WINDOW)
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
        Self::list_section_header(ui, "HARDWARE INTERFACE & BUFFER LATENCY");

        Self::list_row(
            ui,
            "Audio Device",
            "Select the hardware soundcard or PipeWire/ALSA pro-audio interface.",
            |ui| {
                egui::ComboBox::from_id_salt("audio_device_combo")
                    .selected_text(&self.audio_device_name)
                    .width(340.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.audio_device_name,
                            "PipeWire pro-audio / ALSA Default".to_string(),
                            "PipeWire pro-audio / ALSA Default",
                        );
                        ui.selectable_value(
                            &mut self.audio_device_name,
                            "Traktor Kontrol / MX2 USB Audio (hw:MX2,0)".to_string(),
                            "Traktor Kontrol / MX2 USB Audio (hw:MX2,0)",
                        );
                        ui.selectable_value(
                            &mut self.audio_device_name,
                            "USB Audio Class 2.0 Interface (hw:UAC2,0)".to_string(),
                            "USB Audio Class 2.0 Interface (hw:UAC2,0)",
                        );
                    });
            },
        );

        Self::list_row(
            ui,
            "Sample Rate",
            "Operating sample rate of the real-time DSP and hardware DAC.",
            |ui| {
                egui::ComboBox::from_id_salt("sr_combo")
                    .selected_text(format!("{} Hz", self.sample_rate))
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.sample_rate, 44100, "44100 Hz (CD Standard)");
                        ui.selectable_value(&mut self.sample_rate, 48000, "48000 Hz (Pro Audio Default)");
                        ui.selectable_value(&mut self.sample_rate, 88200, "88200 Hz");
                        ui.selectable_value(&mut self.sample_rate, 96000, "96000 Hz (Studio Master)");
                    });
            },
        );

        let latency_ms = (self.buffer_size as f64 / self.sample_rate as f64) * 1000.0;
        Self::list_row(
            ui,
            "Buffer Size (Latency)",
            "Smaller buffers reduce scratch/cue latency; larger buffers increase stability.",
            |ui| {
                egui::ComboBox::from_id_salt("buf_combo")
                    .selected_text(format!("{} frames ({:.1} ms)", self.buffer_size, latency_ms))
                    .width(240.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.buffer_size, 64, "64 frames (1.3 ms — Ultra Low)");
                        ui.selectable_value(&mut self.buffer_size, 128, "128 frames (2.7 ms — Performance)");
                        ui.selectable_value(&mut self.buffer_size, 256, "256 frames (5.3 ms — Safe)");
                        ui.selectable_value(&mut self.buffer_size, 512, "512 frames (10.7 ms — High)");
                    });
            },
        );

        Self::list_row(
            ui,
            "Real-Time Audio Scheduler Status",
            "Lock-free SPSC command ring buffer with non-blocking audio callback.",
            |ui| {
                ui.label(
                    RichText::new(format!("ACTIVE • {:.2} ms Roundtrip • 0 XRuns", latency_ms))
                        .color(TraktorTheme::PLAY_ACTIVE)
                        .font(egui::FontId::monospace(11.5))
                        .strong(),
                );
            },
        );
    }

    fn show_output_routing(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "OUTPUT ROUTING — MASTER, MONITOR & BOOTH");

        Self::list_row(
            ui,
            "Mixing Mode",
            "Route all 4 decks through the internal software mixer or separately to an external hardware mixer.",
            |ui| {
                let modes = [
                    "Internal (Software Mixer)",
                    "External (Hardware Mixer Channels)",
                ];
                egui::ComboBox::from_id_salt("mixing_mode_combo")
                    .selected_text(modes[self.mixing_mode.min(1)])
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.mixing_mode, 0, modes[0]);
                        ui.selectable_value(&mut self.mixing_mode, 1, modes[1]);
                    });
            },
        );

        Self::list_row(
            ui,
            "Output Master (Main PA Speakers)",
            "Primary stereo bus output channels.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.master_out_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.master_out_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Output Monitor (Headphone Cue)",
            "Stereo headphone cue bus for pre-listening.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.monitor_out_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.monitor_out_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Output Booth (DJ Booth Monitor)",
            "Secondary stereo output with independent booth level control.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.booth_out_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.booth_out_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Split Cue Mode",
            "Sums Cue Monitor into the left headphone ear and Master Output into the right ear.",
            |ui| {
                ui.checkbox(&mut self.split_cue, "Enabled");
            },
        );
    }

    fn show_input_routing(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "INPUT ROUTING — LIVE INPUTS, DVS & AUX");

        Self::list_row(
            ui,
            "Input Deck A (Live / DVS Timecode)",
            "Hardware stereo input pair assigned to Deck A.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.deck_a_in_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.deck_a_in_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Input Deck B (Live / DVS Timecode)",
            "Hardware stereo input pair assigned to Deck B.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.deck_b_in_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.deck_b_in_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "Input Aux / Microphone",
            "Auxiliary line or microphone input routed to the master bus.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.aux_in_r).desired_width(170.0));
                ui.label("R:");
                ui.add(egui::TextEdit::singleline(&mut self.aux_in_l).desired_width(170.0));
                ui.label("L:");
            },
        );

        Self::list_row(
            ui,
            "DVS Timecode Engine",
            "2.5 kHz MK2 timecode vinyl & CD carrier tracking calibration.",
            |ui| {
                ui.label(
                    RichText::new("READY (0.0 dB Lead-In)")
                        .color(TraktorTheme::DECK_A)
                        .font(egui::FontId::monospace(11.0))
                        .strong(),
                );
            },
        );
    }

    fn show_external_sync(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "EXTERNAL SYNC, MIDI CLOCK & ABLETON LINK");

        Self::list_row(
            ui,
            "Master Clock Mode",
            "Choose whether Master Tempo follows the active playing deck or remains manually fixed.",
            |ui| {
                let modes = ["Auto Master (Follows Active Deck)", "Manual Master (Fixed Tempo)"];
                egui::ComboBox::from_id_salt("clock_mode_combo")
                    .selected_text(modes[self.clock_mode.min(1)])
                    .width(250.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.clock_mode, 0, modes[0]);
                        ui.selectable_value(&mut self.clock_mode, 1, modes[1]);
                    });
            },
        );

        Self::list_row(
            ui,
            "Send MIDI Clock Output",
            "Transmit 24 PPQN MIDI Clock messages to external hardware drum machines and synths.",
            |ui| {
                ui.checkbox(&mut self.send_midi_clock, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "MIDI Clock Offset",
            "Compensate for external hardware MIDI buffer latency (-50 ms to +50 ms).",
            |ui| {
                ui.add(egui::Slider::new(&mut self.midi_clock_offset_ms, -50.0..=50.0).suffix(" ms"));
            },
        );

        Self::list_row(
            ui,
            "Ableton Link Integration",
            "Synchronize beatgrid phase and tempo with peer applications over local LAN/Wi-Fi.",
            |ui| {
                ui.checkbox(&mut self.ableton_link_enabled, "Enable Link");
            },
        );
    }

    fn show_mixer_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "MIXER EQ MODELS, FILTERS & MASTER LIMITER");

        let models = [
            "Z ISO (Full Kill 3-Band — Traktor Pro 4 Default)",
            "Classic (Standard Warm Curves)",
            "P600 (Club Standard Steep Roll-off)",
            "Nuo (Fat Analog Lows)",
            "Xone:92 (4-Band Dual-Mid Audiophile)",
        ];
        Self::list_row(
            ui,
            "Channel EQ Model",
            "Select the frequency crossover and kill behavior of the channel strip EQs.",
            |ui| {
                egui::ComboBox::from_id_salt("eq_model_combo")
                    .selected_text(models[self.eq_model_idx.min(models.len() - 1)])
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for (i, m) in models.iter().enumerate() {
                            ui.selectable_value(&mut self.eq_model_idx, i, *m);
                        }
                    });
            },
        );

        let filters = [
            "Ladder (Moog-style 4-Pole 24dB/oct resonant)",
            "State Variable (Clean 12dB/oct dual-slope)",
            "Pro-53 (Classic Analog Synth Filter)",
        ];
        Self::list_row(
            ui,
            "Channel Filter Model",
            "Select the analog filter topology for the bipolar channel filter knob.",
            |ui| {
                egui::ComboBox::from_id_salt("filter_model_combo")
                    .selected_text(filters[self.filter_model_idx.min(filters.len() - 1)])
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for (i, f) in filters.iter().enumerate() {
                            ui.selectable_value(&mut self.filter_model_idx, i, *f);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Filter Resonance (Q)",
            "Adjust peak emphasis at the filter cutoff frequency.",
            |ui| {
                ui.add(egui::Slider::new(&mut self.filter_resonance, 0.0..=1.0).show_value(true));
            },
        );

        let curves = [
            "Smooth (Constant-Power blend — House/Trance)",
            "Steep (Linear Blend — Techno/Club)",
            "Scratch Cut (Sharp 5% cut-in lag — Turntablism)",
        ];
        Self::list_row(
            ui,
            "Crossfader Curve",
            "Adjust transition slope from smooth constant-power blend to instant scratch cut.",
            |ui| {
                egui::ComboBox::from_id_salt("xf_curve_combo")
                    .selected_text(curves[self.xfader_curve_idx.min(curves.len() - 1)])
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for (i, c) in curves.iter().enumerate() {
                            ui.selectable_value(&mut self.xfader_curve_idx, i, *c);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Per-Track Autogain",
            "Automatically normalize perceived track loudness based on analyzed ReplayGain/LUFS.",
            |ui| {
                ui.checkbox(&mut self.autogain, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "Master Bus Brickwall Limiter",
            "Prevent digital clipping on the master output bus.",
            |ui| {
                ui.checkbox(&mut self.limiter_enabled, "Enabled");
            },
        );

        if self.limiter_enabled {
            let headrooms = [
                "0.0 dB (Max Volume)",
                "-3.0 dB (Traktor Pro 4 Standard)",
                "-6.0 dB (Club PA Protection)",
            ];
            Self::list_row(
                ui,
                "Limiter Headroom Ceiling",
                "Attenuate pre-limiter master bus headroom to preserve transient punch.",
                |ui| {
                    egui::ComboBox::from_id_salt("limiter_headroom_combo")
                        .selected_text(headrooms[self.limiter_headroom.min(2)])
                        .width(240.0)
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
            "Neural Mix EQ ⇄ Stems Mode",
            "Map channel HI / MID / LOW EQ knobs directly to Vocals, Instruments, and Drums+Bass stems.",
            |ui| {
                ui.checkbox(&mut self.neural_mix_eq_mode, "Enabled");
            },
        );
    }

    fn show_stems_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "STEMS CREATION & META DEMUCS AI ENGINE");

        Self::list_row(
            ui,
            "Meta Demucs AI Neural Separation Engine",
            "Isolates tracks into 4 studio-grade stems (Drums, Bass, Other, Vocals) in the background.",
            |ui| match &self.demucs_install_state {
                crate::demucs::DemucsInstallState::Ready { version, .. } => {
                    ui.label(
                        RichText::new(format!("✔ {version} Ready"))
                            .color(TraktorTheme::PLAY_ACTIVE)
                            .strong(),
                    );
                }
                crate::demucs::DemucsInstallState::Installing { step, percent } => {
                    ui.add(egui::ProgressBar::new(*percent).desired_width(140.0).show_percentage());
                    ui.label(RichText::new(step).color(Color32::from_rgb(255, 185, 45)));
                }
                crate::demucs::DemucsInstallState::NotInstalled => {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Install Demucs AI (~1.2 GB)")
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .fill(TraktorTheme::ROW_SELECTED)
                            .min_size(Vec2::new(210.0, 26.0)),
                        )
                        .clicked()
                    {
                        self.trigger_demucs_install = true;
                    }
                }
                crate::demucs::DemucsInstallState::Failed { error } => {
                    if ui.button("Retry Install").clicked() {
                        self.trigger_demucs_install = true;
                    }
                    ui.label(RichText::new(error).color(Color32::from_rgb(240, 80, 80)));
                }
            },
        );

        let stem_modes = [
            "Hybrid (Neural Demucs AI + Real-Time DSP Fallback)",
            "Real-Time DSP Isolation Only (Zero Dependencies)",
            "Offline Pre-Computed 4-Stem Cache (.stem.m4a / FLAC)",
        ];
        Self::list_row(
            ui,
            "Stem Separation Mode",
            "Choose between hybrid AI separation, zero-latency DSP crossover, or offline stem files.",
            |ui| {
                egui::ComboBox::from_id_salt("stem_engine_mode_combo")
                    .selected_text(stem_modes[self.stems_engine_mode.min(2)])
                    .width(320.0)
                    .show_ui(ui, |ui| {
                        for (i, m) in stem_modes.iter().enumerate() {
                            ui.selectable_value(&mut self.stems_engine_mode, i, *m);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Neural Mix EQ ⇄ Stems Knob Swap",
            "Swap channel HI/MID/LOW EQ knobs to control Vocals, Instruments, and Drums+Bass levels.",
            |ui| {
                ui.checkbox(&mut self.neural_mix_eq_mode, "Enabled");
            },
        );
    }

    fn show_deck_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "DECKS LAYOUT & WAVEFORM DISPLAY");

        Self::list_row(
            ui,
            "4-Deck Workspace Mode (Decks A, B, C, D)",
            "Display four independent deck strips and stacked parallel waveforms.",
            |ui| {
                ui.checkbox(&mut self.four_deck_mode, "Enabled");
            },
        );

        let palettes = [
            "Spectrum (3-Band Frequency Color)",
            "Ultraviolet (Neon Purple Mids & Ice Blue Highs)",
            "High Contrast RGB (Red Bass, Green Mids, Blue Highs)",
        ];
        Self::list_row(
            ui,
            "Waveform Color Palette",
            "Select the frequency-band color mapping used on scrolling and overview waveforms.",
            |ui| {
                egui::ComboBox::from_id_salt("wf_palette_combo")
                    .selected_text(palettes[self.waveform_color_mode.min(2)])
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        for (i, p) in palettes.iter().enumerate() {
                            ui.selectable_value(&mut self.waveform_color_mode, i, *p);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Minute Markers on Stripe Overview",
            "Render vertical 60-second time division ticks across the full-track overview bar.",
            |ui| {
                ui.checkbox(&mut self.show_minute_markers, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "Beatgrid Transient Ticks",
            "Display beatgrid bar and beat lines on scrolling waveforms for visual phase alignment.",
            |ui| {
                ui.checkbox(&mut self.show_beatgrid_ticks, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "Fit Full Track in Stripe Overview",
            "Scale the mini overview waveform to always span 100% of the track duration.",
            |ui| {
                ui.checkbox(&mut self.stripe_fit_full, "Enabled");
            },
        );
    }

    fn show_track_decks_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "TRACK DECKS, SAFETY & VINYL SCRATCH PHYSICS");

        Self::list_row(
            ui,
            "Lock Playing Decks",
            "Prevent loading a new track into a deck that is currently playing live audio.",
            |ui| {
                ui.checkbox(&mut self.lock_playing_decks, "Enabled");
            },
        );

        let ranges = [
            "±8% (Club / House)",
            "±16% (Wide Range)",
            "±32% (Extreme)",
            "±50% (Breakbeat)",
            "±100% (Ultra Pitch)",
        ];
        Self::list_row(
            ui,
            "Tempo Pitch Fader Range",
            "Maximum tempo bend percentage for deck pitch faders.",
            |ui| {
                egui::ComboBox::from_id_salt("pitch_range_combo")
                    .selected_text(ranges[self.pitch_range_idx.min(4)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, r) in ranges.iter().enumerate() {
                            ui.selectable_value(&mut self.pitch_range_idx, i, *r);
                        }
                    });
            },
        );

        let modes = ["HiQ (Phase Vocoder — Scratch Preserving)", "ECO (Low CPU Usage)"];
        Self::list_row(
            ui,
            "Keylock Time-Stretch Quality",
            "Master tempo pitch-lock algorithm quality when changing BPM.",
            |ui| {
                egui::ComboBox::from_id_salt("keylock_mode_combo")
                    .selected_text(modes[self.keylock_quality.min(1)])
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (i, m) in modes.iter().enumerate() {
                            ui.selectable_value(&mut self.keylock_quality, i, *m);
                        }
                    });
            },
        );

        let positions = ["Centered (50% Center)", "Left-Weighted (30% Ahead)"];
        Self::list_row(
            ui,
            "Playhead Marker Position",
            "Horizontal position of the red playhead cursor inside the scrolling waveform viewport.",
            |ui| {
                egui::ComboBox::from_id_salt("play_marker_combo")
                    .selected_text(positions[self.play_marker_pos.min(1)])
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, p) in positions.iter().enumerate() {
                            ui.selectable_value(&mut self.play_marker_pos, i, *p);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Auto-Cue on Track Load",
            "Automatically position playhead at the first beatgrid cue point when loading a track.",
            |ui| {
                ui.checkbox(&mut self.auto_cue_on_load, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "Jogwheel Scratch Sensitivity",
            "Multiplier for capacitive platter vinyl scratch velocity.",
            |ui| {
                ui.add(egui::Slider::new(&mut self.jog_scratch_sensitivity, 0.2..=3.0).show_value(true));
            },
        );

        Self::list_row(
            ui,
            "Turntable Mode (Vinyl Scratch)",
            "Top platter touch engages alpha-beta vinyl scratch; outer rim performs pitch bend nudge.",
            |ui| {
                ui.checkbox(&mut self.jog_mode_turntable, "Enabled");
            },
        );
    }

    fn show_effects_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "EFFECTS ROUTING & MIXER FX ASSIGNMENTS");

        let unit_counts = ["2 FX Units (Standard Decks A & B)", "4 FX Units (Decks A, B, C, D)"];
        Self::list_row(
            ui,
            "Active Master FX Units",
            "Number of assignable multi-effect units in the top rack.",
            |ui| {
                egui::ComboBox::from_id_salt("fx_units_count_combo")
                    .selected_text(unit_counts[self.fx_units_count.min(1)])
                    .width(250.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.fx_units_count, 0, unit_counts[0]);
                        ui.selectable_value(&mut self.fx_units_count, 1, unit_counts[1]);
                    });
            },
        );

        let panel_modes = [
            "Group Mode (3 Chained Effects per Unit)",
            "Single Mode (1 Deep Effect with Parameter Knobs)",
        ];
        Self::list_row(
            ui,
            "FX Panel Display Mode",
            "Choose between 3 simultaneous macro effects or single-effect deep parameter control.",
            |ui| {
                egui::ComboBox::from_id_salt("fx_panel_mode_combo")
                    .selected_text(panel_modes[self.fx_panel_mode.min(1)])
                    .width(280.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.fx_panel_mode, 0, panel_modes[0]);
                        ui.selectable_value(&mut self.fx_panel_mode, 1, panel_modes[1]);
                    });
            },
        );

        Self::list_section_header(ui, "ONE-KNOB MIXER FX SLOT ASSIGNMENTS");
        let fx_options = [
            "Filter (92 Dual Resonant)",
            "Reverb (T3 Space Reverb)",
            "Dual Delay (Ping-Pong Stereo)",
            "White Noise Sweep",
            "Time Gater (Rhythmic Chop)",
            "Barber Pole (Infinite Phaser)",
            "Flanger Flux",
            "Dotted 8th Delay",
            "Bit Crush (Digital Lo-Fi)",
        ];

        for idx in 0..4 {
            let mut slot_val = self.mixer_fx_slots[idx].min(fx_options.len() - 1);
            Self::list_row(
                ui,
                &format!("Mixer FX Slot {}", idx + 1),
                "Effect assigned to channel strip Mixer FX selector button.",
                |ui| {
                    egui::ComboBox::from_id_salt(format!("mfx_slot_{idx}"))
                        .selected_text(fx_options[slot_val])
                        .width(240.0)
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
        Self::list_section_header(ui, "MASTER MIX RECORDER & LIVE BROADCAST");

        let path_str = self.recording_dir.to_string_lossy().to_string();
        Self::list_row(
            ui,
            "Mix Recording Directory",
            "Destination folder where recorded live WAV mixes are saved.",
            |ui| {
                ui.label(RichText::new(path_str).monospace().color(TraktorTheme::DECK_A));
            },
        );

        let formats = [
            "WAV 16-bit / 48 kHz (Lossless CD Quality)",
            "WAV 24-bit / 48 kHz (Studio Master)",
        ];
        Self::list_row(
            ui,
            "Recording Audio Format",
            "Uncompressed PCM bit-depth for master mix recordings.",
            |ui| {
                egui::ComboBox::from_id_salt("rec_format_combo")
                    .selected_text(formats[self.recording_format.min(1)])
                    .width(260.0)
                    .show_ui(ui, |ui| {
                        for (i, f) in formats.iter().enumerate() {
                            ui.selectable_value(&mut self.recording_format, i, *f);
                        }
                    });
            },
        );

        Self::list_section_header(ui, "ICECAST / SHOUTCAST STREAMING SERVER");

        Self::list_row(
            ui,
            "Broadcast Server Host",
            "Hostname or IP address of your Icecast2 / Shoutcast server.",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.broadcast_host).desired_width(200.0));
            },
        );

        Self::list_row(
            ui,
            "Broadcast Server Port",
            "TCP port for Icecast stream (default: 8000).",
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
            "Stream Mount Point",
            "Icecast mount path (e.g. /retraktor.mp3 or /live.ogg).",
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.broadcast_mount).desired_width(200.0));
            },
        );

        Self::list_row(
            ui,
            "Stream Source Password",
            "Authentication password for the broadcast source client.",
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
        Self::list_section_header(ui, "LIBRARY METADATA COLUMNS & VIEW PRESETS");

        Self::list_row(
            ui,
            "Quick Column Presets",
            "Apply a preset column layout to the Track Collection browser table.",
            |ui| {
                if ui.button("Minimal Club View").clicked() {
                    self.library_columns.set_all(false);
                    self.library_columns.bpm = true;
                    self.library_columns.key = true;
                    self.library_columns.stems = true;
                    self.library_columns.duration = true;
                }
                if ui.button("Show All Metadata Fields").clicked() {
                    self.library_columns.set_all(true);
                }
                if ui.button("Standard DJ View (Default)").clicked() {
                    self.library_columns = LibraryColumnsConfig::default();
                }
            },
        );

        Self::list_section_header(ui, "VISIBLE METADATA FIELDS IN LIBRARY TABLE");

        Self::list_row(
            ui,
            "Album",
            "Album or EP title (ID3 TALB / Vorbis ALBUM / NML ALBUM/@TITLE).",
            |ui| {
                ui.checkbox(&mut self.library_columns.album, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "BPM (Tempo)",
            "Detected or tag-stored beats per minute (ID3 TBPM / NML TEMPO/@BPM).",
            |ui| {
                ui.checkbox(&mut self.library_columns.bpm, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Musical Key",
            "Harmonic key in Open Key, Camelot, or Musical notation (ID3 TKEY / NML MUSICAL_KEY).",
            |ui| {
                ui.checkbox(&mut self.library_columns.key, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Stems Status",
            "4-Stem separation availability and 1-click AI Demucs separation trigger.",
            |ui| {
                ui.checkbox(&mut self.library_columns.stems, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Time (Duration)",
            "Track playback length in MM:SS (NML INFO/@PLAYTIME).",
            |ui| {
                ui.checkbox(&mut self.library_columns.duration, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Genre",
            "Musical genre tag (ID3 TCON / Vorbis GENRE / NML INFO/@GENRE).",
            |ui| {
                ui.checkbox(&mut self.library_columns.genre, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Rating",
            "Track star rating 1–5 stars (ID3 POPM / NML INFO/@RANKING).",
            |ui| {
                ui.checkbox(&mut self.library_columns.rating, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Comment",
            "DJ notes, energy cues, and tag comments (ID3 COMM / Vorbis COMMENT / NML INFO/@COMMENT).",
            |ui| {
                ui.checkbox(&mut self.library_columns.comment, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Remixer",
            "Remix artist or producer credit (ID3 TPE4 / Vorbis REMIXER).",
            |ui| {
                ui.checkbox(&mut self.library_columns.remixer, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Label (Publisher)",
            "Record label or imprint (ID3 TPUB / Vorbis LABEL / NML INFO/@PRODUCER).",
            |ui| {
                ui.checkbox(&mut self.library_columns.label, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Year (Release Date)",
            "Release year or date (ID3 TDRC / TYER / Vorbis DATE / NML INFO/@RELEASE_DATE).",
            |ui| {
                ui.checkbox(&mut self.library_columns.year, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Bitrate (kbps)",
            "Audio stream bitrate in kbps (e.g. 320 kbps, 1411 kbps — NML INFO/@BITRATE).",
            |ui| {
                ui.checkbox(&mut self.library_columns.bitrate, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Format (Codec)",
            "Container and audio codec type (FLAC, MP3, WAV, AIFF, M4A, OGG).",
            |ui| {
                ui.checkbox(&mut self.library_columns.format, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Track #",
            "Album track number (ID3 TRCK / Vorbis TRACKNUMBER / NML ALBUM/@TRACK).",
            |ui| {
                ui.checkbox(&mut self.library_columns.track_number, "Show Column");
            },
        );
        Self::list_row(
            ui,
            "Energy",
            "Perceived intensity 1–10 from track analysis (stored as NML INFO/@ENERGY).",
            |ui| {
                ui.checkbox(&mut self.library_columns.energy, "Show Column");
            },
        );
    }

    fn show_analyze_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "TRACK ANALYZE OPTIONS & MUSICAL KEY NOTATION");

        let ranges = [
            "70 - 140 BPM (Standard House / Techno / Pop)",
            "60 - 120 BPM (Downtempo / Hip-Hop / Reggae)",
            "90 - 180 BPM (Drum & Bass / Jungle)",
            "110 - 220 BPM (Hardcore / Speed Garage)",
        ];
        Self::list_row(
            ui,
            "BPM Detection Range",
            "Constrain automatic tempo detection to avoid half/double-time octave errors.",
            |ui| {
                egui::ComboBox::from_id_salt("bpm_range_combo")
                    .selected_text(ranges[self.bpm_detection_range.min(3)])
                    .width(290.0)
                    .show_ui(ui, |ui| {
                        for (i, r) in ranges.iter().enumerate() {
                            ui.selectable_value(&mut self.bpm_detection_range, i, *r);
                        }
                    });
            },
        );

        let keys = [
            "Open Key (Traktor Standard: 1d..12d / 1m..12m)",
            "Camelot Notation (8A / 8B Wheel)",
            "Traditional Musical Key (Am, C, F#m)",
        ];
        Self::list_row(
            ui,
            "Musical Key Display Notation",
            "Choose how harmonic keys are formatted across Decks, Browser, and Match Radar.",
            |ui| {
                egui::ComboBox::from_id_salt("key_notation_combo")
                    .selected_text(keys[self.key_notation_idx.min(2)])
                    .width(290.0)
                    .show_ui(ui, |ui| {
                        for (i, k) in keys.iter().enumerate() {
                            ui.selectable_value(&mut self.key_notation_idx, i, *k);
                        }
                    });
            },
        );

        Self::list_row(
            ui,
            "Auto-Analyze Imported Tracks",
            "Automatically compute BPM, beatgrid transients, and musical key when adding files.",
            |ui| {
                ui.checkbox(&mut self.analyze_new_tracks, "Enabled");
            },
        );
    }

    fn show_controller_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Controller Manager — Traktor MX2 & Traktor Kontrol Setup");
        ui.add_space(4.0);

        // 1. Live Connection Status Banner
        let is_connected = self.mx2_connected_node.is_some();
        let (banner_bg, stroke_col, title_text, subtitle_text) = if is_connected {
            (
                Color32::from_rgb(14, 32, 20),
                TraktorTheme::PLAY_ACTIVE,
                format!("● Traktor MX2 Connected ({})", self.mx2_connected_node.as_deref().unwrap_or("")),
                format!(
                    "{} | {} Packets Received | NHL2 Reports 0x01/0x02/0x03 Active",
                    self.mx2_access_status.as_deref().unwrap_or("Read/Write Active"),
                    self.mx2_packets_count
                ),
            )
        } else if let Some(ref status) = self.mx2_access_status {
            (
                Color32::from_rgb(38, 28, 14),
                Color32::from_rgb(255, 170, 0),
                "⚠ Traktor Controller Detected — Permission Check Needed".to_string(),
                status.clone(),
            )
        } else {
            (
                Color32::from_rgb(24, 26, 34),
                Color32::from_rgb(70, 80, 95),
                "○ No Traktor MX2 Controller Attached via USB".to_string(),
                "Plug in the 15V DC power supply and connect via USB-C cable to auto-detect.".to_string(),
            )
        };

        egui::Frame::none()
            .fill(banner_bg)
            .stroke(Stroke::new(1.0_f32, stroke_col))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(title_text).color(stroke_col).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("🔄 Rescan Hardware").clicked() {
                            self.trigger_rescan = true;
                        }
                    });
                });
                ui.label(RichText::new(subtitle_text).color(Color32::LIGHT_GRAY).font(egui::FontId::monospace(9.5)));
            });

        ui.add_space(6.0);

        // 2. Hardware Checklist & Troubleshooting Card
        egui::CollapsingHeader::new(RichText::new("⚙ Hardware Setup & Troubleshooting Guide").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.label("• 🔌 Dedicated Power Supply: The Traktor MX2 requires the external 15V DC power adapter plugged into the back panel. USB-C bus power alone cannot power the audio DAC, platters, and RGB lighting.");
                ui.label("• Native HID Mode vs MIDI Mode: If the unit is in MIDI mode, press and hold Left SHIFT + Right SHIFT together to switch back to Native HID mode.");
                ui.label("• 🔒 Linux Permissions (udev): Requires /etc/udev/rules.d/60-retraktor-controllers.rules for non-root /dev/hidraw* read/write access.");
                ui.horizontal(|ui| {
                    if ui.button("📋 Copy 1-Click udev Fix Command to Clipboard").clicked() {
                        let cmd = "sudo cp assets/99-retraktor-ni.rules /etc/udev/rules.d/60-retraktor-controllers.rules && sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hidraw";
                        ui.output_mut(|o| o.copied_text = cmd.to_string());
                        self.udev_command_copied = true;
                    }
                    if self.udev_command_copied {
                        ui.label(RichText::new("✔ Copied command to clipboard!").color(TraktorTheme::PLAY_ACTIVE));
                    }
                });
            });

        ui.add_space(6.0);

        // 3. Hardware Test & Verification Actions
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("✨ Test Controller LEDs (Light Show)").strong())).clicked() {
                self.trigger_led_test = true;
            }
            if ui.add(egui::Button::new(RichText::new("▶ Run 114-Control Automated Verification").strong())).clicked() {
                self.trigger_self_test = true;
            }
        });

        if let Some(ref report) = self.self_test_report {
            ui.add_space(4.0);
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 28, 20))
                .stroke(Stroke::new(1.0_f32, TraktorTheme::PLAY_ACTIVE))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.label(RichText::new("✔ Automated Hardware Protocol Verification Passed!").color(TraktorTheme::PLAY_ACTIVE).strong());
                    ui.label(format!(
                        "Tested: {} Buttons ({}), {} Encoders ({}), {} Pots/Faders ({}), {} Jogwheels ({}), {} LEDs ({})",
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
        egui::CollapsingHeader::new(RichText::new("📡 Live HID Input & Control Monitor").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let r_id = self.mx2_last_report_id.map(|id| format!("0x{:02X}", id)).unwrap_or_else(|| "None".to_string());
                    ui.label(RichText::new(format!("Last Report: {}", r_id)).monospace().color(TraktorTheme::DECK_A));
                    ui.label(RichText::new(format!("Total Packets: {}", self.mx2_packets_count)).monospace());
                });
                if let Some(ref hex) = self.mx2_last_hex {
                    ui.label(RichText::new(format!("Raw Hex: {}", hex)).monospace().color(Color32::LIGHT_GRAY).font(egui::FontId::monospace(9.0)));
                }
                if !self.mx2_recent_events.is_empty() {
                    ui.label(RichText::new("Recent Control Events:").strong());
                    for ev in self.mx2_recent_events.iter().rev().take(4) {
                        ui.label(RichText::new(format!("  • {}", ev)).monospace().color(TraktorTheme::PLAY_ACTIVE).font(egui::FontId::monospace(9.5)));
                    }
                } else {
                    ui.label(RichText::new("Touch any button, fader, knob, or jogwheel on the hardware to see live events here.").italics());
                }
            });

        ui.add_space(6.0);

        // 5. Detected Hardware Devices Table
        ui.label(RichText::new("Discovered USB, HIDRAW & ALSA Devices:").strong());
        egui::ScrollArea::vertical()
            .id_salt("prefs_discovered_hw_scroll")
            .max_height(110.0)
            .show(ui, |ui| {
                egui::Grid::new("prefs_discovered_hw_grid")
                    .striped(true)
                    .spacing(Vec2::new(10.0, 3.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Type").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("Device Node / Path").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("Device Model").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("VID:PID").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("Access / Action").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.end_row();

                        for item in &self.discovered_hardware {
                            ui.label(RichText::new(item.category).monospace());
                            ui.label(RichText::new(&item.identifier).monospace().color(TraktorTheme::DECK_A));
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
                                            ui.label(RichText::new("✔ Active").color(TraktorTheme::PLAY_ACTIVE).strong());
                                        } else if ui.button("Connect").clicked() {
                                            self.trigger_connect_node = Some(item.identifier.clone());
                                        }
                                    }
                                    retraktor_hid::DeviceAccessLevel::ReadOnlyOk => {
                                        ui.label(RichText::new("Read-Only").color(Color32::from_rgb(255, 170, 0)));
                                    }
                                    retraktor_hid::DeviceAccessLevel::PermissionDenied => {
                                        ui.label(RichText::new("Permission Denied").color(Color32::from_rgb(255, 80, 80)));
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

        ui.add_space(6.0);

        // 6. Interactive Visual Controller Surface Map & Custom Remapper
        egui::CollapsingHeader::new(RichText::new("Interactive Controller Surface Map (Click Any Control to Remap & Recolor)").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Click any button, pad, knob, or jogwheel below to reassign its target action and LED backlight color:").italics());
                ui.add_space(4.0);

                // Graphical Representation of Traktor MX2 Hardware
                egui::Frame::none()
                    .fill(TraktorTheme::BG_WINDOW)
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 55, 75)))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // --- DECK A (LEFT) ---
                            ui.vertical(|ui| {
                                ui.label(RichText::new("DECK A (LEFT)").color(TraktorTheme::DECK_A).strong());

                                // Platter / Jogwheel
                                let jog_l_selected = self.selected_visual_control.as_deref() == Some("surface.left.jogwheel");
                                let jog_bg = if jog_l_selected { TraktorTheme::DECK_A } else { Color32::from_rgb(26, 30, 40) };
                                if ui.add(
                                    egui::Button::new(RichText::new("◎ JOGWHEEL PLATTER A\n(Touch = Vinyl Scratch | Rim = Pitch Nudge)").strong().color(Color32::WHITE))
                                        .fill(jog_bg)
                                        .min_size(Vec2::new(170.0, 44.0))
                                ).clicked() {
                                    self.selected_visual_control = Some("surface.left.jogwheel".to_string());
                                }

                                ui.horizontal(|ui| {
                                    if ui.button("TT (Scratch)").clicked() {
                                        self.jog_mode_turntable = true;
                                    }
                                    if ui.button("JOG (Bend)").clicked() {
                                        self.jog_mode_turntable = false;
                                    }
                                });

                                ui.add_space(2.0);
                                ui.label(RichText::new("Performance Pads (Stems / Cues):").font(egui::FontId::monospace(9.0)));
                                // 4x2 Pad Grid
                                egui::Grid::new("deck_a_pads_grid").spacing(Vec2::new(4.0, 4.0)).show(ui, |ui| {
                                    let pad_defs = [
                                        ("Pad 1\nDrums", Color32::from_rgb(255, 120, 0), "surface.left.pads.1"),
                                        ("Pad 2\nBass", Color32::from_rgb(30, 144, 255), "surface.left.pads.2"),
                                        ("Pad 3\nInst", Color32::from_rgb(0, 220, 255), "surface.left.pads.3"),
                                        ("Pad 4\nVocal", Color32::from_rgb(220, 60, 255), "surface.left.pads.4"),
                                    ];
                                    for (name, col, id) in &pad_defs {
                                        let sel = self.selected_visual_control.as_deref() == Some(*id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        if ui.add(egui::Button::new(RichText::new(*name).font(egui::FontId::monospace(8.0)).color(Color32::BLACK)).fill(*col).stroke(stroke).min_size(Vec2::new(38.0, 24.0))).clicked() {
                                            self.selected_visual_control = Some(id.to_string());
                                        }
                                    }
                                    ui.end_row();

                                    let pad_defs_row2 = [
                                        ("Pad 5\nFX", Color32::from_rgb(180, 80, 0), "surface.left.pads.5"),
                                        ("Pad 6\nFX", Color32::from_rgb(20, 100, 180), "surface.left.pads.6"),
                                        ("Pad 7\nFX", Color32::from_rgb(0, 150, 180), "surface.left.pads.7"),
                                        ("Pad 8\nFX", Color32::from_rgb(150, 40, 180), "surface.left.pads.8"),
                                    ];
                                    for (name, col, id) in &pad_defs_row2 {
                                        let sel = self.selected_visual_control.as_deref() == Some(*id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        if ui.add(egui::Button::new(RichText::new(*name).font(egui::FontId::monospace(8.0)).color(Color32::WHITE)).fill(*col).stroke(stroke).min_size(Vec2::new(38.0, 24.0))).clicked() {
                                            self.selected_visual_control = Some(id.to_string());
                                        }
                                    }
                                    ui.end_row();
                                });

                                ui.horizontal(|ui| {
                                    let btn_click = |ui: &mut Ui, label: &str, col: Color32, id: &str, sel_ctrl: &Option<String>| -> bool {
                                        let sel = sel_ctrl.as_deref() == Some(id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        ui.add(egui::Button::new(RichText::new(label).strong().color(Color32::BLACK)).fill(col).stroke(stroke).min_size(Vec2::new(48.0, 24.0))).clicked()
                                    };
                                    if btn_click(ui, "▶ PLAY", Color32::from_rgb(0, 230, 118), "surface.left.play", &self.selected_visual_control) {
                                        self.selected_visual_control = Some("surface.left.play".to_string());
                                    }
                                    if btn_click(ui, "● CUE", Color32::from_rgb(0, 144, 255), "surface.left.cue", &self.selected_visual_control) {
                                        self.selected_visual_control = Some("surface.left.cue".to_string());
                                    }
                                    if btn_click(ui, "SYNC", Color32::from_rgb(0, 144, 255), "surface.left.sync", &self.selected_visual_control) {
                                        self.selected_visual_control = Some("surface.left.sync".to_string());
                                    }
                                });
                            });

                            ui.separator();

                            // --- CENTER 2-CHANNEL MIXER ---
                            ui.vertical(|ui| {
                                ui.label(RichText::new("2-CH MIXER").strong());

                                ui.horizontal(|ui| {
                                    // Ch 1 Controls
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("CH 1").color(TraktorTheme::DECK_A).strong().font(egui::FontId::monospace(9.0)));
                                        let f_click = |ui: &mut Ui, label: &str, id: &str, sel_ctrl: &Option<String>| -> bool {
                                            let sel = sel_ctrl.as_deref() == Some(id);
                                            let bg = if sel { TraktorTheme::DECK_A } else { Color32::from_rgb(30, 36, 48) };
                                            ui.add(egui::Button::new(RichText::new(label).font(egui::FontId::monospace(8.0))).fill(bg).min_size(Vec2::new(54.0, 18.0))).clicked()
                                        };
                                        if f_click(ui, "Gain", "surface.mixer.channels.1.gain", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.1.gain".to_string());
                                        }
                                        if f_click(ui, "HI EQ", "surface.mixer.channels.1.eq.high", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.1.eq.high".to_string());
                                        }
                                        if f_click(ui, "MID EQ", "surface.mixer.channels.1.eq.mid", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.1.eq.mid".to_string());
                                        }
                                        if f_click(ui, "LOW EQ", "surface.mixer.channels.1.eq.low", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.1.eq.low".to_string());
                                        }
                                        if f_click(ui, "FILTER", "surface.mixer.channels.1.channel_fx.amount", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.1.channel_fx.amount".to_string());
                                        }
                                        if f_click(ui, "FLTR ON", "surface.mixer.channels.1.channel_fx.on", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.1.channel_fx.on".to_string());
                                        }
                                    });

                                    ui.separator();

                                    // Ch 2 Controls
                                    ui.vertical(|ui| {
                                        ui.label(RichText::new("CH 2").color(TraktorTheme::DECK_B).strong().font(egui::FontId::monospace(9.0)));
                                        let f_click = |ui: &mut Ui, label: &str, id: &str, sel_ctrl: &Option<String>| -> bool {
                                            let sel = sel_ctrl.as_deref() == Some(id);
                                            let bg = if sel { TraktorTheme::DECK_B } else { Color32::from_rgb(30, 36, 48) };
                                            ui.add(egui::Button::new(RichText::new(label).font(egui::FontId::monospace(8.0))).fill(bg).min_size(Vec2::new(54.0, 18.0))).clicked()
                                        };
                                        if f_click(ui, "Gain", "surface.mixer.channels.2.gain", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.2.gain".to_string());
                                        }
                                        if f_click(ui, "HI EQ", "surface.mixer.channels.2.eq.high", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.2.eq.high".to_string());
                                        }
                                        if f_click(ui, "MID EQ", "surface.mixer.channels.2.eq.mid", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.2.eq.mid".to_string());
                                        }
                                        if f_click(ui, "LOW EQ", "surface.mixer.channels.2.eq.low", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.2.eq.low".to_string());
                                        }
                                        if f_click(ui, "FILTER", "surface.mixer.channels.2.channel_fx.amount", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.2.channel_fx.amount".to_string());
                                        }
                                        if f_click(ui, "FLTR ON", "surface.mixer.channels.2.channel_fx.on", &self.selected_visual_control) {
                                            self.selected_visual_control = Some("surface.mixer.channels.2.channel_fx.on".to_string());
                                        }
                                    });
                                });

                                ui.add_space(2.0);
                                ui.label(RichText::new("MIXER FX:").font(egui::FontId::monospace(8.0)));
                                ui.horizontal(|ui| {
                                    let fx_btns = [
                                        ("FILTER", Color32::from_rgb(255, 120, 0), "surface.mixer.channel_fx.filter"),
                                        ("REVERB", Color32::from_rgb(255, 50, 50), "surface.mixer.channel_fx.fx1"),
                                        ("DELAY", Color32::from_rgb(0, 220, 255), "surface.mixer.channel_fx.fx2"),
                                        ("NOISE", Color32::from_rgb(0, 140, 255), "surface.mixer.channel_fx.fx3"),
                                    ];
                                    for (name, col, id) in &fx_btns {
                                        let sel = self.selected_visual_control.as_deref() == Some(*id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        if ui.add(egui::Button::new(RichText::new(*name).font(egui::FontId::monospace(7.5)).color(Color32::BLACK)).fill(*col).stroke(stroke)).clicked() {
                                            self.selected_visual_control = Some(id.to_string());
                                        }
                                    }
                                });
                            });

                            ui.separator();

                            // --- DECK B (RIGHT) ---
                            ui.vertical(|ui| {
                                ui.label(RichText::new("DECK B (RIGHT)").color(TraktorTheme::DECK_B).strong());

                                let jog_r_selected = self.selected_visual_control.as_deref() == Some("surface.right.jogwheel");
                                let jog_bg = if jog_r_selected { TraktorTheme::DECK_B } else { Color32::from_rgb(26, 30, 40) };
                                if ui.add(
                                    egui::Button::new(RichText::new("◎ JOGWHEEL PLATTER B\n(Touch = Vinyl Scratch | Rim = Pitch Nudge)").strong().color(Color32::WHITE))
                                        .fill(jog_bg)
                                        .min_size(Vec2::new(170.0, 44.0))
                                ).clicked() {
                                    self.selected_visual_control = Some("surface.right.jogwheel".to_string());
                                }

                                ui.horizontal(|ui| {
                                    if ui.button("TT (Scratch)").clicked() {
                                        self.jog_mode_turntable = true;
                                    }
                                    if ui.button("JOG (Bend)").clicked() {
                                        self.jog_mode_turntable = false;
                                    }
                                });

                                ui.add_space(2.0);
                                ui.label(RichText::new("Performance Pads (Stems / Cues):").font(egui::FontId::monospace(9.0)));
                                egui::Grid::new("deck_b_pads_grid").spacing(Vec2::new(4.0, 4.0)).show(ui, |ui| {
                                    let pad_defs = [
                                        ("Pad 1\nDrums", Color32::from_rgb(255, 120, 0), "surface.right.pads.1"),
                                        ("Pad 2\nBass", Color32::from_rgb(30, 144, 255), "surface.right.pads.2"),
                                        ("Pad 3\nInst", Color32::from_rgb(0, 220, 255), "surface.right.pads.3"),
                                        ("Pad 4\nVocal", Color32::from_rgb(220, 60, 255), "surface.right.pads.4"),
                                    ];
                                    for (name, col, id) in &pad_defs {
                                        let sel = self.selected_visual_control.as_deref() == Some(*id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        if ui.add(egui::Button::new(RichText::new(*name).font(egui::FontId::monospace(8.0)).color(Color32::BLACK)).fill(*col).stroke(stroke).min_size(Vec2::new(38.0, 24.0))).clicked() {
                                            self.selected_visual_control = Some(id.to_string());
                                        }
                                    }
                                    ui.end_row();

                                    let pad_defs_row2 = [
                                        ("Pad 5\nFX", Color32::from_rgb(180, 80, 0), "surface.right.pads.5"),
                                        ("Pad 6\nFX", Color32::from_rgb(20, 100, 180), "surface.right.pads.6"),
                                        ("Pad 7\nFX", Color32::from_rgb(0, 150, 180), "surface.right.pads.7"),
                                        ("Pad 8\nFX", Color32::from_rgb(150, 40, 180), "surface.right.pads.8"),
                                    ];
                                    for (name, col, id) in &pad_defs_row2 {
                                        let sel = self.selected_visual_control.as_deref() == Some(*id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        if ui.add(egui::Button::new(RichText::new(*name).font(egui::FontId::monospace(8.0)).color(Color32::WHITE)).fill(*col).stroke(stroke).min_size(Vec2::new(38.0, 24.0))).clicked() {
                                            self.selected_visual_control = Some(id.to_string());
                                        }
                                    }
                                    ui.end_row();
                                });

                                ui.horizontal(|ui| {
                                    let btn_click = |ui: &mut Ui, label: &str, col: Color32, id: &str, sel_ctrl: &Option<String>| -> bool {
                                        let sel = sel_ctrl.as_deref() == Some(id);
                                        let stroke = if sel { Stroke::new(2.0_f32, Color32::WHITE) } else { Stroke::NONE };
                                        ui.add(egui::Button::new(RichText::new(label).strong().color(Color32::BLACK)).fill(col).stroke(stroke).min_size(Vec2::new(48.0, 24.0))).clicked()
                                    };
                                    if btn_click(ui, "▶ PLAY", Color32::from_rgb(0, 230, 118), "surface.right.play", &self.selected_visual_control) {
                                        self.selected_visual_control = Some("surface.right.play".to_string());
                                    }
                                    if btn_click(ui, "● CUE", Color32::from_rgb(0, 144, 255), "surface.right.cue", &self.selected_visual_control) {
                                        self.selected_visual_control = Some("surface.right.cue".to_string());
                                    }
                                    if btn_click(ui, "SYNC", Color32::from_rgb(0, 144, 255), "surface.right.sync", &self.selected_visual_control) {
                                        self.selected_visual_control = Some("surface.right.sync".to_string());
                                    }
                                });
                            });
                        });
                    });

                // Detail card for currently selected control
                let mut close_clicked = false;
                if let Some(sel_id) = self.selected_visual_control.clone() {
                    ui.add_space(4.0);
                    egui::Frame::none()
                        .fill(Color32::from_rgb(22, 28, 38))
                        .stroke(Stroke::new(1.0_f32, TraktorTheme::DECK_A))
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(format!("⚙ Editing Control: {}", sel_id)).strong().color(TraktorTheme::DECK_A));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("× Close").clicked() {
                                        close_clicked = true;
                                    }
                                });
                            });

                            if sel_id.contains("jogwheel") {
                                ui.horizontal(|ui| {
                                    ui.label("Jogwheel Mode:");
                                    ui.radio_value(&mut self.jog_mode_turntable, true, "Vinyl Scratch on Touch (Recommended)");
                                    ui.radio_value(&mut self.jog_mode_turntable, false, "CDJ Jog Pitch Bend");
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Scratch Sensitivity:");
                                    ui.add(egui::Slider::new(&mut self.jog_scratch_sensitivity, 0.2..=3.0).text("x Speed"));
                                });
                            } else {
                                ui.horizontal(|ui| {
                                    ui.label("Assigned Action:");
                                    let mut cur_action = self.custom_control_action.get(&sel_id).cloned().unwrap_or_else(|| "Default (Native Mapping)".to_string());
                                    egui::ComboBox::from_id_salt(format!("action_combo_{}", sel_id))
                                        .selected_text(&cur_action)
                                        .show_ui(ui, |ui| {
                                            let actions = [
                                                "Default (Native Mapping)",
                                                "play_deck_a",
                                                "play_deck_b",
                                                "cue_deck_a",
                                                "cue_deck_b",
                                                "sync_deck_a",
                                                "sync_deck_b",
                                                "stem_mute_drums_a",
                                                "stem_mute_bass_a",
                                                "stem_mute_inst_a",
                                                "stem_mute_vocals_a",
                                                "filter_toggle_a",
                                                "filter_toggle_b",
                                            ];
                                            for a in &actions {
                                                if ui.selectable_value(&mut cur_action, a.to_string(), *a).clicked() {
                                                    self.custom_control_action.insert(sel_id.clone(), cur_action.clone());
                                                }
                                            }
                                        });
                                });

                                ui.horizontal(|ui| {
                                    ui.label("LED Color Override:");
                                    let mut cur_color = self.custom_control_color.get(&sel_id).copied().unwrap_or(0x2E);
                                    let color_opts = [
                                        ("Vibrant Blue (0x2E)", 0x2E),
                                        ("Vibrant Green (0x1E)", 0x1E),
                                        ("Warning Red (0x06)", 0x06),
                                        ("Amber Yellow (0x16)", 0x16),
                                        ("Club Orange (0x0E)", 0x0E),
                                        ("Electric Cyan (0x22)", 0x22),
                                        ("Vocal Magenta (0x36)", 0x36),
                                        ("Clean White (0x4A)", 0x4A),
                                    ];
                                    let cur_label = color_opts.iter().find(|(_, val)| *val == cur_color).map(|(l, _)| *l).unwrap_or("Custom");
                                    egui::ComboBox::from_id_salt(format!("color_combo_{}", sel_id))
                                        .selected_text(cur_label)
                                        .show_ui(ui, |ui| {
                                            for (label, val) in &color_opts {
                                                if ui.selectable_value(&mut cur_color, *val, *label).clicked() {
                                                    self.custom_control_color.insert(sel_id.clone(), cur_color);
                                                    self.trigger_led_test = true;
                                                }
                                            }
                                        });
                                });
                            }
                        });
                }
                if close_clicked {
                    self.selected_visual_control = None;
                }
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("MX2 Bottom-Shell LED Brightness:");
            ui.add(egui::Slider::new(&mut self.mx2_bottom_led_brightness, 0.1..=1.0));
        });
    }

    fn show_file_management(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "COLLECTION");
        Self::list_row(
            ui,
            "Autosave",
            "Your collection and playlists save automatically to ~/.local/share/retraktor/collection.nml (Traktor NML v20).",
            |ui| {
                let mut m3u = self.collection_save_format == 2;
                if ui.checkbox(&mut m3u, "Also export collection.m3u8 on exit").changed() {
                    self.collection_save_format = if m3u { 2 } else { 0 };
                }
            },
        );

        Self::list_section_header(ui, "MUSIC FOLDERS WATCHLIST");

        let mut to_remove = None;
        for (idx, folder) in self.music_folders.iter().enumerate() {
            let folder_str = folder.to_string_lossy().to_string();
            Self::list_row(
                ui,
                &folder_str,
                "Watched audio directory included in library scans.",
                |ui| {
                    if ui
                        .button(RichText::new("Remove").color(Color32::from_rgb(240, 80, 80)))
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
            "Add a music folder",
            "ReTraktor scans it for audio files and reads their tags.",
            |ui| {
                if ui.button("Choose folder…").clicked() {
                    self.request_folder_pick = true;
                }
            },
        );

        Self::list_section_header(ui, "SCAN & IMPORT OPTIONS");

        Self::list_row(
            ui,
            "Scan Music Folders on Startup",
            "Look for new files in your music folders each time ReTraktor opens.",
            |ui| {
                ui.checkbox(&mut self.scan_on_startup, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "Auto-Analyze New Tracks",
            "Detect BPM, key and energy for tracks found by a folder scan (existing values are kept).",
            |ui| {
                ui.checkbox(&mut self.analyze_new_tracks, "Enabled");
            },
        );

        Self::list_row(
            ui,
            "Manual Library Rescan",
            &self.file_management_status.clone(),
            |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Rescan All Music Folders")
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(TraktorTheme::ROW_SELECTED)
                        .min_size(Vec2::new(200.0, 26.0)),
                    )
                    .clicked()
                {
                    self.trigger_music_rescan = true;
                }
            },
        );
    }

    fn show_about(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "SYSTEM & APPLICATION DIAGNOSTICS");

        Self::list_row(
            ui,
            "Application Version",
            "Clean-Room Linux Re-Implementation of Traktor Pro 4.",
            |ui| {
                ui.label(
                    RichText::new("ReTraktor Pro 4.5.1")
                        .color(TraktorTheme::DECK_A)
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                );
            },
        );

        Self::list_row(
            ui,
            "Audio & DSP Architecture",
            "Zero-allocation real-time callback with 4-stem Linkwitz-Riley crossover and Demucs AI.",
            |ui| {
                ui.label(RichText::new("Pure Rust • 64-bit / 32-bit Float DSP").monospace());
            },
        );

        Self::list_row(
            ui,
            "Library & Collection Formats",
            "Lossless round-trip support for Traktor .nml (v19/v20), .tsi, .stem.m4a, and Extended .m3u8.",
            |ui| {
                ui.label(RichText::new("NML v20 • M3U8 • Symphonia Tags").monospace());
            },
        );

        Self::list_row(
            ui,
            "Target Platform",
            "Wayland & X11 Desktop Linux with ALSA / PipeWire and hidraw NHL2 support.",
            |ui| {
                ui.label(RichText::new("Linux x86_64 / aarch64").monospace());
            },
        );
    }
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
