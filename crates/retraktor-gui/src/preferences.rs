use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefsTab {
    AudioSetup,
    OutputRouting,
    Mixer,
    Decks,
    TrackDecks,
    Effects,
    StemsAndAi,
    Recording,
    BrowserColumns,
    FileManagement,
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

/// Mixer / deck effect names in `MixerFxKind` order: the index is the kind.
pub const FX_NAMES: [&str; 9] = [
    "Filter",
    "Reverb",
    "Dual delay",
    "Noise sweep",
    "Time gater",
    "Flanger",
    "Barber pole phaser",
    "Dotted eighth delay",
    "Bit crush",
];

pub struct PreferencesDialog {
    /// Set when the user asks for the folder chooser; the app opens it via the library.
    pub request_folder_pick: bool,
    /// The running output stream (device and rate), if any; set by the app each frame.
    pub audio_output: Option<String>,
    /// Channels of the running output, and the routing it plays; set by the app each frame.
    pub audio_output_channels: u16,
    pub audio_route: crate::audio::OutputRoute,
    /// The running output is a system default (chosen, or a fallback); set by the app each frame.
    pub audio_on_system_default: bool,
    /// Rescan was pressed: the app tries the chosen output again if it isn't the one playing.
    pub audio_rescanned: bool,
    /// Outputs found on this computer; `None` until the Audio page first shows.
    pub audio_devices: Option<Vec<crate::audio::OutputChoice>>,
    pub is_open: bool,
    pub current_tab: PrefsTab,
    pub search_query: String,

    // Audio Setup
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub audio_device_name: String,

    // Output Routing
    /// Main and headphone pair indices (0 = channels 1–2) per output ID the user set them for; others use
    /// main 1–2, headphones 3–4. Two distinct pairs below [`crate::audio::MAX_OUTPUT_PAIRS`].
    pub output_pairs: std::collections::BTreeMap<String, [u8; 2]>,
    pub split_cue: bool,

    // Mixer
    pub xfader_curve_idx: usize,
    pub autogain: bool,
    pub limiter_enabled: bool,
    pub neural_mix_eq_mode: bool,

    // Decks & Waveforms
    pub four_deck_mode: bool,
    pub waveform_color_mode: usize, // 0: Spectrum 4-Band, 1: Ultraviolet, 2: High-Contrast RGB

    // Track Decks & Scratch
    pub keylock_quality: usize, // 0: HiQ (Scratch-Preserving), 1: ECO
    pub jog_scratch_sensitivity: f32,
    pub jog_mode_turntable: bool,
    pub lock_playing_decks: bool,

    // Mixer FX
    pub mixer_fx_slots: [usize; 4],

    // Stems & AI
    pub demucs_install_state: crate::demucs::DemucsInstallState,
    pub trigger_demucs_install: bool,
    pub demucs_active_separation: Option<String>,

    // Recording
    pub recording_dir: std::path::PathBuf,

    // File Management & Folders
    pub music_folders: Vec<std::path::PathBuf>,
    pub scan_on_startup: bool,
    pub analyze_new_tracks: bool,
    pub hide_missing_files: bool,
    pub trigger_music_rescan: bool,
    pub file_management_status: String,
    pub library_columns: LibraryColumnsConfig,
    pub collection_save_format: usize, // 0: Traktor .nml (v20), 1: Extended .m3u8, 2: Both (.nml + .m3u8)

    // Controller Manager & Surface Remapper
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
    /// Controller rules file (~/.config/retraktor/controller-rules.json), run before the simple mappings.
    pub controller_rules: crate::rules_file::RulesFile,
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
        // Loaded before the generic controller reader starts, so it reads CCs the authored way from the first message.
        // Unit tests never read or act on the user's own rules.
        let controller_rules = if cfg!(test) {
            crate::rules_file::RulesFile::unloaded(retraktor_hid::controller_rules::default_rules_path())
        } else {
            crate::rules_file::RulesFile::load(retraktor_hid::controller_rules::default_rules_path())
        };

        Self {
            audio_output: None,
            audio_output_channels: 0,
            audio_route: crate::audio::OutputRoute::default(),
            audio_on_system_default: false,
            audio_rescanned: false,
            audio_devices: None,
            is_open: false,
            current_tab: PrefsTab::Controllers,
            search_query: String::new(),
            ai_settings: crate::ai::AiSettings::load(),
            ai_test_runner: None,
            ai_test_status: None,
            sample_rate: 48000,
            buffer_size: 128,
            audio_device_name: String::new(),
            output_pairs: Default::default(),
            split_cue: false,
            xfader_curve_idx: 0, // Smooth
            autogain: true,
            limiter_enabled: true,
            four_deck_mode: false,
            waveform_color_mode: 0,
            keylock_quality: 0, // HiQ
            jog_scratch_sensitivity: 1.0,
            jog_mode_turntable: true,
            lock_playing_decks: true,
            mixer_fx_slots: [1, 2, 3, 4], // MX2 FX1–FX4: Reverb, Delay, Noise, Gater (FILTER has its own button)
            neural_mix_eq_mode: false,
            demucs_install_state: crate::demucs::DemucsInstallState::NotInstalled,
            trigger_demucs_install: false,
            demucs_active_separation: None,
            recording_dir: home.join("Music"),
            music_folders: default_music.into_iter().collect(),
            request_folder_pick: false,
            scan_on_startup: true,
            analyze_new_tracks: true,
            hide_missing_files: false,
            trigger_music_rescan: false,
            file_management_status: "Ready".to_string(),
            library_columns: LibraryColumnsConfig::default(),
            collection_save_format: 0, // Traktor .nml (v20)
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
            generic_manager: crate::midi::GenericControllerManager::with_cc_modes(controller_rules.engine.rules().midi_cc_modes().to_vec()),
            last_surface_control: None,
            controller_rules,
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
        let c = &self.library_columns;
        let folders: Vec<String> = self.music_folders.iter().map(|f| f.to_string_lossy().into_owned()).collect();
        let doc = serde_json::json!({
            "sample_rate": self.sample_rate,
            "buffer_size": self.buffer_size,
            "audio_device_name": self.audio_device_name,
            "split_cue": self.split_cue,
            "output_pairs": output_pairs_value(&self.output_pairs),
            "keylock_quality": self.keylock_quality,
            "four_deck_mode": self.four_deck_mode,
            "waveform_color_mode": self.waveform_color_mode,
            "xfader_curve_idx": self.xfader_curve_idx,
            "autogain": self.autogain,
            "limiter_enabled": self.limiter_enabled,
            "lock_playing_decks": self.lock_playing_decks,
            "scan_on_startup": self.scan_on_startup,
            "analyze_new_tracks": self.analyze_new_tracks,
            "hide_missing_files": self.hide_missing_files,
            "neural_mix_eq_mode": self.neural_mix_eq_mode,
            "recording_dir": self.recording_dir.to_string_lossy(),
            "mixer_fx_slots": self.mixer_fx_slots,
            "collection_save_format": self.collection_save_format,
            "col_album": c.album,
            "col_bpm": c.bpm,
            "col_key": c.key,
            "col_stems": c.stems,
            "col_duration": c.duration,
            "col_genre": c.genre,
            "col_rating": c.rating,
            "col_comment": c.comment,
            "col_remixer": c.remixer,
            "col_label": c.label,
            "col_year": c.year,
            "col_bitrate": c.bitrate,
            "col_format": c.format,
            "col_track_number": c.track_number,
            "col_energy": c.energy,
            "music_folders": folders,
        });
        serde_json::to_string_pretty(&doc).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn apply_json_str(&mut self, text: &str) {
        let doc: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => {
                // Legacy fallback: recover valid JSON key/value lines from an older file with an unescaped path.
                let mut map = serde_json::Map::new();
                for line in text.lines() {
                    let trimmed = line.trim().trim_end_matches(',');
                    if let Some((k, v)) = trimmed.split_once(':') {
                        if let (Ok(key), Ok(val)) = (
                            serde_json::from_str::<String>(k.trim()),
                            serde_json::from_str::<serde_json::Value>(v.trim()),
                        ) {
                            map.insert(key, val);
                        }
                    }
                }
                serde_json::Value::Object(map)
            }
        };
        let Some(obj) = doc.as_object() else { return };
        let b = |k: &str| obj.get(k).and_then(|v| v.as_bool());
        let u = |k: &str| obj.get(k).and_then(|v| v.as_u64());
        if let Some(v) = u("sample_rate") { self.sample_rate = v as u32; }
        if let Some(v) = u("buffer_size") { self.buffer_size = v as u32; }
        if let Some(v) = obj.get("audio_device_name").and_then(|v| v.as_str()) { self.audio_device_name = v.to_string(); }
        if let Some(v) = b("split_cue") { self.split_cue = v; }
        if let Some(v) = obj.get("output_pairs") { self.output_pairs = output_pairs_from(v); }
        if let Some(v) = u("keylock_quality") { self.keylock_quality = v as usize; }
        if let Some(v) = b("four_deck_mode") { self.four_deck_mode = v; }
        if let Some(v) = u("waveform_color_mode") { self.waveform_color_mode = v as usize; }
        if let Some(v) = u("xfader_curve_idx") { self.xfader_curve_idx = v as usize; }
        if let Some(v) = b("autogain") { self.autogain = v; }
        if let Some(v) = b("limiter_enabled") { self.limiter_enabled = v; }
        if let Some(v) = b("lock_playing_decks") { self.lock_playing_decks = v; }
        if let Some(v) = b("scan_on_startup") { self.scan_on_startup = v; }
        if let Some(v) = b("analyze_new_tracks") { self.analyze_new_tracks = v; }
        if let Some(v) = b("hide_missing_files") { self.hide_missing_files = v; }
        if let Some(v) = b("neural_mix_eq_mode") { self.neural_mix_eq_mode = v; }
        if let Some(v) = obj.get("recording_dir").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            self.recording_dir = std::path::PathBuf::from(v);
        }
        if let Some(arr) = obj.get("mixer_fx_slots").and_then(|v| v.as_array()) {
            for (i, item) in arr.iter().take(4).enumerate() {
                if let Some(n) = item.as_u64() {
                    self.mixer_fx_slots[i] = n as usize;
                }
            }
        }
        if let Some(v) = u("collection_save_format") { self.collection_save_format = v as usize; }
        let c = &mut self.library_columns;
        if let Some(v) = b("col_album") { c.album = v; }
        if let Some(v) = b("col_bpm") { c.bpm = v; }
        if let Some(v) = b("col_key") { c.key = v; }
        if let Some(v) = b("col_stems") { c.stems = v; }
        if let Some(v) = b("col_duration") { c.duration = v; }
        if let Some(v) = b("col_genre") { c.genre = v; }
        if let Some(v) = b("col_rating") { c.rating = v; }
        if let Some(v) = b("col_comment") { c.comment = v; }
        if let Some(v) = b("col_remixer") { c.remixer = v; }
        if let Some(v) = b("col_label") { c.label = v; }
        if let Some(v) = b("col_year") { c.year = v; }
        if let Some(v) = b("col_bitrate") { c.bitrate = v; }
        if let Some(v) = b("col_format") { c.format = v; }
        if let Some(v) = b("col_track_number") { c.track_number = v; }
        if let Some(v) = b("col_energy") { c.energy = v; }
        if let Some(arr) = obj.get("music_folders").and_then(|v| v.as_array()) {
            self.music_folders = arr
                .iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
                .collect();
        }
    }

    /// The routing chosen for the selected output.
    pub fn output_route(&self) -> crate::audio::OutputRoute {
        let [main, headphones] = self.output_pairs.get(&self.audio_device_name).copied().unwrap_or([0, 1]);
        if self.split_cue { crate::audio::OutputRoute::Split } else { crate::audio::OutputRoute::Pairs { main, headphones: Some(headphones) } }
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
            PrefsTab::Mixer => ("DECKS & MIXER", "Mixer"),
            PrefsTab::Decks => ("DECKS & MIXER", "Decks Layout & Waveforms"),
            PrefsTab::TrackDecks => ("DECKS & MIXER", "Track Decks & Vinyl Scratch"),
            PrefsTab::Effects => ("DECKS & MIXER", "Channel FX"),
            PrefsTab::StemsAndAi => ("STEMS & RECORDING", "Stems & Demucs AI"),
            PrefsTab::Recording => ("STEMS & RECORDING", "Recording"),
            PrefsTab::BrowserColumns => ("COLLECTION & HARDWARE", "Browser & Metadata Columns"),
            PrefsTab::FileManagement => ("COLLECTION & HARDWARE", "File Management & Folders"),
            PrefsTab::Controllers => ("COLLECTION & HARDWARE", "Controller Manager & Remapper"),
            PrefsTab::OtherControllers => ("COLLECTION & HARDWARE", "Other Controllers"),
            PrefsTab::Look => ("LOOK", "Theme & colours"),
            PrefsTab::Ai => ("AI ASSISTANT", "API Key & Assistant Setup"),
            PrefsTab::About => ("COLLECTION & HARDWARE", "About & Diagnostics"),
        }
    }

    /// Settings pages grouped into the top-level tabs.
    pub const GROUPS: [(&'static str, &'static [PrefsTab]); 8] = [
        ("Audio", &[PrefsTab::AudioSetup, PrefsTab::OutputRouting]),
        ("Decks & Mixer", &[PrefsTab::Decks, PrefsTab::TrackDecks, PrefsTab::Mixer, PrefsTab::Effects]),
        ("Library", &[PrefsTab::FileManagement, PrefsTab::BrowserColumns]),
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
            PrefsTab::Mixer => self.show_mixer_prefs(ui),
            PrefsTab::Decks => self.show_deck_prefs(ui),
            PrefsTab::TrackDecks => self.show_track_decks_prefs(ui),
            PrefsTab::Effects => self.show_effects_prefs(ui),
            PrefsTab::StemsAndAi => self.show_stems_prefs(ui),
            PrefsTab::Recording => self.show_recording_prefs(ui),
            PrefsTab::BrowserColumns => self.show_browser_columns_prefs(ui),
            PrefsTab::FileManagement => self.show_file_management(ui),
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
                                        .hint_text("Find a section…")
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
                    let q = self.search_query.trim().to_lowercase();
                    let any = q.is_empty()
                        || Self::GROUPS.iter().any(|(label, tabs)| label.to_lowercase().contains(&q) || tabs.iter().any(|t| self.get_tab_breadcrumb(*t).1.to_lowercase().contains(&q)));
                    if !any {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("No section matches. Try audio, mixer, library, stems, controllers, look or AI.").color(TraktorTheme::TEXT_SECONDARY()));
                            if ui.small_button("Clear").clicked() {
                                self.search_query.clear();
                            }
                        });
                    }
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
                            // Settings rows stay in a readable column; the controller drawing gets the full width.
                            let controllers = self.current_group() == &[PrefsTab::Controllers, PrefsTab::OtherControllers];
                            let full = avail_w - 16.0;
                            let w = if controllers { full } else { full.min(1000.0) };
                            ui.horizontal_top(|ui| {
                                ui.add_space(((full - w) * 0.5).max(0.0));
                                ui.vertical(|ui| {
                                    ui.set_width(w);
                                    ui.spacing_mut().item_spacing = Vec2::new(6.0, 6.0);
                                    for tab in self.current_group() {
                                        if controllers && *tab != self.current_tab {
                                            continue;
                                        }
                                        self.show_section(ui, *tab);
                                        ui.add_space(12.0);
                                    }
                                });
                            });
                        });
                });
        });
    }

    /// Backwards compatibility shim
    fn show_audio_setup(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Sound card");

        let devices = self.audio_devices.get_or_insert_with(crate::audio::list_outputs).clone();
        let label_of = |c: &crate::audio::OutputChoice| format!("{} ({} ch)", c.label, c.channels);
        let device_label = devices
            .iter()
            .find(|c| c.id == self.audio_device_name)
            .map_or_else(|| if self.audio_device_name.is_empty() { "System default".to_string() } else { self.audio_device_name.clone() }, label_of);
        Self::list_row(
            ui,
            "Audio device",
            "Choose the audio interface your speakers and headphones are wired to. Changes apply straight away.",
            |ui| {
                if ui.button("Rescan").on_hover_text("Look again for sound cards (after plugging one in)").clicked() {
                    self.audio_devices = None;
                    self.audio_rescanned = true;
                }
                egui::ComboBox::from_id_salt("audio_device_combo")
                    .selected_text(device_label)
                    .width(300.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.audio_device_name, String::new(), "System default");
                        for c in &devices {
                            ui.selectable_value(&mut self.audio_device_name, c.id.clone(), label_of(c));
                        }
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
            "The output ReTraktor is actually playing through, as reported by the sound system.",
            |ui| {
                let (text, colour) = if let Some(info) = &self.audio_output {
                    (format!("Output stream opened · {info}"), TraktorTheme::PLAY_ACTIVE())
                } else {
                    ("No audio output: pick another device above".to_string(), TraktorTheme::ACCENT())
                };
                ui.label(
                    RichText::new(text)
                        .color(colour)
                        .font(egui::FontId::monospace(11.5))
                        .strong(),
                );
            },
        );
    }

    fn show_output_routing(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Outputs");

        // The pairs the chosen output has, however wide the open stream is: as listed, else as the open stream
        // when that is the chosen output. The system default, whose wiring is unknown, stays stereo; Split cue
        // uses 1–2 and keeps the pairs for when it is turned off.
        use crate::audio::{pair_label, Placement};
        let devices = self.audio_devices.get_or_insert_with(crate::audio::list_outputs).clone();
        let device = self.audio_device_name.clone();
        let channels = self.audio_output_channels as usize;
        let playing_it = channels > 0 && !self.audio_on_system_default;
        let listed = devices.iter().find(|c| c.id == device).map(|c| c.channels).or(playing_it.then_some(self.audio_output_channels));
        let missing = !device.is_empty() && listed.is_none();
        let offered = if device.is_empty() { 1 } else { listed.map_or(0, |ch| (ch / 2).min(crate::audio::MAX_OUTPUT_PAIRS.into()) as u8) };
        let saved = self.output_pairs.get(&device).copied().unwrap_or([0, 1]);
        let mut pairs = saved;
        let pair_name = |pair: u8| {
            let note = if missing { " (saved; output not found)" } else if pair < offered { "" } else { " (not on this output)" };
            format!("Channels {}{note}", pair_label(2 * pair as usize))
        };
        // A mono output, Split cue and the system default show what they really play, not the pairs kept for
        // later; a missing output shows its saved pairs.
        let mono = !missing && (listed == Some(1) || channels == 1);
        let shown = |bus: usize, pairs: [u8; 2]| match bus {
            _ if mono => ["Channel 1 (mono)", "None on a mono output"][bus].to_string(),
            _ if missing => pair_name(pairs[bus]),
            _ if self.split_cue && channels == 0 => "Split cue (no output open)".to_string(),
            _ if self.split_cue => format!("Channels 1–2, {} ear (split cue)", ["right", "left"][bus]),
            1 if device.is_empty() => "None on the system default".to_string(),
            _ => pair_name(pairs[bus]),
        };
        for (bus, title, about) in [
            (0, "Main output", "The pair wired to your speakers, as the audio interface numbers its outputs."),
            (1, "Headphones", "The pair that feeds the headphone output on its own, for the cue mix. A jack that only mirrors the main output has no pair of its own. Use the mixer's PHONES knobs for level and cue/main blend."),
        ] {
            Self::list_row(ui, title, about, |ui| {
                ui.add_enabled_ui(!self.split_cue && offered >= 2, |ui| {
                    egui::ComboBox::from_id_salt(("output_pair", bus)).selected_text(shown(bus, pairs)).width(260.0).show_ui(ui, |ui| {
                        for pair in 0..offered {
                            if ui.selectable_label(pairs[bus] == pair, pair_name(pair)).clicked() {
                                pairs = choose_pair(pairs, bus, pair);
                            }
                        }
                    });
                });
            });
        }
        // One hint: a chosen card that isn't the one playing (missing at start, or back since) is reopened by Rescan.
        let not_open = !device.is_empty() && (channels == 0 || self.audio_on_system_default);
        if pairs != saved {
            self.output_pairs.insert(device, pairs);
        }

        let (now, ok) = match self.audio_route.place(channels) {
            _ if channels == 0 => ("No output open".to_string(), false),
            Placement::Mono => ("Mono output: the main mix only, no headphones".to_string(), false),
            Placement::Split => ("Channels 1–2 split: left ear = cue, right ear = main (mono)".to_string(), true),
            Placement::Pairs { main, phones: Some(h) } => (format!("Main on {}, headphones on {} of {channels} channels", pair_label(main), pair_label(h)), true),
            Placement::Pairs { main, phones: None } => (format!("Main on {}, no headphones", pair_label(main)), false),
        };
        let instead = if channels == 0 { "nothing is open" } else { "the system default plays" };
        let hint = match () {
            _ if not_open && missing => format!(" · The chosen output isn't connected ({instead}): plug it in, then press Rescan under Sound card."),
            _ if not_open => format!(" · The chosen output isn't open ({instead}): press Rescan under Sound card."),
            _ if ok || channels < 2 => String::new(),
            _ if self.audio_on_system_default => " · Choose an audio interface with a separate headphone pair, or turn on Split cue.".to_string(),
            // The pair selectors are disabled below two pairs, so only offer them when they work.
            _ if offered < 2 => format!(" · This {channels}-channel output has one stereo pair: choose an audio interface with two stereo output pairs, or turn on Split cue."),
            _ => format!(" · This {channels}-channel output has no free pair for them: choose another pair or audio interface, or turn on Split cue."),
        };
        let (now, ok) = (now + &hint, ok && !not_open);
        Self::list_row(ui, "Playing now", "Where the mix really goes on the open output, after any fallback.", |ui| {
            ui.label(RichText::new(now).color(if ok { TraktorTheme::PLAY_ACTIVE() } else { TraktorTheme::ACCENT() }));
        });

        Self::list_row(
            ui,
            "Split cue",
            "For a stereo-only output with a splitter cable: cue in the left ear, main mix in the right, both mono.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.split_cue, TraktorTheme::PLAY_ACTIVE(), "Split cue");
            },
        );
    }

    fn show_mixer_prefs(&mut self, ui: &mut Ui) {
        Self::list_section_header(ui, "Mixer");

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
            "Match volume across quiet and loud tracks automatically. Applies to the next track you load. Its waveform shows the matched level.",
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

        Self::list_row(
            ui,
            "EQ controls stems",
            "Hardware EQ knobs (MX2 or MIDI) set vocal, instrument and drum levels instead of EQ. On screen, the deck STEMS button turns the mixer into stem controls either way.",
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
            "Colours of bass, mids and highs on the deck waveforms and every track overview. Spectrum uses the theme's own waveform colours (Preferences > Look).",
            |ui| {
                egui::ComboBox::from_id_salt("wf_palette_combo")
                    // An unknown choice (a newer version's) draws as Spectrum, so it shows as Spectrum.
                    .selected_text(*palettes.get(self.waveform_color_mode).unwrap_or(&palettes[0]))
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        for (i, p) in palettes.iter().enumerate() {
                            ui.selectable_value(&mut self.waveform_color_mode, i, *p);
                        }
                    });
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
        Self::list_section_header(ui, "Channel FX");
        let fx_options = FX_NAMES;

        for idx in 0..4 {
            let mut slot_val = self.mixer_fx_slots[idx].min(fx_options.len() - 1);
            Self::list_row(
                ui,
                &format!("Slot {}", idx + 1),
                "The mixer channel effect the MX2's FX button of this number picks.",
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
        let ports: Vec<String> = self.generic_manager.devices.iter().filter_map(|d| d.rule_device.strip_prefix("midi.")).map(str::to_string).collect();
        crate::rules_file::show(ui, &mut self.controller_rules, self.last_surface_control.as_deref(), &ports);
        // 6. Line-drawn replica of the MX2: see and change what every control does.
        ui.add_space(8.0);
        let connected = self.mx2_connected_node.is_some();
        let touched = self.last_surface_control.clone();
        if crate::controller_map::show(ui, &mut self.mapping, &mut self.mapper, touched.as_deref(), connected) {
            self.mapper.save_error = self.mapping.save().err().map(|e| e.to_string());
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
        let ports: Vec<String> = self.generic_manager.devices.iter().filter_map(|d| d.rule_device.strip_prefix("midi.")).map(str::to_string).collect();
        crate::rules_file::show(ui, &mut self.controller_rules, self.last_surface_control.as_deref(), &ports);
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

        Self::list_row(
            ui,
            "Hide missing files",
            "Hide tracks whose files cannot be found — handy when switching external drives. They remain visible under Missing.",
            |ui| {
                crate::dot::dot_toggle(ui, &mut self.hide_missing_files, TraktorTheme::PLAY_ACTIVE(), "Hide missing files");
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

/// Main and headphone pairs after choosing `pair` for `bus` (0 = main): the other output's pair swaps the two
/// in one step, so they never share one.
fn choose_pair(mut pairs: [u8; 2], bus: usize, pair: u8) -> [u8; 2] {
    if pairs[1 - bus] == pair {
        pairs[1 - bus] = pairs[bus];
    }
    pairs[bus] = pair;
    pairs
}

/// The saved pairs as a JSON object by first channel: `{"pw:k2": {"main": 3, "headphones": 1}}`.
fn output_pairs_value(pairs: &std::collections::BTreeMap<String, [u8; 2]>) -> serde_json::Value {
    let first_channel = |pair: u8| 2 * pair as u16 + 1;
    let saved = pairs.iter().map(|(id, [main, phones])| (id.clone(), serde_json::json!({ "main": first_channel(*main), "headphones": first_channel(*phones) })));
    serde_json::Value::Object(saved.collect())
}

/// Saved pairs back as pair indices. An entry that isn't two different odd first channels on the first
/// [`crate::audio::MAX_OUTPUT_PAIRS`] pairs is dropped, so that output gets the defaults again.
fn output_pairs_from(saved: &serde_json::Value) -> std::collections::BTreeMap<String, [u8; 2]> {
    let pair = |first: &serde_json::Value| {
        first.as_u64().filter(|c| c % 2 == 1 && *c < 2 * crate::audio::MAX_OUTPUT_PAIRS as u64).map(|c| (c / 2) as u8)
    };
    let entries = saved.as_object().into_iter().flatten();
    entries
        .filter_map(|(id, p)| Some((id.clone(), [pair(&p["main"])?, pair(&p["headphones"])?])).filter(|(_, [main, phones])| main != phones))
        .collect()
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

#[cfg(test)]
mod tests {
    #[test]
    fn error_summary_takes_last_line() {
        let e = "Demucs verification failed: Traceback (most recent call last):\n  File x\nModuleNotFoundError: No module named 'numpy'\n";
        assert_eq!(super::error_summary(e), "ModuleNotFoundError: No module named 'numpy'");
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
        assert!(!prefs.hide_missing_files);
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
        original.waveform_color_mode = 1;
        original.hide_missing_files = true;
        original.neural_mix_eq_mode = true;
        original.recording_dir = PathBuf::from(r#"/mnt/usb:1/Sets "Live"\2026"#);
        original.mixer_fx_slots = [4, 2, 7, 1];
        original.music_folders = vec![
            "/DATA/Media/Music".into(),
            "/home/dj/My Music, Old".into(),
            r#"/mnt/drive:a/Best "Club" Mixes\Techno"#.into(),
        ];

        let json = original.to_json_str();
        assert!(serde_json::from_str::<serde_json::Value>(&json).is_ok(), "must emit valid JSON: {json}");
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
        assert_eq!(restored.waveform_color_mode, 1);
        assert!(restored.hide_missing_files);
        assert!(restored.neural_mix_eq_mode);
        assert_eq!(restored.recording_dir, original.recording_dir);
        assert_eq!(restored.mixer_fx_slots, [4, 2, 7, 1]);
        assert_eq!(restored.music_folders, original.music_folders);
    }

    /// Main and headphone pairs are kept per output, so a Xone:K2 (main 3–4, headphones 1–2) and another
    /// controller keep their own across a switch and a restart, also from a file another tool re-indented.
    /// Older files and other outputs get main 1–2, headphones 3–4; Split cue keeps the pairs for later. A
    /// malformed, out-of-range or shared entry is dropped rather than offered. Device IDs with quotes and
    /// backslashes survive, and an older file with a broken line still gives its pairs. Choosing the other
    /// output's pair in the selector swaps the two, either way round.
    #[test]
    fn output_pairs_are_kept_per_output() {
        use crate::audio::OutputRoute;
        assert_eq!(choose_pair([0, 1], 0, 1), [1, 0], "main to 3–4: headphones to 1–2");
        assert_eq!(choose_pair([0, 1], 1, 0), [1, 0], "headphones to 1–2: main to 3–4");
        assert_eq!(choose_pair([0, 1], 1, 2), [0, 2], "a free pair: main stays");
        assert_eq!(choose_pair([1, 0], 0, 1), [1, 0], "the pair it has");

        let mut prefs = PreferencesDialog::default();
        prefs.apply_json_str("{\n  \"sample_rate\": 48000,\n  \"split_cue\": false\n}");
        assert_eq!(prefs.output_route(), OutputRoute::default(), "an older file");
        let odd_id = r#"alsa:hw:CARD="Q\x",DEV=0"#;
        prefs.audio_device_name = odd_id.into();
        prefs.output_pairs.insert("pw:k2".into(), [1, 0]);
        prefs.output_pairs.insert(odd_id.into(), [0, 2]);
        let json = prefs.to_json_str();
        assert_eq!(json.lines().filter(|l| l.contains("pw:k2")).count(), 1, "{json}");

        let mut restored = PreferencesDialog::default();
        restored.apply_json_str(&json);
        assert_eq!((restored.audio_device_name.as_str(), &restored.output_pairs), (odd_id, &prefs.output_pairs), "{json}");
        let broken_legacy = format!(
            "{{\n  \"audio_device_name\": {},\n  \"output_pairs\": {},\n  \"music_folders\": [\"/a\"b\\c\"]\n}}",
            serde_json::json!(odd_id),
            output_pairs_value(&prefs.output_pairs),
        );
        assert!(serde_json::from_str::<serde_json::Value>(&broken_legacy).is_err(), "{broken_legacy}");
        let mut legacy = PreferencesDialog::default();
        legacy.apply_json_str(&broken_legacy);
        assert_eq!((legacy.audio_device_name.as_str(), &legacy.output_pairs), (odd_id, &prefs.output_pairs), "line by line: {broken_legacy}");
        let pretty = serde_json::to_string_pretty(&serde_json::from_str::<serde_json::Value>(&json).unwrap()).unwrap();
        let mut reindented = PreferencesDialog::default();
        reindented.apply_json_str(&pretty);
        assert_eq!(reindented.output_pairs, prefs.output_pairs, "{pretty}");

        let k2 = OutputRoute::Pairs { main: 1, headphones: Some(0) };
        for (device, split, route) in [
            ("pw:k2", false, k2),
            ("pw:laptop", false, OutputRoute::default()),
            (odd_id, false, OutputRoute::Pairs { main: 0, headphones: Some(2) }),
            ("pw:k2", true, OutputRoute::Split),
            ("pw:k2", false, k2),
        ] {
            restored.audio_device_name = device.into();
            restored.split_cue = split;
            assert_eq!(restored.output_route(), route, "{device}, split cue {split}");
        }

        let bad = r#"{"output_pairs": {"pw:ok": {"main": 63, "headphones": 1}, "pw:even": {"main": 2, "headphones": 1},
            "pw:shared": {"main": 3, "headphones": 3}, "pw:far": {"main": 65, "headphones": 1}, "pw:neg": {"main": -1, "headphones": 1},
            "pw:huge": {"main": 18446744073709551615, "headphones": 1}, "pw:frac": {"main": 1.5, "headphones": 3},
            "pw:half": {"main": 1}, "pw:list": [3, 1], "pw:text": {"main": "3", "headphones": "1"}}}"#;
        for (text, kept) in [(bad, vec![("pw:ok".to_string(), [31, 0])]), (r#"{"output_pairs": "nonsense"}"#, vec![]), ("\"output_pairs\": {not json", vec![])] {
            let mut prefs = PreferencesDialog::default();
            prefs.apply_json_str(text);
            assert_eq!(prefs.output_pairs, kept.into_iter().collect(), "{text}");
        }
    }
}
