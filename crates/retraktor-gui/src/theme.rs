//! Runtime colour theme. Every colour the UI uses is a named token in [`Palette`]; presets
//! can be edited in Preferences > Look and are saved to `~/.config/retraktor/theme.txt`.
#![allow(non_snake_case)]

use eframe::egui::{self, Color32};
use std::sync::{OnceLock, RwLock};

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub BG_WINDOW: Color32,
    pub BG_PANEL: Color32,
    pub BG_PANEL_ALT: Color32,
    pub BG_SECTION: Color32,
    pub BG_INPUT: Color32,
    pub BORDER_COLOR: Color32,
    pub BORDER_ACCENT: Color32,
    pub ROW_SELECTED: Color32,
    pub TEXT_PRIMARY: Color32,
    pub TEXT_SECONDARY: Color32,
    pub TEXT_MUTED: Color32,
    pub TEXT_HIGHLIGHT: Color32,
    pub DECK_A: Color32,
    pub DECK_A_MUTED: Color32,
    pub DECK_B: Color32,
    pub DECK_B_MUTED: Color32,
    pub DECK_C: Color32,
    pub DECK_C_MUTED: Color32,
    pub DECK_D: Color32,
    pub DECK_D_MUTED: Color32,
    pub PLAY_ACTIVE: Color32,
    pub PLAY_INACTIVE: Color32,
    pub CUE_ACTIVE: Color32,
    pub CUE_INACTIVE: Color32,
    pub SYNC_ACTIVE: Color32,
    pub FLUX_ACTIVE: Color32,
    pub LOOP_ACTIVE: Color32,
    pub KEYLOCK_ACTIVE: Color32,
    pub METER_BG: Color32,
    pub METER_GREEN: Color32,
    pub METER_YELLOW: Color32,
    pub METER_RED: Color32,
    pub WAVEFORM_BG: Color32,
    pub WAVEFORM_LOW: Color32,
    pub WAVEFORM_MID: Color32,
    pub WAVEFORM_HIGH: Color32,
    pub WAVEFORM_GRID: Color32,
    pub WAVEFORM_PLAYHEAD: Color32,
    pub HOTCUE_GRID: Color32,
    pub HOTCUE_CUE: Color32,
    pub HOTCUE_LOAD: Color32,
    pub HOTCUE_FADEIN: Color32,
    pub HOTCUE_FADEOUT: Color32,
    pub HOTCUE_LOOP: Color32,
    pub HOTCUE_GENERIC: Color32,
    pub HOTCUE_EMPTY: Color32,
    pub ACCENT: Color32,
    pub CONTROL: Color32,
    pub CONTROL_HOVER: Color32,
}

impl Palette {
    /// Nothing-style: black, white and one red signal. The default; built for a dark bedroom.
    pub const NIGHT: Palette = Palette {
        BG_WINDOW: rgb(10, 10, 10),
        BG_PANEL: rgb(20, 20, 20),
        BG_PANEL_ALT: rgb(15, 15, 15),
        BG_SECTION: rgb(30, 30, 30),
        BG_INPUT: rgb(6, 6, 6),
        BORDER_COLOR: rgb(34, 34, 34),
        BORDER_ACCENT: rgb(60, 60, 60),
        ROW_SELECTED: rgb(120, 18, 24),
        TEXT_PRIMARY: rgb(236, 236, 236),
        TEXT_SECONDARY: rgb(158, 158, 158),
        TEXT_MUTED: rgb(104, 104, 104),
        TEXT_HIGHLIGHT: rgb(255, 255, 255),
        DECK_A: rgb(236, 236, 236),
        DECK_A_MUTED: rgb(70, 70, 70),
        DECK_B: rgb(232, 48, 52),
        DECK_B_MUTED: rgb(90, 24, 26),
        DECK_C: rgb(160, 160, 160),
        DECK_C_MUTED: rgb(56, 56, 56),
        DECK_D: rgb(255, 128, 116),
        DECK_D_MUTED: rgb(92, 46, 42),
        PLAY_ACTIVE: rgb(236, 236, 236),
        PLAY_INACTIVE: rgb(34, 34, 34),
        CUE_ACTIVE: rgb(232, 48, 52),
        CUE_INACTIVE: rgb(34, 34, 34),
        SYNC_ACTIVE: rgb(236, 236, 236),
        FLUX_ACTIVE: rgb(232, 48, 52),
        LOOP_ACTIVE: rgb(236, 236, 236),
        KEYLOCK_ACTIVE: rgb(232, 48, 52),
        METER_BG: rgb(16, 16, 16),
        METER_GREEN: rgb(220, 220, 220),
        METER_YELLOW: rgb(255, 170, 60),
        METER_RED: rgb(232, 48, 52),
        WAVEFORM_BG: rgb(8, 8, 8),
        WAVEFORM_LOW: rgb(232, 48, 52),
        WAVEFORM_MID: rgb(210, 210, 210),
        WAVEFORM_HIGH: rgb(120, 120, 120),
        WAVEFORM_GRID: rgb(70, 70, 70),
        WAVEFORM_PLAYHEAD: rgb(255, 255, 255),
        HOTCUE_GRID: rgb(236, 236, 236),
        HOTCUE_CUE: rgb(232, 48, 52),
        HOTCUE_LOAD: rgb(255, 170, 60),
        HOTCUE_FADEIN: rgb(200, 200, 200),
        HOTCUE_FADEOUT: rgb(140, 140, 140),
        HOTCUE_LOOP: rgb(120, 200, 140),
        HOTCUE_GENERIC: rgb(170, 150, 220),
        HOTCUE_EMPTY: rgb(34, 34, 34),
        ACCENT: rgb(232, 48, 52),
        CONTROL: rgb(30, 30, 30),
        CONTROL_HOVER: rgb(44, 44, 44),
    };
    /// Teenage Engineering-style: warm grey hardware, dark display strips, orange signal.
    pub const FIELD: Palette = Palette {
        BG_WINDOW: rgb(226, 224, 219),
        BG_PANEL: rgb(238, 237, 233),
        BG_PANEL_ALT: rgb(231, 229, 224),
        BG_SECTION: rgb(214, 212, 206),
        BG_INPUT: rgb(250, 249, 246),
        BORDER_COLOR: rgb(200, 198, 192),
        BORDER_ACCENT: rgb(150, 148, 142),
        ROW_SELECTED: rgb(190, 62, 0),
        TEXT_PRIMARY: rgb(22, 22, 22),
        TEXT_SECONDARY: rgb(72, 72, 70),
        TEXT_MUTED: rgb(124, 122, 118),
        TEXT_HIGHLIGHT: rgb(0, 0, 0),
        DECK_A: rgb(24, 110, 210),
        DECK_A_MUTED: rgb(190, 208, 232),
        DECK_B: rgb(230, 82, 0),
        DECK_B_MUTED: rgb(240, 206, 186),
        DECK_C: rgb(110, 80, 190),
        DECK_C_MUTED: rgb(214, 206, 236),
        DECK_D: rgb(24, 140, 104),
        DECK_D_MUTED: rgb(196, 226, 214),
        PLAY_ACTIVE: rgb(40, 160, 80),
        PLAY_INACTIVE: rgb(214, 212, 206),
        CUE_ACTIVE: rgb(230, 82, 0),
        CUE_INACTIVE: rgb(214, 212, 206),
        SYNC_ACTIVE: rgb(24, 110, 210),
        FLUX_ACTIVE: rgb(230, 82, 0),
        LOOP_ACTIVE: rgb(40, 160, 80),
        KEYLOCK_ACTIVE: rgb(200, 40, 40),
        METER_BG: rgb(40, 40, 40),
        METER_GREEN: rgb(90, 200, 110),
        METER_YELLOW: rgb(255, 180, 40),
        METER_RED: rgb(230, 60, 40),
        WAVEFORM_BG: rgb(26, 26, 26),
        WAVEFORM_LOW: rgb(255, 96, 20),
        WAVEFORM_MID: rgb(236, 236, 230),
        WAVEFORM_HIGH: rgb(110, 170, 255),
        WAVEFORM_GRID: rgb(80, 80, 80),
        WAVEFORM_PLAYHEAD: rgb(255, 96, 20),
        HOTCUE_GRID: rgb(236, 236, 230),
        HOTCUE_CUE: rgb(24, 110, 210),
        HOTCUE_LOAD: rgb(230, 160, 0),
        HOTCUE_FADEIN: rgb(230, 82, 0),
        HOTCUE_FADEOUT: rgb(200, 40, 40),
        HOTCUE_LOOP: rgb(40, 160, 80),
        HOTCUE_GENERIC: rgb(110, 80, 190),
        HOTCUE_EMPTY: rgb(214, 212, 206),
        ACCENT: rgb(230, 82, 0),
        CONTROL: rgb(214, 212, 206),
        CONTROL_HOVER: rgb(200, 198, 192),
    };
    /// The original matte charcoal Traktor look.
    pub const CHARCOAL: Palette = Palette {
        BG_WINDOW: rgb(33, 35, 38),
        BG_PANEL: rgb(43, 45, 49),
        BG_PANEL_ALT: rgb(38, 40, 44),
        BG_SECTION: rgb(54, 57, 62),
        BG_INPUT: rgb(28, 30, 33),
        BORDER_COLOR: rgb(24, 25, 28),
        BORDER_ACCENT: rgb(62, 65, 71),
        ROW_SELECTED: rgb(46, 82, 128),
        TEXT_PRIMARY: rgb(232, 234, 238),
        TEXT_SECONDARY: rgb(176, 181, 188),
        TEXT_MUTED: rgb(106, 111, 120),
        TEXT_HIGHLIGHT: rgb(248, 249, 250),
        DECK_A: rgb(84, 164, 214),
        DECK_A_MUTED: rgb(44, 78, 102),
        DECK_B: rgb(214, 124, 46),
        DECK_B_MUTED: rgb(98, 60, 28),
        DECK_C: rgb(142, 116, 198),
        DECK_C_MUTED: rgb(66, 54, 92),
        DECK_D: rgb(92, 176, 148),
        DECK_D_MUTED: rgb(45, 84, 72),
        PLAY_ACTIVE: rgb(184, 208, 92),
        PLAY_INACTIVE: rgb(56, 59, 65),
        CUE_ACTIVE: rgb(84, 164, 214),
        CUE_INACTIVE: rgb(56, 59, 65),
        SYNC_ACTIVE: rgb(84, 164, 214),
        FLUX_ACTIVE: rgb(214, 124, 46),
        LOOP_ACTIVE: rgb(114, 182, 102),
        KEYLOCK_ACTIVE: rgb(208, 92, 92),
        METER_BG: rgb(26, 28, 31),
        METER_GREEN: rgb(84, 164, 214),
        METER_YELLOW: rgb(218, 158, 52),
        METER_RED: rgb(212, 74, 68),
        WAVEFORM_BG: rgb(28, 30, 33),
        WAVEFORM_LOW: rgb(216, 96, 56),
        WAVEFORM_MID: rgb(188, 176, 68),
        WAVEFORM_HIGH: rgb(108, 148, 214),
        WAVEFORM_GRID: rgb(210, 215, 222),
        WAVEFORM_PLAYHEAD: rgb(212, 74, 68),
        HOTCUE_GRID: rgb(220, 224, 230),
        HOTCUE_CUE: rgb(84, 164, 214),
        HOTCUE_LOAD: rgb(214, 174, 64),
        HOTCUE_FADEIN: rgb(214, 124, 46),
        HOTCUE_FADEOUT: rgb(208, 84, 78),
        HOTCUE_LOOP: rgb(104, 182, 108),
        HOTCUE_GENERIC: rgb(148, 118, 206),
        HOTCUE_EMPTY: rgb(52, 55, 60),
        ACCENT: rgb(84, 164, 214),
        CONTROL: rgb(52, 55, 62),
        CONTROL_HOVER: rgb(62, 66, 74),
    };

    pub const PRESETS: [(&'static str, Palette); 3] = [("Night", Self::NIGHT), ("Field", Self::FIELD), ("Charcoal", Self::CHARCOAL)];

    /// Every token by name, for the theme editor and persistence.
    pub fn tokens_mut(&mut self) -> Vec<(&'static str, &mut Color32)> {
        vec![
            ("BG_WINDOW", &mut self.BG_WINDOW),
            ("BG_PANEL", &mut self.BG_PANEL),
            ("BG_PANEL_ALT", &mut self.BG_PANEL_ALT),
            ("BG_SECTION", &mut self.BG_SECTION),
            ("BG_INPUT", &mut self.BG_INPUT),
            ("BORDER_COLOR", &mut self.BORDER_COLOR),
            ("BORDER_ACCENT", &mut self.BORDER_ACCENT),
            ("ROW_SELECTED", &mut self.ROW_SELECTED),
            ("TEXT_PRIMARY", &mut self.TEXT_PRIMARY),
            ("TEXT_SECONDARY", &mut self.TEXT_SECONDARY),
            ("TEXT_MUTED", &mut self.TEXT_MUTED),
            ("TEXT_HIGHLIGHT", &mut self.TEXT_HIGHLIGHT),
            ("DECK_A", &mut self.DECK_A),
            ("DECK_A_MUTED", &mut self.DECK_A_MUTED),
            ("DECK_B", &mut self.DECK_B),
            ("DECK_B_MUTED", &mut self.DECK_B_MUTED),
            ("DECK_C", &mut self.DECK_C),
            ("DECK_C_MUTED", &mut self.DECK_C_MUTED),
            ("DECK_D", &mut self.DECK_D),
            ("DECK_D_MUTED", &mut self.DECK_D_MUTED),
            ("PLAY_ACTIVE", &mut self.PLAY_ACTIVE),
            ("PLAY_INACTIVE", &mut self.PLAY_INACTIVE),
            ("CUE_ACTIVE", &mut self.CUE_ACTIVE),
            ("CUE_INACTIVE", &mut self.CUE_INACTIVE),
            ("SYNC_ACTIVE", &mut self.SYNC_ACTIVE),
            ("FLUX_ACTIVE", &mut self.FLUX_ACTIVE),
            ("LOOP_ACTIVE", &mut self.LOOP_ACTIVE),
            ("KEYLOCK_ACTIVE", &mut self.KEYLOCK_ACTIVE),
            ("METER_BG", &mut self.METER_BG),
            ("METER_GREEN", &mut self.METER_GREEN),
            ("METER_YELLOW", &mut self.METER_YELLOW),
            ("METER_RED", &mut self.METER_RED),
            ("WAVEFORM_BG", &mut self.WAVEFORM_BG),
            ("WAVEFORM_LOW", &mut self.WAVEFORM_LOW),
            ("WAVEFORM_MID", &mut self.WAVEFORM_MID),
            ("WAVEFORM_HIGH", &mut self.WAVEFORM_HIGH),
            ("WAVEFORM_GRID", &mut self.WAVEFORM_GRID),
            ("WAVEFORM_PLAYHEAD", &mut self.WAVEFORM_PLAYHEAD),
            ("HOTCUE_GRID", &mut self.HOTCUE_GRID),
            ("HOTCUE_CUE", &mut self.HOTCUE_CUE),
            ("HOTCUE_LOAD", &mut self.HOTCUE_LOAD),
            ("HOTCUE_FADEIN", &mut self.HOTCUE_FADEIN),
            ("HOTCUE_FADEOUT", &mut self.HOTCUE_FADEOUT),
            ("HOTCUE_LOOP", &mut self.HOTCUE_LOOP),
            ("HOTCUE_GENERIC", &mut self.HOTCUE_GENERIC),
            ("HOTCUE_EMPTY", &mut self.HOTCUE_EMPTY),
            ("ACCENT", &mut self.ACCENT),
            ("CONTROL", &mut self.CONTROL),
            ("CONTROL_HOVER", &mut self.CONTROL_HOVER),
        ]
    }

    pub fn is_dark(&self) -> bool {
        let c = self.BG_WINDOW;
        (c.r() as u32 + c.g() as u32 + c.b() as u32) < 384
    }

    /// `NAME=#rrggbb` lines.
    pub fn to_text(&self) -> String {
        let mut copy = self.clone();
        copy.tokens_mut().into_iter().map(|(n, c)| format!("{n}=#{:02x}{:02x}{:02x}\n", c.r(), c.g(), c.b())).collect()
    }

    /// Applies `NAME=#rrggbb` lines on top of `base`; unknown or malformed lines are ignored.
    pub fn from_text(base: &Palette, text: &str) -> Palette {
        let mut p = base.clone();
        for line in text.lines() {
            let Some((name, hex)) = line.trim().split_once('=') else { continue };
            let hex = hex.trim().trim_start_matches('#');
            let Ok(v) = u32::from_str_radix(hex, 16) else { continue };
            if hex.len() != 6 {
                continue;
            }
            if let Some((_, c)) = p.tokens_mut().into_iter().find(|(n, _)| *n == name.trim()) {
                *c = rgb((v >> 16) as u8, (v >> 8) as u8, v as u8);
            }
        }
        p
    }
}

/// Theme plus the "Look" settings that travel with it.
struct ThemeState {
    current: Palette,
    /// Palette being faded out, and when the fade began.
    from: Option<(Palette, std::time::Instant)>,
    animations: bool,
    scale: f32,
}

const THEME_FADE_SECS: f32 = 0.28;

fn store() -> &'static RwLock<ThemeState> {
    static P: OnceLock<RwLock<ThemeState>> = OnceLock::new();
    P.get_or_init(|| RwLock::new(load()))
}

fn lerp_colour(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Blend between two colours; `t` 0 = `a`, 1 = `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    lerp_colour(a, b, t.clamp(0.0, 1.0))
}

/// The palette to draw with right now (mid-crossfade while a theme change settles).
pub fn palette() -> Palette {
    let Ok(st) = store().read() else { return Palette::NIGHT };
    if let Some((from, start)) = &st.from {
        let t = start.elapsed().as_secs_f32() / THEME_FADE_SECS;
        if t < 1.0 && st.animations {
            let eased = 1.0 - (1.0 - t).powi(3);
            let (mut out, mut a, mut b) = (st.current.clone(), from.clone(), st.current.clone());
            for ((_, o), ((_, x), (_, y))) in out.tokens_mut().into_iter().zip(a.tokens_mut().into_iter().zip(b.tokens_mut())) {
                *o = lerp_colour(*x, *y, eased);
            }
            return out;
        }
    }
    st.current.clone()
}

/// The chosen palette, ignoring any crossfade in progress.
pub fn target_palette() -> Palette {
    store().read().map(|s| s.current.clone()).unwrap_or(Palette::NIGHT)
}

pub fn set_palette(p: Palette) {
    let shown = palette();
    if let Ok(mut st) = store().write() {
        st.from = Some((shown, std::time::Instant::now()));
        st.current = p;
    }
}

/// Whether motion is enabled (Preferences > Look).
pub fn animations() -> bool {
    store().read().map(|s| s.animations).unwrap_or(true)
}

/// Eased 0..=1 that follows `on`; call every frame with the same `id`. Snaps when animations are off.
pub fn fade(ctx: &egui::Context, id: impl std::hash::Hash, on: bool) -> f32 {
    if !animations() {
        return if on { 1.0 } else { 0.0 };
    }
    let t = ctx.animate_bool_with_time(egui::Id::new(("fade", id)), on, 0.16);
    1.0 - (1.0 - t).powi(3)
}

/// 0..=1 pulse that peaks on each beat and decays; steady 1.0 when animations are off.
pub fn beat_pulse(position_sec: f64, bpm: f64) -> f32 {
    if !animations() || bpm <= 0.0 {
        return 1.0;
    }
    let phase = (position_sec.max(0.0) * bpm / 60.0).fract() as f32;
    0.25 + 0.75 * (1.0 - phase).powi(3)
}

fn theme_path() -> std::path::PathBuf {
    std::path::PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/retraktor/theme.txt")
}

fn load() -> ThemeState {
    let text = std::fs::read_to_string(theme_path()).unwrap_or_default();
    let setting = |key: &str| text.lines().find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()));
    ThemeState {
        current: Palette::from_text(&Palette::NIGHT, &text),
        from: None,
        animations: setting("animations=").as_deref() != Some("off"),
        scale: setting("scale=").and_then(|v| v.parse::<f32>().ok()).filter(|v| (0.8..=1.6).contains(v)).unwrap_or(1.0),
    }
}

pub fn save() -> std::io::Result<()> {
    let path = theme_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let (animations, scale) = store().read().map(|s| (s.animations, s.scale)).unwrap_or((true, 1.0));
    let extra = format!("animations={}\nscale={scale:.2}\n", if animations { "on" } else { "off" });
    std::fs::write(path, target_palette().to_text() + &extra)
}

/// Pushes the palette into egui's own widget colours so stock buttons, inputs and tables follow the theme.
pub fn apply_visuals(ctx: &egui::Context) {
    let p = palette();
    let mut v = if p.is_dark() { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = p.BG_WINDOW;
    v.window_fill = p.BG_PANEL;
    v.window_stroke = egui::Stroke::new(1.0_f32, p.BORDER_ACCENT);
    v.extreme_bg_color = p.BG_INPUT;
    v.faint_bg_color = p.BG_PANEL_ALT;
    v.code_bg_color = p.BG_INPUT;
    v.override_text_color = Some(p.TEXT_PRIMARY);
    v.hyperlink_color = p.ACCENT;
    v.selection.bg_fill = p.ROW_SELECTED;
    v.selection.stroke = egui::Stroke::new(1.0_f32, p.TEXT_HIGHLIGHT);
    for (w, fill) in [
        (&mut v.widgets.inactive, p.CONTROL),
        (&mut v.widgets.hovered, p.CONTROL_HOVER),
        (&mut v.widgets.active, p.ACCENT),
        (&mut v.widgets.open, p.CONTROL_HOVER),
    ] {
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.fg_stroke.color = p.TEXT_PRIMARY;
    }
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, p.BORDER_COLOR);
    v.widgets.noninteractive.fg_stroke.color = p.TEXT_SECONDARY;
    v.widgets.noninteractive.weak_bg_fill = p.BG_PANEL;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, p.BORDER_ACCENT);
    ctx.set_visuals(v);
    let (animations, scale) = store().read().map(|s| (s.animations, s.scale)).unwrap_or((true, 1.0));
    ctx.style_mut(|style| style.animation_time = if animations { 0.12 } else { 0.0 });
    if (ctx.zoom_factor() - scale).abs() > 0.001 {
        ctx.set_zoom_factor(scale);
    }
}

/// Black or white, whichever reads better on `fill`.
pub fn on(fill: Color32) -> Color32 {
    if contrast(Color32::BLACK, fill) >= contrast(Color32::WHITE, fill) { Color32::BLACK } else { Color32::WHITE }
}

/// WCAG contrast ratio between two colours (1.0..=21.0).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let lum = |c: Color32| {
        let ch = |v: u8| {
            let v = v as f32 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    };
    let (x, y) = (lum(a), lum(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Preferences > Look: preset cards plus every token, applied live and saved.
pub fn editor(ui: &mut egui::Ui) {
    let mut p = target_palette();
    let mut changed = false;
    let (mut animations, mut scale) = store().read().map(|s| (s.animations, s.scale)).unwrap_or((true, 1.0));
    let mut settings_changed = false;
    ui.label(egui::RichText::new("Theme").size(15.0).strong());
    ui.add_space(6.0);
    ui.horizontal_wrapped(|ui| {
        for (name, preset) in Palette::PRESETS {
            let (rect, r) = ui.allocate_exact_size(egui::vec2(168.0, 104.0), egui::Sense::click());
            let painter = ui.painter();
            let active = p == preset;
            painter.rect_filled(rect, 8.0, preset.BG_WINDOW);
            let panel = egui::Rect::from_min_size(rect.min + egui::vec2(10.0, 10.0), egui::vec2(148.0, 54.0));
            painter.rect_filled(panel, 5.0, preset.BG_PANEL);
            painter.text(panel.min + egui::vec2(10.0, 8.0), egui::Align2::LEFT_TOP, "128.00", dot_font(20.0), preset.TEXT_PRIMARY);
            painter.circle_filled(panel.right_top() + egui::vec2(-16.0, 16.0), 6.0, preset.ACCENT);
            for (i, c) in [preset.DECK_A, preset.DECK_B, preset.WAVEFORM_LOW, preset.WAVEFORM_MID].into_iter().enumerate() {
                painter.rect_filled(egui::Rect::from_min_size(panel.left_bottom() + egui::vec2(10.0 + i as f32 * 20.0, -14.0), egui::vec2(14.0, 6.0)), 1.0, c);
            }
            painter.text(rect.left_bottom() + egui::vec2(12.0, -12.0), egui::Align2::LEFT_BOTTOM, name, egui::FontId::proportional(14.0), preset.TEXT_PRIMARY);
            let ring = if active { egui::Stroke::new(2.5_f32, p.ACCENT) } else if r.hovered() { egui::Stroke::new(1.5_f32, p.TEXT_SECONDARY) } else { egui::Stroke::new(1.0_f32, p.BORDER_ACCENT) };
            painter.rect_stroke(rect, 8.0, ring);
            if r.on_hover_text(format!("Use the {name} theme")).clicked() {
                p = preset.clone();
                changed = true;
            }
        }
    });
    let ratio = contrast(p.TEXT_PRIMARY, p.BG_WINDOW).min(contrast(p.TEXT_SECONDARY, p.BG_PANEL));
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new(if ratio >= 4.5 { format!("Text contrast {ratio:.1}:1 — readable") } else { format!("Text contrast {ratio:.1}:1 — too low; aim for 4.5:1 or more") })
            .size(12.0)
            .color(if ratio >= 4.5 { p.TEXT_SECONDARY } else { p.METER_RED }),
    );
    ui.add_space(10.0);
    const GROUPS: [(&str, &[&str]); 7] = [
        ("Surfaces", &["BG_", "BORDER_", "CONTROL", "ROW_SELECTED"]),
        ("Text", &["TEXT_"]),
        ("Accent & transport", &["ACCENT", "PLAY_", "CUE_", "SYNC_", "FLUX_", "LOOP_", "KEYLOCK_"]),
        ("Decks", &["DECK_"]),
        ("Meters", &["METER_"]),
        ("Waveform", &["WAVEFORM_"]),
        ("Hotcues", &["HOTCUE_"]),
    ];
    let mut tokens = p.tokens_mut();
    for (group, prefixes) in GROUPS {
        egui::CollapsingHeader::new(egui::RichText::new(group).size(13.5)).id_salt(("theme_group", group)).show(ui, |ui| {
            egui::Grid::new(("theme_grid", group)).num_columns(4).spacing([18.0, 8.0]).show(ui, |ui| {
                let mut col = 0;
                for (name, colour) in tokens.iter_mut().filter(|(n, _)| prefixes.iter().any(|pre| n.starts_with(pre))) {
                    if egui::color_picker::color_edit_button_srgba(ui, colour, egui::color_picker::Alpha::Opaque).changed() {
                        changed = true;
                    }
                    let mut label = name.replace('_', " ").to_lowercase();
                    label.replace_range(0..1, &label[0..1].to_uppercase());
                    ui.label(label);
                    col += 1;
                    if col % 2 == 0 {
                        ui.end_row();
                    }
                }
            });
        });
    }
    drop(tokens);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.button("Copy theme as text").on_hover_text("Paste it somewhere to keep or share").clicked() {
            ui.ctx().copy_text(p.to_text());
        }
        ui.label(egui::RichText::new("Saved automatically to ~/.config/retraktor/theme.txt").size(11.5).color(p.TEXT_MUTED));
    });
    ui.add_space(18.0);
    ui.label(egui::RichText::new("Motion & size").size(15.0).strong());
    ui.add_space(6.0);
    settings_changed |= ui
        .checkbox(&mut animations, "Animations")
        .on_hover_text("Fades, the beat lamp and the theme crossfade. Turn off if the app feels slow.")
        .changed();
    ui.horizontal(|ui| {
        ui.label("Text and control size");
        // Applied when the drag ends, so the slider doesn't move under the pointer.
        let pending = egui::Id::new("pending_scale");
        let mut shown = ui.ctx().data(|d| d.get_temp::<f32>(pending)).unwrap_or(scale);
        let r = ui.add(egui::Slider::new(&mut shown, 0.8..=1.6).step_by(0.05).custom_formatter(|v, _| format!("{:.0}%", v * 100.0)));
        if r.dragged() {
            ui.ctx().data_mut(|d| d.insert_temp(pending, shown));
        } else if r.drag_stopped() || r.changed() {
            ui.ctx().data_mut(|d| d.remove::<f32>(pending));
            scale = shown;
            settings_changed = true;
        }
        if ui.button("Reset").clicked() {
            scale = 1.0;
            settings_changed = true;
        }
    });
    if changed {
        set_palette(p);
    }
    if settings_changed {
        if let Ok(mut st) = store().write() {
            st.animations = animations;
            st.scale = scale;
        }
    }
    if changed || settings_changed {
        if let Err(e) = save() {
            eprintln!("retraktor: could not save theme: {e}");
        }
    }
}

/// Font family for the dot-matrix display face (BPM, time, key).
pub fn dot_font(size: f32) -> egui::FontId {
    egui::FontId::new(size, egui::FontFamily::Name("dot".into()))
}

/// Installs Space Grotesk for UI text and Doto as the dot-matrix display face.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert("grotesk".into(), egui::FontData::from_static(include_bytes!("../assets/fonts/SpaceGrotesk.ttf")).into());
    fonts.font_data.insert("doto".into(), egui::FontData::from_static(include_bytes!("../assets/fonts/Doto.ttf")).into());
    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.insert(0, "grotesk".into());
        // Symbols like ▲ ▾ ● live only in the monospace font.
        prop.push("Hack".into());
    }
    fonts.families.insert(egui::FontFamily::Name("dot".into()), vec!["doto".into(), "Hack".into()]);
    ctx.set_fonts(fonts);
}

/// Compatibility layer: `TraktorTheme::BG_PANEL()` reads the live palette.
pub struct TraktorTheme;

impl TraktorTheme {
    pub fn BG_WINDOW() -> Color32 { palette().BG_WINDOW }
    pub fn BG_PANEL() -> Color32 { palette().BG_PANEL }
    pub fn BG_PANEL_ALT() -> Color32 { palette().BG_PANEL_ALT }
    pub fn BG_SECTION() -> Color32 { palette().BG_SECTION }
    pub fn BG_INPUT() -> Color32 { palette().BG_INPUT }
    pub fn BORDER_COLOR() -> Color32 { palette().BORDER_COLOR }
    pub fn BORDER_ACCENT() -> Color32 { palette().BORDER_ACCENT }
    pub fn ROW_SELECTED() -> Color32 { palette().ROW_SELECTED }
    pub fn TEXT_PRIMARY() -> Color32 { palette().TEXT_PRIMARY }
    pub fn TEXT_SECONDARY() -> Color32 { palette().TEXT_SECONDARY }
    pub fn TEXT_MUTED() -> Color32 { palette().TEXT_MUTED }
    pub fn TEXT_HIGHLIGHT() -> Color32 { palette().TEXT_HIGHLIGHT }
    pub fn DECK_A() -> Color32 { palette().DECK_A }
    pub fn DECK_A_MUTED() -> Color32 { palette().DECK_A_MUTED }
    pub fn DECK_B() -> Color32 { palette().DECK_B }
    pub fn DECK_B_MUTED() -> Color32 { palette().DECK_B_MUTED }
    pub fn DECK_C() -> Color32 { palette().DECK_C }
    pub fn DECK_C_MUTED() -> Color32 { palette().DECK_C_MUTED }
    pub fn DECK_D() -> Color32 { palette().DECK_D }
    pub fn DECK_D_MUTED() -> Color32 { palette().DECK_D_MUTED }
    pub fn PLAY_ACTIVE() -> Color32 { palette().PLAY_ACTIVE }
    pub fn PLAY_INACTIVE() -> Color32 { palette().PLAY_INACTIVE }
    pub fn CUE_ACTIVE() -> Color32 { palette().CUE_ACTIVE }
    pub fn CUE_INACTIVE() -> Color32 { palette().CUE_INACTIVE }
    pub fn SYNC_ACTIVE() -> Color32 { palette().SYNC_ACTIVE }
    pub fn FLUX_ACTIVE() -> Color32 { palette().FLUX_ACTIVE }
    pub fn LOOP_ACTIVE() -> Color32 { palette().LOOP_ACTIVE }
    pub fn KEYLOCK_ACTIVE() -> Color32 { palette().KEYLOCK_ACTIVE }
    pub fn METER_BG() -> Color32 { palette().METER_BG }
    pub fn METER_GREEN() -> Color32 { palette().METER_GREEN }
    pub fn METER_YELLOW() -> Color32 { palette().METER_YELLOW }
    pub fn METER_RED() -> Color32 { palette().METER_RED }
    pub fn WAVEFORM_BG() -> Color32 { palette().WAVEFORM_BG }
    pub fn WAVEFORM_LOW() -> Color32 { palette().WAVEFORM_LOW }
    pub fn WAVEFORM_MID() -> Color32 { palette().WAVEFORM_MID }
    pub fn WAVEFORM_HIGH() -> Color32 { palette().WAVEFORM_HIGH }
    pub fn WAVEFORM_GRID() -> Color32 { palette().WAVEFORM_GRID }
    pub fn WAVEFORM_PLAYHEAD() -> Color32 { palette().WAVEFORM_PLAYHEAD }
    pub fn HOTCUE_GRID() -> Color32 { palette().HOTCUE_GRID }
    pub fn HOTCUE_CUE() -> Color32 { palette().HOTCUE_CUE }
    pub fn HOTCUE_LOAD() -> Color32 { palette().HOTCUE_LOAD }
    pub fn HOTCUE_FADEIN() -> Color32 { palette().HOTCUE_FADEIN }
    pub fn HOTCUE_FADEOUT() -> Color32 { palette().HOTCUE_FADEOUT }
    pub fn HOTCUE_LOOP() -> Color32 { palette().HOTCUE_LOOP }
    pub fn HOTCUE_GENERIC() -> Color32 { palette().HOTCUE_GENERIC }
    pub fn HOTCUE_EMPTY() -> Color32 { palette().HOTCUE_EMPTY }
    pub fn ACCENT() -> Color32 { palette().ACCENT }
    pub fn CONTROL() -> Color32 { palette().CONTROL }
    pub fn CONTROL_HOVER() -> Color32 { palette().CONTROL_HOVER }

    pub fn deck_color(deck_idx: usize) -> Color32 {
        match deck_idx {
            2 => Self::DECK_B(),
            3 => Self::DECK_C(),
            4 => Self::DECK_D(),
            _ => Self::DECK_A(),
        }
    }

    pub fn hotcue_color(cue_idx: usize) -> Color32 {
        match cue_idx {
            3 => Self::HOTCUE_LOAD(),
            4 => Self::HOTCUE_LOOP(),
            5 => Self::HOTCUE_FADEIN(),
            6 => Self::HOTCUE_GENERIC(),
            7 => Self::HOTCUE_FADEOUT(),
            _ => Self::HOTCUE_CUE(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_text_roundtrip_and_bad_lines_ignored() {
        let mut p = Palette::FIELD;
        p.ACCENT = rgb(1, 2, 3);
        let back = Palette::from_text(&Palette::NIGHT, &(p.to_text() + "BOGUS=#zzzzzz\nACCENT=#12\n"));
        assert_eq!(back, p);
        assert!(Palette::NIGHT.is_dark() && !Palette::FIELD.is_dark());
        for (name, p) in Palette::PRESETS {
            assert!(contrast(p.TEXT_PRIMARY, p.BG_WINDOW) >= 7.0, "{name} body text");
            assert!(contrast(p.TEXT_SECONDARY, p.BG_PANEL) >= 4.5, "{name} secondary text");
        }
    }
}
