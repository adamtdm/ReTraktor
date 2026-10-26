use crate::audio::{AudioSystem, LoadedTrackAudio};
use crate::library_ui::{LibraryManager, UiTrackEntry};
use crate::preferences::PreferencesDialog;
use crate::theme::TraktorTheme;
use crate::widgets::{
    crossfader, cue_pad, rotary_knob_sized, scrolling_waveform, stripe_overview, vertical_fader,
    vu_meter,
};
use eframe::egui::{self, Align, Color32, Key, Layout, RichText, Stroke, Ui, Vec2};
use retraktor_core::PropertyBus;
use retraktor_dsp::MixerFxKind;
use retraktor_library::KeyWheel;

/// Traktor Pro 4 Workspace Layouts (switchable from Top Header or F1..F5)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TraktorLayout {
    /// Stacked full-width parallel waveforms at top + transport/mixer row + browser
    Parallel,
    /// Side-by-side Decks A & B + 4-channel center mixer + browser
    Extended,
    /// Clean 2-Deck + 2-Channel mixer + large browser
    Essential,
    /// All 4 Decks (A, B, C, D) visible + 4-channel mixer + browser
    FourDeck,
    /// Compact mini-decks at top + maximized full-screen library & file explorer
    Browser,
}

impl TraktorLayout {
    pub fn label(self) -> &'static str {
        match self {
            Self::Parallel => "Parallel",
            Self::Extended => "Extended",
            Self::Essential => "Essential",
            Self::FourDeck => "4-Deck",
            Self::Browser => "Browser",
        }
    }
}

pub struct ReTraktorApp {
    pub audio: AudioSystem,
    pub library: LibraryManager,
    pub preferences: PreferencesDialog,
    pub bus: PropertyBus,
    pub layout: TraktorLayout,

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
    pub deck_loop_start_sec: [f64; 4],
    pub deck_loop_size_beats: [u32; 4],
    pub deck_hotcues: [Vec<(u8, f64)>; 4],
}

impl Default for ReTraktorApp {
    fn default() -> Self {
        let bus = PropertyBus::new_traktor_pro4_default();
        Self {
            audio: AudioSystem::new(),
            library: LibraryManager::default(),
            preferences: PreferencesDialog::default(),
            bus,
            layout: TraktorLayout::Parallel, // Default to Parallel layout!
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
            deck_loop_start_sec: [0.0, 0.0, 0.0, 0.0],
            deck_loop_size_beats: [4, 4, 4, 4],
            deck_hotcues: [
                vec![(1, 0.0), (2, 15.0), (3, 30.0), (4, 60.0)],
                vec![(1, 0.0), (2, 16.0), (3, 32.0), (4, 64.0)],
                vec![(1, 0.0), (2, 15.0)],
                vec![(1, 0.0), (2, 15.0)],
            ],
        }
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

                // 1. Top FX Rack, Master Clock, Layout Switcher & Telemetry Bar
                self.show_top_bar(ui);

                // 2. Active Workspace Layout
                match self.layout {
                    TraktorLayout::Parallel => {
                        self.show_parallel_layout(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Extended => {
                        self.show_extended_layout(ui, false);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Essential => {
                        self.show_extended_layout(ui, true);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::FourDeck => {
                        self.show_four_deck_layout(ui);
                        self.show_browser_panel(ui);
                    }
                    TraktorLayout::Browser => {
                        self.show_mini_decks_bar(ui);
                        self.show_browser_panel(ui);
                    }
                }
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
                state.channel_gain[d] = self.channel_gain[d];
                state.engine.strips[d].volume = self.channel_volume[d];
                state.engine.strips[d].cue_enabled = self.channel_cue[d];
                state.engine.strips[d].filter.knob = self.channel_filter[d];
                state.engine.strips[d].filter.enabled = self.channel_filter_on[d];
                state.engine.strips[d].filter.kind = self.channel_fx_kind[d];
                state.engine.strips[d].eq.high = self.channel_eq_hi[d];
                state.engine.strips[d].eq.mid = self.channel_eq_mid[d];
                state.engine.strips[d].eq.low = self.channel_eq_low[d];
                state.eq_kill_hi[d] = self.eq_kill_hi[d];
                state.eq_kill_mid[d] = self.eq_kill_mid[d];
                state.eq_kill_low[d] = self.eq_kill_low[d];

                state.engine.strips[d].xfader_left = self.channel_xfader_assign[d] == 0;
                state.engine.strips[d].xfader_right = self.channel_xfader_assign[d] == 2;

                let pitch_pct = self.deck_pitch_fader[d] as f64 / 100.0;
                state.pitch_ratio[d] = 1.0 + pitch_pct;

                let bpm = state.loaded_tracks[d]
                    .as_ref()
                    .map(|t| t.bpm)
                    .unwrap_or(128.0);
                state.loop_active[d] = self.deck_loop_active[d];
                state.loop_start_sec[d] = self.deck_loop_start_sec[d];
                state.loop_len_sec[d] = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
            }

            state.engine.master_volume = self.master_volume;
            state.engine.clock.bpm = self.master_tempo;
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Comma)) {
            self.preferences.is_open = !self.preferences.is_open;
        }

        // Layout shortcuts F1..F5
        if ctx.input(|i| i.key_pressed(Key::F1)) {
            self.layout = TraktorLayout::Parallel;
        }
        if ctx.input(|i| i.key_pressed(Key::F2)) {
            self.layout = TraktorLayout::Extended;
        }
        if ctx.input(|i| i.key_pressed(Key::F3)) {
            self.layout = TraktorLayout::Essential;
        }
        if ctx.input(|i| i.key_pressed(Key::F4)) {
            self.layout = TraktorLayout::FourDeck;
        }
        if ctx.input(|i| i.key_pressed(Key::F5)) {
            self.layout = TraktorLayout::Browser;
        }

        // Only process single-key deck shortcuts when not typing in a text field
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::Space)) {
                self.toggle_play(1);
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::Space)) {
                self.toggle_play(2);
            }
            if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::C)) {
                self.jump_to_cue(1, 1);
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::C)) {
                self.jump_to_cue(2, 1);
            }
            if ctx.input(|i| !i.modifiers.shift && i.key_pressed(Key::S)) {
                self.sync_deck(1);
            }
            if ctx.input(|i| i.modifiers.shift && i.key_pressed(Key::S)) {
                self.sync_deck(2);
            }
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
        } else if let Ok(state) = self.audio.shared.lock() {
            let current = state.playhead_sec[d];
            self.deck_hotcues[d].push((cue_num, current));
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

    fn toggle_loop(&mut self, deck_idx: usize) {
        let d = deck_idx - 1;
        self.deck_loop_active[d] = !self.deck_loop_active[d];
        if self.deck_loop_active[d] {
            if let Ok(state) = self.audio.shared.lock() {
                self.deck_loop_start_sec[d] = state.playhead_sec[d];
            }
        }
    }

    /// Top Header Bar with Brand, Master Clock, FX 1 & 2, Layout Switcher, and Master Limiter
    fn show_top_bar(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // 1. Brand Logo
                    ui.label(
                        RichText::new("TRAKTOR")
                            .color(Color32::WHITE)
                            .font(egui::FontId::proportional(15.0))
                            .strong(),
                    );
                    ui.label(
                        RichText::new("PRO 4")
                            .color(TraktorTheme::DECK_A)
                            .font(egui::FontId::proportional(15.0))
                            .strong(),
                    );

                    ui.separator();

                    // 2. Master Clock
                    ui.label(
                        RichText::new("MASTER")
                            .color(TraktorTheme::TEXT_SECONDARY)
                            .font(egui::FontId::proportional(9.5)),
                    );
                    ui.label(
                        RichText::new(format!("{:.2}", self.master_tempo))
                            .color(Color32::WHITE)
                            .font(egui::FontId::monospace(13.5))
                            .strong(),
                    );
                    if ui
                        .add(egui::Button::new("TAP").min_size(Vec2::new(30.0, 18.0)))
                        .clicked()
                    {
                        self.master_tempo = (self.master_tempo.round()).clamp(60.0, 200.0);
                    }
                    if ui
                        .add(egui::Button::new("-").min_size(Vec2::new(18.0, 18.0)))
                        .clicked()
                    {
                        self.master_tempo = (self.master_tempo - 0.5).max(40.0);
                    }
                    if ui
                        .add(egui::Button::new("+").min_size(Vec2::new(18.0, 18.0)))
                        .clicked()
                    {
                        self.master_tempo = (self.master_tempo + 0.5).min(240.0);
                    }

                    ui.separator();

                    // 3. Layout Switcher Pills (Parallel | Extended | Essential | 4-Deck | Browser)
                    ui.label(
                        RichText::new("LAYOUT:")
                            .color(TraktorTheme::TEXT_SECONDARY)
                            .font(egui::FontId::monospace(9.5))
                            .strong(),
                    );
                    let layouts = [
                        TraktorLayout::Parallel,
                        TraktorLayout::Extended,
                        TraktorLayout::Essential,
                        TraktorLayout::FourDeck,
                        TraktorLayout::Browser,
                    ];
                    for lay in layouts {
                        let active = self.layout == lay;
                        let btn = ui.add(
                            egui::Button::new(
                                RichText::new(lay.label())
                                    .color(if active {
                                        Color32::BLACK
                                    } else {
                                        TraktorTheme::TEXT_PRIMARY
                                    })
                                    .font(egui::FontId::proportional(11.0))
                                    .strong(),
                            )
                            .fill(if active {
                                TraktorTheme::DECK_A
                            } else {
                                Color32::from_rgb(34, 37, 45)
                            })
                            .min_size(Vec2::new(58.0, 20.0)),
                        );
                        if btn.clicked() {
                            self.layout = lay;
                            self.preferences.four_deck_mode = lay == TraktorLayout::FourDeck;
                        }
                    }

                    ui.separator();

                    // 4. FX 1 & FX 2 Compact Racks
                    let fx1_btn = ui.add(
                        egui::Button::new(
                            RichText::new("FX 1")
                                .color(if self.fx1_on {
                                    Color32::BLACK
                                } else {
                                    TraktorTheme::DECK_A
                                })
                                .font(egui::FontId::monospace(9.5))
                                .strong(),
                        )
                        .fill(if self.fx1_on {
                            TraktorTheme::DECK_A
                        } else {
                            Color32::from_rgb(28, 30, 36)
                        })
                        .min_size(Vec2::new(32.0, 20.0)),
                    );
                    if fx1_btn.clicked() {
                        self.fx1_on = !self.fx1_on;
                    }
                    rotary_knob_sized(
                        ui,
                        &mut self.fx1_dry_wet,
                        0.0,
                        1.0,
                        0.3,
                        "D/W",
                        TraktorTheme::DECK_A,
                        28.0,
                    );
                    rotary_knob_sized(
                        ui,
                        &mut self.fx1_param[0],
                        0.0,
                        1.0,
                        0.5,
                        "REV",
                        TraktorTheme::DECK_A,
                        28.0,
                    );

                    ui.separator();

                    let fx2_btn = ui.add(
                        egui::Button::new(
                            RichText::new("FX 2")
                                .color(if self.fx2_on {
                                    Color32::BLACK
                                } else {
                                    TraktorTheme::DECK_B
                                })
                                .font(egui::FontId::monospace(9.5))
                                .strong(),
                        )
                        .fill(if self.fx2_on {
                            TraktorTheme::DECK_B
                        } else {
                            Color32::from_rgb(28, 30, 36)
                        })
                        .min_size(Vec2::new(32.0, 20.0)),
                    );
                    if fx2_btn.clicked() {
                        self.fx2_on = !self.fx2_on;
                    }
                    rotary_knob_sized(
                        ui,
                        &mut self.fx2_dry_wet,
                        0.0,
                        1.0,
                        0.3,
                        "D/W",
                        TraktorTheme::DECK_B,
                        28.0,
                    );
                    rotary_knob_sized(
                        ui,
                        &mut self.fx2_param[0],
                        0.0,
                        1.0,
                        0.5,
                        "DLY",
                        TraktorTheme::DECK_B,
                        28.0,
                    );

                    // 5. Right-aligned Master Output, Meter & Preferences Gear
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("⚙").font(egui::FontId::proportional(15.0)),
                                )
                                .min_size(Vec2::new(26.0, 22.0)),
                            )
                            .on_hover_text("Preferences (Ctrl+,)")
                            .clicked()
                        {
                            self.preferences.is_open = !self.preferences.is_open;
                        }

                        rotary_knob_sized(
                            ui,
                            &mut self.master_volume,
                            0.0,
                            1.0,
                            0.8,
                            "MAIN",
                            Color32::WHITE,
                            28.0,
                        );

                        let (peak_l, peak_r) = if let Ok(state) = self.audio.shared.lock() {
                            (state.master_peak_l, state.master_peak_r)
                        } else {
                            (0.0, 0.0)
                        };
                        vu_meter(ui, peak_l, peak_r, 30.0);

                        ui.label(
                            RichText::new("● PipeWire 48kHz")
                                .color(TraktorTheme::PLAY_ACTIVE)
                                .font(egui::FontId::monospace(9.5)),
                        );
                    });
                });
            });
    }

    /// LAYOUT 1: PARALLEL WAVEFORMS LAYOUT
    /// Stacked full-width waveforms for Deck A and Deck B right on top of each other so beatgrids
    /// align pixel-for-pixel vertically, followed by Deck A controls | Center Mixer | Deck B controls.
    fn show_parallel_layout(&mut self, ui: &mut Ui) {
        // A. Full-Width Stacked Parallel Waveforms Panel
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(6.0, 4.0))
            .show(ui, |ui| {
                self.show_parallel_waveform_row(ui, 1, TraktorTheme::DECK_A, 66.0);
                ui.add_space(2.0);
                self.show_parallel_waveform_row(ui, 2, TraktorTheme::DECK_B, 66.0);
            });

        // B. Deck Controls + Center Mixer Row (strictly top-aligned via horizontal_top!)
        ui.horizontal_top(|ui| {
            let avail_w = ui.available_width();
            let mixer_w = 340.0_f32;
            let deck_w = ((avail_w - mixer_w - 8.0) / 2.0).max(260.0);

            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 165.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 1, TraktorTheme::DECK_A, false, 0.0);
                },
            );

            ui.allocate_ui_with_layout(
                Vec2::new(mixer_w, 165.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(mixer_w);
                    self.show_mixer_section(ui, false);
                },
            );

            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 165.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 2, TraktorTheme::DECK_B, false, 0.0);
                },
            );
        });
    }

    fn show_parallel_waveform_row(
        &mut self,
        ui: &mut Ui,
        deck_idx: usize,
        accent_color: Color32,
        height: f32,
    ) {
        let d = deck_idx - 1;
        let deck_letter = ["A", "B", "C", "D"][d.min(3)];

        let (title, artist, bpm, key, duration_sec, elapsed_sec, peaks) = {
            if let Ok(state) = self.audio.shared.lock() {
                let el = state.playhead_sec[d];
                if let Some(ref t) = state.loaded_tracks[d] {
                    (
                        t.title.clone(),
                        t.artist.clone(),
                        t.bpm,
                        t.key.clone(),
                        t.duration_sec,
                        el,
                        t.waveform_peaks.clone(),
                    )
                } else {
                    (
                        format!("Deck {deck_letter} Empty"),
                        String::new(),
                        128.0,
                        "4m".to_string(),
                        120.0,
                        0.0,
                        Vec::new(),
                    )
                }
            } else {
                (
                    format!("Deck {deck_letter}"),
                    String::new(),
                    128.0,
                    "4m".to_string(),
                    120.0,
                    0.0,
                    Vec::new(),
                )
            }
        };

        // Compact info bar right above the parallel waveform
        ui.horizontal(|ui| {
            egui::Frame::none()
                .fill(accent_color)
                .rounding(2.0)
                .inner_margin(egui::Margin::symmetric(5.0, 1.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("DECK {deck_letter}"))
                            .color(Color32::BLACK)
                            .font(egui::FontId::monospace(10.0))
                            .strong(),
                    );
                });
            ui.label(
                RichText::new(&title)
                    .color(Color32::WHITE)
                    .font(egui::FontId::proportional(12.0))
                    .strong(),
            );
            ui.label(
                RichText::new(format!("— {artist}"))
                    .color(TraktorTheme::TEXT_SECONDARY)
                    .font(egui::FontId::proportional(11.0)),
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let rem_sec = (duration_sec - elapsed_sec).max(0.0);
                let rem_m = (rem_sec / 60.0).floor() as u32;
                let rem_s = (rem_sec % 60.0).floor() as u32;
                ui.label(
                    RichText::new(format!("-{rem_m:02}:{rem_s:02}"))
                        .color(accent_color)
                        .font(egui::FontId::monospace(12.5))
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("KEY {key}"))
                        .color(TraktorTheme::PLAY_ACTIVE)
                        .font(egui::FontId::monospace(10.5))
                        .strong(),
                );
                let eff_bpm = bpm * (1.0 + self.deck_pitch_fader[d] as f64 / 100.0);
                ui.label(
                    RichText::new(format!("{eff_bpm:.2} BPM"))
                        .color(Color32::WHITE)
                        .font(egui::FontId::monospace(11.5))
                        .strong(),
                );
            });
        });

        let cues = self.deck_hotcues[d].clone();
        let loop_len = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
        let wave_resp = scrolling_waveform(
            ui,
            elapsed_sec,
            bpm,
            &cues,
            &peaks,
            self.deck_loop_active[d],
            self.deck_loop_start_sec[d],
            loop_len,
            Some((deck_letter, accent_color)),
            height,
        );
        if wave_resp.dragged() {
            let delta_x = -wave_resp.drag_delta().x;
            let dt = delta_x as f64 * (1.0 / 85.0);
            if let Ok(mut state) = self.audio.shared.lock() {
                state.playhead_sec[d] = (state.playhead_sec[d] + dt).clamp(0.0, duration_sec);
            }
        }
    }

    /// LAYOUT 2 & 3: EXTENDED / ESSENTIAL SIDE-BY-SIDE LAYOUT
    fn show_extended_layout(&mut self, ui: &mut Ui, two_channel_mixer_only: bool) {
        ui.horizontal_top(|ui| {
            let avail_w = ui.available_width();
            let mixer_w = if two_channel_mixer_only {
                200.0_f32
            } else {
                348.0_f32
            };
            let deck_w = ((avail_w - mixer_w - 8.0) / 2.0).max(260.0);

            // Left: Deck A (Cyan)
            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 235.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 1, TraktorTheme::DECK_A, true, 72.0);
                },
            );

            // Center: Mixer
            ui.allocate_ui_with_layout(
                Vec2::new(mixer_w, 235.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(mixer_w);
                    self.show_mixer_section(ui, two_channel_mixer_only);
                },
            );

            // Right: Deck B (Orange)
            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 235.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 2, TraktorTheme::DECK_B, true, 72.0);
                },
            );
        });
    }

    /// LAYOUT 4: 4-DECK PRO LAYOUT (Decks A & C on Left, 4-Ch Mixer in Center, Decks B & D on Right)
    fn show_four_deck_layout(&mut self, ui: &mut Ui) {
        ui.horizontal_top(|ui| {
            let avail_w = ui.available_width();
            let mixer_w = 348.0_f32;
            let deck_w = ((avail_w - mixer_w - 8.0) / 2.0).max(260.0);

            // Left Stack: Deck A + Deck C
            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 340.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 1, TraktorTheme::DECK_A, true, 46.0);
                    ui.add_space(3.0);
                    self.show_single_deck(ui, 3, TraktorTheme::DECK_C, true, 46.0);
                },
            );

            // Center: 4-Channel Mixer
            ui.allocate_ui_with_layout(
                Vec2::new(mixer_w, 340.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(mixer_w);
                    self.show_mixer_section(ui, false);
                },
            );

            // Right Stack: Deck B + Deck D
            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 340.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 2, TraktorTheme::DECK_B, true, 46.0);
                    ui.add_space(3.0);
                    self.show_single_deck(ui, 4, TraktorTheme::DECK_D, true, 46.0);
                },
            );
        });
    }

    /// LAYOUT 5: BROWSER / PREPARATION MINI-DECKS BAR
    fn show_mini_decks_bar(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    self.show_mini_deck_strip(ui, 1, TraktorTheme::DECK_A);
                    ui.separator();
                    crossfader(ui, &mut self.crossfader_pos, 120.0);
                    ui.separator();
                    self.show_mini_deck_strip(ui, 2, TraktorTheme::DECK_B);
                });
            });
    }

    fn show_mini_deck_strip(&mut self, ui: &mut Ui, deck_idx: usize, accent: Color32) {
        let d = deck_idx - 1;
        let letter = ["A", "B", "C", "D"][d.min(3)];
        let (title, bpm, is_playing) = if let Ok(state) = self.audio.shared.lock() {
            let t_name = state.loaded_tracks[d]
                .as_ref()
                .map(|t| t.title.clone())
                .unwrap_or_else(|| "Empty".to_string());
            let b = state.loaded_tracks[d]
                .as_ref()
                .map(|t| t.bpm)
                .unwrap_or(128.0);
            (t_name, b, state.is_playing[d])
        } else {
            ("Empty".to_string(), 128.0, false)
        };

        if ui
            .add(
                egui::Button::new(
                    RichText::new(if is_playing {
                        format!("⏸ DECK {letter}")
                    } else {
                        format!("▶ DECK {letter}")
                    })
                    .color(if is_playing {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    })
                    .strong(),
                )
                .fill(if is_playing {
                    TraktorTheme::PLAY_ACTIVE
                } else {
                    accent.gamma_multiply(0.4)
                }),
            )
            .clicked()
        {
            self.toggle_play(deck_idx);
        }
        ui.label(RichText::new(title).color(Color32::WHITE).strong());
        ui.label(RichText::new(format!("{bpm:.2} BPM")).color(accent).monospace());
    }

    fn show_single_deck(
        &mut self,
        ui: &mut Ui,
        deck_idx: usize,
        accent_color: Color32,
        include_scrolling_waveform: bool,
        waveform_height: f32,
    ) {
        let d = deck_idx - 1;
        let deck_letter = match deck_idx {
            1 => "A",
            2 => "B",
            3 => "C",
            4 => "D",
            _ => "A",
        };

        let (title, artist, bpm, key, duration_sec, elapsed_sec, _is_playing, peaks) = {
            if let Ok(state) = self.audio.shared.lock() {
                let is_p = state.is_playing[d];
                let el = state.playhead_sec[d];
                if let Some(ref t) = state.loaded_tracks[d] {
                    (
                        t.title.clone(),
                        t.artist.clone(),
                        t.bpm,
                        t.key.clone(),
                        t.duration_sec,
                        el,
                        is_p,
                        t.waveform_peaks.clone(),
                    )
                } else {
                    (
                        format!("Deck {deck_letter} Empty"),
                        "No Track Loaded".to_string(),
                        128.0,
                        "4m".to_string(),
                        120.0,
                        0.0,
                        false,
                        Vec::new(),
                    )
                }
            } else {
                (
                    format!("Deck {deck_letter}"),
                    String::new(),
                    128.0,
                    "4m".to_string(),
                    120.0,
                    0.0,
                    false,
                    Vec::new(),
                )
            }
        };

        let is_playing = if let Ok(state) = self.audio.shared.lock() {
            state.is_playing[d]
        } else {
            false
        };

        let is_harmonic = if deck_idx != 1 {
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
            true
        };

        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, accent_color))
            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
            .show(ui, |ui| {
                // 1. Deck Header Strip
                ui.horizontal(|ui| {
                    egui::Frame::none()
                        .fill(accent_color)
                        .rounding(2.0)
                        .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(deck_letter)
                                    .color(Color32::BLACK)
                                    .font(egui::FontId::monospace(13.0))
                                    .strong(),
                            );
                        });

                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(0.0, 1.0);
                        ui.label(
                            RichText::new(&title)
                                .color(TraktorTheme::TEXT_PRIMARY)
                                .font(egui::FontId::proportional(12.5))
                                .strong(),
                        );
                        ui.label(
                            RichText::new(&artist)
                                .color(TraktorTheme::TEXT_SECONDARY)
                                .font(egui::FontId::proportional(10.5)),
                        );
                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let rem_sec = (duration_sec - elapsed_sec).max(0.0);
                        let rem_m = (rem_sec / 60.0).floor() as u32;
                        let rem_s = (rem_sec % 60.0).floor() as u32;
                        ui.label(
                            RichText::new(format!("-{rem_m:02}:{rem_s:02}"))
                                .color(accent_color)
                                .font(egui::FontId::monospace(13.0))
                                .strong(),
                        );

                        let key_bg = if is_harmonic {
                            TraktorTheme::PLAY_ACTIVE
                        } else {
                            Color32::from_rgb(38, 42, 50)
                        };
                        let key_fg = if is_harmonic {
                            Color32::BLACK
                        } else {
                            Color32::WHITE
                        };
                        egui::Frame::none()
                            .fill(key_bg)
                            .rounding(2.0)
                            .inner_margin(egui::Margin::symmetric(5.0, 1.5))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("KEY {key}"))
                                        .color(key_fg)
                                        .font(egui::FontId::monospace(9.5))
                                        .strong(),
                                );
                            });

                        let pitch_pct = self.deck_pitch_fader[d];
                        let effective_bpm = bpm * (1.0 + pitch_pct as f64 / 100.0);
                        ui.label(
                            RichText::new(format!("{effective_bpm:.2} BPM ({pitch_pct:+.1}%)"))
                                .color(TraktorTheme::TEXT_SECONDARY)
                                .font(egui::FontId::monospace(10.5)),
                        );
                    });
                });

                // 2. Optional Scrolling Frequency Waveform (in Extended / 4-Deck views)
                let cues = self.deck_hotcues[d].clone();
                let loop_len_sec = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
                if include_scrolling_waveform && waveform_height > 10.0 {
                    ui.add_space(2.0);
                    let wave_resp = scrolling_waveform(
                        ui,
                        elapsed_sec,
                        bpm,
                        &cues,
                        &peaks,
                        self.deck_loop_active[d],
                        self.deck_loop_start_sec[d],
                        loop_len_sec,
                        None,
                        waveform_height,
                    );
                    if wave_resp.dragged() {
                        let delta_x = -wave_resp.drag_delta().x;
                        let dt = delta_x as f64 * (1.0 / 85.0);
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.playhead_sec[d] =
                                (state.playhead_sec[d] + dt).clamp(0.0, duration_sec);
                        }
                    }
                }

                ui.add_space(2.0);

                // 3. Full-Track Stripe Overview Minimap with Scrubber
                let mut scrub_pos = elapsed_sec;
                let stripe_resp = stripe_overview(
                    ui,
                    &mut scrub_pos,
                    duration_sec,
                    &peaks,
                    &cues,
                    self.deck_loop_active[d],
                    self.deck_loop_start_sec[d],
                    loop_len_sec,
                    accent_color,
                );
                if stripe_resp.changed() {
                    if let Ok(mut state) = self.audio.shared.lock() {
                        state.playhead_sec[d] = scrub_pos;
                    }
                }

                ui.add_space(3.0);

                // 4. Loop Strip & Hotcue 1..8 Grid
                ui.horizontal(|ui| {
                    if ui
                        .add(egui::Button::new("/2").min_size(Vec2::new(24.0, 20.0)))
                        .clicked()
                    {
                        self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] / 2).max(1);
                    }
                    if ui
                        .add(egui::Button::new("x2").min_size(Vec2::new(24.0, 20.0)))
                        .clicked()
                    {
                        self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] * 2).min(32);
                    }

                    let loop_lbl = format!("LOOP {}b", self.deck_loop_size_beats[d]);
                    let loop_active_btn = ui.add(
                        egui::Button::new(
                            RichText::new(loop_lbl)
                                .color(if self.deck_loop_active[d] {
                                    Color32::BLACK
                                } else {
                                    TraktorTheme::LOOP_ACTIVE
                                })
                                .font(egui::FontId::monospace(9.0))
                                .strong(),
                        )
                        .fill(if self.deck_loop_active[d] {
                            TraktorTheme::LOOP_ACTIVE
                        } else {
                            Color32::TRANSPARENT
                        })
                        .min_size(Vec2::new(56.0, 20.0)),
                    );
                    if loop_active_btn.clicked() {
                        self.toggle_loop(deck_idx);
                    }

                    ui.separator();

                    let cue_w = ((ui.available_width() - 28.0) / 8.0).max(16.0);
                    for i in 1..=8 {
                        let has_cue = self.deck_hotcues[d].iter().any(|(c, _)| *c == i as u8);
                        let pad_resp = cue_pad(ui, i, has_cue, false, cue_w);
                        if pad_resp.clicked() {
                            self.jump_to_cue(deck_idx, i as u8);
                        }
                        if pad_resp.secondary_clicked() {
                            self.deck_hotcues[d].retain(|(c, _)| *c != i as u8);
                        }
                    }
                });

                ui.add_space(3.0);

                // 5. Transport Bottom Strip (Play, Cue, Sync, Flux, Pitch Fader)
                ui.horizontal(|ui| {
                    let play_color = if is_playing {
                        TraktorTheme::PLAY_ACTIVE
                    } else {
                        TraktorTheme::PLAY_INACTIVE
                    };
                    let play_text_color = if is_playing {
                        Color32::BLACK
                    } else {
                        Color32::WHITE
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(if is_playing { "⏸ PAUSE" } else { "▶ PLAY" })
                                    .color(play_text_color)
                                    .font(egui::FontId::proportional(12.0))
                                    .strong(),
                            )
                            .fill(play_color)
                            .min_size(Vec2::new(68.0, 24.0)),
                        )
                        .clicked()
                    {
                        self.toggle_play(deck_idx);
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("CUE")
                                    .color(Color32::WHITE)
                                    .font(egui::FontId::proportional(11.5))
                                    .strong(),
                            )
                            .fill(TraktorTheme::CUE_INACTIVE)
                            .min_size(Vec2::new(46.0, 24.0)),
                        )
                        .clicked()
                    {
                        self.jump_to_cue(deck_idx, 1);
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("SYNC")
                                    .color(TraktorTheme::SYNC_ACTIVE)
                                    .font(egui::FontId::monospace(10.5))
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(20, 36, 44))
                            .min_size(Vec2::new(46.0, 24.0)),
                        )
                        .clicked()
                    {
                        self.sync_deck(deck_idx);
                    }

                    let flux_active = self.deck_flux_active[d];
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("FLUX")
                                    .color(if flux_active {
                                        Color32::BLACK
                                    } else {
                                        TraktorTheme::FLUX_ACTIVE
                                    })
                                    .font(egui::FontId::monospace(9.5))
                                    .strong(),
                            )
                            .fill(if flux_active {
                                TraktorTheme::FLUX_ACTIVE
                            } else {
                                Color32::TRANSPARENT
                            })
                            .min_size(Vec2::new(42.0, 24.0)),
                        )
                        .clicked()
                    {
                        self.deck_flux_active[d] = !self.deck_flux_active[d];
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button("0%")
                            .on_hover_text("Reset pitch to 0.0%")
                            .clicked()
                        {
                            self.deck_pitch_fader[d] = 0.0;
                        }
                        ui.add(
                            egui::Slider::new(&mut self.deck_pitch_fader[d], -8.0..=8.0)
                                .suffix("%")
                                .show_value(false),
                        );
                    });
                });
            });
    }

    /// Center Mixer Section — Strictly top-aligned with side-by-side EQ + Fader/VU columns per strip
    /// so all 4 channels line up horizontally with zero staircase/staggering!
    fn show_mixer_section(&mut self, ui: &mut Ui, two_channel_only: bool) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL_ALT)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(6.0, 5.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    if two_channel_only {
                        ui.columns(2, |cols| {
                            self.show_channel_strip(&mut cols[0], 0);
                            self.show_channel_strip(&mut cols[1], 1);
                        });
                    } else {
                        // 4 Channel Strips side-by-side in locked equal columns: Ch 1 (A), Ch 3 (C), Ch 4 (D), Ch 2 (B)
                        ui.columns(4, |cols| {
                            self.show_channel_strip(&mut cols[0], 0);
                            self.show_channel_strip(&mut cols[1], 2);
                            self.show_channel_strip(&mut cols[2], 3);
                            self.show_channel_strip(&mut cols[3], 1);
                        });
                    }

                    ui.add_space(2.0);
                    ui.separator();

                    // Master Crossfader Row
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("◄ A")
                                .color(TraktorTheme::DECK_A)
                                .font(egui::FontId::monospace(10.5))
                                .strong(),
                        );
                        let xf_w = (ui.available_width() - 48.0).max(90.0);
                        crossfader(ui, &mut self.crossfader_pos, xf_w);
                        ui.label(
                            RichText::new("B ►")
                                .color(TraktorTheme::DECK_B)
                                .font(egui::FontId::monospace(10.5))
                                .strong(),
                        );
                    });
                });
            });
    }

    /// Authentic Traktor Channel Strip:
    /// Left sub-column = HI, MID, LOW, FLTR rotary knobs
    /// Right sub-column = GAIN knob, CUE button, Vertical Volume Fader + Stereo VU Meter
    fn show_channel_strip(&mut self, ui: &mut Ui, ch: usize) {
        let ch_num = ch + 1;
        let ch_letter = ["A", "B", "C", "D"][ch.min(3)];
        let accent = TraktorTheme::deck_color(ch_num);

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);

            // Channel Header + Cue button
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("CH{ch_num} ({ch_letter})"))
                        .color(accent)
                        .font(egui::FontId::monospace(9.5))
                        .strong(),
                );
                let cue_active = self.channel_cue[ch];
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("🎧")
                                .color(if cue_active {
                                    Color32::BLACK
                                } else {
                                    TraktorTheme::CUE_ACTIVE
                                })
                                .font(egui::FontId::proportional(9.5)),
                        )
                        .fill(if cue_active {
                            TraktorTheme::CUE_ACTIVE
                        } else {
                            Color32::from_rgb(28, 30, 36)
                        })
                        .min_size(Vec2::new(20.0, 15.0)),
                    )
                    .on_hover_text("Headphone Cue Listen")
                    .clicked()
                {
                    self.channel_cue[ch] = !self.channel_cue[ch];
                }
            });

            // Side-by-side EQ/Filter column & Fader/Meter column
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 1.0);

                // Sub-column 1: HI, MID, LOW, FLTR
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(1.0, 1.0);
                    let hi_col = if self.eq_kill_hi[ch] {
                        TraktorTheme::METER_RED
                    } else {
                        accent
                    };
                    if rotary_knob_sized(
                        ui,
                        &mut self.channel_eq_hi[ch],
                        0.0,
                        1.0,
                        0.5,
                        "HI",
                        hi_col,
                        28.0,
                    )
                    .secondary_clicked()
                    {
                        self.eq_kill_hi[ch] = !self.eq_kill_hi[ch];
                    }

                    let mid_col = if self.eq_kill_mid[ch] {
                        TraktorTheme::METER_RED
                    } else {
                        accent
                    };
                    if rotary_knob_sized(
                        ui,
                        &mut self.channel_eq_mid[ch],
                        0.0,
                        1.0,
                        0.5,
                        "MID",
                        mid_col,
                        28.0,
                    )
                    .secondary_clicked()
                    {
                        self.eq_kill_mid[ch] = !self.eq_kill_mid[ch];
                    }

                    let low_col = if self.eq_kill_low[ch] {
                        TraktorTheme::METER_RED
                    } else {
                        accent
                    };
                    if rotary_knob_sized(
                        ui,
                        &mut self.channel_eq_low[ch],
                        0.0,
                        1.0,
                        0.5,
                        "LOW",
                        low_col,
                        28.0,
                    )
                    .secondary_clicked()
                    {
                        self.eq_kill_low[ch] = !self.eq_kill_low[ch];
                    }

                    let fltr_col = if self.channel_filter_on[ch] {
                        TraktorTheme::PLAY_ACTIVE
                    } else {
                        TraktorTheme::TEXT_MUTED
                    };
                    let fltr_resp = rotary_knob_sized(
                        ui,
                        &mut self.channel_filter[ch],
                        0.0,
                        1.0,
                        0.5,
                        "FLTR",
                        fltr_col,
                        28.0,
                    );
                    if fltr_resp.changed() {
                        self.channel_filter_on[ch] = (self.channel_filter[ch] - 0.5).abs() > 0.02;
                    }
                });

                // Sub-column 2: GAIN + Volume Fader + VU Meter
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                    rotary_knob_sized(
                        ui,
                        &mut self.channel_gain[ch],
                        0.0,
                        1.0,
                        0.5,
                        "GAIN",
                        accent,
                        28.0,
                    );

                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                        vertical_fader(ui, &mut self.channel_volume[ch], 0.0, 1.0, accent, 94.0);
                        let peak = if let Ok(state) = self.audio.shared.lock() {
                            state.channel_peaks[ch]
                        } else {
                            0.0
                        };
                        vu_meter(ui, peak, peak * 0.94, 94.0);
                    });
                });
            });

            // Crossfader Assign Buttons [A | T | B]
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 0.0);
                let assign = &mut self.channel_xfader_assign[ch];
                for (label, val) in [("A", 0usize), ("T", 1usize), ("B", 2usize)] {
                    let is_sel = *assign == val;
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(label)
                                    .color(if is_sel {
                                        Color32::BLACK
                                    } else {
                                        TraktorTheme::TEXT_MUTED
                                    })
                                    .font(egui::FontId::monospace(8.5))
                                    .strong(),
                            )
                            .fill(if is_sel {
                                accent
                            } else {
                                Color32::from_rgb(24, 26, 32)
                            })
                            .min_size(Vec2::new(18.0, 14.0)),
                        )
                        .clicked()
                    {
                        *assign = val;
                    }
                }
            });
        });
    }

    fn show_browser_panel(&mut self, ui: &mut Ui) {
        let avail_h = ui.available_height().max(160.0);
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(8.0, 6.0))
            .show(ui, |ui| {
                ui.set_min_height(avail_h - 12.0);
                ui.horizontal_top(|ui| {
                    // Left Navigation Tree
                    ui.allocate_ui_with_layout(
                        Vec2::new(178.0, avail_h - 16.0),
                        Layout::top_down(Align::Min),
                        |ui| {
                            ui.set_width(178.0);
                            self.library.show_nav_tree(ui);
                        },
                    );

                    ui.separator();

                    let current_deck_a_key = if let Ok(state) = self.audio.shared.lock() {
                        state.loaded_tracks[0]
                            .as_ref()
                            .map(|t| t.key.clone())
                            .unwrap_or_else(|| "4m".to_string())
                    } else {
                        "4m".to_string()
                    };

                    let four_deck_mode =
                        self.layout == TraktorLayout::FourDeck || self.preferences.four_deck_mode;
                    let shared_audio = self.audio.shared.clone();

                    self.library.show_browser_content(
                        ui,
                        &current_deck_a_key,
                        four_deck_mode,
                        move |deck_idx, track: &UiTrackEntry| {
                            let d = (deck_idx - 1).min(3);
                            // If track has a real audio file on disk, decode it with Symphonia!
                            let loaded = if let Some(ref path) = track.file_path {
                                if path.is_file() {
                                    LoadedTrackAudio::from_file(
                                        path,
                                        &track.title,
                                        &track.artist,
                                        track.bpm,
                                        &track.musical_key,
                                    )
                                    .unwrap_or_else(|_| {
                                        let style = if track.bpm >= 130.0 { 1 } else { 0 };
                                        LoadedTrackAudio::new_synthetic(
                                            &track.title,
                                            &track.artist,
                                            track.bpm,
                                            &track.musical_key,
                                            track.duration_sec,
                                            style,
                                        )
                                    })
                                } else {
                                    let style = if track.bpm >= 130.0 { 1 } else { 0 };
                                    LoadedTrackAudio::new_synthetic(
                                        &track.title,
                                        &track.artist,
                                        track.bpm,
                                        &track.musical_key,
                                        track.duration_sec,
                                        style,
                                    )
                                }
                            } else {
                                let style = match track.id % 3 {
                                    1 => 0,
                                    2 => 1,
                                    _ => 2,
                                };
                                LoadedTrackAudio::new_synthetic(
                                    &track.title,
                                    &track.artist,
                                    track.bpm,
                                    &track.musical_key,
                                    track.duration_sec,
                                    style,
                                )
                            };

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
