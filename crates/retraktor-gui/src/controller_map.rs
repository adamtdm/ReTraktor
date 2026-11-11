//! Line-drawn replica of the Traktor MX2 for seeing and changing what every control does.
//! Positions are traced from the top panel (905 × 572 board units) and scale to any width.

use crate::theme::TraktorTheme;
use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use std::collections::HashMap;
use std::path::PathBuf;

const BOARD: Vec2 = Vec2::new(905.0, 572.0);
/// Right deck is the left deck shifted by this many board units.
const RIGHT_DX: f32 = 556.0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Knob(f32),
    Encoder(f32),
    Ring(f32),
    Round(f32),
    Rect(f32, f32),
    Pad(f32, f32),
    VFader(f32),
    HFader(f32),
    Jog(f32),
}

impl Kind {
    fn continuous(self) -> bool {
        matches!(self, Kind::Knob(_) | Kind::VFader(_) | Kind::HFader(_))
    }
}

#[derive(Clone)]
struct Ctl {
    id: String,
    kind: Kind,
    at: Vec2,
}

fn deck_controls(side: &str, dx: f32) -> Vec<Ctl> {
    let c = |id: &str, kind, x: f32, y: f32| Ctl { id: format!("surface.{side}.{id}"), kind, at: Vec2::new(x + dx, y) };
    // Jogwheel first: later controls get the click where hit areas overlap.
    let mut v = vec![c("jogwheel.touch", Kind::Jog(135.0), 173.0, 257.0)];
    for (i, x) in [43.0, 95.0, 148.0, 201.0].into_iter().enumerate() {
        v.push(c(&format!("fx.knobs.{}", i + 1), Kind::Knob(16.0), x, 32.0));
        v.push(c(&format!("fx.buttons.{}", i + 1), Kind::Ring(14.0), x, 78.0));
    }
    v.extend([
        c("browse.encoder", Kind::Encoder(20.0), 294.0, 65.0),
        c("browse.favorite", Kind::Rect(31.5, 15.0), 276.0, 22.0),
        c("browse.add_to_list", Kind::Rect(31.5, 15.0), 312.0, 22.0),
        c("browse.preview", Kind::Rect(31.5, 15.0), 276.0, 108.0),
        c("browse.view", Kind::Rect(31.5, 15.0), 312.0, 108.0),
        c("flux", Kind::Round(13.0), 50.0, 147.0),
        c("reverse", Kind::Round(13.0), 26.0, 182.0),
        c("mode.turntable", Kind::Round(13.0), 298.0, 147.0),
        c("mode.jogwheel", Kind::Round(13.0), 323.0, 182.0),
        c("shift", Kind::Rect(46.0, 18.0), 41.0, 404.0),
        c("sync", Kind::Rect(33.5, 18.0), 289.0, 404.0),
        c("master", Kind::Rect(33.5, 18.0), 326.0, 404.0),
        c("loop_move", Kind::Encoder(14.0), 105.0, 432.0),
        c("key_lock", Kind::Rect(46.0, 18.0), 175.0, 432.0),
        c("loop_size", Kind::Encoder(14.0), 243.0, 432.0),
        c("hotcues", Kind::Rect(42.0, 16.0), 107.0, 469.0),
        c("stems", Kind::Rect(42.0, 16.0), 152.0, 469.0),
        c("pattern", Kind::Rect(42.0, 16.0), 198.0, 469.0),
        c("flux_loop", Kind::Rect(42.0, 16.0), 243.0, 469.0),
        c("cue", Kind::Round(21.0), 40.0, 467.0),
        c("play", Kind::Round(21.0), 40.0, 529.0),
        c("pitch.fader", Kind::VFader(120.0), 308.0, 497.0),
    ]);
    for (i, (x, y)) in [(107.0, 504.0), (152.0, 504.0), (198.0, 504.0), (243.0, 504.0), (107.0, 539.0), (152.0, 539.0), (198.0, 539.0), (243.0, 539.0)]
        .into_iter()
        .enumerate()
    {
        v.push(c(&format!("pads.{}", i + 1), Kind::Pad(40.0, 26.0), x, y));
    }
    v
}

fn mx2_layout() -> Vec<Ctl> {
    let m = |id: &str, kind, x: f32, y: f32| Ctl { id: format!("surface.mixer.{id}"), kind, at: Vec2::new(x, y) };
    let mut v = deck_controls("left", 0.0);
    v.extend(deck_controls("right", RIGHT_DX));
    for (ch, x) in [(1, 387.0), (2, 516.0)] {
        let dx = if ch == 1 { -12.0 } else { -14.0 };
        v.extend([
            m(&format!("channels.{ch}.gain"), Kind::Knob(15.0), x, 29.0),
            m(&format!("channels.{ch}.deck_fx_assign.1"), Kind::Rect(25.0, 15.0), x + dx - 1.0, 74.0),
            m(&format!("channels.{ch}.deck_fx_assign.2"), Kind::Rect(25.0, 15.0), x + dx + 26.0, 74.0),
            m(&format!("channels.{ch}.eq.high"), Kind::Knob(15.0), x, 119.0),
            m(&format!("channels.{ch}.eq.mid"), Kind::Knob(15.0), x, 174.0),
            m(&format!("channels.{ch}.eq.low"), Kind::Knob(15.0), x, 229.0),
            m(&format!("channels.{ch}.channel_fx.amount"), Kind::Knob(15.0), x, 284.0),
            m(&format!("channels.{ch}.channel_fx.on"), Kind::Ring(13.0), x, 327.0),
            m(&format!("channels.{ch}.cue"), Kind::Rect(38.0, 16.0), x, 369.0),
            m(&format!("channels.{ch}.volume"), Kind::VFader(90.0), x, 452.0),
        ]);
    }
    v.extend([
        m("main.volume", Kind::Knob(17.0), 452.0, 29.0),
        m("cue_mix", Kind::Knob(15.0), 452.0, 174.0),
        m("cue_vol", Kind::Knob(15.0), 452.0, 229.0),
        m("channel_fx.fx1", Kind::Rect(24.0, 16.0), 439.0, 279.0),
        m("channel_fx.fx2", Kind::Rect(24.0, 16.0), 466.0, 279.0),
        m("channel_fx.fx3", Kind::Rect(24.0, 16.0), 439.0, 304.0),
        m("channel_fx.fx4", Kind::Rect(24.0, 16.0), 466.0, 304.0),
        m("channel_fx.filter", Kind::Rect(48.0, 16.0), 452.5, 329.0),
        m("mic", Kind::Round(11.0), 452.0, 372.0),
        m("xfader", Kind::HFader(80.0), 453.0, 537.0),
    ]);
    v
}

/// Mappable actions: (id, label, group). `{d}` is replaced by the deck number.
pub(crate) const BUTTON_ACTIONS: &[(&str, &str, &str)] = &[
    ("play:{d}", "Play / pause", "Transport"),
    ("cue:{d}", "Cue", "Transport"),
    ("cueplay:{d}", "Cue & play", "Transport"),
    ("sync:{d}", "Sync", "Transport"),
    ("flux:{d}", "Flux (slip)", "Transport"),
    ("loop:{d}", "Loop on / off", "Loops"),
    ("loop_half:{d}", "Halve loop", "Loops"),
    ("loop_double:{d}", "Double loop", "Loops"),
    ("hotcue:{d}:1", "Hotcue 1", "Hotcues"),
    ("hotcue:{d}:2", "Hotcue 2", "Hotcues"),
    ("hotcue:{d}:3", "Hotcue 3", "Hotcues"),
    ("hotcue:{d}:4", "Hotcue 4", "Hotcues"),
    ("hotcue:{d}:5", "Hotcue 5", "Hotcues"),
    ("hotcue:{d}:6", "Hotcue 6", "Hotcues"),
    ("hotcue:{d}:7", "Hotcue 7", "Hotcues"),
    ("hotcue:{d}:8", "Hotcue 8", "Hotcues"),
    ("stem:{d}:1", "Mute drums", "Stems"),
    ("stem:{d}:2", "Mute bass", "Stems"),
    ("stem:{d}:3", "Mute other", "Stems"),
    ("stem:{d}:4", "Mute vocals", "Stems"),
    ("stem_view:{d}", "Stem view", "Stems"),
    ("pads:{d}:hotcues", "Pads: hotcues", "Pad modes"),
    ("pads:{d}:stems", "Pads: stems", "Pad modes"),
    ("pads:{d}:pattern", "Pads: pattern", "Pad modes"),
    ("pads:{d}:loop", "Pads: loop roll", "Pad modes"),
    ("load:{d}", "Load selected track", "Library"),
    ("browse_down", "Next track", "Library"),
    ("browse_up", "Previous track", "Library"),
    ("filter_on:{d}", "Filter on / off", "Mixer"),
    ("headphone:{d}", "Headphone cue", "Mixer"),
    ("fx_on:1", "FX unit 1 on", "Mixer"),
    ("fx_on:2", "FX unit 2 on", "Mixer"),
    ("none", "Do nothing", "Other"),
];

pub(crate) const CONTINUOUS_ACTIONS: &[(&str, &str, &str)] = &[
    ("volume:{d}", "Channel volume", "Mixer"),
    ("gain:{d}", "Gain", "Mixer"),
    ("eq_hi:{d}", "EQ high", "Mixer"),
    ("eq_mid:{d}", "EQ mid", "Mixer"),
    ("eq_low:{d}", "EQ low", "Mixer"),
    ("filter:{d}", "Filter", "Mixer"),
    ("pitch:{d}", "Tempo", "Deck"),
    ("xfader", "Crossfader", "Mixer"),
    ("master_volume", "Main volume", "Mixer"),
    ("fx_mix:1", "FX 1 dry / wet", "FX"),
    ("fx_mix:2", "FX 2 dry / wet", "FX"),
    ("none", "Do nothing", "Other"),
];

/// MX2 RGB LED colour codes and how they look.
pub const LED_COLOURS: [(&str, u8, Color32); 8] = [
    ("Blue", 0x2E, Color32::from_rgb(40, 120, 255)),
    ("Green", 0x1E, Color32::from_rgb(60, 200, 90)),
    ("Red", 0x06, Color32::from_rgb(230, 50, 50)),
    ("Amber", 0x16, Color32::from_rgb(255, 190, 40)),
    ("Orange", 0x0E, Color32::from_rgb(255, 120, 20)),
    ("Cyan", 0x22, Color32::from_rgb(40, 210, 230)),
    ("Magenta", 0x36, Color32::from_rgb(220, 60, 200)),
    ("White", 0x4A, Color32::from_rgb(235, 235, 235)),
];

fn deck_of(id: &str) -> usize {
    if id.contains(".right.") || id.contains("channels.2") {
        2
    } else {
        1
    }
}

/// What a control does when not remapped (mirrors the built-in MX2 handling).
fn default_action(id: &str) -> String {
    let d = deck_of(id);
    let tail = id.rsplit_once(".left.").or_else(|| id.rsplit_once(".right.")).map(|(_, t)| t).unwrap_or(id);
    let s = match tail {
        "play" => format!("play:{d}"),
        "cue" if !id.contains("mixer") => format!("cue:{d}"),
        "sync" => format!("sync:{d}"),
        "flux" => format!("flux:{d}"),
        "hotcues" => format!("pads:{d}:hotcues"),
        "stems" => format!("pads:{d}:stems"),
        "pattern" => format!("pads:{d}:pattern"),
        "flux_loop" => format!("pads:{d}:loop"),
        "pitch.fader" => format!("pitch:{d}"),
        "browse.encoder" => "browse".into(),
        "loop_size" => format!("loop_size:{d}"),
        "loop_move" => format!("loop_move:{d}"),
        t if t.starts_with("pads.") => format!("pad:{d}:{}", &t[5..]),
        // Built-in functions with no remappable equivalent; shown by their panel name.
        "reverse" => "native:Reverse".into(),
        "mode.turntable" => "native:TT mode".into(),
        "mode.jogwheel" => "native:Jog mode".into(),
        "shift" => "native:Shift".into(),
        "master" => "native:Master".into(),
        "key_lock" => "native:Key lock".into(),
        "browse.favorite" => "native:Fav".into(),
        "browse.add_to_list" => "native:Add".into(),
        "browse.preview" => "native:Preview".into(),
        "browse.view" => "native:View".into(),
        "jogwheel.touch" => format!("jog:{d}"),
        _ => match id {
            "surface.mixer.mic" => "native:Mic".into(),
            "surface.mixer.channel_fx.filter" => "native:Filter".into(),
            _ if id.contains("channel_fx.fx") => format!("native:FX {}", &id[id.len() - 1..]),
            _ if id.contains("deck_fx_assign") => format!("native:FX{} on", &id[id.len() - 1..]),
            "surface.mixer.xfader" => "xfader".into(),
            "surface.mixer.main.volume" => "master_volume".into(),
            "surface.mixer.cue_mix" => "cue_mix".into(),
            "surface.mixer.cue_vol" => "cue_vol".into(),
            _ if id.ends_with(".volume") => format!("volume:{d}"),
            _ if id.ends_with(".gain") => format!("gain:{d}"),
            _ if id.ends_with("eq.high") => format!("eq_hi:{d}"),
            _ if id.ends_with("eq.mid") => format!("eq_mid:{d}"),
            _ if id.ends_with("eq.low") => format!("eq_low:{d}"),
            _ if id.ends_with("channel_fx.amount") => format!("filter:{d}"),
            _ if id.ends_with("channel_fx.on") => format!("filter_on:{d}"),
            _ if id.ends_with(".cue") => format!("headphone:{d}"),
            _ if id.contains(".fx.knobs.1") => format!("fx_mix:{d}"),
            _ if id.contains(".fx.knobs.") => format!("fx_param:{d}:{}", &id[id.len() - 1..]),
            _ if id.contains(".fx.buttons.") => format!("fx_button:{d}:{}", &id[id.len() - 1..]),
            _ if id.contains("jogwheel") => format!("jog:{d}"),
            _ => String::new(),
        },
    };
    s
}

/// Human label for an action id (`play:1` → "Play / pause A").
pub fn action_label(action: &str) -> String {
    let deck = |n: &str| match n {
        "1" => "A",
        "2" => "B",
        "3" => "C",
        "4" => "D",
        other => other,
    }
    .to_string();
    let parts: Vec<&str> = action.split(':').collect();
    let pattern = match parts.as_slice() {
        [verb, _, rest] => format!("{verb}:{{d}}:{rest}"),
        [verb, _] => format!("{verb}:{{d}}"),
        _ => action.to_string(),
    };
    let known = BUTTON_ACTIONS.iter().chain(CONTINUOUS_ACTIONS).find(|(id, _, _)| *id == pattern || *id == action);
    let suffix = parts.get(1).filter(|_| known.is_some_and(|(id, _, _)| id.contains("{d}"))).map(|d| format!(" {}", deck(d))).unwrap_or_default();
    match known {
        Some((_, label, _)) => format!("{label}{suffix}"),
        None => match parts.as_slice() {
            ["pad", d, n] => format!("Pad {n} {}", deck(d)),
            ["fx_param", d, n] => format!("FX {} knob {n}", deck(d)),
            ["fx_button", d, n] => format!("FX {} button {n}", deck(d)),
            ["jog", d] => format!("Jogwheel {}", deck(d)),
            ["jog_touch", d] => format!("Jog touch {}", deck(d)),
            ["loop_size", d] => format!("Loop size {}", deck(d)),
            ["loop_move", d] => format!("Move loop {}", deck(d)),
            ["browse"] => "Browse library".into(),
            ["native", label] => label.to_string(),
            ["cue_mix"] => "Headphone mix".into(),
            ["cue_vol"] => "Headphone volume".into(),
            [""] => "Not used".into(),
            _ => action.to_string(),
        },
    }
}

/// Very short label drawn on the replica itself; position already says which deck.
fn short_label(action: &str) -> String {
    let p: Vec<&str> = action.split(':').collect();
    let arg = p.get(2).copied().unwrap_or("");
    let s = match p[0] {
        "play" => "Play",
        "cue" => "Cue",
        "cueplay" => "Cue+Play",
        "sync" => "Sync",
        "flux" => "Flux",
        "loop" => "Loop",
        "loop_half" => "Loop ½",
        "loop_double" => "Loop ×2",
        "hotcue" => return format!("HC {arg}"),
        "pad" => return format!("Pad {arg}"),
        "stem" => ["Drums", "Bass", "Other", "Vocals"].get(arg.parse::<usize>().unwrap_or(1).saturating_sub(1)).copied().unwrap_or("Stem"),
        "stem_view" => "Stems",
        "pads" => match arg {
            "stems" => "Stems",
            "pattern" => "Pat",
            "loop" => "Roll",
            _ => "Cues",
        },
        "load" => "Load",
        "browse_down" => "Next",
        "browse_up" => "Prev",
        "browse" => "Browse",
        "filter_on" => "On",
        "filter" => "Filter",
        "headphone" => "Cue",
        "fx_on" => return format!("FX{}", p.get(1).unwrap_or(&"1")),
        "fx_mix" => "FX mix",
        "fx_param" => return format!("FX {arg}"),
        "fx_button" => return format!("FX {arg}"),
        "volume" => "Vol",
        "gain" => "Gain",
        "eq_hi" => "Hi",
        "eq_mid" => "Mid",
        "eq_low" => "Low",
        "pitch" => "Tempo",
        "xfader" => "X-fade",
        "master_volume" => "Main",
        "cue_mix" => "Mix",
        "cue_vol" => "Phones",
        "loop_size" => "Size",
        "loop_move" => "Move",
        "jog" => "Jog",
        "native" => match *p.get(1).unwrap_or(&"") {
            "Master" => "Mst",
            "Preview" => "Prev",
            "Key lock" => "Key",
            "Reverse" => "Rev",
            "TT mode" => "TT",
            "Jog mode" => "Jog",
            "FX1 on" => "FX1",
            "FX2 on" => "FX2",
            "FX 1" => "FX1",
            "FX 2" => "FX2",
            "FX 3" => "FX3",
            "FX 4" => "FX4",
            other => return other.to_string(),
        },
        "none" => "—",
        _ => return action_label(action),
    };
    s.to_string()
}

/// Persistent user mapping, saved as `control<TAB>action<TAB>led` lines.
#[derive(Default, Clone, PartialEq, Debug)]
pub struct Mapping {
    pub action: HashMap<String, String>,
    pub colour: HashMap<String, u8>,
}

impl Mapping {
    fn path() -> PathBuf {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/retraktor/mx2-mapping.tsv")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).map(|t| Self::parse(&t)).unwrap_or_default()
    }

    fn parse(text: &str) -> Self {
        let mut m = Self::default();
        for line in text.lines() {
            let mut f = line.split('\t');
            let (Some(id), Some(action), led) = (f.next(), f.next(), f.next()) else { continue };
            if !id.starts_with("surface.") {
                continue;
            }
            if !action.is_empty() {
                m.action.insert(id.to_string(), action.to_string());
            }
            if let Some(code) = led.and_then(|l| u8::from_str_radix(l.trim_start_matches("0x"), 16).ok()) {
                m.colour.insert(id.to_string(), code);
            }
        }
        m
    }

    fn to_text(&self) -> String {
        let mut ids: Vec<&String> = self.action.keys().chain(self.colour.keys()).collect();
        ids.sort();
        ids.dedup();
        ids.into_iter()
            .map(|id| {
                let a = self.action.get(id).map(String::as_str).unwrap_or("");
                let c = self.colour.get(id).map(|c| format!("{c:02x}")).unwrap_or_default();
                format!("{id}\t{a}\t{c}\n")
            })
            .collect()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.to_text())
    }
}

/// UI state for the mapper page.
#[derive(Default)]
pub struct MapperState {
    pub selected: Option<String>,
    /// Select whatever is touched on the hardware.
    pub learn: bool,
}

fn board_rect(ui: &Ui) -> (Rect, f32) {
    // Fit both the available width and leave room below the board for the legend line and detail panel.
    let sh = ui.ctx().screen_rect().height();
    let max_h = (sh - 230.0).clamp(360.0, sh * 0.68);
    let scale = (ui.available_width().min(1500.0) / BOARD.x).min(max_h / BOARD.y);
    let min = ui.cursor().min + Vec2::new(((ui.available_width() - BOARD.x * scale) / 2.0).max(0.0), 0.0);
    (Rect::from_min_size(min, BOARD * scale), scale)
}

fn hit_rect(c: &Ctl, origin: Pos2, s: f32) -> Rect {
    let center = origin + c.at * s;
    let size = match c.kind {
        Kind::Knob(r) | Kind::Encoder(r) | Kind::Ring(r) | Kind::Round(r) => Vec2::splat(r * 2.0 + 4.0),
        Kind::Rect(w, h) | Kind::Pad(w, h) => Vec2::new(w + 2.0, h + 2.0),
        Kind::VFader(l) => Vec2::new(26.0, l + 10.0),
        Kind::HFader(l) => Vec2::new(l + 10.0, 26.0),
        Kind::Jog(r) => Vec2::splat(r * 2.0),
    };
    let screen_size = (size * s).max(Vec2::splat(24.1));
    Rect::from_center_size(center, screen_size)
}

/// Label anchor in board units and alignment for any control.
fn label_anchor(c: &Ctl) -> (Vec2, Align2) {
    match c.kind {
        Kind::Pad(..) | Kind::Rect(..) | Kind::Jog(_) => (c.at, Align2::CENTER_CENTER),
        Kind::Knob(rad) | Kind::Encoder(rad) | Kind::Ring(rad) | Kind::Round(rad) => (c.at + Vec2::new(0.0, rad + 7.5), Align2::CENTER_CENTER),
        Kind::VFader(len) => (c.at + Vec2::new(0.0, len / 2.0 + 8.0), Align2::CENTER_CENTER),
        Kind::HFader(_) => (c.at + Vec2::new(0.0, 16.0), Align2::CENTER_CENTER),
    }
}

/// Finds the nearest control in the given cardinal direction (`dir` unit vector).
fn navigate_control<'a>(controls: &'a [Ctl], from_id: &str, dir: Vec2) -> Option<&'a str> {
    let cur = controls.iter().find(|c| c.id == from_id)?;
    controls
        .iter()
        .filter(|c| c.id != from_id)
        .filter_map(|c| {
            let delta = c.at - cur.at;
            let along = delta.dot(dir);
            if along <= 2.0 {
                return None;
            }
            let perp = (delta - dir * along).length();
            if perp > along * 1.5 {
                return None;
            }
            Some((c.id.as_str(), along + perp * 2.2))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// Draws the replica and the detail panel. Returns true when the mapping changed.
pub fn show(ui: &mut Ui, map: &mut Mapping, state: &mut MapperState, last_touched: Option<&str>, connected: bool) -> bool {
    let mut changed = false;
    if state.learn {
        if let Some(t) = last_touched {
            state.selected = Some(t.to_string());
        }
    }
    let controls = mx2_layout();
    if !ui.ctx().wants_keyboard_input() {
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.selected = None;
        } else if let Some(sel) = state.selected.clone() {
            let dir = ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowLeft) {
                    Some(Vec2::new(-1.0, 0.0))
                } else if i.key_pressed(egui::Key::ArrowRight) {
                    Some(Vec2::new(1.0, 0.0))
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    Some(Vec2::new(0.0, -1.0))
                } else if i.key_pressed(egui::Key::ArrowDown) {
                    Some(Vec2::new(0.0, 1.0))
                } else {
                    None
                }
            });
            if let Some(d) = dir {
                if let Some(next) = navigate_control(&controls, &sel, d) {
                    state.selected = Some(next.to_string());
                }
            }
        }
    }

    ui.horizontal(|ui| {
        ui.label(RichText::new("Traktor MX2").size(16.0).strong());
        ui.label(RichText::new(if connected { "connected" } else { "not connected" }).size(12.0).color(if connected { TraktorTheme::PLAY_ACTIVE() } else { TraktorTheme::TEXT_MUTED() }));
        ui.add_space(12.0);
        ui.add_enabled(connected, egui::Checkbox::new(&mut state.learn, "Press a control on the MX2 to select it"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !map.action.is_empty() || !map.colour.is_empty() {
                if ui.button("Reset all to default").clicked() {
                    *map = Mapping::default();
                    changed = true;
                }
            }
            ui.label(RichText::new(format!("{} remapped", map.action.len())).size(12.0).color(TraktorTheme::TEXT_SECONDARY()));
        });
    });
    ui.add_space(6.0);

    let (board, s) = board_rect(ui);
    let response = ui.allocate_rect(board, Sense::hover());
    let painter = ui.painter_at(board.expand(4.0));
    let line = TraktorTheme::TEXT_SECONDARY();
    let faint = TraktorTheme::BORDER_ACCENT();
    let accent = TraktorTheme::ACCENT();
    let stroke = |c: Color32, w: f32| Stroke::new(w * s.max(0.8), c);
    painter.rect_filled(board, 10.0 * s, TraktorTheme::BG_WINDOW());
    painter.rect_stroke(board, 10.0 * s, stroke(line, 1.2));
    // Section dividers and meters: context only, not interactive.
    for x in [362.0, 543.0] {
        painter.line_segment([board.min + Vec2::new(x, 14.0) * s, board.min + Vec2::new(x, 558.0) * s], stroke(faint, 0.8));
    }
    for (x, n) in [(440.0, 9), (465.0, 9)] {
        for i in 0..n {
            painter.circle_filled(board.min + Vec2::new(x, 70.0 + i as f32 * 8.0) * s, 1.6 * s, faint);
        }
    }
    let label_font = FontId::proportional((9.5 * s).clamp(9.0, 12.0));

    let mut clicked: Option<String> = None;
    let mut hovered_ctl: Option<&Ctl> = None;
    let mut dropped_labels = false;
    for c in &controls {
        let hit = hit_rect(c, board.min, s);
        let r = ui.interact(hit, ui.id().with(&c.id), Sense::click());
        if r.hovered() {
            hovered_ctl = Some(c);
        }
        let selected = state.selected.as_deref() == Some(c.id.as_str());
        let custom = map.action.contains_key(&c.id) || map.colour.contains_key(&c.id);
        let col = if selected { accent } else if r.hovered() { TraktorTheme::TEXT_PRIMARY() } else if custom { accent.gamma_multiply(0.85) } else { line };
        let w = if selected { 2.4 } else { 1.1 };
        let center = board.min + c.at * s;
        let led = map.colour.get(&c.id).and_then(|code| LED_COLOURS.iter().find(|(_, v, _)| v == code)).map(|(_, _, col)| *col);
        match c.kind {
            Kind::Knob(rad) => {
                painter.circle_stroke(center, rad * s, stroke(col, w));
                painter.line_segment([center, center + Vec2::new(0.0, -rad * 0.8) * s], stroke(col, w));
            }
            Kind::Encoder(rad) => {
                painter.circle_stroke(center, rad * s, stroke(col, w));
                painter.circle_stroke(center, rad * 0.7 * s, stroke(col, w * 0.7));
            }
            Kind::Ring(rad) | Kind::Round(rad) => {
                if let Some(l) = led {
                    painter.circle_filled(center, rad * s, l);
                }
                painter.circle_stroke(center, rad * s, stroke(if selected { accent } else { led.unwrap_or(col) }, w));
                if matches!(c.kind, Kind::Ring(_)) {
                    painter.circle_stroke(center, rad * 0.6 * s, stroke(col, w * 0.7));
                }
            }
            Kind::Rect(rw, rh) | Kind::Pad(rw, rh) => {
                let rr = Rect::from_center_size(center, Vec2::new(rw, rh) * s);
                if let Some(l) = led {
                    painter.rect_filled(rr, 3.0 * s, l);
                }
                painter.rect_stroke(rr, 3.0 * s, stroke(if selected { accent } else { led.unwrap_or(col) }, w));
            }
            Kind::VFader(len) => {
                painter.line_segment([center - Vec2::new(0.0, len / 2.0) * s, center + Vec2::new(0.0, len / 2.0) * s], stroke(col, w));
                painter.rect_stroke(Rect::from_center_size(center, Vec2::new(20.0, 8.0) * s), 2.0 * s, stroke(col, w));
            }
            Kind::HFader(len) => {
                painter.line_segment([center - Vec2::new(len / 2.0, 0.0) * s, center + Vec2::new(len / 2.0, 0.0) * s], stroke(col, w));
                painter.rect_stroke(Rect::from_center_size(center, Vec2::new(8.0, 20.0) * s), 2.0 * s, stroke(col, w));
            }
            Kind::Jog(rad) => {
                painter.circle_stroke(center, rad * s, stroke(col, w));
                painter.circle_stroke(center, rad * 0.83 * s, stroke(col, w * 0.7));
                painter.line_segment([center + Vec2::new(0.0, -rad * 0.8) * s, center + Vec2::new(0.0, -rad * 0.5) * s], stroke(col, w));
            }
        }

        // Non-colour markers for remapped (corner dot) and selected (corner brackets + top marker dot).
        let bounds = match c.kind {
            Kind::Knob(r) | Kind::Encoder(r) | Kind::Ring(r) | Kind::Round(r) | Kind::Jog(r) => Rect::from_center_size(center, Vec2::splat(r * 2.0 * s)),
            Kind::Rect(rw, rh) | Kind::Pad(rw, rh) => Rect::from_center_size(center, Vec2::new(rw, rh) * s),
            Kind::VFader(len) => Rect::from_center_size(center, Vec2::new(20.0, len) * s),
            Kind::HFader(len) => Rect::from_center_size(center, Vec2::new(len, 20.0) * s),
        };
        if custom {
            let dot_pos = bounds.right_top() + Vec2::new(-2.0, 2.0);
            painter.circle_filled(dot_pos, 3.2, accent);
            painter.circle_stroke(dot_pos, 3.2, Stroke::new(1.0_f32, crate::theme::on(accent)));
        }
        if selected {
            let b = bounds.expand(3.5);
            let arm = 5.0_f32.min(b.width() * 0.3).min(b.height() * 0.3);
            let st = Stroke::new(2.0_f32, accent);
            for (corner, dx, dy) in [
                (b.left_top(), arm, arm),
                (b.right_top(), -arm, arm),
                (b.left_bottom(), arm, -arm),
                (b.right_bottom(), -arm, -arm),
            ] {
                painter.line_segment([corner, corner + Vec2::new(dx, 0.0)], st);
                painter.line_segment([corner, corner + Vec2::new(0.0, dy)], st);
            }
            painter.circle_filled(Pos2::new(b.center().x, b.top() - 2.0), 3.0, accent);
        }

        // What it does, written on the replica (font floor 9 px).
        let action = map.action.get(&c.id).cloned().unwrap_or_else(|| default_action(&c.id));
        let text = short_label(&action);
        let fill_col = match c.kind {
            Kind::Rect(..) | Kind::Pad(..) => led,
            _ => None,
        };
        let text_col = if let Some(fill) = fill_col {
            crate::theme::on(fill)
        } else if custom || selected {
            accent
        } else {
            TraktorTheme::TEXT_PRIMARY()
        };
        let (anchor, align) = label_anchor(c);
        let pos = board.min + anchor * s;
        match c.kind {
            Kind::Pad(rw, _) | Kind::Rect(rw, _) => {
                let mut drawn = false;
                for size in [label_font.size, 9.0] {
                    let font = FontId::proportional(size);
                    let galley = painter.layout_no_wrap(text.clone(), font.clone(), text_col);
                    if galley.size().x <= rw * s - 0.5 {
                        painter.text(pos, align, &text, font, text_col);
                        drawn = true;
                        break;
                    }
                }
                if !drawn {
                    dropped_labels = true;
                }
            }
            Kind::Jog(_) | Kind::VFader(_) | Kind::HFader(_) => {
                painter.text(pos, align, &text, label_font.clone(), text_col);
            }
            Kind::Knob(_) | Kind::Encoder(_) | Kind::Ring(_) | Kind::Round(_) => {
                let font = FontId::proportional(label_font.size.max(9.0));
                let galley = painter.layout_no_wrap(text.clone(), font.clone(), text_col);
                if galley.size().x <= 46.0 * s {
                    painter.text(pos, align, &text, font, text_col);
                } else {
                    dropped_labels = true;
                }
            }
        }
        let r = r.on_hover_text(format!("{}\n{}", action_label(&action), pretty_control(&c.id)));
        if r.clicked() {
            clicked = Some(c.id.clone());
        }
    }
    if let Some(id) = clicked {
        state.selected = if state.selected.as_ref() == Some(&id) { None } else { Some(id) };
    }
    let _ = response;
    ui.add_space(4.0);

    // Fixed status / legend line under the board so small windows & hover aren't tooltip-only.
    let focus_ctl = hovered_ctl.or_else(|| state.selected.as_deref().and_then(|id| controls.iter().find(|c| c.id == id)));
    ui.horizontal_wrapped(|ui| {
        if let Some(c) = focus_ctl {
            let act = map.action.get(&c.id).cloned().unwrap_or_else(|| default_action(&c.id));
            ui.label(RichText::new(format!("{} → {}", pretty_control(&c.id), action_label(&act))).size(12.0).strong().color(TraktorTheme::TEXT_PRIMARY()));
        } else {
            ui.label(RichText::new("Hover or click any control (or use arrow keys) to inspect its action.").size(12.0).color(TraktorTheme::TEXT_SECONDARY()));
        }
        if dropped_labels || s < 1.0 {
            ui.label(
                RichText::new("· Legend: Fav/Add/Prev/View (library) · TT/Jog/Rev/Flux (jog) · Sync/Mst (tempo) · Cues/Stems/Pat/Roll (pads)")
                    .size(11.0)
                    .color(TraktorTheme::TEXT_MUTED()),
            );
        }
    });
    ui.add_space(4.0);

    // Detail panel for the selected control.
    let Some(sel) = state.selected.clone() else {
        ui.label(RichText::new("Click any control to see or change what it does.").color(TraktorTheme::TEXT_SECONDARY()));
        return changed;
    };
    let Some(ctl) = controls.iter().find(|c| c.id == sel) else { return changed };
    let d = deck_of(&sel);
    let default = default_action(&sel);
    let current = map.action.get(&sel).cloned().unwrap_or_else(|| default.clone());
    egui::Frame::none().fill(TraktorTheme::BG_PANEL()).rounding(6.0).inner_margin(14.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(pretty_control(&sel)).size(16.0).strong());
            ui.label(RichText::new(&sel).size(11.0).color(TraktorTheme::TEXT_MUTED()));
        });
        ui.add_space(6.0);
        if matches!(ctl.kind, Kind::Jog(_)) {
            ui.label("Jogwheel behaviour (scratch or nudge) and sensitivity are under Decks & Mixer.");
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Does");
            let catalogue = if ctl.kind.continuous() { CONTINUOUS_ACTIONS } else { BUTTON_ACTIONS };
            egui::ComboBox::from_id_salt(("map_action", &sel)).selected_text(action_label(&current)).width(240.0).height(420.0).show_ui(ui, |ui| {
                if ui.selectable_label(!map.action.contains_key(&sel), format!("Default: {}", action_label(&default))).clicked() {
                    map.action.remove(&sel);
                    changed = true;
                }
                let mut group = "";
                for (id, _, g) in catalogue {
                    if *g != group {
                        group = g;
                        ui.label(RichText::new(*g).size(11.0).color(TraktorTheme::TEXT_MUTED()));
                    }
                    let concrete = id.replace("{d}", &d.to_string());
                    if ui.selectable_label(current == concrete, action_label(&concrete)).clicked() {
                        map.action.insert(sel.clone(), concrete);
                        changed = true;
                    }
                }
            });
            if map.action.contains_key(&sel) && ui.button("Use default").clicked() {
                map.action.remove(&sel);
                changed = true;
            }
        });
        if matches!(ctl.kind, Kind::Ring(_) | Kind::Round(_) | Kind::Rect(..) | Kind::Pad(..)) {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Light");
                let cur = map.colour.get(&sel).copied();
                for (name, code, col) in LED_COLOURS {
                    let (rect, r) = ui.allocate_exact_size(Vec2::splat(26.0), Sense::click());
                    ui.painter().circle_filled(rect.center(), 10.0, col);
                    if cur == Some(code) {
                        ui.painter().circle_stroke(rect.center(), 12.5, Stroke::new(2.0_f32, TraktorTheme::TEXT_PRIMARY()));
                        ui.painter().circle_filled(rect.center(), 3.0, crate::theme::on(col));
                    }
                    if r.on_hover_text(name).clicked() {
                        map.colour.insert(sel.clone(), code);
                        changed = true;
                    }
                }
                if cur.is_some() && ui.button("Default light").clicked() {
                    map.colour.remove(&sel);
                    changed = true;
                }
            });
        }
    });
    changed
}

/// "surface.left.pads.3" → "Left deck · Pad 3".
fn pretty_control(id: &str) -> String {
    let rest = id.trim_start_matches("surface.");
    let (area, tail) = rest.split_once('.').unwrap_or((rest, ""));
    let area = match area {
        "left" => "Left deck",
        "right" => "Right deck",
        _ => "Mixer",
    };
    let words = tail
        .replace("pads.", "pad ")
        .replace("channels.", "channel ")
        .replace("fx.knobs.", "FX knob ")
        .replace("fx.buttons.", "FX button ")
        .replace("channel_fx.", "mixer FX ")
        .replace("eq.", "EQ ")
        .replace(['.', '_'], " ");
    let mut c = words.chars();
    let cap: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    format!("{area} · {cap}")
}

/// A user mapping resolved to something the app can execute.
#[derive(Debug, PartialEq)]
pub enum Mapped {
    Button { verb: String, deck: usize, arg: String },
    Value { target: String, deck: usize },
    Nothing,
}

/// Parses an action id; `None` means "not remapped / unknown, use built-in handling".
pub fn parse(action: &str, continuous: bool) -> Option<Mapped> {
    if action == "none" {
        return Some(Mapped::Nothing);
    }
    let mut p = action.split(':');
    let verb = p.next()?.to_string();
    let deck = p.next().and_then(|d| d.parse().ok()).unwrap_or(1);
    let arg = p.next().unwrap_or("").to_string();
    Some(if continuous { Mapped::Value { target: verb, deck } } else { Mapped::Button { verb, deck, arg } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_covers_every_mx2_control_once() {
        let ids: Vec<String> = mx2_layout().into_iter().map(|c| c.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "duplicate control ids");
        for name in retraktor_hid::MX2_POT_FADER_NAMES.iter().chain(retraktor_hid::MX2_ENCODER_NAMES.iter()) {
            assert!(ids.iter().any(|i| i == name), "missing {name}");
        }
        for name in retraktor_hid::MX2_BUTTON_NAMES.iter().filter(|n| !n.ends_with(".push") && !n.contains("jogwheel")) {
            assert!(ids.iter().any(|i| i == name), "missing {name}");
        }
    }

    #[test]
    fn mapping_roundtrips_and_labels_read_well() {
        let mut m = Mapping::default();
        m.action.insert("surface.left.pads.1".into(), "hotcue:1:5".into());
        m.colour.insert("surface.left.pads.1".into(), 0x36);
        m.colour.insert("surface.right.play".into(), 0x1E);
        assert_eq!(Mapping::parse(&(m.to_text() + "garbage\nbad\tline\n")), m);
        assert_eq!(action_label("hotcue:2:5"), "Hotcue 5 B");
        assert_eq!(action_label(&default_action("surface.right.play")), "Play / pause B");
        assert_eq!(action_label(&default_action("surface.mixer.channels.2.eq.low")), "EQ low B");
        assert_eq!(parse("stem:2:4", false), Some(Mapped::Button { verb: "stem".into(), deck: 2, arg: "4".into() }));
        assert_eq!(parse("none", true), Some(Mapped::Nothing));
    }

    #[test]
    fn label_anchors_and_hit_rects_do_not_collide_in_row() {
        let controls = mx2_layout();
        // At 1024x640, scale is 410 / 572 ≈ 0.717; verify every hit_rect is >= 24x24 and same-row anchors/rects stay separated.
        let s = (640.0 - 230.0) / BOARD.y;
        for (i, a) in controls.iter().enumerate() {
            let hr = hit_rect(a, Pos2::ZERO, s);
            assert!(hr.width() >= 24.0 && hr.height() >= 24.0, "{} hit_rect too small: {:?}", a.id, hr.size());
            let (pa, _) = label_anchor(a);
            for b in &controls[i + 1..] {
                let (pb, _) = label_anchor(b);
                if (pa.y - pb.y).abs() < 6.0 {
                    assert!((pa.x - pb.x).abs() >= 20.0, "label anchors collide: {} vs {}", a.id, b.id);
                }
                if !matches!(a.kind, Kind::Jog(_)) && !matches!(b.kind, Kind::Jog(_)) && (a.at.y - b.at.y).abs() < 6.0 {
                    let hrb = hit_rect(b, Pos2::ZERO, s);
                    assert!(hr.intersect(hrb).width() < hr.width() * 0.5, "hit rects collide on row: {} vs {}", a.id, b.id);
                }
            }
        }
        assert_eq!(navigate_control(&controls, "surface.left.sync", Vec2::new(1.0, 0.0)), Some("surface.left.master"));
        assert_eq!(navigate_control(&controls, "surface.left.master", Vec2::new(-1.0, 0.0)), Some("surface.left.sync"));
    }
}

