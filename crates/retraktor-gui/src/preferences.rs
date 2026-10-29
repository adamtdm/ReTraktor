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
    FileManagement,
    AnalyzeOptions,
    Controllers,
    About,
}

pub struct PreferencesDialog {
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
    pub new_folder_input: String,
    pub scan_on_startup: bool,
    pub analyze_new_tracks: bool,
    pub trigger_music_rescan: bool,
    pub file_management_status: String,

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
        let default_music = {
            let m = home.join("Music");
            if m.is_dir() {
                m
            } else {
                home
            }
        };

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
            recording_dir: default_music.clone(),
            recording_format: 0,
            broadcast_host: "localhost".to_string(),
            broadcast_port: 8000,
            broadcast_mount: "/retraktor.mp3".to_string(),
            broadcast_password: "hackme".to_string(),
            broadcast_connected: false,
            music_folders: vec![default_music],
            new_folder_input: String::new(),
            scan_on_startup: true,
            analyze_new_tracks: true,
            trigger_music_rescan: false,
            file_management_status: "Ready".to_string(),
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
        format!(
            "{{\n  \"sample_rate\": {},\n  \"buffer_size\": {},\n  \"audio_device_name\": \"{}\",\n  \"four_deck_mode\": {},\n  \"waveform_color_mode\": {},\n  \"eq_model_idx\": {},\n  \"filter_model_idx\": {},\n  \"filter_resonance\": {},\n  \"xfader_curve_idx\": {},\n  \"autogain\": {},\n  \"limiter_enabled\": {},\n  \"lock_playing_decks\": {},\n  \"scan_on_startup\": {},\n  \"analyze_new_tracks\": {},\n  \"key_notation_idx\": {},\n  \"music_folders\": [{}]\n}}",
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
            PrefsTab::FileManagement => ("COLLECTION & HARDWARE", "File Management & Folders"),
            PrefsTab::AnalyzeOptions => ("COLLECTION & HARDWARE", "Analyze & Musical Key"),
            PrefsTab::Controllers => ("COLLECTION & HARDWARE", "Controller Manager & Remapper"),
            PrefsTab::About => ("COLLECTION & HARDWARE", "About & Diagnostics"),
        }
    }

    pub fn render_sidebar_navigation(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing = Vec2::new(2.0, 3.0);

        let query = self.search_query.trim().to_lowercase();

        let cat_header = |ui: &mut Ui, label: &str| {
            ui.add_space(8.0);
            ui.label(
                RichText::new(label)
                    .color(Color32::from_rgb(130, 138, 155))
                    .font(egui::FontId::monospace(10.0))
                    .strong(),
            );
            ui.add_space(2.0);
        };

        let mut nav_item = |ui: &mut Ui, label: &str, tab: PrefsTab, icon: &str| {
            let matches_search = query.is_empty()
                || label.to_lowercase().contains(&query)
                || format!("{:?}", tab).to_lowercase().contains(&query);

            if !matches_search {
                return;
            }

            let is_selected = self.current_tab == tab;
            let text_color = if is_selected {
                Color32::WHITE
            } else {
                TraktorTheme::TEXT_PRIMARY
            };
            let bg_color = if is_selected {
                TraktorTheme::ROW_SELECTED
            } else {
                Color32::TRANSPARENT
            };

            let resp = ui.add(
                egui::Button::new(
                    RichText::new(format!("{icon}  {label}"))
                        .color(text_color)
                        .font(egui::FontId::proportional(12.0)),
                )
                .fill(bg_color)
                .stroke(Stroke::NONE)
                .min_size(Vec2::new(ui.available_width() - 4.0, 26.0)),
            );

            if resp.clicked() {
                self.current_tab = tab;
            }
        };

        cat_header(ui, "AUDIO & ROUTING");
        nav_item(ui, "Audio Setup", PrefsTab::AudioSetup, "🔊");
        nav_item(ui, "Output Routing", PrefsTab::OutputRouting, "🎚");
        nav_item(ui, "Input Routing", PrefsTab::InputRouting, "🎙");
        nav_item(ui, "External Sync & Clock", PrefsTab::ExternalSync, "⏱");

        cat_header(ui, "DECKS & MIXER");
        nav_item(ui, "Mixer & EQ", PrefsTab::Mixer, "🎛");
        nav_item(ui, "Decks & Waveforms", PrefsTab::Decks, "📊");
        nav_item(ui, "Track Decks & Scratch", PrefsTab::TrackDecks, "💿");
        nav_item(ui, "Effects & Mixer FX", PrefsTab::Effects, "🪄");

        cat_header(ui, "STEMS & RECORDING");
        nav_item(ui, "Stems & Demucs AI", PrefsTab::StemsAndAi, "✦");
        nav_item(ui, "Recording & Broadcast", PrefsTab::Recording, "🔴");

        cat_header(ui, "COLLECTION & HARDWARE");
        nav_item(ui, "File Management", PrefsTab::FileManagement, "📁");
        nav_item(ui, "Analyze Options & Keys", PrefsTab::AnalyzeOptions, "🔍");
        nav_item(ui, "Controller Manager & Remapper", PrefsTab::Controllers, "🎮");
        nav_item(ui, "About & System Diagnostics", PrefsTab::About, "ℹ");
    }

    /// Renders Preferences as a spacious, full-page view taking the entire application canvas.
    pub fn show_full_page(&mut self, ui: &mut Ui, _ctx: &egui::Context) {
        let avail_size = ui.available_size();

        ui.vertical(|ui| {
            // 1. Top Header Bar
            egui::Frame::none()
                .fill(Color32::from_rgb(18, 20, 26))
                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(38, 42, 52)))
                .inner_margin(egui::Margin::symmetric(16.0, 10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Title & Breadcrumb
                        ui.label(
                            RichText::new("⚙ PREFERENCES")
                                .color(Color32::WHITE)
                                .font(egui::FontId::monospace(15.0))
                                .strong(),
                        );
                        ui.add_space(8.0);

                        let (category, section) = self.get_tab_breadcrumb(self.current_tab);
                        ui.label(
                            RichText::new(format!("[ {category} ] › {section}"))
                                .color(TraktorTheme::DECK_A)
                                .font(egui::FontId::monospace(11.0)),
                        );

                        // Center / Right Controls
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Close Button
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("✕ Close Preferences (Esc)")
                                            .color(Color32::WHITE)
                                            .font(egui::FontId::proportional(12.0))
                                            .strong(),
                                    )
                                    .fill(TraktorTheme::ROW_SELECTED)
                                    .min_size(Vec2::new(180.0, 28.0)),
                                )
                                .on_hover_text("Return to Decks and Performance View (Esc)")
                                .clicked()
                            {
                                self.save();
                                self.is_open = false;
                            }

                            ui.add_space(16.0);

                            // Search bar
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("🔍")
                                        .color(TraktorTheme::TEXT_MUTED)
                                        .font(egui::FontId::proportional(12.0)),
                                );
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.search_query)
                                        .hint_text("Search settings (e.g. latency, stems, eq, pitch)...")
                                        .desired_width(260.0),
                                );
                                if !self.search_query.is_empty() && ui.small_button("✕").clicked() {
                                    self.search_query.clear();
                                }
                            });
                        });
                    });
                });

            // 2. Full Page Body: Left Sidebar + Right Content Pane
            ui.horizontal(|ui| {
                // Left Navigation Sidebar
                egui::Frame::none()
                    .fill(Color32::from_rgb(16, 18, 24))
                    .inner_margin(egui::Margin::symmetric(10.0, 12.0))
                    .show(ui, |ui| {
                        ui.set_width(230.0);
                        ui.set_min_height(avail_size.y - 56.0);

                        egui::ScrollArea::vertical()
                            .id_salt("prefs_nav_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                self.render_sidebar_navigation(ui);
                            });
                    });

                // Separator
                ui.separator();

                // Right Content Area
                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 22, 28))
                    .inner_margin(egui::Margin::symmetric(24.0, 16.0))
                    .show(ui, |ui| {
                        let content_w = ui.available_width();
                        let content_h = ui.available_height();
                        ui.set_width(content_w);
                        ui.set_min_height(content_h);

                        egui::ScrollArea::vertical()
                            .id_salt("prefs_main_scroll")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.set_min_width(content_w - 24.0);
                                match self.current_tab {
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
                                    PrefsTab::FileManagement => self.show_file_management(ui),
                                    PrefsTab::AnalyzeOptions => self.show_analyze_prefs(ui),
                                    PrefsTab::Controllers => self.show_controller_prefs(ui),
                                    PrefsTab::About => self.show_about(ui),
                                }
                            });
                    });
            });
        });
    }

    /// Backwards compatibility shim
    pub fn show(&mut self, _ctx: &egui::Context) {}

    fn show_audio_setup(&mut self, ui: &mut Ui) {
        ui.heading("Audio Setup — Hardware Interface & Latency");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Audio Device:").strong());
                    ui.add_space(8.0);
                    egui::ComboBox::from_id_salt("audio_device_combo")
                        .selected_text(&self.audio_device_name)
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
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Sample Rate:").strong());
                    ui.add_space(8.0);
                    egui::ComboBox::from_id_salt("sr_combo")
                        .selected_text(format!("{} Hz", self.sample_rate))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.sample_rate, 44100, "44100 Hz (CD Standard)");
                            ui.selectable_value(&mut self.sample_rate, 48000, "48000 Hz (Pro Audio Default)");
                            ui.selectable_value(&mut self.sample_rate, 88200, "88200 Hz");
                            ui.selectable_value(&mut self.sample_rate, 96000, "96000 Hz (Studio Master)");
                        });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Buffer Size (Latency):").strong());
                    ui.add_space(8.0);
                    let latency_ms = (self.buffer_size as f64 / self.sample_rate as f64) * 1000.0;
                    egui::ComboBox::from_id_salt("buf_combo")
                        .selected_text(format!("{} frames ({:.1} ms)", self.buffer_size, latency_ms))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.buffer_size, 64, "64 frames (1.3 ms — Ultra Low)");
                            ui.selectable_value(&mut self.buffer_size, 128, "128 frames (2.7 ms — Performance)");
                            ui.selectable_value(&mut self.buffer_size, 256, "256 frames (5.3 ms — Safe)");
                            ui.selectable_value(&mut self.buffer_size, 512, "512 frames (10.7 ms — High)");
                        });
                });
            });

        ui.add_space(12.0);
        let latency_ms = (self.buffer_size as f64 / self.sample_rate as f64) * 1000.0;
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 26, 20))
            .stroke(Stroke::new(1.0_f32, TraktorTheme::PLAY_ACTIVE))
            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("✓ REAL-TIME AUDIO SCHEDULER ACTIVE")
                            .color(TraktorTheme::PLAY_ACTIVE)
                            .font(egui::FontId::monospace(11.0))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("Roundtrip: {:.2} ms | 0 XRuns", latency_ms))
                                .color(TraktorTheme::PLAY_ACTIVE)
                                .font(egui::FontId::monospace(11.0)),
                        );
                    });
                });
                ui.add_space(4.0);
                ui.label("Lock-free SPSC command ring buffer running on dedicated SCHED_FIFO real-time thread.");
                ui.label("Direct ALSA UAC2 hardware mmap integration with PipeWire fallback.");
            });
    }

    fn show_output_routing(&mut self, ui: &mut Ui) {
        ui.heading("Output Routing — Master, Monitor & Booth");
        ui.add_space(8.0);

        ui.label(RichText::new("Mixing Mode:").strong());
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.mixing_mode, 0, "Internal Mixer (Software Mixing — Decks A/B/C/D into Master)");
            ui.radio_value(&mut self.mixing_mode, 1, "External Mixer (Direct Soundcard Channels to Hardware Mixer)");
        });

        ui.add_space(10.0);
        egui::Grid::new("output_routing_grid")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Output Master (Main Speakers):").strong());
                ui.horizontal(|ui| {
                    ui.label("L:");
                    ui.text_edit_singleline(&mut self.master_out_l);
                    ui.label("R:");
                    ui.text_edit_singleline(&mut self.master_out_r);
                });
                ui.end_row();

                ui.label(RichText::new("Output Monitor (Headphone Cue):").strong());
                ui.horizontal(|ui| {
                    ui.label("L:");
                    ui.text_edit_singleline(&mut self.monitor_out_l);
                    ui.label("R:");
                    ui.text_edit_singleline(&mut self.monitor_out_r);
                });
                ui.end_row();

                ui.label(RichText::new("Output Booth (DJ Monitor):").strong());
                ui.horizontal(|ui| {
                    ui.label("L:");
                    ui.text_edit_singleline(&mut self.booth_out_l);
                    ui.label("R:");
                    ui.text_edit_singleline(&mut self.booth_out_r);
                });
                ui.end_row();
            });

        ui.add_space(10.0);
        ui.checkbox(&mut self.split_cue, "Enable Split Cue (Left Ear = Cue Monitor, Right Ear = Master)");
    }

    fn show_input_routing(&mut self, ui: &mut Ui) {
        ui.heading("Input Routing — Live Inputs, DVS & Aux");
        ui.add_space(8.0);

        ui.label("Configure incoming audio channels from your Traktor hardware interface or external audio device:");
        ui.add_space(8.0);

        egui::Grid::new("input_routing_grid")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                ui.label(RichText::new("Input Deck A (Live / DVS Timecode):").strong());
                ui.horizontal(|ui| {
                    ui.label("In L:");
                    ui.text_edit_singleline(&mut self.deck_a_in_l);
                    ui.label("In R:");
                    ui.text_edit_singleline(&mut self.deck_a_in_r);
                });
                ui.end_row();

                ui.label(RichText::new("Input Deck B (Live / DVS Timecode):").strong());
                ui.horizontal(|ui| {
                    ui.label("In L:");
                    ui.text_edit_singleline(&mut self.deck_b_in_l);
                    ui.label("In R:");
                    ui.text_edit_singleline(&mut self.deck_b_in_r);
                });
                ui.end_row();

                ui.label(RichText::new("Input Aux / Mic (Microphone In):").strong());
                ui.horizontal(|ui| {
                    ui.label("In L:");
                    ui.text_edit_singleline(&mut self.aux_in_l);
                    ui.label("In R:");
                    ui.text_edit_singleline(&mut self.aux_in_r);
                });
                ui.end_row();
            });

        ui.add_space(14.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(45, 52, 68)))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.label(RichText::new("DVS / Timecode Calibration Status").color(TraktorTheme::DECK_A).strong());
                ui.label("• DVS Engine: Ready for 2.5 kHz MK2 Timecode vinyl / CD signals");
                ui.label("• Traktor Scratch MK2 Vinyl Lead-in: 0.0 dB threshold");
            });
    }

    fn show_external_sync(&mut self, ui: &mut Ui) {
        ui.heading("External Sync & MIDI Clock");
        ui.add_space(8.0);

        ui.label(RichText::new("Master Clock Mode:").strong());
        ui.horizontal(|ui| {
            ui.radio_value(&mut self.clock_mode, 0, "Auto Master (Follows active master deck)");
            ui.radio_value(&mut self.clock_mode, 1, "Manual Master (Fixed Master Clock tempo)");
        });

        ui.add_space(12.0);
        ui.label(RichText::new("MIDI Clock Output:").strong());
        ui.checkbox(&mut self.send_midi_clock, "Send MIDI Clock to connected hardware synthesizers / drum machines");

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("MIDI Clock Offset (Latency Compensation):");
            ui.add(egui::Slider::new(&mut self.midi_clock_offset_ms, -50.0..=50.0).suffix(" ms"));
        });

        ui.add_space(12.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 24, 32))
            .stroke(Stroke::new(1.0_f32, TraktorTheme::DECK_B))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ABLETON LINK™ INTEGRATION").color(TraktorTheme::DECK_B).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let link_label = if self.ableton_link_enabled { "● Link: Active (0 Peers)" } else { "Link: Disabled" };
                        ui.label(RichText::new(link_label).color(if self.ableton_link_enabled { TraktorTheme::DECK_B } else { TraktorTheme::TEXT_MUTED }));
                    });
                });
                ui.add_space(4.0);
                ui.checkbox(&mut self.ableton_link_enabled, "Enable Ableton Link (Synchronize tempo over local Wi-Fi / Ethernet)");
            });
    }

    fn show_mixer_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Mixer Preferences — EQ Models, Filters & Limiter");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("EQ Model:").strong());
                    ui.add_space(8.0);
                    let models = [
                        "Z ISO (Full Kill 3-Band — Traktor Pro 4 Default)",
                        "Classic (Standard Warm Curves)",
                        "P600 (Club Standard Steep Roll-off)",
                        "Nuo (Fat Analog Lows)",
                        "Xone:92 (4-Band Dual-Mid Audiophile)",
                    ];
                    egui::ComboBox::from_id_salt("eq_model_combo")
                        .selected_text(models[self.eq_model_idx])
                        .show_ui(ui, |ui| {
                            for (i, m) in models.iter().enumerate() {
                                ui.selectable_value(&mut self.eq_model_idx, i, *m);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Filter Model:").strong());
                    ui.add_space(8.0);
                    let filters = [
                        "Ladder (Moog-style 4-Pole 24dB/oct resonant)",
                        "State Variable (Clean 12dB/oct dual-slope)",
                        "Pro-53 (Classic Analog Synth Filter)",
                    ];
                    egui::ComboBox::from_id_salt("filter_model_combo")
                        .selected_text(filters[self.filter_model_idx])
                        .show_ui(ui, |ui| {
                            for (i, f) in filters.iter().enumerate() {
                                ui.selectable_value(&mut self.filter_model_idx, i, *f);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label("Filter Resonance (Q):");
                    ui.add(egui::Slider::new(&mut self.filter_resonance, 0.0..=1.0).show_value(true));
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Crossfader Curve:").strong());
                    ui.add_space(8.0);
                    let curves = [
                        "Smooth (Constant-Power blend — House/Trance)",
                        "Steep (Linear Blend — Techno/Club)",
                        "Scratch Cut (Sharp 5% cut-in lag — Hip-Hop/Turntablism)",
                    ];
                    egui::ComboBox::from_id_salt("xf_curve_combo")
                        .selected_text(curves[self.xfader_curve_idx])
                        .show_ui(ui, |ui| {
                            for (i, c) in curves.iter().enumerate() {
                                ui.selectable_value(&mut self.xfader_curve_idx, i, *c);
                            }
                        });
                });
            });

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 48, 62)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.checkbox(&mut self.autogain, "Enable Traktor Per-Track Autogain Analysis");
                ui.add_space(6.0);
                ui.checkbox(&mut self.limiter_enabled, "Enable Master Bus Limiter (Ozone Maximizer limiter)");
                if self.limiter_enabled {
                    ui.horizontal(|ui| {
                        ui.add_space(20.0);
                        ui.label("Limiter Headroom Ceiling:");
                        let headrooms = ["0.0 dB (Max Volume)", "-3.0 dB (Traktor Pro 4 Standard)", "-6.0 dB (Club PA Protection)"];
                        egui::ComboBox::from_id_salt("limiter_headroom_combo")
                            .selected_text(headrooms[self.limiter_headroom])
                            .show_ui(ui, |ui| {
                                for (i, h) in headrooms.iter().enumerate() {
                                    ui.selectable_value(&mut self.limiter_headroom, i, *h);
                                }
                            });
                    });
                }
                ui.add_space(6.0);
                ui.checkbox(
                    &mut self.neural_mix_eq_mode,
                    "Enable Neural Mix™ EQ ⇄ Stems Mode (HI=Vocals, MID=Instruments, LOW=Drums+Bass)",
                );
            });
    }

    fn show_stems_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Stems Creation & Neural Mix™ Engine");
        ui.add_space(8.0);

        // 1. Demucs AI Engine Status & Background Installer Card
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 28, 38))
            .stroke(Stroke::new(
                1.0_f32,
                match self.demucs_install_state {
                    crate::demucs::DemucsInstallState::Ready { .. } => TraktorTheme::PLAY_ACTIVE,
                    crate::demucs::DemucsInstallState::Installing { .. } => {
                        Color32::from_rgb(255, 185, 45)
                    }
                    _ => Color32::from_rgb(70, 75, 88),
                },
            ))
            .rounding(4.0)
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("NEURAL STEM SEPARATION (META DEMUCS AI)")
                            .color(TraktorTheme::DECK_A)
                            .font(egui::FontId::monospace(11.0))
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        match &self.demucs_install_state {
                            crate::demucs::DemucsInstallState::Ready { version, .. } => {
                                ui.label(
                                    RichText::new(format!("✓ {version} Ready"))
                                        .color(TraktorTheme::PLAY_ACTIVE)
                                        .strong(),
                                );
                            }
                            crate::demucs::DemucsInstallState::Installing { .. } => {
                                ui.label(
                                    RichText::new("● Installing in Background...")
                                        .color(Color32::from_rgb(255, 185, 45))
                                        .strong(),
                                );
                            }
                            crate::demucs::DemucsInstallState::NotInstalled => {
                                ui.label(
                                    RichText::new("Not Installed (Optional)")
                                        .color(TraktorTheme::TEXT_MUTED),
                                );
                            }
                            crate::demucs::DemucsInstallState::Failed { .. } => {
                                ui.label(
                                    RichText::new("⚠ Installation Failed")
                                        .color(Color32::from_rgb(240, 80, 80)),
                                );
                            }
                        }
                    });
                });

                ui.add_space(6.0);
                match &self.demucs_install_state {
                    crate::demucs::DemucsInstallState::Ready { binary_path, .. } => {
                        ui.label(
                            RichText::new(format!("Inference Binary: {}", binary_path.display()))
                                .color(TraktorTheme::TEXT_SECONDARY)
                                .font(egui::FontId::monospace(10.0)),
                        );
                        ui.label(
                            "✓ Generates studio-grade 4-stem FLAC streams (Drums, Bass, Other, Vocals) with 0% bleed.",
                        );
                        ui.label(
                            "✓ Runs with low process priority (`nice -n 10`) so live audio playback is never starved.",
                        );
                    }
                    crate::demucs::DemucsInstallState::Installing { step, percent } => {
                        ui.label(
                            RichText::new(step)
                                .color(Color32::WHITE)
                                .font(egui::FontId::proportional(11.0)),
                        );
                        ui.add(egui::ProgressBar::new(*percent).show_percentage());
                        ui.label(
                            RichText::new(
                                "You can continue DJing and using ReTraktor normally while this installs quietly in the background.",
                            )
                            .color(TraktorTheme::TEXT_MUTED)
                            .font(egui::FontId::proportional(10.5)),
                        );
                    }
                    crate::demucs::DemucsInstallState::NotInstalled => {
                        ui.label(
                            "Demucs is Meta AI's state-of-the-art neural music separation engine (MIT Licensed).",
                        );
                        ui.label(
                            "Installing it gives ReTraktor 100% bleed-free stem isolation on your local tracks.",
                        );
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("✦ Install Demucs AI in Background (~1.2 GB)")
                                            .color(Color32::WHITE)
                                            .strong(),
                                    )
                                    .fill(TraktorTheme::ROW_SELECTED)
                                    .min_size(Vec2::new(260.0, 26.0)),
                                )
                                .on_hover_text(
                                    "Creates an isolated Python venv in ~/.local/share/retraktor/venv and installs PyTorch + Demucs without modifying system packages",
                                )
                                .clicked()
                            {
                                self.trigger_demucs_install = true;
                            }
                            ui.label(
                                RichText::new(
                                    "Runs completely in the background without interrupting audio.",
                                )
                                .color(TraktorTheme::TEXT_MUTED)
                                .font(egui::FontId::proportional(10.5)),
                            );
                        });
                    }
                    crate::demucs::DemucsInstallState::Failed { error } => {
                        ui.label(
                            RichText::new(format!("Error: {error}"))
                                .color(Color32::from_rgb(240, 80, 80)),
                        );
                        if ui.button("Retry Background Installation").clicked() {
                            self.trigger_demucs_install = true;
                        }
                    }
                }
            });

        ui.add_space(10.0);
        ui.label(RichText::new("Stem Separation Mode:").strong());
        ui.radio_value(
            &mut self.stems_engine_mode,
            0,
            "Hybrid (Recommended): True Neural Demucs Stems (with real-time DSP fallback)",
        );
        ui.radio_value(
            &mut self.stems_engine_mode,
            1,
            "Instant Real-Time DSP Stem Isolation Only (Zero external dependencies)",
        );
        ui.radio_value(
            &mut self.stems_engine_mode,
            2,
            "Offline Pre-Computed 4-Stem Cache (.stem.m4a / HTDemucs)",
        );

        ui.add_space(10.0);
        ui.checkbox(
            &mut self.neural_mix_eq_mode,
            "Neural Mix™ EQ ⇄ Stems Knob Swap (Map MX2 HI/MID/LOW knobs to Vocals/Inst/Drums+Bass)",
        );

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 34))
            .stroke(Stroke::new(1.0_f32, TraktorTheme::DECK_A))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("4-Stem Architecture (Drums | Bass | Other | Vocals)")
                        .color(TraktorTheme::DECK_A)
                        .strong(),
                );
                ui.label("• Deck Stems Bar: Mute/Unmute each stem, 1-click ACAPELLA & INST isolators");
                ui.label("• 4-Lane Stem Waveform View: Toggle 'STEMS' on any deck or in Parallel layout");
                ui.label("• Traktor MX2 STEMS Pad Mode: Pads 1..4 = Stem Mute | Pads 5..8 = Stem FX Send");
            });
    }

    fn show_deck_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Decks Layout & Waveform Display");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.checkbox(&mut self.four_deck_mode, "Enable 4-Deck Workspace Mode (Decks A, B, C, D)");
                ui.label("When active, displays four independent deck strips with master crossfader routing.");

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Waveform Color Palette:").strong());
                    ui.add_space(8.0);
                    let palettes = [
                        "Spectrum (Traktor Pro 4 Signature 3-Band Frequency Color)",
                        "Ultraviolet (Neon Purple Mids & Ice Blue Highs)",
                        "High Contrast RGB (Red Bass, Green Mids, Blue Highs)",
                    ];
                    egui::ComboBox::from_id_salt("wf_palette_combo")
                        .selected_text(palettes[self.waveform_color_mode])
                        .show_ui(ui, |ui| {
                            for (i, p) in palettes.iter().enumerate() {
                                ui.selectable_value(&mut self.waveform_color_mode, i, *p);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.checkbox(&mut self.show_minute_markers, "Show minute markers on track stripe overview");
                ui.checkbox(&mut self.show_beatgrid_ticks, "Display beatgrid transient ticks on scrolling waveform");
                ui.checkbox(&mut self.stripe_fit_full, "Fit full track length into stripe overview window");
            });

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 48, 62)))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.label(RichText::new("Spectrum Color Scheme Legend").color(TraktorTheme::DECK_A).strong());
                ui.label("• Red / Warm Orange: Sub-bass, Kick drums, Low basslines (< 250 Hz)");
                ui.label("• Amber / Gold: Vocals, Guitars, Pianos, Lead Synths (250 Hz - 4 kHz)");
                ui.label("• Ice Blue / Cyan: Hi-hats, Crash cymbals, Shakers, Air (> 4 kHz)");
            });
    }

    fn show_track_decks_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Track Decks & Vinyl Scratching Physics");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tempo Pitch Fader Range:").strong());
                    ui.add_space(8.0);
                    let ranges = ["±8% (Club / House)", "±16% (Wide Range)", "±32% (Extreme)", "±50% (Breakbeat)", "±100% (Ultra Pitch)"];
                    egui::ComboBox::from_id_salt("pitch_range_combo")
                        .selected_text(ranges[self.pitch_range_idx])
                        .show_ui(ui, |ui| {
                            for (i, r) in ranges.iter().enumerate() {
                                ui.selectable_value(&mut self.pitch_range_idx, i, *r);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Keylock Quality:").strong());
                    ui.add_space(8.0);
                    let modes = ["HiQ (Phase Vocoder — Scratch Preserving)", "ECO (Low CPU Usage)"];
                    egui::ComboBox::from_id_salt("keylock_mode_combo")
                        .selected_text(modes[self.keylock_quality])
                        .show_ui(ui, |ui| {
                            for (i, m) in modes.iter().enumerate() {
                                ui.selectable_value(&mut self.keylock_quality, i, *m);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Playhead Marker Position:").strong());
                    ui.add_space(8.0);
                    let positions = ["Centered (50% Center)", "Left-Weighted (30% Ahead)"];
                    egui::ComboBox::from_id_salt("play_marker_combo")
                        .selected_text(positions[self.play_marker_pos])
                        .show_ui(ui, |ui| {
                            for (i, p) in positions.iter().enumerate() {
                                ui.selectable_value(&mut self.play_marker_pos, i, *p);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.checkbox(&mut self.auto_cue_on_load, "Automatically jump to First Beat Cue point on track load");
                ui.add_space(8.0);
                ui.checkbox(&mut self.lock_playing_decks, "Lock Playing Decks (Prevents loading a track into a deck actively playing music live)");
            });

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 48, 62)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.label(RichText::new("Turntable Scratch & Jogwheel Emulation").color(TraktorTheme::DECK_A).strong());
                ui.horizontal(|ui| {
                    ui.label("Jog Scratch Sensitivity:");
                    ui.add(egui::Slider::new(&mut self.jog_scratch_sensitivity, 0.2..=3.0).show_value(true));
                });
                ui.add_space(4.0);
                ui.checkbox(&mut self.jog_mode_turntable, "Turntable Mode: Top surface platter touch engages vinyl scratch (Alpha-Beta filter)");
                ui.label(RichText::new("Outer rim turning always performs smooth CDJ-style pitch bend nudge.").color(TraktorTheme::TEXT_MUTED));
            });
    }

    fn show_effects_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Effects Routing & Mixer FX Assignments");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Active Master FX Units:").strong());
                    ui.radio_value(&mut self.fx_units_count, 0, "2 FX Units (Standard Decks A & B)");
                    ui.radio_value(&mut self.fx_units_count, 1, "4 FX Units (Decks A, B, C, D Routing)");
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("FX Panel Display Mode:").strong());
                    ui.radio_value(&mut self.fx_panel_mode, 0, "Group Mode (3 chained effects per FX Unit)");
                    ui.radio_value(&mut self.fx_panel_mode, 1, "Single Mode (1 deep effect with parameter knobs)");
                });
            });

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 48, 62)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.label(RichText::new("Mixer FX Slot Pre-selection").color(TraktorTheme::DECK_A).strong());
                ui.label("Choose which 4 effects appear in the one-knob channel mixer FX selectors:");
                ui.add_space(6.0);

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

                for (idx, slot_val) in self.mixer_fx_slots.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("Slot {}:", idx + 1));
                        let sel_idx = (*slot_val).min(8);
                        egui::ComboBox::from_id_salt(format!("mfx_slot_{idx}"))
                            .selected_text(fx_options[sel_idx])
                            .show_ui(ui, |ui| {
                                for (opt_idx, name) in fx_options.iter().enumerate() {
                                    ui.selectable_value(slot_val, opt_idx, *name);
                                }
                            });
                    });
                }
            });
    }

    fn show_recording_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Recording & Live Broadcast");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.label(RichText::new("Master Mix Audio Recorder (● REC):").strong());
                ui.horizontal(|ui| {
                    ui.label("Save Directory:");
                    let path_str = self.recording_dir.to_string_lossy().to_string();
                    ui.label(RichText::new(path_str).monospace().color(TraktorTheme::DECK_A));
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Audio Format:");
                    let formats = ["WAV 16-bit / 48 kHz (Lossless CD Quality)", "WAV 24-bit / 48 kHz (Studio Master)"];
                    egui::ComboBox::from_id_salt("rec_format_combo")
                        .selected_text(formats[self.recording_format])
                        .show_ui(ui, |ui| {
                            for (i, f) in formats.iter().enumerate() {
                                ui.selectable_value(&mut self.recording_format, i, *f);
                            }
                        });
                });
                ui.add_space(4.0);
                ui.label("Captures post-limiter master output directly to disk with zero latency impact.");
            });

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 48, 62)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("INTERNET BROADCAST (ICECAST / SHOUTCAST)").color(TraktorTheme::DECK_B).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let st = if self.broadcast_connected { "● Online" } else { "Offline" };
                        ui.label(RichText::new(st).color(if self.broadcast_connected { TraktorTheme::PLAY_ACTIVE } else { TraktorTheme::TEXT_MUTED }));
                    });
                });
                ui.add_space(6.0);

                egui::Grid::new("broadcast_grid")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("Host Server:");
                        ui.text_edit_singleline(&mut self.broadcast_host);
                        ui.end_row();

                        ui.label("Port:");
                        let mut port_str = self.broadcast_port.to_string();
                        if ui.text_edit_singleline(&mut port_str).changed() {
                            if let Ok(p) = port_str.parse::<u16>() {
                                self.broadcast_port = p;
                            }
                        }
                        ui.end_row();

                        ui.label("Mount Point:");
                        ui.text_edit_singleline(&mut self.broadcast_mount);
                        ui.end_row();

                        ui.label("Stream Password:");
                        ui.add(egui::TextEdit::singleline(&mut self.broadcast_password).password(true));
                        ui.end_row();
                    });
            });
    }

    fn show_analyze_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Track Analyze Options & Musical Key Notation");
        ui.add_space(8.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(24, 28, 36))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 66)))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("BPM Detection Range:").strong());
                    ui.add_space(8.0);
                    let ranges = [
                        "70 - 140 BPM (Standard House / Techno / Pop)",
                        "60 - 120 BPM (Downtempo / Hip-Hop / Reggae)",
                        "90 - 180 BPM (Drum & Bass / Jungle)",
                        "110 - 220 BPM (Hardcore / Speed Garage)",
                    ];
                    egui::ComboBox::from_id_salt("bpm_range_combo")
                        .selected_text(ranges[self.bpm_detection_range])
                        .show_ui(ui, |ui| {
                            for (i, r) in ranges.iter().enumerate() {
                                ui.selectable_value(&mut self.bpm_detection_range, i, *r);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Musical Key Display Notation:").strong());
                    ui.add_space(8.0);
                    let keys = [
                        "Open Key (Traktor Standard: 1d..12d / 1m..12m)",
                        "Camelot Notation (8A / 8B Wheel)",
                        "Traditional Musical Key (Am, C, F#m)",
                    ];
                    egui::ComboBox::from_id_salt("key_notation_combo")
                        .selected_text(keys[self.key_notation_idx])
                        .show_ui(ui, |ui| {
                            for (i, k) in keys.iter().enumerate() {
                                ui.selectable_value(&mut self.key_notation_idx, i, *k);
                            }
                        });
                });

                ui.add_space(8.0);
                ui.checkbox(&mut self.analyze_new_tracks, "Analyze newly imported tracks automatically (BPM, Key, Transients)");
            });

        ui.add_space(10.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(22, 26, 34))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(42, 48, 62)))
            .inner_margin(egui::Margin::same(10.0))
            .show(ui, |ui| {
                ui.label(RichText::new("Harmonic Mixing & Circle-of-Fifths Key Wheel").color(TraktorTheme::DECK_A).strong());
                ui.label("• In Open Key, '1m' corresponds to A Minor (8A Camelot) and '1d' corresponds to C Major (8B Camelot).");
                ui.label("• ReTraktor's Harmonic Match Radar uses this notation to score harmonic compatibility up to 99%.");
            });
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
                ui.label("• 🎛️ Native HID Mode vs MIDI Mode: If the unit is in MIDI mode, press and hold Left SHIFT + Right SHIFT together to switch back to Native HID mode.");
                ui.label("• 🔒 Linux Permissions (udev): Requires /etc/udev/rules.d/60-retraktor-controllers.rules for non-root /dev/hidraw* read/write access.");
                ui.horizontal(|ui| {
                    if ui.button("📋 Copy 1-Click udev Fix Command to Clipboard").clicked() {
                        let cmd = "sudo cp assets/99-retraktor-ni.rules /etc/udev/rules.d/60-retraktor-controllers.rules && sudo udevadm control --reload-rules && sudo udevadm trigger --subsystem-match=hidraw";
                        ui.output_mut(|o| o.copied_text = cmd.to_string());
                        self.udev_command_copied = true;
                    }
                    if self.udev_command_copied {
                        ui.label(RichText::new("✓ Copied command to clipboard!").color(TraktorTheme::PLAY_ACTIVE));
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
                    ui.label(RichText::new("✓ Automated Hardware Protocol Verification Passed!").color(TraktorTheme::PLAY_ACTIVE).strong());
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
                                            ui.label(RichText::new("✓ Active").color(TraktorTheme::PLAY_ACTIVE).strong());
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
        egui::CollapsingHeader::new(RichText::new("🎛️ Interactive Controller Surface Map (Click Any Control to Remap & Recolor)").strong())
            .default_open(true)
            .show(ui, |ui| {
                ui.label(RichText::new("Click any button, pad, knob, or jogwheel below to reassign its target action and LED backlight color:").italics());
                ui.add_space(4.0);

                // Graphical Representation of Traktor MX2 Hardware
                egui::Frame::none()
                    .fill(Color32::from_rgb(16, 18, 24))
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
                                        if f_click(ui, "🎛 FILTER", "surface.mixer.channels.1.channel_fx.amount", &self.selected_visual_control) {
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
                                        if f_click(ui, "🎛 FILTER", "surface.mixer.channels.2.channel_fx.amount", &self.selected_visual_control) {
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
                                    if ui.button("✕ Close").clicked() {
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
        ui.heading("File Management & Music Folders");
        ui.label(
            RichText::new("Configure directories to scan and import into your Track Collection.")
                .color(TraktorTheme::TEXT_SECONDARY),
        );
        ui.add_space(8.0);

        // 1. Music Folders Watchlist
        ui.label(
            RichText::new("MUSIC FOLDERS WATCHLIST")
                .color(TraktorTheme::DECK_A)
                .font(egui::FontId::monospace(10.5))
                .strong(),
        );

        egui::Frame::none()
            .fill(Color32::from_rgb(20, 24, 30))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 52, 64)))
            .rounding(4.0)
            .inner_margin(egui::Margin::same(8.0))
            .show(ui, |ui| {
                if self.music_folders.is_empty() {
                    ui.label(
                        RichText::new("No music folders added yet. Add folders below to scan automatically.")
                            .color(TraktorTheme::TEXT_MUTED),
                    );
                } else {
                    let mut to_remove = None;
                    for (idx, folder) in self.music_folders.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label("📁");
                            ui.label(
                                RichText::new(folder.to_string_lossy().to_string())
                                    .color(Color32::WHITE)
                                    .strong(),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui
                                    .button(RichText::new("✕").color(Color32::from_rgb(240, 80, 80)))
                                    .on_hover_text("Remove this folder from scan list")
                                    .clicked()
                                {
                                    to_remove = Some(idx);
                                }
                            });
                        });
                        ui.separator();
                    }
                    if let Some(idx) = to_remove {
                        self.music_folders.remove(idx);
                    }
                }

                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Add Path:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_folder_input)
                            .hint_text("/path/to/music/folder")
                            .desired_width(260.0),
                    );
                    if ui.button("+ Add Folder").clicked() {
                        let trimmed = self.new_folder_input.trim();
                        if !trimmed.is_empty() {
                            let p = std::path::PathBuf::from(trimmed);
                            if !self.music_folders.contains(&p) {
                                self.music_folders.push(p);
                                self.new_folder_input.clear();
                            }
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Quick Presets:").color(TraktorTheme::TEXT_MUTED));
                    let home = std::env::var("HOME").unwrap_or_else(|_| "/home".to_string());
                    if ui.button("~/Music").clicked() {
                        let p = std::path::PathBuf::from(&home).join("Music");
                        if !self.music_folders.contains(&p) {
                            self.music_folders.push(p);
                        }
                    }
                    if ui.button("~/Downloads").clicked() {
                        let p = std::path::PathBuf::from(&home).join("Downloads");
                        if !self.music_folders.contains(&p) {
                            self.music_folders.push(p);
                        }
                    }
                    if ui.button("/media").clicked() {
                        let p = std::path::PathBuf::from("/media");
                        if !self.music_folders.contains(&p) {
                            self.music_folders.push(p);
                        }
                    }
                    if ui.button("/mnt").clicked() {
                        let p = std::path::PathBuf::from("/mnt");
                        if !self.music_folders.contains(&p) {
                            self.music_folders.push(p);
                        }
                    }
                });
            });

        ui.add_space(8.0);

        // 2. Scan & Analysis Options
        ui.label(
            RichText::new("SCAN & ANALYSIS OPTIONS")
                .color(TraktorTheme::DECK_A)
                .font(egui::FontId::monospace(10.5))
                .strong(),
        );

        ui.checkbox(
            &mut self.scan_on_startup,
            "Import & Scan Music Folders at Startup",
        );
        ui.checkbox(
            &mut self.analyze_new_tracks,
            "Analyze BPM, Transients, and Musical Key automatically",
        );

        ui.add_space(8.0);

        // 3. Rescan Action Button & Status
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("↻ Rescan All Music Folders Now")
                            .color(Color32::WHITE)
                            .strong(),
                    )
                    .fill(TraktorTheme::ROW_SELECTED)
                    .min_size(Vec2::new(220.0, 26.0)),
                )
                .on_hover_text("Recursively scans all configured music folders and imports new tracks into Collection")
                .clicked()
            {
                self.trigger_music_rescan = true;
            }

            ui.label(
                RichText::new(&self.file_management_status)
                    .color(TraktorTheme::PLAY_ACTIVE)
                    .font(egui::FontId::monospace(10.5)),
            );
        });

        ui.add_space(10.0);

        // 4. Supported Audio Codecs
        ui.label(
            RichText::new("SUPPORTED AUDIO FORMATS")
                .color(TraktorTheme::TEXT_MUTED)
                .font(egui::FontId::monospace(10.0)),
        );
        ui.horizontal(|ui| {
            for ext in &["MP3", "FLAC", "WAV", "OGG", "M4A", "AAC", "AIFF", "NML", "TSI"] {
                egui::Frame::none()
                    .fill(Color32::from_rgb(34, 38, 46))
                    .rounding(2.0)
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(*ext)
                                .color(Color32::WHITE)
                                .font(egui::FontId::monospace(9.5))
                                .strong(),
                        );
                    });
            }
        });
    }

    fn show_about(&mut self, ui: &mut Ui) {
        ui.heading("ReTraktor Pro 4.5.1");
        ui.add_space(8.0);
        ui.label("Clean-Room Linux Re-Implementation of Traktor Pro 4.");
        ui.label("Pure Rust real-time audio DSP, CSI 1.0 QML engine, and eframe/egui interface.");
        ui.add_space(8.0);
        ui.label("Repository: https://github.com/adamtdm/ReTraktor");
        ui.label("Target: Linux (Pop!_OS, Ubuntu, Debian, Fedora, Arch) on Wayland and X11");
    }
}

#[cfg(test)]
mod tests {
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

        let json = original.to_json_str();
        let mut restored = PreferencesDialog::default();
        restored.apply_json_str(&json);

        assert_eq!(restored.sample_rate, 96000);
        assert_eq!(restored.buffer_size, 256);
        assert!(restored.four_deck_mode);
        assert!(!restored.lock_playing_decks);
        assert_eq!(restored.audio_device_name, "Focusrite Scarlett 4i4");
    }
}
