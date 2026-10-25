use crate::audio::{AudioSystem, LoadedTrackAudio};
use crate::library_ui::{LibraryManager, UiTrackEntry};
use crate::preferences::PreferencesDialog;
use crate::theme::TraktorTheme;
use crate::widgets::{
    crossfader, cue_pad, rotary_knob, scrolling_waveform, stripe_overview, vertical_fader, vu_meter,
};
use eframe::egui::{
    self, Align, Color32, Key, Layout, RichText, Stroke, Ui, Vec2,
};
use retraktor_core::PropertyBus;
use retraktor_dsp::MixerFxKind;
use retraktor_library::KeyWheel;

pub struct ReTraktorApp {
    pub audio: AudioSystem,
    pub library: LibraryManager,
    pub preferences: PreferencesDialog,
    pub bus: PropertyBus,

    // Mixer UI State
    pub channel_gain: [f32; 4],
    pub channel_eq_hi: [f32; 4],
    pub channel_eq_mid: [f32; 4],
    pub channel_eq_low: [f32; 4],
    pub channel_filter: [f32; 4],
    pub channel_filter_on: [bool; 4],
    pub channel_fx_kind: [MixerFxKind; 4],
    pub channel_volume: [f32; 4],
    pub channel_cue: [bool; 4],
    pub channel_xfader_assign: [usize; 4], // 0: Left, 1: Thru, 2: Right

    pub master_volume: f32,
    pub master_tempo: f64,
    pub crossfader_pos: f32,

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
    pub deck_loop_size_beats: [u32; 4],
    pub deck_hotcues: [Vec<(u8, f64)>; 4],
}

impl Default for ReTraktorApp {
    fn default() -> Self {
        let bus = PropertyBus::new_traktor_pro4_default();
        let app = Self {
            audio: AudioSystem::new(),
            library: LibraryManager::default(),
            preferences: PreferencesDialog::default(),
            bus,
            channel_gain: [0.5, 0.5, 0.5, 0.5],
            channel_eq_hi: [0.5, 0.5, 0.5, 0.5],
            channel_eq_mid: [0.5, 0.5, 0.5, 0.5],
            channel_eq_low: [0.5, 0.5, 0.5, 0.5],
            channel_filter: [0.5, 0.5, 0.5, 0.5],
            channel_filter_on: [false, false, false, false],
            channel_fx_kind: [MixerFxKind::Filter, MixerFxKind::Filter, MixerFxKind::Filter, MixerFxKind::Filter],
            channel_volume: [0.85, 0.85, 0.85, 0.85],
            channel_cue: [false, false, false, false],
            channel_xfader_assign: [0, 2, 1, 1], // Deck A: Left, Deck B: Right, C/D: Thru
            master_volume: 0.8,
            master_tempo: 128.00,
            crossfader_pos: 0.5,
            fx1_dry_wet: 0.3,
            fx1_param: [0.5, 0.5, 0.5],
            fx1_on: false,
            fx2_dry_wet: 0.3,
            fx2_param: [0.5, 0.5, 0.5],
            fx2_on: false,
            deck_pitch_fader: [0.0, 0.0, 0.0, 0.0],
            deck_flux_active: [false, false, false, false],
            deck_loop_active: [false, false, false, false],
            deck_loop_size_beats: [4, 4, 4, 4],
            deck_hotcues: [
                vec![(1, 0.0), (2, 15.0), (3, 30.0), (4, 60.0)],
                vec![(1, 0.0), (2, 16.0), (3, 32.0), (4, 64.0)],
                vec![(1, 0.0)],
                vec![(1, 0.0)],
            ],
        };
        app
    }
}

impl eframe::App for ReTraktorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request 60 FPS continuous repaint for smooth scrolling waveforms and VU meters
        ctx.request_repaint();

        self.handle_keyboard_shortcuts(ctx);
        self.sync_audio_state();

        // Authentic Traktor Dark Theme Canvas
        let mut visual = egui::Visuals::dark();
        visual.panel_fill = TraktorTheme::BG_WINDOW;
        visual.window_fill = TraktorTheme::BG_PANEL;
        visual.override_text_color = Some(TraktorTheme::TEXT_PRIMARY);
        ctx.set_visuals(visual);

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(TraktorTheme::BG_WINDOW))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);

                // 1. Top FX Rack, Master Clock & Telemetry Bar
                self.show_top_bar(ui);

                // 2. Decks & 4-Channel Mixer Area
                self.show_decks_and_mixer(ui);

                // 3. Bottom Track Collection Browser
                self.show_browser_panel(ui);
            });

        // Preferences Dialog
        self.preferences.show(ctx);
    }
}

impl ReTraktorApp {
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

            for d in 0..4 {
                state.engine.strips[d].volume = self.channel_volume[d];
                state.engine.strips[d].cue_enabled = self.channel_cue[d];
                state.engine.strips[d].filter.knob = self.channel_filter[d];
                state.engine.strips[d].filter.enabled = self.channel_filter_on[d];
                state.engine.strips[d].filter.kind = self.channel_fx_kind[d];
                state.engine.strips[d].eq.high = self.channel_eq_hi[d];
                state.engine.strips[d].eq.mid = self.channel_eq_mid[d];
                state.engine.strips[d].eq.low = self.channel_eq_low[d];

                state.engine.strips[d].xfader_left = self.channel_xfader_assign[d] == 0;
                state.engine.strips[d].xfader_right = self.channel_xfader_assign[d] == 2;

                // Calculate pitch ratio from pitch fader
                let pitch_pct = self.deck_pitch_fader[d] as f64 / 100.0;
                state.pitch_ratio[d] = 1.0 + pitch_pct;
            }

            state.engine.master_volume = self.master_volume;
            state.engine.clock.bpm = self.master_tempo;
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Comma)) {
            self.preferences.is_open = !self.preferences.is_open;
        }

        // Spacebar toggles Play on Deck A
        if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::Space)) {
            self.toggle_play(1);
        }
        // Shift+Spacebar toggles Play on Deck B
        if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::Space)) {
            self.toggle_play(2);
        }

        // C key for Cue Deck A
        if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::C)) {
            self.jump_to_cue(1, 1);
        }
        // Shift+C key for Cue Deck B
        if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::C)) {
            self.jump_to_cue(2, 1);
        }

        // S key for Sync Deck A / Deck B
        if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::S)) {
            self.sync_deck(1);
        }
        if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::S)) {
            self.sync_deck(2);
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
        } else {
            // Set hotcue at current playhead
            if let Ok(state) = self.audio.shared.lock() {
                let current = state.playhead_sec[d];
                self.deck_hotcues[d].push((cue_num, current));
            }
        }
    }

    fn sync_deck(&mut self, deck_idx: usize) {
        let d = deck_idx - 1;
        if let Ok(state) = self.audio.shared.lock() {
            if let Some(track) = &state.loaded_tracks[d] {
                let target_bpm = self.master_tempo;
                let pct = ((target_bpm - track.bpm) / track.bpm) * 100.0;
                self.deck_pitch_fader[d] = pct.clamp(-8.0, 8.0) as f32;
            }
        }
    }

    fn show_top_bar(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Logo and Software Label
                    ui.label(
                        RichText::new("TRAKTOR")
                            .color(Color32::WHITE)
                            .font(egui::FontId::proportional(16.0))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("PRO 4")
                            .color(TraktorTheme::DECK_A)
                            .font(egui::FontId::proportional(16.0))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("4.5.1 Linux")
                            .color(TraktorTheme::TEXT_MUTED)
                            .font(egui::FontId::monospace(10.0)),
                    );

                    ui.separator();

                    // Master Clock Module
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("MASTER")
                                    .color(TraktorTheme::TEXT_SECONDARY)
                                    .font(egui::FontId::proportional(10.0)),
                            );
                            ui.label(
                                RichText::new(format!("{:.2}", self.master_tempo))
                                    .color(Color32::WHITE)
                                    .font(egui::FontId::monospace(14.0))
                                    .strong(),
                            );
                            ui.label(
                                RichText::new("BPM")
                                    .color(TraktorTheme::TEXT_MUTED)
                                    .font(egui::FontId::proportional(10.0)),
                            );

                            if ui.add(egui::Button::new("TAP").min_size(Vec2::new(32.0, 18.0))).clicked() {
                                // Tap tempo
                            }
                            if ui.add(egui::Button::new("-").min_size(Vec2::new(20.0, 18.0))).clicked() {
                                self.master_tempo = (self.master_tempo - 0.1).max(30.0);
                            }
                            if ui.add(egui::Button::new("+").min_size(Vec2::new(20.0, 18.0))).clicked() {
                                self.master_tempo = (self.master_tempo + 0.1).min(300.0);
                            }
                        });
                    });

                    ui.separator();

                    // FX Unit 1
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let fx1_btn = ui.add(
                                egui::Button::new(
                                    RichText::new("FX 1")
                                        .color(if self.fx1_on { Color32::BLACK } else { TraktorTheme::DECK_A })
                                        .font(egui::FontId::monospace(10.0))
                                        .strong(),
                                )
                                .fill(if self.fx1_on { TraktorTheme::DECK_A } else { Color32::TRANSPARENT })
                                .min_size(Vec2::new(36.0, 18.0)),
                            );
                            if fx1_btn.clicked() {
                                self.fx1_on = !self.fx1_on;
                            }

                            rotary_knob(ui, &mut self.fx1_dry_wet, 0.0, 1.0, 0.3, "D/W", TraktorTheme::DECK_A);
                            rotary_knob(ui, &mut self.fx1_param[0], 0.0, 1.0, 0.5, "REV", TraktorTheme::DECK_A);
                            rotary_knob(ui, &mut self.fx1_param[1], 0.0, 1.0, 0.5, "DLY", TraktorTheme::DECK_A);
                        });
                    });

                    // FX Unit 2
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            let fx2_btn = ui.add(
                                egui::Button::new(
                                    RichText::new("FX 2")
                                        .color(if self.fx2_on { Color32::BLACK } else { TraktorTheme::DECK_B })
                                        .font(egui::FontId::monospace(10.0))
                                        .strong(),
                                )
                                .fill(if self.fx2_on { TraktorTheme::DECK_B } else { Color32::TRANSPARENT })
                                .min_size(Vec2::new(36.0, 18.0)),
                            );
                            if fx2_btn.clicked() {
                                self.fx2_on = !self.fx2_on;
                            }

                            rotary_knob(ui, &mut self.fx2_dry_wet, 0.0, 1.0, 0.3, "D/W", TraktorTheme::DECK_B);
                            rotary_knob(ui, &mut self.fx2_param[0], 0.0, 1.0, 0.5, "FLG", TraktorTheme::DECK_B);
                            rotary_knob(ui, &mut self.fx2_param[1], 0.0, 1.0, 0.5, "CRS", TraktorTheme::DECK_B);
                        });
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Preferences Gear Button
                        if ui.add(
                            egui::Button::new(RichText::new("⚙").font(egui::FontId::proportional(16.0)))
                                .min_size(Vec2::new(28.0, 24.0)),
                        ).clicked() {
                            self.preferences.is_open = !self.preferences.is_open;
                        }

                        // Master Volume Knob
                        rotary_knob(ui, &mut self.master_volume, 0.0, 1.0, 0.8, "MAIN", Color32::WHITE);

                        // Master VU Meter & Limiter Status
                        let (peak_l, peak_r) = if let Ok(state) = self.audio.shared.lock() {
                            (state.master_peak_l, state.master_peak_r)
                        } else {
                            (0.0, 0.0)
                        };
                        vu_meter(ui, peak_l, peak_r, 36.0);

                        // PipeWire Status Indicator
                        ui.label(
                            RichText::new("● PipeWire 48kHz (128 frames)")
                                .color(TraktorTheme::PLAY_ACTIVE)
                                .font(egui::FontId::monospace(10.0)),
                        );
                    });
                });
            });
    }

    fn show_decks_and_mixer(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let avail_w = ui.available_width();
            let mixer_w = 340.0_f32;
            let deck_w = ((avail_w - mixer_w - 12.0) / 2.0).max(280.0);

            // Left: Deck A (Cyan)
            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 290.0),
                Layout::top_down(Align::Min),
                |ui| {
                    self.show_single_deck(ui, 1, TraktorTheme::DECK_A);
                },
            );

            // Center: 4-Channel Mixer
            ui.allocate_ui_with_layout(
                Vec2::new(mixer_w, 290.0),
                Layout::top_down(Align::Min),
                |ui| {
                    self.show_mixer_section(ui);
                },
            );

            // Right: Deck B (Orange)
            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 290.0),
                Layout::top_down(Align::Min),
                |ui| {
                    self.show_single_deck(ui, 2, TraktorTheme::DECK_B);
                },
            );
        });
    }

    fn show_single_deck(&mut self, ui: &mut Ui, deck_idx: usize, accent_color: Color32) {
        let d = deck_idx - 1;
        let deck_letter = match deck_idx {
            1 => "A",
            2 => "B",
            3 => "C",
            4 => "D",
            _ => "A",
        };

        let (title, artist, bpm, key, duration_sec, elapsed_sec, is_playing) = {
            if let Ok(state) = self.audio.shared.lock() {
                let track = state.loaded_tracks[d].clone();
                let is_p = state.is_playing[d];
                let el = state.playhead_sec[d];
                if let Some(t) = track {
                    (t.title, t.artist, t.bpm, t.key, t.duration_sec, el, is_p)
                } else {
                    (format!("Deck {deck_letter} Empty"), "No Track Loaded".to_string(), 120.0, "1d".to_string(), 300.0, 0.0, false)
                }
            } else {
                (format!("Deck {deck_letter}"), "".to_string(), 120.0, "1d".to_string(), 300.0, 0.0, false)
            }
        };

        // Check harmonic match against Deck A if we are Deck B
        let is_harmonic = if deck_idx == 2 {
            if let Ok(state) = self.audio.shared.lock() {
                if let Some(track_a) = &state.loaded_tracks[0] {
                    KeyWheel::are_harmonically_compatible(&track_a.key, &key)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            false
        };

        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, accent_color))
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                // Deck Header Strip
                ui.horizontal(|ui| {
                    // Deck Letter Badge
                    egui::Frame::none()
                        .fill(accent_color)
                        .rounding(2.0)
                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(deck_letter)
                                    .color(Color32::BLACK)
                                    .font(egui::FontId::monospace(14.0))
                                    .strong(),
                            );
                        });

                    // Track Title & Artist
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(&title)
                                .color(TraktorTheme::TEXT_PRIMARY)
                                .font(egui::FontId::proportional(13.0))
                                .strong(),
                        );
                        ui.label(
                            RichText::new(&artist)
                                .color(TraktorTheme::TEXT_SECONDARY)
                                .font(egui::FontId::proportional(11.0)),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Time Remaining & Elapsed
                        let rem_sec = (duration_sec - elapsed_sec).max(0.0);
                        let rem_m = (rem_sec / 60.0).floor() as u32;
                        let rem_s = (rem_sec % 60.0).floor() as u32;
                        ui.label(
                            RichText::new(format!("-{rem_m:02}:{rem_s:02}"))
                                .color(accent_color)
                                .font(egui::FontId::monospace(14.0))
                                .strong(),
                        );

                        // Musical Key Badge with Harmonic Glow
                        let key_bg = if is_harmonic {
                            TraktorTheme::PLAY_ACTIVE
                        } else {
                            Color32::from_rgb(38, 42, 50)
                        };
                        let key_fg = if is_harmonic { Color32::BLACK } else { Color32::WHITE };
                        egui::Frame::none()
                            .fill(key_bg)
                            .rounding(2.0)
                            .inner_margin(egui::Margin::symmetric(5.0, 2.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("KEY {key}"))
                                        .color(key_fg)
                                        .font(egui::FontId::monospace(10.0))
                                        .strong(),
                                );
                            });

                        // Current Tempo & Pitch Fader %
                        let pitch_pct = self.deck_pitch_fader[d];
                        let effective_bpm = bpm * (1.0 + pitch_pct as f64 / 100.0);
                        ui.label(
                            RichText::new(format!("{effective_bpm:.2} BPM ({pitch_pct:+.1}%)"))
                                .color(TraktorTheme::TEXT_MUTED)
                                .font(egui::FontId::monospace(11.0)),
                        );
                    });
                });

                ui.add_space(4.0);

                // Scrolling Frequency Waveform
                let cues = self.deck_hotcues[d].clone();
                let wave_resp = scrolling_waveform(
                    ui,
                    elapsed_sec,
                    bpm,
                    is_playing,
                    &cues,
                    accent_color,
                    80.0,
                );
                if wave_resp.dragged() {
                    let delta_x = -wave_resp.drag_delta().x;
                    let dt = delta_x as f64 * (1.0 / 80.0);
                    if let Ok(mut state) = self.audio.shared.lock() {
                        state.playhead_sec[d] = (state.playhead_sec[d] + dt).clamp(0.0, duration_sec);
                    }
                }

                ui.add_space(3.0);

                // Stripe Overview Minimap with Scrubber
                let mut scrub_pos = elapsed_sec;
                let loop_active = self.deck_loop_active[d];
                let stripe_resp = stripe_overview(
                    ui,
                    &mut scrub_pos,
                    duration_sec,
                    loop_active,
                    elapsed_sec,
                    self.deck_loop_size_beats[d] as f64 * (60.0 / bpm),
                    accent_color,
                );
                if stripe_resp.changed() {
                    if let Ok(mut state) = self.audio.shared.lock() {
                        state.playhead_sec[d] = scrub_pos;
                    }
                }

                ui.add_space(4.0);

                // Loop Strip & Hotcue 1..8 Grid
                ui.horizontal(|ui| {
                    // Loop Size controls
                    if ui.add(egui::Button::new("/2").min_size(Vec2::new(26.0, 20.0))).clicked() {
                        self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] / 2).max(1);
                    }
                    if ui.add(egui::Button::new("x2").min_size(Vec2::new(26.0, 20.0))).clicked() {
                        self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] * 2).min(32);
                    }

                    let loop_lbl = format!("LOOP: {}b", self.deck_loop_size_beats[d]);
                    let loop_active_btn = ui.add(
                        egui::Button::new(
                            RichText::new(loop_lbl)
                                .color(if self.deck_loop_active[d] { Color32::BLACK } else { TraktorTheme::LOOP_ACTIVE })
                                .font(egui::FontId::monospace(9.0))
                                .strong(),
                        )
                        .fill(if self.deck_loop_active[d] { TraktorTheme::LOOP_ACTIVE } else { Color32::TRANSPARENT })
                        .min_size(Vec2::new(60.0, 20.0)),
                    );
                    if loop_active_btn.clicked() {
                        self.deck_loop_active[d] = !self.deck_loop_active[d];
                    }

                    ui.separator();

                    // 8 Hotcue Pads
                    let cue_w = ((ui.available_width() - 8.0) / 8.0).max(18.0);
                    for i in 1..=8 {
                        let has_cue = self.deck_hotcues[d].iter().any(|(c, _)| *c == i as u8);
                        let pad_resp = cue_pad(ui, i, has_cue, false, cue_w);
                        if pad_resp.clicked() {
                            self.jump_to_cue(deck_idx, i as u8);
                        }
                    }
                });

                ui.add_space(6.0);

                // Transport Bottom Strip (Play, Cue, Sync, Flux, Pitch Fader)
                ui.horizontal(|ui| {
                    // PLAY Button
                    let play_color = if is_playing { TraktorTheme::PLAY_ACTIVE } else { TraktorTheme::PLAY_INACTIVE };
                    let play_text_color = if is_playing { Color32::BLACK } else { Color32::WHITE };
                    let play_btn = ui.add(
                        egui::Button::new(
                            RichText::new("▶ PLAY")
                                .color(play_text_color)
                                .font(egui::FontId::proportional(13.0))
                                .strong(),
                        )
                        .fill(play_color)
                        .min_size(Vec2::new(72.0, 28.0)),
                    );
                    if play_btn.clicked() {
                        self.toggle_play(deck_idx);
                    }

                    // CUE Button
                    let cue_btn = ui.add(
                        egui::Button::new(
                            RichText::new("CUE")
                                .color(Color32::WHITE)
                                .font(egui::FontId::proportional(12.0))
                                .strong(),
                        )
                        .fill(TraktorTheme::CUE_INACTIVE)
                        .min_size(Vec2::new(54.0, 28.0)),
                    );
                    if cue_btn.clicked() {
                        self.jump_to_cue(deck_idx, 1);
                    }

                    // SYNC Button
                    let sync_btn = ui.add(
                        egui::Button::new(
                            RichText::new("SYNC")
                                .color(TraktorTheme::SYNC_ACTIVE)
                                .font(egui::FontId::monospace(11.0))
                                .strong(),
                        )
                        .fill(Color32::from_rgb(20, 36, 44))
                        .min_size(Vec2::new(50.0, 28.0)),
                    );
                    if sync_btn.clicked() {
                        self.sync_deck(deck_idx);
                    }

                    // FLUX (Slip Mode) Button
                    let flux_active = self.deck_flux_active[d];
                    let flux_btn = ui.add(
                        egui::Button::new(
                            RichText::new("FLUX")
                                .color(if flux_active { Color32::BLACK } else { TraktorTheme::FLUX_ACTIVE })
                                .font(egui::FontId::monospace(10.0))
                                .strong(),
                        )
                        .fill(if flux_active { TraktorTheme::FLUX_ACTIVE } else { Color32::TRANSPARENT })
                        .min_size(Vec2::new(46.0, 28.0)),
                    );
                    if flux_btn.clicked() {
                        self.deck_flux_active[d] = !self.deck_flux_active[d];
                    }

                    // Pitch Slider (-8% to +8%)
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.deck_pitch_fader[d], -8.0..=8.0)
                                .suffix("%")
                                .orientation(egui::SliderOrientation::Horizontal)
                                .text("PITCH"),
                        );
                    });
                });
            });
    }

    fn show_mixer_section(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL_ALT)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(6.0, 6.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // 4 Channel Strips side-by-side: Ch 1 (A), Ch 3 (C), Ch 4 (D), Ch 2 (B)
                    ui.horizontal(|ui| {
                        let channels = [0, 2, 3, 1]; // Visual order
                        for &ch in &channels {
                            self.show_channel_strip(ui, ch);
                        }
                    });

                    ui.add_space(4.0);
                    ui.separator();

                    // Master Crossfader Area
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("A")
                                .color(TraktorTheme::DECK_A)
                                .font(egui::FontId::monospace(12.0))
                                .strong(),
                        );
                        crossfader(ui, &mut self.crossfader_pos);
                        ui.label(
                            RichText::new("B")
                                .color(TraktorTheme::DECK_B)
                                .font(egui::FontId::monospace(12.0))
                                .strong(),
                        );
                    });
                });
            });
    }

    fn show_channel_strip(&mut self, ui: &mut Ui, ch: usize) {
        let ch_num = ch + 1;
        let accent = TraktorTheme::deck_color(ch_num);

        ui.allocate_ui_with_layout(
            Vec2::new(76.0, 240.0),
            Layout::top_down(Align::Center),
            |ui| {
                // Channel Header
                ui.label(
                    RichText::new(format!("CH {ch_num}"))
                        .color(accent)
                        .font(egui::FontId::monospace(10.0))
                        .strong(),
                );

                // Gain Knob
                rotary_knob(ui, &mut self.channel_gain[ch], 0.0, 1.0, 0.5, "GAIN", accent);

                // 3-Band EQ Knobs
                rotary_knob(ui, &mut self.channel_eq_hi[ch], 0.0, 1.0, 0.5, "HI", accent);
                rotary_knob(ui, &mut self.channel_eq_mid[ch], 0.0, 1.0, 0.5, "MID", accent);
                rotary_knob(ui, &mut self.channel_eq_low[ch], 0.0, 1.0, 0.5, "LOW", accent);

                // Bipolar Filter / Mixer FX Knob
                let filter_accent = if self.channel_filter_on[ch] { TraktorTheme::PLAY_ACTIVE } else { TraktorTheme::TEXT_MUTED };
                rotary_knob(ui, &mut self.channel_filter[ch], 0.0, 1.0, 0.5, "FLTR", filter_accent);

                // FX On Toggle Button
                let fx_on_btn = ui.add(
                    egui::Button::new(
                        RichText::new("ON")
                            .color(if self.channel_filter_on[ch] { Color32::BLACK } else { TraktorTheme::TEXT_MUTED })
                            .font(egui::FontId::monospace(8.0))
                            .strong(),
                    )
                    .fill(if self.channel_filter_on[ch] { TraktorTheme::PLAY_ACTIVE } else { Color32::TRANSPARENT })
                    .min_size(Vec2::new(30.0, 14.0)),
                );
                if fx_on_btn.clicked() {
                    self.channel_filter_on[ch] = !self.channel_filter_on[ch];
                }

                // Headphone Cue Listen Button
                let cue_active = self.channel_cue[ch];
                let cue_btn = ui.add(
                    egui::Button::new(
                        RichText::new("🎧 CUE")
                            .color(if cue_active { Color32::BLACK } else { TraktorTheme::CUE_ACTIVE })
                            .font(egui::FontId::monospace(9.0))
                            .strong(),
                    )
                    .fill(if cue_active { TraktorTheme::CUE_ACTIVE } else { Color32::TRANSPARENT })
                    .min_size(Vec2::new(48.0, 16.0)),
                );
                if cue_btn.clicked() {
                    self.channel_cue[ch] = !self.channel_cue[ch];
                }

                // Volume Fader & Stereo VU Meter side-by-side
                ui.horizontal(|ui| {
                    vertical_fader(ui, &mut self.channel_volume[ch], 0.0, 1.0, accent);

                    let peak = if let Ok(state) = self.audio.shared.lock() {
                        state.channel_peaks[ch]
                    } else {
                        0.0
                    };
                    vu_meter(ui, peak, peak * 0.95, 120.0);
                });

                // Crossfader Assignment Selector (Left / Thru / Right)
                ui.horizontal(|ui| {
                    let assign = &mut self.channel_xfader_assign[ch];
                    let mut xf_btn = |label: &str, val: usize| {
                        let is_sel = *assign == val;
                        let b = ui.add(
                            egui::Button::new(
                                RichText::new(label)
                                    .color(if is_sel { Color32::BLACK } else { TraktorTheme::TEXT_MUTED })
                                    .font(egui::FontId::monospace(8.0)),
                            )
                            .fill(if is_sel { Color32::WHITE } else { Color32::TRANSPARENT })
                            .min_size(Vec2::new(16.0, 14.0)),
                        );
                        if b.clicked() {
                            *assign = val;
                        }
                    };
                    xf_btn("L", 0);
                    xf_btn("T", 1);
                    xf_btn("R", 2);
                });
            },
        );
    }

    fn show_browser_panel(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Left Navigation Tree
                    ui.allocate_ui_with_layout(
                        Vec2::new(170.0, 240.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            self.library.show_nav_tree(ui);
                        },
                    );

                    ui.separator();

                    // Current Deck A key for Harmonic Compatibility matching
                    let current_deck_a_key = if let Ok(state) = self.audio.shared.lock() {
                        state.loaded_tracks[0]
                            .as_ref()
                            .map(|t| t.key.clone())
                            .unwrap_or_else(|| "4m".to_string())
                    } else {
                        "4m".to_string()
                    };

                    // Right Track Table
                    let shared_audio = self.audio.shared.clone();
                    self.library.show_track_table(
                        ui,
                        &current_deck_a_key,
                        move |deck_idx, track: &UiTrackEntry| {
                            let d = deck_idx - 1;
                            let style = if track.bpm >= 130.0 { 1 } else { 0 };
                            let loaded = LoadedTrackAudio::new_synthetic(
                                &track.title,
                                &track.artist,
                                track.bpm,
                                &track.musical_key,
                                track.duration_sec,
                                style,
                            );
                            if let Ok(mut state) = shared_audio.lock() {
                                state.loaded_tracks[d] = Some(loaded);
                                state.playhead_sec[d] = 0.0;
                            }
                        },
                    );
                });
            });
    }
}
