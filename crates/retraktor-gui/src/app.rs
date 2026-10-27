use crate::audio::{AudioSystem, LoadedTrackAudio};
use crate::library_ui::{LibraryManager, UiTrackEntry};
use crate::preferences::{PreferencesDialog, PrefsTab};
use crate::theme::TraktorTheme;
use crate::widgets::{
    crossfader, cue_pad, rotary_knob_sized, scrolling_stem_waveforms, scrolling_waveform,
    stripe_overview, vertical_fader, vu_meter,
};
use eframe::egui::{self, Align, Color32, Key, Layout, RichText, Stroke, Ui, Vec2};
use retraktor_core::{PropValue, PropertyBus, SurfaceEvent};
use retraktor_dsp::MixerFxKind;
use retraktor_hid::{
    scan_linux_ni_hidraw_devices, Mx2BottomShellState, Mx2HidDriver, TRAKTOR_MX2_JOG_TICKS_PER_REV,
    TRAKTOR_MX2_PID,
};
use retraktor_library::KeyWheel;
use std::env;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

/// Traktor Pro 4 + Next-Gen Workspace Layouts (switchable from Top Header or F1..F6)
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
    /// DJ.Studio-style Horizontal Timeline Transition Editor + Stem Automation
    Timeline,
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
            Self::Timeline => "Timeline",
            Self::Browser => "Browser",
        }
    }
}

/// Traktor MX2 & Deck Performance Pad Modes
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeckPadMode {
    Hotcues,
    Stems,
    Pattern,
    FluxLoop,
}

impl DeckPadMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Hotcues => "HOTCUES",
            Self::Stems => "STEMS",
            Self::Pattern => "PATTERN",
            Self::FluxLoop => "LOOP ROLL",
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
    pub deck_pad_mode: [DeckPadMode; 4],

    // Hybrid 4-Stem Separation State ([Drums, Bass, Instruments, Vocals])
    pub deck_stem_volumes: [[f32; 4]; 4],
    pub deck_stem_muted: [[bool; 4]; 4],
    pub deck_stem_fx_send: [[bool; 4]; 4],
    pub deck_stem_view: [bool; 4],

    // Traktor Pro 4 16-Step Pattern Player State
    pub show_pattern_player: bool,
    pub pattern_enabled: bool,
    pub pattern_selected_voice: usize,
    pub pattern_steps: [[bool; 16]; 4],
    pub pattern_volume: f32,
    pub pattern_pitch: f32,
    pub pattern_decay: f32,

    // DJ.Studio Timeline Transition Editor State
    pub timeline_transition_bars: u32,
    pub timeline_preset_idx: usize, // 0: Smooth Crossfade, 1: Bass Swap, 2: Vocal Stem Bridge

    // Traktor MX2 (VID 0x17CC, PID 0x2420) Live Hardware State
    pub mx2_driver: Mx2HidDriver,
    pub mx2_hidraw_file: Option<File>,
    pub mx2_dev_node: Option<String>,
    pub mx2_model_name: Option<String>,
    pub mx2_scan_counter: u32,
}

impl Default for ReTraktorApp {
    fn default() -> Self {
        let bus = PropertyBus::new_traktor_pro4_default();
        let mut default_steps = [[false; 16]; 4];
        // Default 4x4 Kick + Offbeat Hat + Clap on 5 & 13
        for s in (0..16).step_by(4) {
            default_steps[0][s] = true;
        }
        default_steps[1][4] = true;
        default_steps[1][12] = true;
        for s in (2..16).step_by(4) {
            default_steps[2][s] = true;
        }
        default_steps[3][7] = true;
        default_steps[3][15] = true;

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
            deck_pad_mode: [
                DeckPadMode::Hotcues,
                DeckPadMode::Hotcues,
                DeckPadMode::Hotcues,
                DeckPadMode::Hotcues,
            ],
            deck_stem_volumes: [[1.0; 4]; 4],
            deck_stem_muted: [[false; 4]; 4],
            deck_stem_fx_send: [[false; 4]; 4],
            deck_stem_view: [false, false, false, false],
            show_pattern_player: false,
            pattern_enabled: false,
            pattern_selected_voice: 0,
            pattern_steps: default_steps,
            pattern_volume: 0.75,
            pattern_pitch: 0.5,
            pattern_decay: 0.5,
            timeline_transition_bars: 16,
            timeline_preset_idx: 1,
            mx2_driver: Mx2HidDriver::new(),
            mx2_hidraw_file: None,
            mx2_dev_node: None,
            mx2_model_name: None,
            mx2_scan_counter: 0,
        }
    }
}

impl eframe::App for ReTraktorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Request 60 FPS continuous repaint for smooth scrolling waveforms and VU meters
        ctx.request_repaint();

        if self.library.open_prefs_requested {
            self.library.open_prefs_requested = false;
            self.preferences.is_open = true;
            self.preferences.current_tab = PrefsTab::Controllers;
        }

        self.handle_keyboard_shortcuts(ctx);
        self.poll_mx2_hardware();
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

                // 1. Top FX Rack, Master Clock, Layout Switcher, Pattern Toggle & Mix Recorder
                self.show_top_bar(ui);

                // 1b. Optional Collapsible Traktor Pro 4 16-Step Pattern Player Strip
                if self.show_pattern_player {
                    self.show_pattern_player_bar(ui);
                }

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
                    TraktorLayout::Timeline => {
                        self.show_timeline_studio_layout(ui);
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

            state.stem_volumes = self.deck_stem_volumes;
            state.stem_muted = self.deck_stem_muted;
            state.stem_fx_send = self.deck_stem_fx_send;

            state.pattern_enabled = self.pattern_enabled;
            state.pattern_steps = self.pattern_steps;
            state.pattern_volume = self.pattern_volume;
            state.pattern_pitch = self.pattern_pitch;
            state.pattern_decay = self.pattern_decay;

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

            // Sync telemetry into PropertyBus for MX2 105-LED & 9-segment VU meter feedback
            for d in 0..4 {
                let idx = d + 1;
                self.bus.set(
                    &format!("app.traktor.decks.{idx}.play"),
                    PropValue::Bool(state.is_playing[d]),
                );
                self.bus.set(
                    &format!("app.traktor.decks.{idx}.flux.enabled"),
                    PropValue::Bool(self.deck_flux_active[d]),
                );
                self.bus.set(
                    &format!("app.traktor.mixer.channels.{idx}.cue"),
                    PropValue::Bool(self.channel_cue[d]),
                );
                self.bus.set(
                    &format!("app.traktor.mixer.channels.{idx}.fx.on"),
                    PropValue::Bool(self.channel_filter_on[d]),
                );
                self.bus.set(
                    &format!("app.traktor.mixer.channels.{idx}.level.prefader.linear.sum"),
                    PropValue::Float(state.channel_peaks[d] as f64),
                );
            }
            self.bus.set(
                "app.traktor.mixer.master.level.clip.left",
                PropValue::Bool(state.master_peak_l >= 0.98),
            );
            self.bus.set(
                "app.traktor.mixer.master.level.clip.right",
                PropValue::Bool(state.master_peak_r >= 0.98),
            );
        }
    }

    /// Polls Linux `/dev/hidraw*` for a connected Traktor MX2 (`17CC:2420`)
    /// or other Traktor Kontrol surface, decodes Input Reports `0x01`/`0x02`/`0x03`, and writes
    /// Output Report `0x80` (105 LEDs + 6 RGB Bottom-Shell LEDs per deck).
    fn poll_mx2_hardware(&mut self) {
        if self.mx2_hidraw_file.is_none() {
            self.mx2_scan_counter = self.mx2_scan_counter.wrapping_add(1);
            if self.mx2_scan_counter % 120 == 1 {
                let detected = scan_linux_ni_hidraw_devices();
                // Prefer Traktor MX2 if multiple NI devices are attached
                if let Some(dev) = detected
                    .iter()
                    .find(|d| d.descriptor.pid == TRAKTOR_MX2_PID)
                    .or_else(|| detected.first())
                {
                    let mut opts = OpenOptions::new();
                    opts.read(true).write(true);
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::OpenOptionsExt;
                        opts.custom_flags(0x0800); // O_NONBLOCK on Linux
                    }
                    if let Ok(mut file) = opts.open(&dev.dev_node) {
                        // Send NHL2 mode activation report [0xA0, 0x00, 0xFF]
                        let _ = file.write_all(&Mx2HidDriver::mode_enable_nhl_report());
                        self.mx2_dev_node = Some(dev.dev_node.clone());
                        self.mx2_model_name = Some(dev.descriptor.model.to_string());
                        self.mx2_hidraw_file = Some(file);
                    } else {
                        // Detected in sysfs even if hidraw node needs udev reload
                        self.mx2_dev_node = Some(dev.dev_node.clone());
                        self.mx2_model_name = Some(dev.descriptor.model.to_string());
                    }
                }
            }
            return;
        }

        // Non-blocking read of up to 8 queued HID input reports per frame
        let mut pending_events = Vec::new();
        let mut disconnected = false;
        if let Some(ref mut file) = self.mx2_hidraw_file {
            let mut buf = [0u8; 64];
            for _ in 0..8 {
                match file.read(&mut buf) {
                    Ok(n) if n > 0 => {
                        let evs = self.mx2_driver.decode_input_report(&buf[..n]);
                        pending_events.extend(evs);
                    }
                    Ok(_) => break,
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(_) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }

        if disconnected {
            self.mx2_hidraw_file = None;
            self.mx2_dev_node = None;
            return;
        }

        for ev in pending_events {
            self.apply_mx2_surface_event(ev);
        }

        // Compute Bottom Shell 6-LED state (`bottom.leds.1..6`) for Deck A and Deck B
        let left_shell = if self.deck_loop_active[0] {
            Mx2BottomShellState::ActiveLoopGreen
        } else {
            Mx2BottomShellState::DefaultGlow
        };
        let right_shell = if self.deck_loop_active[1] {
            Mx2BottomShellState::ActiveLoopGreen
        } else {
            Mx2BottomShellState::DefaultGlow
        };

        let led_report = self
            .mx2_driver
            .encode_led_report(&self.bus, left_shell, right_shell);
        if let Some(ref mut file) = self.mx2_hidraw_file {
            let _ = file.write_all(&led_report);
        }
    }

    pub fn apply_mx2_surface_event(&mut self, ev: SurfaceEvent) {
        let neural_eq = self.preferences.neural_mix_eq_mode;
        match ev {
            SurfaceEvent::Fader { control, value } => {
                let v = value as f32;
                match control.as_str() {
                    "surface.mixer.xfader" => self.crossfader_pos = v,
                    "surface.mixer.channels.1.volume" => self.channel_volume[0] = v,
                    "surface.mixer.channels.2.volume" => self.channel_volume[1] = v,
                    "surface.mixer.channels.1.gain" => self.channel_gain[0] = v,
                    "surface.mixer.channels.2.gain" => self.channel_gain[1] = v,
                    "surface.mixer.channels.1.eq.high" => {
                        if neural_eq {
                            self.deck_stem_volumes[0][3] = v; // Vocals
                        } else {
                            self.channel_eq_hi[0] = v;
                        }
                    }
                    "surface.mixer.channels.1.eq.mid" => {
                        if neural_eq {
                            self.deck_stem_volumes[0][2] = v; // Instruments
                        } else {
                            self.channel_eq_mid[0] = v;
                        }
                    }
                    "surface.mixer.channels.1.eq.low" => {
                        if neural_eq {
                            self.deck_stem_volumes[0][0] = v; // Drums
                            self.deck_stem_volumes[0][1] = v; // Bass
                        } else {
                            self.channel_eq_low[0] = v;
                        }
                    }
                    "surface.mixer.channels.2.eq.high" => {
                        if neural_eq {
                            self.deck_stem_volumes[1][3] = v; // Vocals
                        } else {
                            self.channel_eq_hi[1] = v;
                        }
                    }
                    "surface.mixer.channels.2.eq.mid" => {
                        if neural_eq {
                            self.deck_stem_volumes[1][2] = v; // Instruments
                        } else {
                            self.channel_eq_mid[1] = v;
                        }
                    }
                    "surface.mixer.channels.2.eq.low" => {
                        if neural_eq {
                            self.deck_stem_volumes[1][0] = v; // Drums
                            self.deck_stem_volumes[1][1] = v; // Bass
                        } else {
                            self.channel_eq_low[1] = v;
                        }
                    }
                    "surface.mixer.channels.1.channel_fx.amount" => self.channel_filter[0] = v,
                    "surface.mixer.channels.2.channel_fx.amount" => self.channel_filter[1] = v,
                    "surface.mixer.main.volume" => self.master_volume = v,
                    "surface.left.pitch.fader" => self.deck_pitch_fader[0] = (v * 16.0) - 8.0,
                    "surface.right.pitch.fader" => self.deck_pitch_fader[1] = (v * 16.0) - 8.0,
                    "surface.left.fx.knobs.1" => self.fx1_dry_wet = v,
                    "surface.left.fx.knobs.2" => self.fx1_param[0] = v,
                    "surface.left.fx.knobs.3" => self.fx1_param[1] = v,
                    "surface.left.fx.knobs.4" => self.fx1_param[2] = v,
                    "surface.right.fx.knobs.1" => self.fx2_dry_wet = v,
                    "surface.right.fx.knobs.2" => self.fx2_param[0] = v,
                    "surface.right.fx.knobs.3" => self.fx2_param[1] = v,
                    "surface.right.fx.knobs.4" => self.fx2_param[2] = v,
                    _ => {}
                }
            }
            SurfaceEvent::Button {
                control,
                pressed: true,
            } => match control.as_str() {
                "surface.left.play" => self.toggle_play(1),
                "surface.right.play" => self.toggle_play(2),
                "surface.left.cue" => self.jump_to_cue(1, 1),
                "surface.right.cue" => self.jump_to_cue(2, 1),
                "surface.left.sync" => self.sync_deck(1),
                "surface.right.sync" => self.sync_deck(2),
                "surface.left.flux" => self.deck_flux_active[0] = !self.deck_flux_active[0],
                "surface.right.flux" => self.deck_flux_active[1] = !self.deck_flux_active[1],
                "surface.left.loop_size.push" => self.toggle_loop(1),
                "surface.right.loop_size.push" => self.toggle_loop(2),
                "surface.left.pad_mode.hotcues" => self.deck_pad_mode[0] = DeckPadMode::Hotcues,
                "surface.left.pad_mode.stems" => {
                    self.deck_pad_mode[0] = DeckPadMode::Stems;
                    self.deck_stem_view[0] = true;
                }
                "surface.left.pad_mode.pattern" => {
                    self.deck_pad_mode[0] = DeckPadMode::Pattern;
                    self.show_pattern_player = true;
                }
                "surface.left.pad_mode.flux_loop" => self.deck_pad_mode[0] = DeckPadMode::FluxLoop,
                "surface.right.pad_mode.hotcues" => self.deck_pad_mode[1] = DeckPadMode::Hotcues,
                "surface.right.pad_mode.stems" => {
                    self.deck_pad_mode[1] = DeckPadMode::Stems;
                    self.deck_stem_view[1] = true;
                }
                "surface.right.pad_mode.pattern" => {
                    self.deck_pad_mode[1] = DeckPadMode::Pattern;
                    self.show_pattern_player = true;
                }
                "surface.right.pad_mode.flux_loop" => self.deck_pad_mode[1] = DeckPadMode::FluxLoop,
                "surface.left.browse.view" | "surface.right.browse.view" => {
                    self.layout = if self.layout == TraktorLayout::Browser {
                        TraktorLayout::Parallel
                    } else {
                        TraktorLayout::Browser
                    };
                }
                "surface.left.fx.buttons.1" => self.fx1_on = !self.fx1_on,
                "surface.right.fx.buttons.1" => self.fx2_on = !self.fx2_on,
                "surface.mixer.channels.1.cue" => self.channel_cue[0] = !self.channel_cue[0],
                "surface.mixer.channels.2.cue" => self.channel_cue[1] = !self.channel_cue[1],
                "surface.mixer.channels.1.channel_fx.on" => {
                    self.channel_filter_on[0] = !self.channel_filter_on[0];
                }
                "surface.mixer.channels.2.channel_fx.on" => {
                    self.channel_filter_on[1] = !self.channel_filter_on[1];
                }
                "surface.mixer.channel_fx.filter" => {
                    self.channel_fx_kind[0] = MixerFxKind::Filter;
                    self.channel_fx_kind[1] = MixerFxKind::Filter;
                }
                "surface.mixer.channel_fx.fx1" => {
                    self.channel_fx_kind[0] = MixerFxKind::Reverb;
                    self.channel_fx_kind[1] = MixerFxKind::Reverb;
                }
                "surface.mixer.channel_fx.fx2" => {
                    self.channel_fx_kind[0] = MixerFxKind::DualDelay;
                    self.channel_fx_kind[1] = MixerFxKind::DualDelay;
                }
                "surface.mixer.channel_fx.fx3" => {
                    self.channel_fx_kind[0] = MixerFxKind::Noise;
                    self.channel_fx_kind[1] = MixerFxKind::Noise;
                }
                "surface.mixer.channel_fx.fx4" => {
                    self.channel_fx_kind[0] = MixerFxKind::TimeGater;
                    self.channel_fx_kind[1] = MixerFxKind::TimeGater;
                }
                s if s.starts_with("surface.left.pads.") => {
                    if let Ok(pad) = s.trim_start_matches("surface.left.pads.").parse::<u8>() {
                        self.trigger_performance_pad(1, pad);
                    }
                }
                s if s.starts_with("surface.right.pads.") => {
                    if let Ok(pad) = s.trim_start_matches("surface.right.pads.").parse::<u8>() {
                        self.trigger_performance_pad(2, pad);
                    }
                }
                _ => {}
            },
            SurfaceEvent::Encoder { control, delta } => match control.as_str() {
                "surface.left.jogwheel.rotation" => {
                    if let Ok(mut state) = self.audio.shared.lock() {
                        let nudge_sec = (delta as f64 / TRAKTOR_MX2_JOG_TICKS_PER_REV as f64) * 1.8;
                        state.playhead_sec[0] = (state.playhead_sec[0] + nudge_sec).max(0.0);
                    }
                }
                "surface.right.jogwheel.rotation" => {
                    if let Ok(mut state) = self.audio.shared.lock() {
                        let nudge_sec = (delta as f64 / TRAKTOR_MX2_JOG_TICKS_PER_REV as f64) * 1.8;
                        state.playhead_sec[1] = (state.playhead_sec[1] + nudge_sec).max(0.0);
                    }
                }
                "surface.left.loop_size" => {
                    if delta > 0 {
                        self.deck_loop_size_beats[0] = (self.deck_loop_size_beats[0] * 2).min(32);
                    } else if delta < 0 {
                        self.deck_loop_size_beats[0] = (self.deck_loop_size_beats[0] / 2).max(1);
                    }
                }
                "surface.right.loop_size" => {
                    if delta > 0 {
                        self.deck_loop_size_beats[1] = (self.deck_loop_size_beats[1] * 2).min(32);
                    } else if delta < 0 {
                        self.deck_loop_size_beats[1] = (self.deck_loop_size_beats[1] / 2).max(1);
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn trigger_performance_pad(&mut self, deck_idx: usize, pad_num: u8) {
        let d = (deck_idx - 1).min(3);
        let pad_idx = (pad_num.saturating_sub(1) as usize).min(7);
        match self.deck_pad_mode[d] {
            DeckPadMode::Hotcues => {
                self.jump_to_cue(deck_idx, pad_num);
            }
            DeckPadMode::Stems => {
                if pad_idx < 4 {
                    self.deck_stem_muted[d][pad_idx] = !self.deck_stem_muted[d][pad_idx];
                } else {
                    let s = pad_idx - 4;
                    self.deck_stem_fx_send[d][s] = !self.deck_stem_fx_send[d][s];
                }
            }
            DeckPadMode::Pattern => {
                self.pattern_enabled = true;
                self.show_pattern_player = true;
                let step_offset = if deck_idx == 2 { 8 } else { 0 };
                let step = (step_offset + pad_idx).min(15);
                let v = self.pattern_selected_voice.min(3);
                self.pattern_steps[v][step] = !self.pattern_steps[v][step];
            }
            DeckPadMode::FluxLoop => {
                let sizes = [1, 1, 2, 4, 8, 16, 32, 32];
                self.deck_loop_size_beats[d] = sizes[pad_idx];
                self.deck_flux_active[d] = true;
                self.toggle_loop(deck_idx);
            }
        }
    }

    fn handle_keyboard_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Comma)) {
            self.preferences.is_open = !self.preferences.is_open;
        }

        // Layout shortcuts F1..F6
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
        if ctx.input(|i| i.key_pressed(Key::F6)) {
            self.layout = TraktorLayout::Timeline;
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

                    // 3. Layout Switcher Pills (Parallel | Extended | Essential | 4-Deck | Timeline | Browser)
                    ui.label(
                        RichText::new("LAYOUT")
                            .color(TraktorTheme::TEXT_SECONDARY)
                            .font(egui::FontId::proportional(10.0)),
                    );
                    let layouts = [
                        TraktorLayout::Parallel,
                        TraktorLayout::Extended,
                        TraktorLayout::Essential,
                        TraktorLayout::FourDeck,
                        TraktorLayout::Timeline,
                        TraktorLayout::Browser,
                    ];
                    for lay in layouts {
                        let active = self.layout == lay;
                        let btn = ui.add(
                            egui::Button::new(
                                RichText::new(lay.label())
                                    .color(if active {
                                        Color32::WHITE
                                    } else {
                                        TraktorTheme::TEXT_SECONDARY
                                    })
                                    .font(egui::FontId::proportional(10.5))
                                    .strong(),
                            )
                            .fill(if active {
                                TraktorTheme::ROW_SELECTED
                            } else {
                                Color32::from_rgb(48, 51, 57)
                            })
                            .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                            .min_size(Vec2::new(54.0, 20.0)),
                        );
                        if btn.clicked() {
                            self.layout = lay;
                            self.preferences.four_deck_mode = lay == TraktorLayout::FourDeck;
                        }
                    }

                    ui.separator();

                    // 3b. Traktor Pro 4 Pattern Player Toggle
                    let pat_btn = ui.add(
                        egui::Button::new(
                            RichText::new(if self.pattern_enabled {
                                "● PATTERN"
                            } else {
                                "PATTERN"
                            })
                            .color(if self.show_pattern_player || self.pattern_enabled {
                                Color32::BLACK
                            } else {
                                TraktorTheme::TEXT_SECONDARY
                            })
                            .font(egui::FontId::monospace(9.5))
                            .strong(),
                        )
                        .fill(if self.show_pattern_player || self.pattern_enabled {
                            TraktorTheme::DECK_B
                        } else {
                            Color32::from_rgb(48, 51, 57)
                        })
                        .min_size(Vec2::new(66.0, 20.0)),
                    );
                    if pat_btn
                        .on_hover_text("Toggle Traktor Pro 4 16-Step Pattern Player (Drum Machine)")
                        .clicked()
                    {
                        self.show_pattern_player = !self.show_pattern_player;
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

                    // 5. Right-aligned Master Output, Mix Recorder, Meter & Preferences Gear
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("⚙").font(egui::FontId::proportional(15.0)),
                                )
                                .min_size(Vec2::new(26.0, 22.0)),
                            )
                            .on_hover_text("Preferences: Audio, Stems & Traktor MX2 Settings (Ctrl+,)")
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

                        let (peak_l, peak_r, is_rec, rec_sec) =
                            if let Ok(state) = self.audio.shared.lock() {
                                let secs =
                                    state.recorded_samples.len() as f64 / (48_000.0 * 2.0);
                                (
                                    state.master_peak_l,
                                    state.master_peak_r,
                                    state.is_recording,
                                    secs,
                                )
                            } else {
                                (0.0, 0.0, false, 0.0)
                            };
                        vu_meter(ui, peak_l, peak_r, 30.0);

                        // Master Mix Recorder Button (`● REC`)
                        let rec_m = (rec_sec / 60.0).floor() as u32;
                        let rec_s = (rec_sec % 60.0).floor() as u32;
                        let rec_label = if is_rec {
                            format!("■ REC {rec_m:02}:{rec_s:02}")
                        } else {
                            "● REC".to_string()
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(rec_label)
                                        .color(if is_rec {
                                            Color32::WHITE
                                        } else {
                                            Color32::from_rgb(220, 95, 90)
                                        })
                                        .font(egui::FontId::monospace(9.5))
                                        .strong(),
                                )
                                .fill(if is_rec {
                                    Color32::from_rgb(175, 42, 42)
                                } else {
                                    Color32::from_rgb(48, 51, 57)
                                }),
                            )
                            .on_hover_text(
                                "Record live Master Mix to ~/Music/ReTraktor_Master_Mix.wav (16-bit 48kHz Stereo WAV)",
                            )
                            .clicked()
                        {
                            let mut samples_to_save: Option<Vec<i16>> = None;
                            if let Ok(mut state) = self.audio.shared.lock() {
                                if state.is_recording {
                                    state.is_recording = false;
                                    samples_to_save =
                                        Some(std::mem::take(&mut state.recorded_samples));
                                } else {
                                    state.recorded_samples.clear();
                                    state.is_recording = true;
                                }
                            }
                            if let Some(samples) = samples_to_save {
                                let home = env::var("HOME")
                                    .map(PathBuf::from)
                                    .unwrap_or_else(|_| PathBuf::from("."));
                                let music_dir = home.join("Music");
                                let out_dir = if music_dir.is_dir() { music_dir } else { home };
                                let out_path = out_dir.join("ReTraktor_Master_Mix.wav");
                                match AudioSystem::write_stereo_wav_file(&out_path, &samples, 48_000)
                                {
                                    Ok(dur_s) => {
                                        self.library.status_message = format!(
                                            "✓ Saved Master Mix recording ({dur_s:.1}s) to {}",
                                            out_path.display()
                                        );
                                    }
                                    Err(e) => {
                                        self.library.status_message =
                                            format!("⚠ Failed to save WAV recording: {e}");
                                    }
                                }
                            }
                        }

                        let (mx2_label, mx2_color) = if let Some(ref node) = self.mx2_dev_node {
                            (
                                format!("● MX2 ({node})"),
                                TraktorTheme::PLAY_ACTIVE,
                            )
                        } else {
                            (
                                "MX2 READY".to_string(),
                                TraktorTheme::TEXT_SECONDARY,
                            )
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(mx2_label)
                                        .color(mx2_color)
                                        .font(egui::FontId::monospace(9.5))
                                        .strong(),
                                )
                                .frame(false),
                            )
                            .on_hover_text(
                                "Click to open Traktor MX2 [17CC:2420] & Controller Settings in Preferences",
                            )
                            .clicked()
                        {
                            self.preferences.is_open = true;
                            self.preferences.current_tab = PrefsTab::Controllers;
                        }

                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("● PipeWire 48kHz")
                                        .color(TraktorTheme::PLAY_ACTIVE)
                                        .font(egui::FontId::monospace(9.5)),
                                )
                                .frame(false),
                            )
                            .on_hover_text("Click to open Audio & UAC2 Routing Settings in Preferences")
                            .clicked()
                        {
                            self.preferences.is_open = true;
                            self.preferences.current_tab = PrefsTab::AudioSetup;
                        }
                    });
                });
            });
    }

    /// Traktor Pro 4 16-Step Pattern Player (Master-Clock-Synced Drum Machine)
    fn show_pattern_player_bar(&mut self, ui: &mut Ui) {
        let cur_step = if let Ok(state) = self.audio.shared.lock() {
            state.pattern_current_step
        } else {
            0
        };

        egui::Frame::none()
            .fill(Color32::from_rgb(20, 22, 29))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(255, 195, 0)))
            .inner_margin(egui::Margin::symmetric(8.0, 4.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let on_btn = ui.add(
                        egui::Button::new(
                            RichText::new(if self.pattern_enabled {
                                "⏸ PATTERN ON"
                            } else {
                                "▶ PATTERN OFF"
                            })
                            .color(if self.pattern_enabled {
                                Color32::BLACK
                            } else {
                                Color32::from_rgb(255, 195, 0)
                            })
                            .font(egui::FontId::monospace(10.0))
                            .strong(),
                        )
                        .fill(if self.pattern_enabled {
                            Color32::from_rgb(255, 195, 0)
                        } else {
                            Color32::from_rgb(35, 36, 44)
                        }),
                    );
                    if on_btn.clicked() {
                        self.pattern_enabled = !self.pattern_enabled;
                    }

                    ui.separator();

                    // 4 Percussion Kit Voices
                    let voices = [
                        (0usize, "808 KICK", TraktorTheme::DECK_A),
                        (1usize, "909 CLAP", TraktorTheme::DECK_B),
                        (2usize, "909 HAT", Color32::from_rgb(205, 175, 75)),
                        (3usize, "RAVE PERC", Color32::from_rgb(155, 115, 205)),
                    ];
                    for (v_idx, v_lbl, v_col) in voices {
                        let sel = self.pattern_selected_voice == v_idx;
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(v_lbl)
                                        .color(if sel { Color32::BLACK } else { v_col })
                                        .font(egui::FontId::monospace(9.5))
                                        .strong(),
                                )
                                .fill(if sel {
                                    v_col
                                } else {
                                    Color32::from_rgb(28, 30, 38)
                                }),
                            )
                            .clicked()
                        {
                            self.pattern_selected_voice = v_idx;
                        }
                    }

                    ui.separator();

                    // 16-Step Sequencer Buttons for Selected Voice
                    let v = self.pattern_selected_voice.min(3);
                    let voice_col = [
                        Color32::from_rgb(0, 215, 255),
                        Color32::from_rgb(255, 145, 0),
                        Color32::from_rgb(255, 215, 0),
                        Color32::from_rgb(195, 95, 255),
                    ][v];

                    for step in 0..16 {
                        let active = self.pattern_steps[v][step];
                        let is_playhead = self.pattern_enabled && cur_step == step;
                        let fill = if active {
                            voice_col
                        } else if step % 4 == 0 {
                            Color32::from_rgb(40, 44, 54)
                        } else {
                            Color32::from_rgb(26, 28, 35)
                        };
                        let border = if is_playhead {
                            Stroke::new(1.8_f32, Color32::WHITE)
                        } else {
                            Stroke::new(0.6_f32, TraktorTheme::BORDER_COLOR)
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{}", step + 1))
                                        .color(if active {
                                            Color32::BLACK
                                        } else {
                                            TraktorTheme::TEXT_SECONDARY
                                        })
                                        .font(egui::FontId::monospace(9.0))
                                        .strong(),
                                )
                                .fill(fill)
                                .stroke(border)
                                .min_size(Vec2::new(21.0, 20.0)),
                            )
                            .clicked()
                        {
                            self.pattern_steps[v][step] = !self.pattern_steps[v][step];
                        }
                    }

                    ui.separator();

                    if ui.small_button("4x4").on_hover_text("Load 4x4 Techno Pattern").clicked() {
                        self.pattern_steps[v] = [false; 16];
                        for s in (0..16).step_by(4) {
                            self.pattern_steps[v][s] = true;
                        }
                    }
                    if ui.small_button("Roll").on_hover_text("Load 16th Snare/Hat Build Roll").clicked() {
                        self.pattern_steps[v] = [true; 16];
                    }
                    if ui.small_button("Clr").on_hover_text("Clear Selected Voice Steps").clicked() {
                        self.pattern_steps[v] = [false; 16];
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.pattern_volume, 0.0..=1.0)
                                .text("LVL")
                                .show_value(false),
                        );
                    });
                });
            });
    }

    /// LAYOUT 6: DJ.STUDIO-STYLE TIMELINE TRANSITION EDITOR (`F6`)
    fn show_timeline_studio_layout(&mut self, ui: &mut Ui) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::DECK_A))
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("TIMELINE TRANSITION & STEM AUTOMATION")
                            .color(TraktorTheme::DECK_A)
                            .font(egui::FontId::monospace(11.5))
                            .strong(),
                    );
                    ui.separator();
                    ui.label("Transition Window:");
                    for bars in [8u32, 16, 32] {
                        if ui
                            .selectable_label(
                                self.timeline_transition_bars == bars,
                                format!("{bars} Bars"),
                            )
                            .clicked()
                        {
                            self.timeline_transition_bars = bars;
                        }
                    }
                    ui.separator();
                    ui.label("Automation Curve:");
                    for (idx, name) in [
                        (0usize, "Smooth Crossfade"),
                        (1usize, "Instant Bass-Swap"),
                        (2usize, "Vocal Stem Bridge"),
                    ] {
                        if ui
                            .selectable_label(self.timeline_preset_idx == idx, name)
                            .clicked()
                        {
                            self.timeline_preset_idx = idx;
                            // Apply stem automation preset to Decks A & B
                            match idx {
                                0 => {
                                    self.deck_stem_muted[0] = [false; 4];
                                    self.deck_stem_muted[1] = [false; 4];
                                }
                                1 => {
                                    // Outgoing Deck A drops Bass; Incoming Deck B brings Bass
                                    self.deck_stem_muted[0][1] = true;
                                    self.deck_stem_muted[1][1] = false;
                                }
                                _ => {
                                    // Outgoing Deck A isolates Vocals over Incoming Deck B Instrumental
                                    self.deck_stem_muted[0] = [true, true, true, false];
                                    self.deck_stem_muted[1] = [false, false, false, true];
                                }
                            }
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("▶ Audition Transition")
                                        .color(Color32::BLACK)
                                        .strong(),
                                )
                                .fill(TraktorTheme::PLAY_ACTIVE),
                            )
                            .clicked()
                        {
                            self.sync_deck(2);
                            if let Ok(mut state) = self.audio.shared.lock() {
                                state.is_playing[0] = true;
                                state.is_playing[1] = true;
                            }
                        }
                    });
                });

                ui.add_space(4.0);

                // Render Deck A & Deck B Parallel Stem Timelines + Crossfade Envelope Canvas
                self.show_parallel_waveform_row(ui, 1, TraktorTheme::DECK_A, 72.0);
                ui.add_space(3.0);
                self.show_parallel_waveform_row(ui, 2, TraktorTheme::DECK_B, 72.0);
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
                Vec2::new(deck_w, 192.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(deck_w);
                    self.show_single_deck(ui, 1, TraktorTheme::DECK_A, false, 0.0);
                },
            );

            ui.allocate_ui_with_layout(
                Vec2::new(mixer_w, 192.0),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.set_width(mixer_w);
                    self.show_mixer_section(ui, false);
                },
            );

            ui.allocate_ui_with_layout(
                Vec2::new(deck_w, 192.0),
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

        let (title, artist, bpm, key, duration_sec, elapsed_sec, peaks, stem_peaks) = {
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
                        t.stem_peaks.clone(),
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
                    Vec::new(),
                )
            }
        };

        // Compact info bar + Quick Stems / Acapella / Instrumental controls right above the parallel waveform
        ui.horizontal(|ui| {
            egui::Frame::none()
                .fill(Color32::from_rgb(52, 55, 62))
                .stroke(Stroke::new(0.8_f32, accent_color))
                .rounding(2.0)
                .inner_margin(egui::Margin::symmetric(6.0, 1.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("DECK {deck_letter}"))
                            .color(accent_color)
                            .font(egui::FontId::monospace(10.0))
                            .strong(),
                    );
                });
            ui.label(
                RichText::new(&title)
                    .color(TraktorTheme::TEXT_PRIMARY)
                    .font(egui::FontId::proportional(12.0))
                    .strong(),
            );
            ui.label(
                RichText::new(format!("— {artist}"))
                    .color(TraktorTheme::TEXT_SECONDARY)
                    .font(egui::FontId::proportional(11.0)),
            );

            ui.separator();

            // 4-Lane Stem Waveform Toggle (`STEMS`)
            let stem_view_on = self.deck_stem_view[d];
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("STEMS")
                            .color(if stem_view_on {
                                Color32::WHITE
                            } else {
                                TraktorTheme::TEXT_SECONDARY
                            })
                            .font(egui::FontId::monospace(9.0))
                            .strong(),
                    )
                    .fill(if stem_view_on {
                        TraktorTheme::ROW_SELECTED
                    } else {
                        Color32::from_rgb(48, 51, 57)
                    }),
                )
                .on_hover_text("Toggle Traktor Pro 4 4-Lane Stem Waveform View (Drums / Bass / Other / Vocals)")
                .clicked()
            {
                self.deck_stem_view[d] = !self.deck_stem_view[d];
            }

            // 1-Click Acapella & Instrumental Isolator Buttons
            let is_acapella = self.deck_stem_muted[d] == [true, true, true, false];
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("ACAPELLA")
                            .color(if is_acapella {
                                Color32::WHITE
                            } else {
                                TraktorTheme::TEXT_SECONDARY
                            })
                            .font(egui::FontId::monospace(9.0))
                            .strong(),
                    )
                    .fill(if is_acapella {
                        TraktorTheme::ROW_SELECTED
                    } else {
                        Color32::from_rgb(48, 51, 57)
                    }),
                )
                .on_hover_text("Isolate Vocals Stem (Mute Drums, Bass & Other)")
                .clicked()
            {
                if is_acapella {
                    self.deck_stem_muted[d] = [false; 4];
                } else {
                    self.deck_stem_muted[d] = [true, true, true, false];
                }
            }

            let is_inst = self.deck_stem_muted[d] == [false, false, false, true];
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("INST")
                            .color(if is_inst {
                                Color32::WHITE
                            } else {
                                TraktorTheme::TEXT_SECONDARY
                            })
                            .font(egui::FontId::monospace(9.0))
                            .strong(),
                    )
                    .fill(if is_inst {
                        TraktorTheme::ROW_SELECTED
                    } else {
                        Color32::from_rgb(48, 51, 57)
                    }),
                )
                .on_hover_text("Isolate Instrumental (Mute Vocals Stem)")
                .clicked()
            {
                if is_inst {
                    self.deck_stem_muted[d] = [false; 4];
                } else {
                    self.deck_stem_muted[d] = [false, false, false, true];
                }
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let rem_sec = (duration_sec - elapsed_sec).max(0.0);
                let rem_m = (rem_sec / 60.0).floor() as u32;
                let rem_s = (rem_sec % 60.0).floor() as u32;
                ui.label(
                    RichText::new(format!("-{rem_m:02}:{rem_s:02}"))
                        .color(TraktorTheme::TEXT_PRIMARY)
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("KEY {key}"))
                        .color(TraktorTheme::TEXT_SECONDARY)
                        .font(egui::FontId::monospace(10.5)),
                );
                let eff_bpm = bpm * (1.0 + self.deck_pitch_fader[d] as f64 / 100.0);
                ui.label(
                    RichText::new(format!("{eff_bpm:.2} BPM"))
                        .color(TraktorTheme::TEXT_PRIMARY)
                        .font(egui::FontId::monospace(11.5))
                        .strong(),
                );
            });
        });

        let cues = self.deck_hotcues[d].clone();
        let loop_len = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
        let use_stem_waves = self.deck_stem_view[d];
        let wave_resp = if use_stem_waves {
            scrolling_stem_waveforms(
                ui,
                elapsed_sec,
                bpm,
                &cues,
                &stem_peaks,
                &self.deck_stem_volumes[d],
                &mut self.deck_stem_muted[d],
                self.deck_loop_active[d],
                self.deck_loop_start_sec[d],
                loop_len,
                Some((deck_letter, accent_color)),
                height,
            )
        } else {
            scrolling_waveform(
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
            )
        };
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

        let (title, artist, bpm, key, duration_sec, elapsed_sec, _is_playing, peaks, stem_peaks) = {
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
                        t.stem_peaks.clone(),
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
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(8.0, 5.0))
            .show(ui, |ui| {
                // 1. Authentic Traktor Pro 4 Deck Header Strip:
                //    Left: Dark album-art record square + Title / Artist
                //    Right: Remaining Time + BPM + Pitch% + Key + Large Deck Letter (A / B)
                ui.horizontal(|ui| {
                    // Mini Album Art / Vinyl Record Icon on Left
                    let (art_rect, _) =
                        ui.allocate_exact_size(Vec2::new(28.0, 28.0), egui::Sense::hover());
                    ui.painter()
                        .rect_filled(art_rect, 2.0, Color32::from_rgb(24, 26, 30));
                    ui.painter().rect_stroke(
                        art_rect,
                        2.0,
                        Stroke::new(0.8_f32, TraktorTheme::BORDER_COLOR),
                    );
                    ui.painter()
                        .circle_stroke(art_rect.center(), 9.0, Stroke::new(1.2_f32, accent_color));
                    ui.painter()
                        .circle_filled(art_rect.center(), 2.5, accent_color);

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
                        // Large Traktor Deck Letter (A / B) on the far right
                        ui.label(
                            RichText::new(deck_letter)
                                .color(accent_color)
                                .font(egui::FontId::proportional(20.0))
                                .strong(),
                        );

                        ui.add_space(4.0);

                        let rem_sec = (duration_sec - elapsed_sec).max(0.0);
                        let rem_m = (rem_sec / 60.0).floor() as u32;
                        let rem_s = (rem_sec % 60.0).floor() as u32;
                        ui.label(
                            RichText::new(format!("-{rem_m:02}:{rem_s:02}"))
                                .color(TraktorTheme::TEXT_PRIMARY)
                                .font(egui::FontId::monospace(12.5))
                                .strong(),
                        );

                        let key_col = if is_harmonic {
                            TraktorTheme::PLAY_ACTIVE
                        } else {
                            TraktorTheme::TEXT_SECONDARY
                        };
                        ui.label(
                            RichText::new(&key)
                                .color(key_col)
                                .font(egui::FontId::monospace(10.5))
                                .strong(),
                        );

                        let pitch_pct = self.deck_pitch_fader[d];
                        let effective_bpm = bpm * (1.0 + pitch_pct as f64 / 100.0);
                        ui.label(
                            RichText::new(format!("{effective_bpm:.2} ({pitch_pct:+.1}%)"))
                                .color(TraktorTheme::TEXT_SECONDARY)
                                .font(egui::FontId::monospace(10.5)),
                        );
                    });
                });

                // 1b. Traktor Sub-Header Bar: [SYNC] [MASTER] | [STEMS] [DRUMS] [BASS] [OTHER] [VOCALS] | [CUE|STEM|PAT|ROLL]
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("SYNC")
                                    .color(TraktorTheme::TEXT_PRIMARY)
                                    .font(egui::FontId::monospace(8.5))
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(52, 55, 62))
                            .min_size(Vec2::new(36.0, 16.0)),
                        )
                        .clicked()
                    {
                        self.sync_deck(deck_idx);
                    }

                    let is_master = (self.master_tempo - bpm).abs() < 0.05;
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("MASTER")
                                    .color(if is_master {
                                        accent_color
                                    } else {
                                        TraktorTheme::TEXT_SECONDARY
                                    })
                                    .font(egui::FontId::monospace(8.5))
                                    .strong(),
                            )
                            .fill(Color32::from_rgb(46, 49, 55))
                            .min_size(Vec2::new(44.0, 16.0)),
                        )
                        .clicked()
                    {
                        self.master_tempo = bpm;
                    }

                    let stem_view_on = self.deck_stem_view[d];
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("STEMS")
                                    .color(if stem_view_on {
                                        Color32::WHITE
                                    } else {
                                        TraktorTheme::TEXT_SECONDARY
                                    })
                                    .font(egui::FontId::monospace(8.5))
                                    .strong(),
                            )
                            .fill(if stem_view_on {
                                TraktorTheme::ROW_SELECTED
                            } else {
                                Color32::from_rgb(46, 49, 55)
                            })
                            .min_size(Vec2::new(40.0, 16.0)),
                        )
                        .on_hover_text("Toggle Traktor Pro 4 4-Stem Lane View (Drums / Bass / Other / Vocals)")
                        .clicked()
                    {
                        self.deck_stem_view[d] = !self.deck_stem_view[d];
                    }

                    ui.separator();

                    // Subtle matte Stem Mute Pills
                    let stem_defs = [
                        (0usize, "DRUM", Color32::from_rgb(84, 164, 214)),
                        (1usize, "BASS", Color32::from_rgb(208, 92, 62)),
                        (2usize, "OTHER", Color32::from_rgb(96, 172, 132)),
                        (3usize, "VOCAL", Color32::from_rgb(214, 148, 56)),
                    ];
                    for (s_idx, s_lbl, s_col) in stem_defs {
                        let muted = self.deck_stem_muted[d][s_idx];
                        let resp = ui
                            .add(
                                egui::Button::new(
                                    RichText::new(s_lbl)
                                        .color(if muted {
                                            TraktorTheme::TEXT_MUTED
                                        } else {
                                            Color32::WHITE
                                        })
                                        .font(egui::FontId::monospace(8.0))
                                        .strong(),
                                )
                                .fill(if muted {
                                    Color32::from_rgb(35, 37, 42)
                                } else {
                                    s_col.gamma_multiply(0.55)
                                })
                                .stroke(Stroke::new(
                                    0.6_f32,
                                    if muted {
                                        TraktorTheme::BORDER_COLOR
                                    } else {
                                        s_col
                                    },
                                ))
                                .min_size(Vec2::new(36.0, 16.0)),
                            )
                            .on_hover_text("Left-click: Mute/Unmute Stem | Right-click: Send Stem to FX");
                        if resp.clicked() {
                            self.deck_stem_muted[d][s_idx] = !self.deck_stem_muted[d][s_idx];
                        }
                        if resp.secondary_clicked() {
                            self.deck_stem_fx_send[d][s_idx] = !self.deck_stem_fx_send[d][s_idx];
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for mode in [
                            DeckPadMode::FluxLoop,
                            DeckPadMode::Pattern,
                            DeckPadMode::Stems,
                            DeckPadMode::Hotcues,
                        ] {
                            let active = self.deck_pad_mode[d] == mode;
                            let short_lbl = match mode {
                                DeckPadMode::Hotcues => "CUE",
                                DeckPadMode::Stems => "STEM",
                                DeckPadMode::Pattern => "PAT",
                                DeckPadMode::FluxLoop => "ROLL",
                            };
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(short_lbl)
                                            .color(if active {
                                                Color32::WHITE
                                            } else {
                                                TraktorTheme::TEXT_SECONDARY
                                            })
                                            .font(egui::FontId::monospace(8.0))
                                            .strong(),
                                    )
                                    .fill(if active {
                                        TraktorTheme::ROW_SELECTED
                                    } else {
                                        Color32::from_rgb(44, 47, 52)
                                    })
                                    .min_size(Vec2::new(26.0, 16.0)),
                                )
                                .on_hover_text(format!("MX2 Performance Pad Mode: {}", mode.label()))
                                .clicked()
                            {
                                self.deck_pad_mode[d] = mode;
                                if mode == DeckPadMode::Pattern {
                                    self.show_pattern_player = true;
                                }
                            }
                        }
                    });
                });

                // 2. Optional Scrolling Frequency / 4-Stem Waveform (in Extended / 4-Deck views)
                let cues = self.deck_hotcues[d].clone();
                let loop_len_sec = self.deck_loop_size_beats[d] as f64 * (60.0 / bpm.max(30.0));
                if include_scrolling_waveform && waveform_height > 10.0 {
                    ui.add_space(2.0);
                    let use_stem_waves = self.deck_stem_view[d];
                    let wave_resp = if use_stem_waves {
                        scrolling_stem_waveforms(
                            ui,
                            elapsed_sec,
                            bpm,
                            &cues,
                            &stem_peaks,
                            &self.deck_stem_volumes[d],
                            &mut self.deck_stem_muted[d],
                            self.deck_loop_active[d],
                            self.deck_loop_start_sec[d],
                            loop_len_sec,
                            None,
                            waveform_height,
                        )
                    } else {
                        scrolling_waveform(
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
                        )
                    };
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

                // 4. Authentic Traktor Pro 4 Transport & Loop Selector Row:
                //    Left: [▶] [CUE] [CUP] [FLX]   |   Right: [<] [1] [2] [4] [8] [16] [>] + Pitch Slider
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);

                    let play_color = if is_playing {
                        TraktorTheme::PLAY_ACTIVE
                    } else {
                        TraktorTheme::PLAY_INACTIVE
                    };
                    let play_text_color = if is_playing {
                        Color32::BLACK
                    } else {
                        TraktorTheme::TEXT_PRIMARY
                    };
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("▶")
                                    .color(play_text_color)
                                    .font(egui::FontId::proportional(12.0))
                                    .strong(),
                            )
                            .fill(play_color)
                            .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                            .min_size(Vec2::new(34.0, 20.0)),
                        )
                        .on_hover_text("Play / Pause Deck")
                        .clicked()
                    {
                        self.toggle_play(deck_idx);
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("CUE")
                                    .color(TraktorTheme::TEXT_PRIMARY)
                                    .font(egui::FontId::monospace(9.5))
                                    .strong(),
                            )
                            .fill(TraktorTheme::CUE_INACTIVE)
                            .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                            .min_size(Vec2::new(34.0, 20.0)),
                        )
                        .clicked()
                    {
                        self.jump_to_cue(deck_idx, 1);
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("CUP")
                                    .color(TraktorTheme::TEXT_SECONDARY)
                                    .font(egui::FontId::monospace(9.0)),
                            )
                            .fill(TraktorTheme::CUE_INACTIVE)
                            .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                            .min_size(Vec2::new(32.0, 20.0)),
                        )
                        .on_hover_text("Cue Play (Jump to Cue 1 & Play)")
                        .clicked()
                    {
                        self.jump_to_cue(deck_idx, 1);
                        if let Ok(mut state) = self.audio.shared.lock() {
                            state.is_playing[d] = true;
                        }
                    }

                    let flux_active = self.deck_flux_active[d];
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("FLX")
                                    .color(if flux_active {
                                        Color32::BLACK
                                    } else {
                                        TraktorTheme::TEXT_SECONDARY
                                    })
                                    .font(egui::FontId::monospace(9.0))
                                    .strong(),
                            )
                            .fill(if flux_active {
                                TraktorTheme::FLUX_ACTIVE
                            } else {
                                TraktorTheme::CUE_INACTIVE
                            })
                            .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                            .min_size(Vec2::new(30.0, 20.0)),
                        )
                        .on_hover_text("Toggle Flux / Slip Mode")
                        .clicked()
                    {
                        self.deck_flux_active[d] = !self.deck_flux_active[d];
                    }

                    ui.separator();

                    // Traktor Loop Bar: [<] [1] [2] [4] [8] [16] [>]
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("◄")
                                    .color(TraktorTheme::TEXT_SECONDARY)
                                    .font(egui::FontId::monospace(8.5)),
                            )
                            .fill(Color32::from_rgb(48, 51, 57))
                            .min_size(Vec2::new(18.0, 20.0)),
                        )
                        .on_hover_text("Halve Loop Size (/2)")
                        .clicked()
                    {
                        self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] / 2).max(1);
                    }

                    for beats in [1u32, 2, 4, 8, 16] {
                        let is_selected_size = self.deck_loop_size_beats[d] == beats;
                        let is_active_loop = self.deck_loop_active[d] && is_selected_size;
                        let fill = if is_active_loop {
                            TraktorTheme::LOOP_ACTIVE
                        } else if is_selected_size {
                            Color32::from_rgb(64, 68, 76)
                        } else {
                            Color32::from_rgb(46, 49, 55)
                        };
                        let fg = if is_active_loop {
                            Color32::BLACK
                        } else if is_selected_size {
                            TraktorTheme::LOOP_ACTIVE
                        } else {
                            TraktorTheme::TEXT_SECONDARY
                        };
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(format!("{beats}"))
                                        .color(fg)
                                        .font(egui::FontId::monospace(9.0))
                                        .strong(),
                                )
                                .fill(fill)
                                .min_size(Vec2::new(20.0, 20.0)),
                            )
                            .clicked()
                        {
                            if self.deck_loop_size_beats[d] == beats {
                                self.toggle_loop(deck_idx);
                            } else {
                                self.deck_loop_size_beats[d] = beats;
                                if !self.deck_loop_active[d] {
                                    self.toggle_loop(deck_idx);
                                }
                            }
                        }
                    }

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("►")
                                    .color(TraktorTheme::TEXT_SECONDARY)
                                    .font(egui::FontId::monospace(8.5)),
                            )
                            .fill(Color32::from_rgb(48, 51, 57))
                            .min_size(Vec2::new(18.0, 20.0)),
                        )
                        .on_hover_text("Double Loop Size (x2)")
                        .clicked()
                    {
                        self.deck_loop_size_beats[d] = (self.deck_loop_size_beats[d] * 2).min(32);
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("0%")
                                        .color(TraktorTheme::TEXT_SECONDARY)
                                        .font(egui::FontId::monospace(8.5)),
                                )
                                .fill(Color32::from_rgb(48, 51, 57))
                                .min_size(Vec2::new(24.0, 20.0)),
                            )
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

                ui.add_space(2.0);

                // 5. Slim Understated Hotcue / Performance Pads 1..8 Row (matching Traktor Pro 4 bottom deck strip)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                    let cue_w = ((ui.available_width() - 21.0) / 8.0).max(16.0);
                    for i in 1..=8 {
                        let pad_active = match self.deck_pad_mode[d] {
                            DeckPadMode::Hotcues => {
                                self.deck_hotcues[d].iter().any(|(c, _)| *c == i as u8)
                            }
                            DeckPadMode::Stems => {
                                if i <= 4 {
                                    !self.deck_stem_muted[d][i - 1]
                                } else {
                                    self.deck_stem_fx_send[d][i - 5]
                                }
                            }
                            DeckPadMode::Pattern => {
                                let v = self.pattern_selected_voice.min(3);
                                let step_offset = if deck_idx == 2 { 8 } else { 0 };
                                self.pattern_steps[v][(step_offset + i - 1).min(15)]
                            }
                            DeckPadMode::FluxLoop => self.deck_loop_active[d],
                        };
                        let pad_resp = cue_pad(ui, i, pad_active, false, cue_w);
                        if pad_resp.clicked() {
                            self.trigger_performance_pad(deck_idx, i as u8);
                        }
                        if pad_resp.secondary_clicked() && self.deck_pad_mode[d] == DeckPadMode::Hotcues {
                            self.deck_hotcues[d].retain(|(c, _)| *c != i as u8);
                        }
                    }
                });
            });
    }

    /// Center Mixer Section — Strictly top-aligned with side-by-side EQ/Stems + Fader/VU columns per strip
    /// so all 4 channels line up horizontally with zero staircase/staggering!
    fn show_mixer_section(&mut self, ui: &mut Ui, two_channel_only: bool) {
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL_ALT)
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
            .inner_margin(egui::Margin::symmetric(6.0, 5.0))
            .show(ui, |ui| {
                ui.vertical(|ui| {
                    // Algoriddim djay Pro-style Neural Mix™ EQ ⇄ STEMS Mode Toggle Bar
                    ui.horizontal(|ui| {
                        let neural = self.preferences.neural_mix_eq_mode;
                        let mode_btn = ui.add(
                            egui::Button::new(
                                RichText::new(if neural {
                                    "● KNOBS: NEURAL STEMS (VOC / INST / D+B)"
                                } else {
                                    "KNOBS: 3-BAND EQ (HI / MID / LOW)  |  STEMS"
                                })
                                .color(if neural {
                                    Color32::WHITE
                                } else {
                                    TraktorTheme::TEXT_SECONDARY
                                })
                                .font(egui::FontId::monospace(8.5))
                                .strong(),
                            )
                            .fill(if neural {
                                TraktorTheme::ROW_SELECTED
                            } else {
                                Color32::from_rgb(42, 45, 50)
                            })
                            .min_size(Vec2::new(ui.available_width(), 16.0)),
                        );
                        if mode_btn
                            .on_hover_text(
                                "Swap Mixer & Traktor MX2 HI/MID/LOW EQ knobs between 3-Band EQ and Real-Time Stems (Vocals / Instruments / Drums+Bass)",
                            )
                            .clicked()
                        {
                            self.preferences.neural_mix_eq_mode =
                                !self.preferences.neural_mix_eq_mode;
                        }
                    });

                    ui.add_space(2.0);

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
    /// Left sub-column = HI, MID, LOW (or VOC, INST, D+B in Neural Stem mode), FLTR rotary knobs
    /// Right sub-column = GAIN knob, CUE button, Vertical Volume Fader + Stereo VU Meter
    fn show_channel_strip(&mut self, ui: &mut Ui, ch: usize) {
        let ch_num = ch + 1;
        let ch_letter = ["A", "B", "C", "D"][ch.min(3)];
        let accent = TraktorTheme::deck_color(ch_num);
        let neural_mode = self.preferences.neural_mix_eq_mode;

        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);

            // Channel Header + Cue button
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("CH{ch_num} ({ch_letter})"))
                        .color(TraktorTheme::TEXT_PRIMARY)
                        .font(egui::FontId::monospace(9.5))
                        .strong(),
                );
                let cue_active = self.channel_cue[ch];
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("CUE")
                                .color(if cue_active {
                                    Color32::BLACK
                                } else {
                                    TraktorTheme::TEXT_SECONDARY
                                })
                                .font(egui::FontId::monospace(8.0))
                                .strong(),
                        )
                        .fill(if cue_active {
                            TraktorTheme::CUE_ACTIVE
                        } else {
                            Color32::from_rgb(46, 49, 55)
                        })
                        .min_size(Vec2::new(24.0, 15.0)),
                    )
                    .on_hover_text("Headphone Cue Listen")
                    .clicked()
                {
                    self.channel_cue[ch] = !self.channel_cue[ch];
                }
            });

            // Side-by-side EQ/Stems/Filter column & Fader/Meter column
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 1.0);

                // Sub-column 1: HI/VOC, MID/INST, LOW/D+B, FLTR
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(1.0, 1.0);
                    if neural_mode {
                        let voc_col = if self.deck_stem_muted[ch][3] {
                            TraktorTheme::METER_RED
                        } else {
                            Color32::from_rgb(190, 85, 255)
                        };
                        if rotary_knob_sized(
                            ui,
                            &mut self.deck_stem_volumes[ch][3],
                            0.0,
                            1.0,
                            1.0,
                            "VOC",
                            voc_col,
                            28.0,
                        )
                        .secondary_clicked()
                        {
                            self.deck_stem_muted[ch][3] = !self.deck_stem_muted[ch][3];
                        }

                        let inst_col = if self.deck_stem_muted[ch][2] {
                            TraktorTheme::METER_RED
                        } else {
                            Color32::from_rgb(255, 195, 0)
                        };
                        if rotary_knob_sized(
                            ui,
                            &mut self.deck_stem_volumes[ch][2],
                            0.0,
                            1.0,
                            1.0,
                            "INST",
                            inst_col,
                            28.0,
                        )
                        .secondary_clicked()
                        {
                            self.deck_stem_muted[ch][2] = !self.deck_stem_muted[ch][2];
                        }

                        let mut db_vol =
                            (self.deck_stem_volumes[ch][0] + self.deck_stem_volumes[ch][1]) * 0.5;
                        let db_muted =
                            self.deck_stem_muted[ch][0] && self.deck_stem_muted[ch][1];
                        let db_col = if db_muted {
                            TraktorTheme::METER_RED
                        } else {
                            Color32::from_rgb(0, 210, 255)
                        };
                        let db_resp = rotary_knob_sized(
                            ui,
                            &mut db_vol,
                            0.0,
                            1.0,
                            1.0,
                            "D+B",
                            db_col,
                            28.0,
                        );
                        if db_resp.changed() {
                            self.deck_stem_volumes[ch][0] = db_vol;
                            self.deck_stem_volumes[ch][1] = db_vol;
                        }
                        if db_resp.secondary_clicked() {
                            let next_mute = !db_muted;
                            self.deck_stem_muted[ch][0] = next_mute;
                            self.deck_stem_muted[ch][1] = next_mute;
                        }
                    } else {
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
