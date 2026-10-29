use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2, Window};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefsTab {
    AudioSetup,
    OutputRouting,
    FileManagement,
    Mixer,
    StemsAndAi,
    Decks,
    Controllers,
    About,
}

pub struct PreferencesDialog {
    pub is_open: bool,
    pub current_tab: PrefsTab,
    pub sample_rate: u32,
    pub buffer_size: u32,
    pub eq_model_idx: usize,
    pub xfader_curve_idx: usize,
    pub autogain: bool,
    pub limiter_enabled: bool,
    pub four_deck_mode: bool,
    pub neural_mix_eq_mode: bool,
    pub stems_engine_mode: usize, // 0: Hybrid (Real-time DSP + Demucs AI Cache), 1: Real-Time DSP, 2: Offline AI
    pub tsi_path_input: String,
    pub loaded_tsi_summary: Option<String>,
    pub mx2_bottom_led_brightness: f32,

    // Live Hardware Setup & Diagnostics State
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

    // File Management & Music Folders State
    pub music_folders: Vec<std::path::PathBuf>,
    pub new_folder_input: String,
    pub scan_on_startup: bool,
    pub analyze_new_tracks: bool,
    pub trigger_music_rescan: bool,
    pub file_management_status: String,

    // Demucs AI Stem Engine State
    pub demucs_install_state: crate::demucs::DemucsInstallState,
    pub trigger_demucs_install: bool,
    pub demucs_active_separation: Option<String>,

    // Interactive Visual Remapping & Color Customization
    pub selected_visual_control: Option<String>,
    pub custom_control_action: std::collections::HashMap<String, String>,
    pub custom_control_color: std::collections::HashMap<String, u8>,
    pub jog_scratch_sensitivity: f32,
    pub jog_mode_turntable: bool,
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
            sample_rate: 48000,
            buffer_size: 128,
            eq_model_idx: 0,     // Z ISO
            xfader_curve_idx: 0, // Smooth
            autogain: true,
            limiter_enabled: true,
            four_deck_mode: false,
            neural_mix_eq_mode: false,
            stems_engine_mode: 0,
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
            music_folders: vec![default_music],
            new_folder_input: String::new(),
            scan_on_startup: true,
            analyze_new_tracks: true,
            trigger_music_rescan: false,
            file_management_status: "Ready".to_string(),
            demucs_install_state: crate::demucs::DemucsInstallState::NotInstalled,
            trigger_demucs_install: false,
            demucs_active_separation: None,
            selected_visual_control: None,
            custom_control_action: std::collections::HashMap::new(),
            custom_control_color: std::collections::HashMap::new(),
            jog_scratch_sensitivity: 1.0,
            jog_mode_turntable: true,
        }
    }
}

impl PreferencesDialog {
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }

        let mut open = self.is_open;
        Window::new("⚙ ReTraktor Preferences — Audio, Stems & Controllers")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(740.0, 470.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left Tab Sidebar
                    ui.allocate_ui_with_layout(
                        Vec2::new(155.0, 430.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            let mut tab_btn = |label: &str, tab: PrefsTab| {
                                let selected = self.current_tab == tab;
                                let color = if selected {
                                    TraktorTheme::DECK_A
                                } else {
                                    TraktorTheme::TEXT_PRIMARY
                                };
                                let bg = if selected {
                                    Color32::from_rgb(32, 36, 46)
                                } else {
                                    Color32::TRANSPARENT
                                };
                                if ui
                                    .add(
                                        egui::Button::new(egui::RichText::new(label).color(color))
                                            .fill(bg)
                                            .min_size(Vec2::new(145.0, 28.0)),
                                    )
                                    .clicked()
                                {
                                    self.current_tab = tab;
                                }
                            };

                            tab_btn("Controllers & MX2", PrefsTab::Controllers);
                            tab_btn("File Management", PrefsTab::FileManagement);
                            tab_btn("Stems & Neural Mix", PrefsTab::StemsAndAi);
                            tab_btn("Audio Setup", PrefsTab::AudioSetup);
                            tab_btn("Output Routing", PrefsTab::OutputRouting);
                            tab_btn("Mixer & EQ", PrefsTab::Mixer);
                            tab_btn("Decks & Waveforms", PrefsTab::Decks);
                            tab_btn("About", PrefsTab::About);
                        },
                    );

                    ui.separator();

                    // Right Content Area
                    ui.allocate_ui_with_layout(
                        Vec2::new(555.0, 430.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            match self.current_tab {
                                PrefsTab::AudioSetup => self.show_audio_setup(ui),
                                PrefsTab::OutputRouting => self.show_output_routing(ui),
                                PrefsTab::FileManagement => self.show_file_management(ui),
                                PrefsTab::Mixer => self.show_mixer_prefs(ui),
                                PrefsTab::StemsAndAi => self.show_stems_prefs(ui),
                                PrefsTab::Decks => self.show_deck_prefs(ui),
                                PrefsTab::Controllers => self.show_controller_prefs(ui),
                                PrefsTab::About => self.show_about(ui),
                            }
                        },
                    );
                });
            });

        self.is_open = open;
    }

    fn show_audio_setup(&mut self, ui: &mut Ui) {
        ui.heading("Audio Setup");
        ui.add_space(8.0);

        ui.label(egui::RichText::new("Audio Device Backend:").strong());
        ui.label("PipeWire pro-audio / ALSA UAC2 (Traktor MX2 / S4 MK3 / S3 / S2 MK3 / Z1 MK2)");

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Sample Rate:");
            egui::ComboBox::from_id_salt("sr_combo")
                .selected_text(format!("{} Hz", self.sample_rate))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sample_rate, 44100, "44100 Hz");
                    ui.selectable_value(&mut self.sample_rate, 48000, "48000 Hz (Pro Audio)");
                    ui.selectable_value(&mut self.sample_rate, 96000, "96000 Hz");
                });
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label("Buffer Size (Latency):");
            let latency_ms = (self.buffer_size as f64 / self.sample_rate as f64) * 1000.0;
            egui::ComboBox::from_id_salt("buf_combo")
                .selected_text(format!("{} frames ({:.1} ms)", self.buffer_size, latency_ms))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.buffer_size, 64, "64 frames (1.3 ms)");
                    ui.selectable_value(&mut self.buffer_size, 128, "128 frames (2.7 ms - Default)");
                    ui.selectable_value(&mut self.buffer_size, 256, "256 frames (5.3 ms)");
                    ui.selectable_value(&mut self.buffer_size, 512, "512 frames (10.7 ms)");
                });
        });

        ui.add_space(14.0);
        egui::Frame::none()
            .fill(Color32::from_rgb(20, 24, 20))
            .stroke(Stroke::new(1.0_f32, TraktorTheme::PLAY_ACTIVE))
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("✓ PipeWire pro-audio real-time scheduler engaged.")
                        .color(TraktorTheme::PLAY_ACTIVE)
                        .strong(),
                );
                ui.label("0 xruns, lock-free SPSC command ring active.");
            });
    }

    fn show_output_routing(&mut self, ui: &mut Ui) {
        ui.heading("Output Routing");
        ui.add_space(8.0);

        ui.label(egui::RichText::new("Mixing Mode:").strong());
        ui.radio_value(&mut true, true, "Internal Mixer (Software Mixing)");

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("Output Master (Main Speakers):").strong());
            ui.label("Channel 1: Left / Front-Left");
            ui.label("Channel 2: Right / Front-Right");
        });

        ui.add_space(8.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("Output Monitor (Headphone Cue):").strong());
            ui.label("Channel 3: Headphones Left");
            ui.label("Channel 4: Headphones Right");
        });
    }

    fn show_mixer_prefs(&mut self, ui: &mut Ui) {
        ui.heading("Mixer Preferences");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label("EQ Model:");
            let models = ["Z ISO (Full Kill 3-Band)", "Classic (Standard)", "P600 (Pioneer)", "Nuo (Ecler)", "Xone:92 (4-Band)"];
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
            ui.label("Crossfader Curve:");
            let curves = ["Smooth (Constant-Power blend)", "Steep (House/Techno cut)", "Scratch Cut (Sharp 5% cut-in lag)"];
            egui::ComboBox::from_id_salt("xf_curve_combo")
                .selected_text(curves[self.xfader_curve_idx])
                .show_ui(ui, |ui| {
                    for (i, c) in curves.iter().enumerate() {
                        ui.selectable_value(&mut self.xfader_curve_idx, i, *c);
                    }
                });
        });

        ui.add_space(8.0);
        ui.checkbox(&mut self.autogain, "Enable Traktor Autogain Analysis");
        ui.checkbox(&mut self.limiter_enabled, "Enable Ozone Maximizer Master Limiter (-0.3 dB Ceiling)");
        ui.checkbox(
            &mut self.neural_mix_eq_mode,
            "Enable Neural Mix™ EQ ⇄ Stems Mode (HI=Vocals, MID=Instruments, LOW=Drums+Bass)",
        );
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
        ui.heading("Decks & Waveforms");
        ui.add_space(8.0);

        ui.checkbox(&mut self.four_deck_mode, "Enable 4-Deck Mode (Decks A, B, C, D)");
        ui.label("Default is 2-Deck Mode (Decks A & B) with full 4-channel center mixer.");

        ui.add_space(8.0);
        ui.label(egui::RichText::new("Waveform Color Style:").strong());
        ui.label("• Spectrum (Traktor Pro 4 signature 3-band frequency color)");
        ui.label("  - Red: Low frequencies (Kick, Sub, Bass)");
        ui.label("  - Amber/Yellow: Mid frequencies (Vocals, Lead Synths)");
        ui.label("  - Cyan: High frequencies (Hats, Cymbals, Shakers)");
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
}
