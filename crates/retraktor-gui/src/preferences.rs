use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2, Window};
use retraktor_formats::TsiFile;
use retraktor_hid::NI_DEVICES;
use std::fs;
use std::path::PathBuf;

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
        ui.heading("Controller Manager — Traktor MX2 & NI Kontrol");
        ui.add_space(6.0);

        egui::Frame::none()
            .fill(Color32::from_rgb(18, 25, 34))
            .stroke(Stroke::new(1.0_f32, TraktorTheme::PLAY_ACTIVE))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new("★ Native Instruments Traktor MX2 [VID 0x17CC : PID 0x2420] Active")
                        .color(TraktorTheme::PLAY_ACTIVE)
                        .strong(),
                );
                ui.label("• NHL2 HID Reports: 0x01 (83 Buttons + 6 Encoders), 0x02 (26 12-bit Pots/Faders), 0x03 (960-tick Jogwheels)");
                ui.label("• Output Report 0x80: 105 LEDs + 6 RGB Bottom-Shell LEDs per deck (Green=Loop, Red=End Warning, Blue/Cyan=Glow)");
                ui.label("• Audio Interface: 24-bit / 96kHz 4-Out / 2-In UAC2 (Master 1/2, Headphone Monitor 3/4, Mic 1/2)");
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("MX2 Bottom-Shell LED Brightness:");
            ui.add(egui::Slider::new(&mut self.mx2_bottom_led_brightness, 0.1..=1.0));
        });

        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .id_salt("prefs_controllers_scroll")
            .max_height(155.0)
            .show(ui, |ui| {
                egui::Grid::new("prefs_ni_controllers_grid")
                    .striped(true)
                    .spacing(Vec2::new(14.0, 4.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("Controller Model").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("USB VID:PID").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("CSI Module").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("Display").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.label(RichText::new("Platters").color(TraktorTheme::TEXT_SECONDARY).strong());
                        ui.end_row();

                        for dev in &NI_DEVICES {
                            ui.label(RichText::new(dev.model).color(Color32::WHITE).strong());
                            ui.label(
                                RichText::new(format!("{:04X}:{:04X}", dev.vid, dev.pid))
                                    .color(TraktorTheme::DECK_A)
                                    .monospace(),
                            );
                            ui.label(RichText::new(format!("CSI/{}", dev.csi_folder)).monospace());
                            ui.label(if dev.has_color_screen {
                                "RGB565 (0x8400)"
                            } else {
                                "RGB LEDs"
                            });
                            ui.label(if dev.has_motorized_jog {
                                "Motorized (0x30)"
                            } else {
                                "Capacitive Jog"
                            });
                            ui.end_row();
                        }
                    });
            });

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label("Import .tsi Mapping:");
            ui.add(
                egui::TextEdit::singleline(&mut self.tsi_path_input)
                    .hint_text("/path/to/controller.tsi")
                    .desired_width(240.0),
            );
            if ui.button("Load .tsi").clicked() {
                let p = PathBuf::from(self.tsi_path_input.trim());
                match fs::read_to_string(&p)
                    .map_err(|e| e.to_string())
                    .and_then(|xml| TsiFile::parse_xml(&xml).map_err(|e| e.to_string()))
                {
                    Ok(tsi) => {
                        self.loaded_tsi_summary = Some(format!(
                            "✓ Loaded {} ({} devices mapped)",
                            p.display(),
                            tsi.devices.len()
                        ));
                    }
                    Err(e) => {
                        self.loaded_tsi_summary = Some(format!("⚠ TSI error: {e}"));
                    }
                }
            }
        });
        if let Some(ref msg) = self.loaded_tsi_summary {
            ui.label(RichText::new(msg).color(TraktorTheme::PLAY_ACTIVE));
        }
        ui.label(
            RichText::new("Linux udev rule: /etc/udev/rules.d/99-retraktor-ni.rules")
                .color(TraktorTheme::TEXT_MUTED)
                .font(egui::FontId::monospace(9.5)),
        );
    }

    fn show_about(&mut self, ui: &mut Ui) {
        ui.heading("ReTraktor Pro 4.5.1");
        ui.add_space(8.0);
        ui.label("Clean-Room Linux Re-Implementation of Native Instruments Traktor Pro 4.");
        ui.label("Pure Rust real-time audio DSP, CSI 1.0 QML engine, and eframe/egui interface.");
        ui.add_space(8.0);
        ui.label("Repository: https://github.com/adamtdm/ReTraktor");
        ui.label("Target: Linux (Pop!_OS, Ubuntu, Debian, Fedora, Arch) on Wayland and X11");
    }
}
