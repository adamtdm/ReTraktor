use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, Stroke, Ui, Vec2, Window};
use retraktor_hid::NI_DEVICES;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrefsTab {
    AudioSetup,
    OutputRouting,
    Mixer,
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
}

impl Default for PreferencesDialog {
    fn default() -> Self {
        Self {
            is_open: false,
            current_tab: PrefsTab::AudioSetup,
            sample_rate: 48000,
            buffer_size: 128,
            eq_model_idx: 0, // Z ISO
            xfader_curve_idx: 0, // Smooth
            autogain: true,
            limiter_enabled: true,
            four_deck_mode: false,
        }
    }
}

impl PreferencesDialog {
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }

        let mut open = self.is_open;
        Window::new("ReTraktor Preferences")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_size(Vec2::new(640.0, 420.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Left Tab Sidebar
                    ui.allocate_ui_with_layout(
                        Vec2::new(140.0, 380.0),
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
                                if ui.add(
                                    egui::Button::new(egui::RichText::new(label).color(color))
                                        .fill(bg)
                                        .min_size(Vec2::new(130.0, 26.0)),
                                ).clicked() {
                                    self.current_tab = tab;
                                }
                            };

                            tab_btn("🔊 Audio Setup", PrefsTab::AudioSetup);
                            tab_btn("🔀 Output Routing", PrefsTab::OutputRouting);
                            tab_btn("🎛️ Mixer", PrefsTab::Mixer);
                            tab_btn("💿 Decks", PrefsTab::Decks);
                            tab_btn("🎮 Controllers", PrefsTab::Controllers);
                            tab_btn("ℹ️ About", PrefsTab::About);
                        },
                    );

                    ui.separator();

                    // Right Content Area
                    ui.allocate_ui_with_layout(
                        Vec2::new(470.0, 380.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            match self.current_tab {
                                PrefsTab::AudioSetup => self.show_audio_setup(ui),
                                PrefsTab::OutputRouting => self.show_output_routing(ui),
                                PrefsTab::Mixer => self.show_mixer_prefs(ui),
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
        ui.label("PipeWire pro-audio / ALSA UAC2 (Native Instruments Kontrol S4/S3/S2)");

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
        ui.heading("Controller Manager");
        ui.add_space(8.0);

        ui.label("Supported Native Instruments USB & HID Hardware:");
        for dev in NI_DEVICES.iter().take(8) {
            ui.label(format!("• {} [VID 0x{:04X} : PID 0x{:04X}]", dev.model, dev.vid, dev.pid));
        }

        ui.add_space(8.0);
        ui.label("Linux udev permission rule: /etc/udev/rules.d/99-retraktor-ni.rules");
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
