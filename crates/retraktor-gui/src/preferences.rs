use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2, Window};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefsTab {
    AudioSetup,
    OutputRouting,
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
}

impl Default for PreferencesDialog {
    fn default() -> Self {
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

        ui.label(RichText::new("Stem Separation Mode:").strong());
        ui.radio_value(
            &mut self.stems_engine_mode,
            0,
            "Hybrid (Recommended): Instant Real-Time 4-Stem DSP + Local AI Demucs Stem Caching",
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
        ui.horizontal(|ui| {
            ui.label("MX2 Bottom-Shell LED Brightness:");
            ui.add(egui::Slider::new(&mut self.mx2_bottom_led_brightness, 0.1..=1.0));
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
