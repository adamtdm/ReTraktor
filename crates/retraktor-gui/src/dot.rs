//! Reusable dot-matrix display primitives for the Mix view and companion screens.
//!
//! All colours come from [`crate::theme::TraktorTheme`] and [`crate::theme`] helpers;
//! all motion is gated by [`crate::theme::animations`].

use crate::theme::{self, TraktorTheme};
use eframe::egui::{Align2, Color32, FontId, Key, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

/// Visual state of a single dot on a dot-matrix grid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DotCell {
    /// Faint background grid dot so the matrix structure always reads.
    Unlit,
    /// Fully lit filled dot at normal radius.
    Filled(Color32),
    /// Lit filled dot with a radius multiplier (e.g. shrunk above a fader bracket).
    Scaled(Color32, f32),
    /// Hollow ring dot (used for killed EQ bands, muted stems, or stopped beat indicators).
    Hollow(Color32),
}

/// Rendering mode for ASCII sprite `#` cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpriteMode {
    /// Render lit dots as filled circles.
    Filled,
    /// Render lit dots as hollow ring circles (or dim outline when off).
    Hollow,
}

/// A tiny dot-matrix sprite defined as ASCII bitmaps in code.
/// Each frame is an array of string slices representing rows from top to bottom.
/// In each row:
/// '#' or '*' = lit dot (filled or hollow depending on `SpriteMode`)
/// 'o' or 'O' = hollow dot
/// '.' or ' ' = unlit dot
#[derive(Clone, Copy, Debug)]
pub struct Sprite {
    pub name: &'static str,
    pub frames: &'static [&'static [&'static str]],
}

impl Sprite {
    pub const fn new(name: &'static str, frames: &'static [&'static [&'static str]]) -> Self {
        Self { name, frames }
    }

    pub fn frame(&self, index: usize) -> &'static [&'static str] {
        if self.frames.is_empty() {
            &[]
        } else {
            self.frames[index % self.frames.len()]
        }
    }

    pub fn dimensions(&self, frame_index: usize) -> (usize, usize) {
        let f = self.frame(frame_index);
        let h = f.len();
        let w = if h > 0 { f[0].len() } else { 0 };
        (w, h)
    }
}

/// Pure helper: returns the vertical bounce offset in whole dots at elapsed seconds `elapsed_sec`
/// within a bounce animation of total duration `duration_sec` (e.g. 0.6 s).
/// Returns 0 when elapsed_sec >= duration_sec or if animations are disabled.
pub fn bounce_dot_offset(elapsed_sec: f32, duration_sec: f32, max_hop_dots: i32) -> i32 {
    if !theme::animations() || elapsed_sec <= 0.0 || elapsed_sec >= duration_sec || duration_sec <= 0.0 {
        return 0;
    }
    let t = (elapsed_sec / duration_sec).clamp(0.0, 1.0);
    // 2.5 decaying sinusoidal hops
    let decay = (1.0 - t).powi(2);
    let wave = (t * std::f32::consts::PI * 3.0).sin().abs();
    (wave * decay * max_hop_dots as f32).round() as i32
}

/// Paints a dot-matrix sprite centered at `center` with dot spacing `pitch`.
/// `bounce_dots`: vertical offset in integer dots (positive = bounced upwards by `bounce_dots * pitch.y`),
/// keeping the sprite strictly on the grid.
/// `draw_unlit`: if true, draws unlit dots with `unlit_col`; if false, leaves them transparent.
pub fn paint_sprite(
    painter: &Painter,
    center: Pos2,
    pitch: Vec2,
    dot_radius: f32,
    sprite: &Sprite,
    frame_index: usize,
    bounce_dots: i32,
    mode: SpriteMode,
    color: Color32,
    unlit_col: Color32,
    draw_unlit: bool,
) {
    let frame = sprite.frame(frame_index);
    let h = frame.len();
    if h == 0 {
        return;
    }
    let w = frame[0].len();
    if w == 0 {
        return;
    }

    let start_x = center.x - (w as f32 - 1.0) * 0.5 * pitch.x;
    let start_y = center.y - (h as f32 - 1.0) * 0.5 * pitch.y - (bounce_dots as f32 * pitch.y);

    for (r, row_str) in frame.iter().enumerate() {
        let y = start_y + r as f32 * pitch.y;
        for (c, ch) in row_str.chars().enumerate() {
            let x = start_x + c as f32 * pitch.x;
            let pt = Pos2::new(x, y);
            match ch {
                '#' | '*' => {
                    let cell = match mode {
                        SpriteMode::Filled => DotCell::Filled(color),
                        SpriteMode::Hollow => DotCell::Hollow(color),
                    };
                    paint_dot(painter, pt, dot_radius, cell, unlit_col);
                }
                'o' | 'O' => {
                    paint_dot(painter, pt, dot_radius, DotCell::Hollow(color), unlit_col);
                }
                '.' | ' ' => {
                    if draw_unlit {
                        paint_dot(painter, pt, dot_radius, DotCell::Unlit, unlit_col);
                    }
                }
                _ => {}
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Stem & EQ Sprites
// ---------------------------------------------------------------------------

pub static SPRITE_DRUMS: Sprite = Sprite::new(
    "DRUMS",
    &[&[
        "#...#",
        "#####",
        "#...#",
        "#.#.#",
        "#####",
    ]],
);

pub static SPRITE_BASS: Sprite = Sprite::new(
    "BASS",
    &[&[
        "..#..",
        "..#..",
        ".###.",
        "#.#.#",
        "#####",
        ".###.",
    ]],
);

pub static SPRITE_OTHER: Sprite = Sprite::new(
    "OTHER",
    &[&[
        "#####",
        "#.#.#",
        "#.#.#",
        "#####",
        "#####",
    ]],
);

pub static SPRITE_VOCALS: Sprite = Sprite::new(
    "VOCALS",
    &[&[
        ".###.",
        "#####",
        "#####",
        ".###.",
        "..#..",
    ]],
);

pub static STEM_SPRITES: [&Sprite; 4] = [
    &SPRITE_DRUMS,
    &SPRITE_BASS,
    &SPRITE_OTHER,
    &SPRITE_VOCALS,
];

pub static SPRITE_KILL_X: Sprite = Sprite::new(
    "KILL",
    &[&[
        "#...#",
        ".#.#.",
        "..#..",
        ".#.#.",
        "#...#",
    ]],
);

pub static SPRITE_TICK: Sprite = Sprite::new(
    "RESTORE",
    &[&[
        "....#",
        "...#.",
        "#..#.",
        ".#.#.",
        "..#..",
    ]],
);

// ---------------------------------------------------------------------------
// Crossfade Cheer Sprites (10 distinct animations)
// ---------------------------------------------------------------------------

pub static SPRITE_THUMBS_UP: Sprite = Sprite::new(
    "THUMBS UP",
    &[&[
        "...##....",
        "...#.#...",
        "...#.#...",
        ".###.#...",
        "#..#.#...",
        "#..###...",
        "#........",
        ".#######.",
        ".#.....#.",
        ".#######.",
    ]],
);

pub static SPRITE_SMILEY: Sprite = Sprite::new(
    "SMILEY",
    &[
        &[
            "..#####..",
            ".#.....#.",
            "#..#.#..#",
            "#.......#",
            "#.#...#.#",
            "#..###..#",
            "#.......#",
            ".#.....#.",
            "..#####..",
        ],
        &[
            "..#####..",
            ".#.....#.",
            "#..###..#",
            "#.......#",
            "#.#...#.#",
            "#..###..#",
            "#.......#",
            ".#.....#.",
            "..#####..",
        ],
    ],
);

pub static SPRITE_HEART: Sprite = Sprite::new(
    "HEART",
    &[
        &[
            ".##...##.",
            "####.####",
            "#########",
            "#########",
            ".#######.",
            "..#####..",
            "...###...",
            "....#....",
            ".........",
        ],
        &[
            "###...###",
            "#########",
            "#########",
            "#########",
            ".#######.",
            "..#####..",
            "...###...",
            "....#....",
            ".........",
        ],
    ],
);

pub static SPRITE_STAR: Sprite = Sprite::new(
    "STAR",
    &[
        &[
            "....#....",
            "....#....",
            "..#####..",
            ".#######.",
            "#########",
            "..#####..",
            ".##...##.",
            "#.......#",
            ".........",
        ],
        &[
            "#...#...#",
            ".#..#..#.",
            "..#####..",
            ".#######.",
            "###.#.###",
            "..#####..",
            ".#..#..#.",
            "#...#...#",
            "....#....",
        ],
    ],
);

pub static SPRITE_FLAME: Sprite = Sprite::new(
    "FLAME",
    &[
        &[
            "....#....",
            "...###...",
            "...###...",
            "..#####..",
            ".######..",
            ".#######.",
            "#########",
            ".#######.",
            "..#####..",
        ],
        &[
            ".....#...",
            "....##...",
            "...###...",
            "..#####..",
            "..######.",
            ".#######.",
            "#########",
            ".#######.",
            "..#####..",
        ],
    ],
);

pub static SPRITE_ROCKET: Sprite = Sprite::new(
    "ROCKET",
    &[
        &[
            "....#....",
            "...###...",
            "...###...",
            "..#####..",
            "..#####..",
            ".#######.",
            ".##.#.##.",
            "##..#..##",
            "....#....",
            "...###...",
            "....#....",
        ],
        &[
            "....#....",
            "...###...",
            "...###...",
            "..#####..",
            "..#####..",
            ".#######.",
            ".##.#.##.",
            "##..#..##",
            "...###...",
            "....#....",
            "...###...",
        ],
    ],
);

pub static SPRITE_NICE: Sprite = Sprite::new(
    "NICE",
    &[&[
        "#...#..###..###..####",
        "##..#...#...#....#...",
        "#.#.#...#...#....###.",
        "#..##...#...#....#...",
        "#...#..###..###..####",
    ]],
);

pub static SPRITE_SMOOTH: Sprite = Sprite::new(
    "SMOOTH",
    &[&[
        ".##.#...#.###.###.###.#.#",
        "#...##.##.#.#.#.#..#..#.#",
        ".##.#.#.#.#.#.#.#..#..###",
        "..#.#...#.#.#.#.#..#..#.#",
        "##..#...#.###.###..#..#.#",
    ]],
);

pub static SPRITE_SHADES: Sprite = Sprite::new(
    "SUNGLASSES",
    &[&[
        "..#####..",
        ".#.....#.",
        "#########",
        "#.##.##.#",
        "#########",
        "#.......#",
        "#..###..#",
        ".#.....#.",
        "..#####..",
    ]],
);

pub static SPRITE_TROPHY: Sprite = Sprite::new(
    "TROPHY",
    &[&[
        ".#######.",
        ".#######.",
        "#.#####.#",
        "#..###..#",
        "...###...",
        "....#....",
        "....#....",
        "..#####..",
        ".#######.",
    ]],
);

pub static CHEER_SPRITES: [&Sprite; 10] = [
    &SPRITE_THUMBS_UP,
    &SPRITE_SMILEY,
    &SPRITE_HEART,
    &SPRITE_STAR,
    &SPRITE_FLAME,
    &SPRITE_ROCKET,
    &SPRITE_NICE,
    &SPRITE_SMOOTH,
    &SPRITE_SHADES,
    &SPRITE_TROPHY,
];

pub const CHEER_SPRITES_COUNT: usize = 10;

// ---------------------------------------------------------------------------
// Crossfade Cheer Detection & State
// ---------------------------------------------------------------------------

/// Active cheer animation state on a deck.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveCheer {
    pub deck: usize,
    pub cheer_index: usize,
    pub start_sec: f64,
}

/// Track side for the crossfader (< 0.1 or > 0.9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XfaderSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum CheerState {
    Neutral,
    AtSide {
        side: XfaderSide,
        arrived_at: f64,
        can_cheer: bool,
        cheered: bool,
    },
    InTransit {
        from_side: XfaderSide,
        both_playing: bool,
    },
}

/// Pure state machine detecting a completed crossfade transition between decks A and B.
///
/// When the crossfader has travelled from one side (< 0.1 or > 0.9) to the other
/// while both decks were playing, and then rests at the far side for 2 seconds,
/// it emits a cheer event with the cheer sprite index.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossfadeCheerStateMachine {
    state: CheerState,
    last_cheer_index: Option<usize>,
    pub target_deck: usize,
}

impl Default for CrossfadeCheerStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

impl CrossfadeCheerStateMachine {
    pub const fn new() -> Self {
        Self {
            state: CheerState::Neutral,
            last_cheer_index: None,
            target_deck: 0,
        }
    }

    pub fn update(&mut self, crossfader: f32, both_playing: bool, now_sec: f64) -> Option<usize> {
        let current_side = if crossfader < 0.15 {
            Some(XfaderSide::Left)
        } else if crossfader > 0.85 {
            Some(XfaderSide::Right)
        } else {
            None
        };

        match self.state {
            CheerState::Neutral => {
                if let Some(side) = current_side {
                    self.state = CheerState::AtSide {
                        side,
                        arrived_at: now_sec,
                        can_cheer: false,
                        cheered: false,
                    };
                }
            }
            CheerState::AtSide { side, arrived_at, can_cheer, cheered } => {
                match current_side {
                    Some(curr) if curr == side => {
                        // Still resting at the same side
                        if can_cheer && !cheered && (now_sec - arrived_at >= 2.0) {
                            self.state = CheerState::AtSide {
                                side,
                                arrived_at,
                                can_cheer,
                                cheered: true,
                            };
                            self.target_deck = if side == XfaderSide::Left { 0 } else { 1 };
                            let next_idx = match self.last_cheer_index {
                                Some(last) => (last + 1) % CHEER_SPRITES_COUNT,
                                None => (now_sec.floor() as usize) % CHEER_SPRITES_COUNT,
                            };
                            self.last_cheer_index = Some(next_idx);
                            return Some(next_idx);
                        }
                    }
                    Some(curr) => {
                        // Direct jump from one side to the other in a single frame
                        self.state = CheerState::AtSide {
                            side: curr,
                            arrived_at: now_sec,
                            can_cheer: both_playing,
                            cheered: false,
                        };
                    }
                    None => {
                        // Moving into transit from `side`
                        self.state = CheerState::InTransit {
                            from_side: side,
                            both_playing,
                        };
                    }
                }
            }
            CheerState::InTransit { from_side, both_playing: was_both_playing } => {
                let still_both_playing = was_both_playing && both_playing;
                match current_side {
                    None => {
                        // Still in transit
                        self.state = CheerState::InTransit {
                            from_side,
                            both_playing: still_both_playing,
                        };
                    }
                    Some(curr) if curr == from_side => {
                        // Returned back to origin without reaching far side
                        self.state = CheerState::AtSide {
                            side: from_side,
                            arrived_at: now_sec,
                            can_cheer: false,
                            cheered: false,
                        };
                    }
                    Some(curr) => {
                        // Reached the far side!
                        self.state = CheerState::AtSide {
                            side: curr,
                            arrived_at: now_sec,
                            can_cheer: still_both_playing,
                            cheered: false,
                        };
                    }
                }
            }
        }
        None
    }
}

/// Faint unlit dot colour on a regular panel surface (`BG_PANEL` / `BG_PANEL_ALT`).
pub fn unlit_on_panel() -> Color32 {
    theme::mix(TraktorTheme::BG_PANEL(), TraktorTheme::TEXT_MUTED(), 0.32)
}

/// Faint unlit dot colour inside a dark display inset (`WAVEFORM_BG`).
pub fn unlit_on_inset() -> Color32 {
    let bg = TraktorTheme::WAVEFORM_BG();
    theme::mix(bg, theme::on(bg), 0.16)
}

/// Ensures `c` has sufficient visibility on the dark `WAVEFORM_BG` display inset.
pub fn visible_on_inset(c: Color32) -> Color32 {
    let bg = TraktorTheme::WAVEFORM_BG();
    if theme::contrast(c, bg) < 2.6 {
        theme::mix(c, theme::on(bg), 0.45)
    } else {
        c
    }
}

/// Paints a single dot cell at `center` with base `radius`.
pub fn paint_dot(painter: &Painter, center: Pos2, radius: f32, cell: DotCell, unlit_col: Color32) {
    match cell {
        DotCell::Unlit => {
            painter.circle_filled(center, radius, unlit_col);
        }
        DotCell::Filled(col) => {
            painter.circle_filled(center, radius, col);
        }
        DotCell::Scaled(col, scale) => {
            painter.circle_filled(center, (radius * scale).max(0.8), col);
        }
        DotCell::Hollow(col) => {
            painter.circle_stroke(center, (radius - 0.4).max(1.0), Stroke::new(1.1_f32, col));
        }
    }
}

/// Paints a regular `cols × rows` dot grid centred in `rect`.
/// `row = 0` is the bottom row and `row = rows - 1` is the top row.
pub fn paint_dot_grid(
    painter: &Painter,
    rect: Rect,
    cols: usize,
    rows: usize,
    dot_radius: f32,
    unlit_col: Color32,
    cell_fn: impl Fn(usize, usize) -> DotCell,
) {
    if cols == 0 || rows == 0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let step_x = if cols > 1 {
        (rect.width() - dot_radius * 2.0).max(1.0) / (cols - 1) as f32
    } else {
        0.0
    };
    let step_y = if rows > 1 {
        (rect.height() - dot_radius * 2.0).max(1.0) / (rows - 1) as f32
    } else {
        0.0
    };
    let start_x = if cols > 1 { rect.left() + dot_radius } else { rect.center().x };
    let start_y = if rows > 1 { rect.bottom() - dot_radius } else { rect.center().y };

    for col in 0..cols {
        let x = start_x + col as f32 * step_x;
        for row in 0..rows {
            let y = start_y - row as f32 * step_y;
            paint_dot(painter, Pos2::new(x, y), dot_radius, cell_fn(col, row), unlit_col);
        }
    }
}

/// Pure helper: whether dot `index` (out of `count` dots along the 270° arc) is lit
/// for a normalized value `norm_val` (`0.0..=1.0`) and origin `origin_norm` (`0.5` for bipolar, `0.0` for unipolar).
pub fn ring_dot_lit(index: usize, count: usize, norm_val: f32, origin_norm: f32) -> bool {
    if count <= 1 {
        return norm_val > 0.0;
    }
    let q = index as f32 / (count - 1) as f32;
    let v = norm_val.clamp(0.0, 1.0);
    if (origin_norm - 0.5).abs() < 0.05 {
        if v >= 0.5 {
            q >= 0.5 - 0.01 && q <= v + 0.015
        } else {
            q <= 0.5 + 0.01 && q >= v - 0.015
        }
    } else {
        v > 0.02 && q <= v + 0.015
    }
}

/// Pure helper: maps a normalized `0.0..=1.0` value to a number of lit dot rows (`0..=rows`).
pub fn value_to_lit_rows(norm_val: f32, rows: usize) -> usize {
    ((norm_val.clamp(0.0, 1.0) * rows as f32).round() as usize).min(rows)
}

/// Formats a `0.0..=1.0` fader or gain value as a short dB string for dot-matrix readouts.
/// `default_norm` is the 0 dB reference point (e.g. `0.5` for gain/EQ, `0.85` for channel faders).
pub fn format_db(norm_val: f32, default_norm: f32) -> String {
    let v = norm_val.clamp(0.0, 1.0);
    if v <= 0.01 {
        return "-∞".to_string();
    }
    let db = if v >= default_norm {
        ((v - default_norm) / (1.0 - default_norm).max(1e-4)) * 6.0
    } else {
        -((default_norm - v) / default_norm.max(1e-4)).powf(1.25) * 18.0
    };
    let rounded = db.round() as i32;
    if rounded > 0 {
        format!("+{rounded}")
    } else if rounded == 0 {
        "0".to_string()
    } else {
        format!("{rounded}")
    }
}

/// Formats an EQ band value (`0.0..=1.0`, `0.5` = 0 dB) or killed state.
pub fn format_eq_db(norm_val: f32, killed: bool) -> String {
    if killed {
        "CUT".to_string()
    } else {
        let db = ((norm_val.clamp(0.0, 1.0) - 0.5) * 24.0).round() as i32;
        if db > 0 {
            format!("+{db}")
        } else if db == 0 {
            "0".to_string()
        } else {
            format!("{db}")
        }
    }
}

/// Formats a filter knob (`0.0..=1.0`, `0.5` = bypass) for the top-bar FILTER ring readout.
pub fn format_filter_val(norm_val: f32, filter_on: bool) -> String {
    if !filter_on || (norm_val - 0.5).abs() <= 0.03 {
        "0".to_string()
    } else if norm_val > 0.5 {
        "HP".to_string()
    } else {
        "LP".to_string()
    }
}

/// Interactive 11-dot ring readout with the value in `dot_font` in the centre and a caption below.
/// Dragging vertically adjusts `value`; double-clicking resets to `default`.
#[allow(clippy::too_many_arguments)]
pub fn dot_ring(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    value_text: &str,
    label: &str,
    accent: Color32,
    lit: bool,
) -> Response {
    let size = Vec2::new(42.0, 42.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if response.dragged() {
        let delta = -response.drag_delta().y * 0.005 * (max - min);
        *value = (*value + delta).clamp(min, max);
        response.mark_changed();
    }
    if response.double_clicked() {
        *value = default;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let center = Pos2::new(rect.center().x, rect.top() + 15.5);
        let ring_r = 12.5_f32;
        let norm = ((*value - min) / (max - min).max(1e-5)).clamp(0.0, 1.0);
        let origin_norm = ((default - min) / (max - min).max(1e-5)).clamp(0.0, 1.0);
        let unlit_col = unlit_on_panel();
        let count = 11usize;

        for i in 0..count {
            let q = i as f32 / (count - 1) as f32;
            let angle = (-135.0 + 270.0 * q).to_radians();
            let pos = Pos2::new(center.x + angle.sin() * ring_r, center.y - angle.cos() * ring_r);
            let dot_r = if i == count / 2 { 2.1 } else { 1.65 };
            let is_on = lit && ring_dot_lit(i, count, norm, origin_norm);
            let cell = if is_on { DotCell::Filled(accent) } else { DotCell::Unlit };
            paint_dot(painter, pos, dot_r, cell, unlit_col);
        }

        painter.text(
            center + Vec2::new(0.0, 1.0),
            Align2::CENTER_CENTER,
            value_text,
            theme::dot_font(10.5),
            if lit { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_MUTED() },
        );
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 4.0),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(8.0),
            TraktorTheme::TEXT_SECONDARY(),
        );
    }

    response
}

/// Clickable dot lamp with a short status caption (`ON` / `OFF`) below it, used for FX on/off in the top bar.
pub fn dot_lamp(ui: &mut Ui, on: &mut bool, accent: Color32, hover: &str) -> Response {
    let size = Vec2::new(24.0, 42.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let c = Pos2::new(rect.center().x, rect.top() + 15.5);
        if *on {
            painter.circle_filled(c, 3.6, accent);
        } else {
            painter.circle_filled(c, 3.0, unlit_on_panel());
            painter.circle_stroke(c, 3.2, Stroke::new(1.0_f32, TraktorTheme::TEXT_MUTED()));
        }
        painter.text(
            Pos2::new(rect.center().x, rect.top() + 28.0),
            Align2::CENTER_CENTER,
            if *on { "ON" } else { "OFF" },
            FontId::proportional(7.5),
            if *on { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_MUTED() },
        );
        if resp.hovered() {
            painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::TEXT_SECONDARY()));
        }
    }
    resp.on_hover_text(hover)
}

/// Pure timing/easing helper: computes circular ripple wave intensity (0.0..=1.0)
/// across the dot matrix during a crossfade cheer.
pub fn cheer_wave_intensity(
    col: usize,
    center_col: usize,
    elapsed_sec: f32,
    duration_sec: f32,
    speed_cols_per_sec: f32,
) -> f32 {
    if elapsed_sec <= 0.0 || elapsed_sec >= duration_sec {
        return 0.0;
    }
    let wave_front = elapsed_sec * speed_cols_per_sec;
    let dist = (col as f32 - center_col as f32).abs();
    let diff = (dist - wave_front).abs();
    let thickness = 2.8_f32;
    if diff >= thickness {
        return 0.0;
    }
    let shape = 1.0 - diff / thickness;
    let fade = (1.0 - elapsed_sec / duration_sec).clamp(0.0, 1.0);
    (shape * fade).clamp(0.0, 1.0)
}

/// Pure timing/easing helper: computes beam column wipe progress (0.0..=1.0)
/// across a deck's half of the mixer display when a new track is loaded.
pub fn track_load_wipe_col(
    col: usize,
    start_col: usize,
    end_col: usize,
    elapsed_sec: f32,
    duration_sec: f32,
) -> f32 {
    if elapsed_sec <= 0.0 || elapsed_sec >= duration_sec {
        return 0.0;
    }
    let t = (elapsed_sec / duration_sec).clamp(0.0, 1.0);
    let min_c = start_col.min(end_col) as f32;
    let max_c = start_col.max(end_col) as f32;
    let beam_col = if start_col <= end_col {
        min_c + (max_c - min_c) * t
    } else {
        max_c - (max_c - min_c) * t
    };
    let diff = (col as f32 - beam_col).abs();
    let beam_width = 2.4_f32;
    if diff >= beam_width {
        return 0.0;
    }
    let shape = 1.0 - diff / beam_width;
    let fade = (1.0 - t * 0.35).clamp(0.0, 1.0);
    (shape * fade).clamp(0.0, 1.0)
}

/// Pure timing/easing helper: computes a traveling pulse packet (0.0..=1.0)
/// across the dot lattice from the master deck to the newly locked synced deck.
pub fn sync_pulse_col(
    col: usize,
    from_col: usize,
    to_col: usize,
    elapsed_sec: f32,
    duration_sec: f32,
) -> f32 {
    if elapsed_sec <= 0.0 || elapsed_sec >= duration_sec {
        return 0.0;
    }
    let t = (elapsed_sec / duration_sec).clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    let pulse_c = from_col as f32 + (to_col as f32 - from_col as f32) * eased;
    let diff = (col as f32 - pulse_c).abs();
    let pulse_width = 2.6_f32;
    if diff >= pulse_width {
        return 0.0;
    }
    let shape = 1.0 - diff / pulse_width;
    let fade = (1.0 - t * 0.45).clamp(0.0, 1.0);
    (shape * fade).clamp(0.0, 1.0)
}

/// Pure helper: returns true during the ON phase of a slow 1 Hz blinking dot when recording is active.
pub fn recording_blink_lit(now_sec: f64, is_recording: bool) -> bool {
    if !is_recording {
        return false;
    }
    (now_sec.fract() + 1.0).fract() < 0.5
}

/// Pure helper: returns true when two track keys are harmonically compatible.
pub fn harmonically_compatible_link_dots(key_a: Option<&str>, key_b: Option<&str>) -> bool {
    match (key_a, key_b) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => {
            retraktor_library::KeyWheel::are_harmonically_compatible(a, b)
        }
        _ => false,
    }
}

/// Inclusive column range within the shared dot lattice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ColRange {
    pub start: usize,
    pub end: usize,
}

impl ColRange {
    #[inline]
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end: end.max(start) }
    }
    #[inline]
    pub fn len(&self) -> usize {
        self.end - self.start + 1
    }
    #[inline]
    pub fn contains(&self, col: usize) -> bool {
        col >= self.start && col <= self.end
    }
    #[inline]
    pub fn center(&self) -> f32 {
        (self.start + self.end) as f32 * 0.5
    }
}

/// Symmetrical zone layout across the shared lattice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MixerZoneLayout {
    pub cols: usize,
    pub mid: usize,
    pub main_meter_l: usize,
    pub main_meter_r: usize,
    pub fader_meter: [Option<ColRange>; 4],
    pub eq_stem_zone: [Option<ColRange>; 4],
    pub is_compact: [bool; 4],
    pub xfader_start: usize,
    pub xfader_end: usize,
}

pub fn compute_mixer_zone_layout(cols: usize, two_channel_only: bool) -> MixerZoneLayout {
    let mid = cols / 2;
    let margin = 2usize;
    let max_span = mid.saturating_sub(margin).min(cols.saturating_sub(margin + 1).saturating_sub(mid));
    let xfader_start = mid.saturating_sub(max_span);
    let xfader_end = mid + max_span;
    let main_meter_l = mid.saturating_sub(1);
    let main_meter_r = mid + 1;

    let mut fader_meter = [None; 4];
    let mut eq_stem_zone = [None; 4];
    let mut is_compact = [false; 4];

    if two_channel_only {
        let a_fader_e = mid.saturating_sub(4);
        let a_fader_s = mid.saturating_sub(5);
        let b_fader_s = mid + 4;
        let b_fader_e = mid + 5;

        let a_eq_s = mid.saturating_sub(max_span);
        let a_eq_e = mid.saturating_sub(8);
        let b_eq_s = mid + 8;
        let b_eq_e = mid + max_span;

        fader_meter[0] = Some(ColRange::new(a_fader_s, a_fader_e));
        fader_meter[1] = Some(ColRange::new(b_fader_s, b_fader_e));
        if a_eq_e >= a_eq_s {
            eq_stem_zone[0] = Some(ColRange::new(a_eq_s, a_eq_e));
        }
        if b_eq_e >= b_eq_s {
            eq_stem_zone[1] = Some(ColRange::new(b_eq_s, b_eq_e));
        }
    } else {
        // 4-Deck layout:
        // Left: Deck A (outer), Deck C (inner).
        // Right: Deck D (inner), Deck B (outer).
        let c_fader_e = mid.saturating_sub(3);
        let c_fader_s = mid.saturating_sub(4);
        let d_fader_s = mid + 3;
        let d_fader_e = mid + 4;

        let c_eq_e = mid.saturating_sub(6);
        let c_eq_s = mid.saturating_sub(11);
        let d_eq_s = mid + 6;
        let d_eq_e = mid + 11;

        let a_fader_e = mid.saturating_sub(14);
        let a_fader_s = mid.saturating_sub(15);
        let b_fader_s = mid + 14;
        let b_fader_e = mid + 15;

        let a_eq_s = mid.saturating_sub(max_span);
        let a_eq_e = mid.saturating_sub(18);
        let b_eq_s = mid + 18;
        let b_eq_e = mid + max_span;

        fader_meter[0] = Some(ColRange::new(a_fader_s, a_fader_e));
        fader_meter[1] = Some(ColRange::new(b_fader_s, b_fader_e));
        fader_meter[2] = Some(ColRange::new(c_fader_s, c_fader_e));
        fader_meter[3] = Some(ColRange::new(d_fader_s, d_fader_e));

        eq_stem_zone[2] = Some(ColRange::new(c_eq_s, c_eq_e));
        eq_stem_zone[3] = Some(ColRange::new(d_eq_s, d_eq_e));
        is_compact[2] = true;
        is_compact[3] = true;

        if a_eq_e >= a_eq_s {
            eq_stem_zone[0] = Some(ColRange::new(a_eq_s, a_eq_e));
        }
        if b_eq_e >= b_eq_s {
            eq_stem_zone[1] = Some(ColRange::new(b_eq_s, b_eq_e));
        }
    }

    MixerZoneLayout {
        cols,
        mid,
        main_meter_l,
        main_meter_r,
        fader_meter,
        eq_stem_zone,
        is_compact,
        xfader_start,
        xfader_end,
    }
}


/// Pure helper: computes how much a filter (`0.0..=1.0`, `0.5` = neutral) attenuates a normalized
/// frequency position `freq_pos` (`0.0` = LOW, `0.5` = MID, `1.0` = HI). Returns `1.0` when uncut,
/// down to `0.25` when fully in the cut region.
pub fn filter_band_gain(filter: f32, filter_on: bool, freq_pos: f32) -> f32 {
    if !filter_on || (filter - 0.5).abs() <= 0.02 {
        return 1.0;
    }
    let f = filter.clamp(0.0, 1.0);
    let pos = freq_pos.clamp(0.0, 1.0);
    if f < 0.5 {
        // Low-pass: cuts from HI (1.0) down towards LOW (0.0) as f goes 0.5 -> 0.0
        let cutoff = f * 2.0; // 1.0 at centre, 0.0 at full LP
        if pos > cutoff {
            (1.0 - (pos - cutoff) * 1.4).clamp(0.25, 1.0)
        } else {
            1.0
        }
    } else {
        // High-pass: cuts from LOW (0.0) up towards HI (1.0) as f goes 0.5 -> 1.0
        let cutoff = (f - 0.5) * 2.0; // 0.0 at centre, 1.0 at full HP
        if pos < cutoff {
            (1.0 - (cutoff - pos) * 1.4).clamp(0.25, 1.0)
        } else {
            1.0
        }
    }
}

const DECK_LETTERS: [&str; 4] = ["A", "B", "C", "D"];
const XFADER_TIP: &str = "Crossfader. Drag to blend A and B; double-click to centre";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum MixerDragTarget {
    #[default]
    Crossfader,
    Fader(usize),
    EqBand(usize, usize),
}

/// Unified centre mixer continuous dot-matrix canvas.
/// Integrates EQ/stems, fader-on-meters, MAIN meter, and crossfader ribbon into ONE
/// continuous dot-matrix lattice, mirrored around the centre.
pub struct ContinuousMixer<'a> {
    pub two_channel_only: bool,
    pub volume: &'a mut [f32; 4],
    pub cue: &'a mut [bool; 4],
    pub eq_low: &'a mut [f32; 4],
    pub eq_mid: &'a mut [f32; 4],
    pub eq_hi: &'a mut [f32; 4],
    pub eq_kill_low: &'a mut [bool; 4],
    pub eq_kill_mid: &'a mut [bool; 4],
    pub eq_kill_hi: &'a mut [bool; 4],
    pub filter: [f32; 4],
    pub filter_on: [bool; 4],
    pub stem_view: [bool; 4],
    pub stem_muted: &'a mut [[bool; 4]; 4],
    pub stem_fx_send: &'a mut [[bool; 4]; 4],
    pub crossfader: &'a mut f32,
    pub channel_peaks: [f32; 4],
    pub band_peaks: [[f32; 3]; 4],
    pub stem_peaks: [[f32; 4]; 4],
    pub main_peaks: (f32, f32),
    pub stem_progress: [Option<f32>; 4],
    pub active_cheer: &'a mut Option<ActiveCheer>,
    pub deck_track_ids: [Option<usize>; 4],
    pub deck_sync_enabled: [bool; 4],
    pub in_key: bool,
    pub is_recording: bool,
    pub size: Vec2,
}

impl<'a> ContinuousMixer<'a> {
    pub fn show(&mut self, ui: &mut Ui) -> Response {
        let (rect, mut response) = ui.allocate_exact_size(self.size, Sense::click_and_drag());
        if !ui.is_rect_visible(rect) {
            return response;
        }

        let painter = ui.painter();
        let bg = TraktorTheme::WAVEFORM_BG();
        let ink = theme::on(bg);
        let unlit = unlit_on_inset();
        let anim = theme::animations();
        let now_sec = ui.input(|i| i.time);

        // Hardware display bezel
        painter.rect_filled(rect, 6.0, bg);
        painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

        let pad_x = 8.0_f32;
        let pad_y = 6.0_f32;
        let inner_rect = rect.shrink2(Vec2::new(pad_x, pad_y));

        let pitch = 5.2_f32;
        let dot_radius = 1.35_f32;
        let raw_cols = ((inner_rect.width() - dot_radius * 2.0).max(10.0) / pitch).round() as usize;
        let cols = (if raw_cols % 2 == 0 { raw_cols + 1 } else { raw_cols }).clamp(55, 141);
        let rows = (((inner_rect.height() - dot_radius * 2.0).max(10.0) / pitch).round() as usize).clamp(26, 60);

        let step_x = if cols > 1 { (inner_rect.width() - dot_radius * 2.0).max(1.0) / (cols - 1) as f32 } else { 0.0 };
        let step_y = if rows > 1 { (inner_rect.height() - dot_radius * 2.0).max(1.0) / (rows - 1) as f32 } else { 0.0 };
        let start_x = inner_rect.left() + dot_radius;
        let start_y = inner_rect.bottom() - dot_radius;

        let col_x = |c: usize| start_x + c as f32 * step_x;
        let row_y = |r: usize| start_y - r as f32 * step_y;
        let dot_pos = |c: usize, r: usize| Pos2::new(col_x(c), row_y(r));

        let layout = compute_mixer_zone_layout(cols, self.two_channel_only);
        let mid = layout.mid;

        let body_bot_row = 7usize;
        let body_top_row = rows.saturating_sub(4);
        let track_rows = body_top_row.saturating_sub(body_bot_row) + 1;
        let track_h = (row_y(body_bot_row) - row_y(body_top_row)).max(20.0);
        let header_y = row_y(rows.saturating_sub(2));
        let footer_label_y = row_y(5);
        let cue_lamp_y = footer_label_y;

        let ribbon_left_x = col_x(layout.xfader_start);
        let ribbon_right_x = col_x(layout.xfader_end);
        let ribbon_w = (ribbon_right_x - ribbon_left_x).max(20.0);

        let active_channels: &[usize] = if self.two_channel_only { &[0, 1] } else { &[0, 2, 3, 1] };

        // Drag target persistence
        let drag_id = response.id.with("drag_target");
        let mut active_drag: Option<MixerDragTarget> = ui.ctx().data(|d| d.get_temp(drag_id));

        if response.drag_started() {
            if let Some(pos) = ui.input(|i| i.pointer.press_origin()) {
                let target = if pos.y >= row_y(4) {
                    Some(MixerDragTarget::Crossfader)
                } else {
                    let mut found = None;
                    for &d in active_channels {
                        if let Some(f_rng) = layout.fader_meter[d] {
                            let fx1 = col_x(f_rng.start) - step_x * 1.5;
                            let fx2 = col_x(f_rng.end) + step_x * 1.5;
                            if pos.x >= fx1 && pos.x <= fx2 && pos.y >= row_y(body_top_row) - 8.0 && pos.y <= row_y(body_bot_row) + 8.0 {
                                found = Some(MixerDragTarget::Fader(d));
                                break;
                            }
                        }
                        if let Some(eq_rng) = layout.eq_stem_zone[d] {
                            let zx1 = col_x(eq_rng.start) - step_x * 0.5;
                            let zx2 = col_x(eq_rng.end) + step_x * 0.5;
                            if pos.x >= zx1 && pos.x <= zx2 && !self.stem_view[d] {
                                let mirrored = d == 1 || d == 3;
                                let frac = ((pos.x - zx1) / (zx2 - zx1).max(1.0)).clamp(0.0, 0.999);
                                let slot = (frac * 3.0).floor() as usize;
                                let b = if mirrored { 2 - slot.min(2) } else { slot.min(2) };
                                found = Some(MixerDragTarget::EqBand(d, b));
                                break;
                            }
                        }
                    }
                    found
                };
                active_drag = target;
                ui.ctx().data_mut(|data| {
                    data.remove_temp::<MixerDragTarget>(drag_id);
                    if let Some(target) = target {
                        data.insert_temp(drag_id, target);
                    }
                });
            }
        }

        if response.dragged() {
            let delta = response.drag_delta();
            if let Some(target) = active_drag {
                match target {
                    MixerDragTarget::Crossfader => {
                        *self.crossfader = (*self.crossfader + delta.x / ribbon_w).clamp(0.0, 1.0);
                        response.mark_changed();
                    }
                    MixerDragTarget::Fader(d) => {
                        self.volume[d] = (self.volume[d] - delta.y / track_h).clamp(0.0, 1.0);
                        response.mark_changed();
                    }
                    MixerDragTarget::EqBand(d, b) => {
                        let delta_val = -delta.y / (track_h * 0.7).max(20.0);
                        let val = match b {
                            0 => &mut self.eq_low[d],
                            1 => &mut self.eq_mid[d],
                            _ => &mut self.eq_hi[d],
                        };
                        *val = (*val + delta_val).clamp(0.0, 1.0);
                        response.mark_changed();
                    }
                }
            }
        } else {
            ui.ctx().data_mut(|d| d.remove_temp::<MixerDragTarget>(drag_id));
        }

        // Clicks, double clicks and right clicks
        if let Some(pos) = response.interact_pointer_pos() {
            if response.clicked() {
                if pos.y >= row_y(4) {
                    *self.crossfader = ((pos.x - ribbon_left_x) / ribbon_w).clamp(0.0, 1.0);
                    response.mark_changed();
                } else {
                    for &d in active_channels {
                        if let Some(f_rng) = layout.fader_meter[d] {
                            let fx1 = col_x(f_rng.start) - step_x * 1.5;
                            let fx2 = col_x(f_rng.end) + step_x * 1.5;
                            if pos.x >= fx1 && pos.x <= fx2 && pos.y >= row_y(body_bot_row) + 2.0 {
                                self.cue[d] = !self.cue[d];
                                response.mark_changed();
                            }
                        }
                        if let Some(eq_rng) = layout.eq_stem_zone[d] {
                            let zx1 = col_x(eq_rng.start) - step_x * 0.5;
                            let zx2 = col_x(eq_rng.end) + step_x * 0.5;
                            if pos.x >= zx1 && pos.x <= zx2 {
                                if self.stem_view[d] {
                                    let mirrored = d == 1 || d == 3;
                                    let frac = ((pos.x - zx1) / (zx2 - zx1).max(1.0)).clamp(0.0, 0.999);
                                    let slot = (frac * 4.0).floor() as usize;
                                    let s = if mirrored { 3 - slot.min(3) } else { slot.min(3) };
                                    self.stem_muted[d][s] = !self.stem_muted[d][s];
                                    response.mark_changed();
                                } else if pos.y > row_y(body_bot_row + 5) {
                                    let mirrored = d == 1 || d == 3;
                                    let frac = ((pos.x - zx1) / (zx2 - zx1).max(1.0)).clamp(0.0, 0.999);
                                    let slot = (frac * 3.0).floor() as usize;
                                    let b = if mirrored { 2 - slot.min(2) } else { slot.min(2) };
                                    let kill = match b {
                                        0 => &mut self.eq_kill_low[d],
                                        1 => &mut self.eq_kill_mid[d],
                                        _ => &mut self.eq_kill_hi[d],
                                    };
                                    *kill = !*kill;
                                    response.mark_changed();
                                }
                            }
                        }
                    }
                }
            }
            if response.secondary_clicked() {
                for &d in active_channels {
                    if let Some(eq_rng) = layout.eq_stem_zone[d] {
                        let zx1 = col_x(eq_rng.start) - step_x * 0.5;
                        let zx2 = col_x(eq_rng.end) + step_x * 0.5;
                        if pos.x >= zx1 && pos.x <= zx2 {
                            if self.stem_view[d] {
                                let mirrored = d == 1 || d == 3;
                                let frac = ((pos.x - zx1) / (zx2 - zx1).max(1.0)).clamp(0.0, 0.999);
                                let slot = (frac * 4.0).floor() as usize;
                                let s = if mirrored { 3 - slot.min(3) } else { slot.min(3) };
                                self.stem_fx_send[d][s] = !self.stem_fx_send[d][s];
                                response.mark_changed();
                            } else {
                                let mirrored = d == 1 || d == 3;
                                let frac = ((pos.x - zx1) / (zx2 - zx1).max(1.0)).clamp(0.0, 0.999);
                                let slot = (frac * 3.0).floor() as usize;
                                let b = if mirrored { 2 - slot.min(2) } else { slot.min(2) };
                                let kill = match b {
                                    0 => &mut self.eq_kill_low[d],
                                    1 => &mut self.eq_kill_mid[d],
                                    _ => &mut self.eq_kill_hi[d],
                                };
                                *kill = !*kill;
                                response.mark_changed();
                            }
                        }
                    }
                }
            }
            if response.double_clicked() {
                if pos.y >= row_y(4) {
                    *self.crossfader = 0.5;
                    response.mark_changed();
                } else {
                    for &d in active_channels {
                        if let Some(f_rng) = layout.fader_meter[d] {
                            let fx1 = col_x(f_rng.start) - step_x * 1.5;
                            let fx2 = col_x(f_rng.end) + step_x * 1.5;
                            if pos.x >= fx1 && pos.x <= fx2 {
                                self.volume[d] = 0.85;
                                response.mark_changed();
                            }
                        }
                        if let Some(eq_rng) = layout.eq_stem_zone[d] {
                            let zx1 = col_x(eq_rng.start) - step_x * 0.5;
                            let zx2 = col_x(eq_rng.end) + step_x * 0.5;
                            if pos.x >= zx1 && pos.x <= zx2 && !self.stem_view[d] {
                                let mirrored = d == 1 || d == 3;
                                let frac = ((pos.x - zx1) / (zx2 - zx1).max(1.0)).clamp(0.0, 0.999);
                                let slot = (frac * 3.0).floor() as usize;
                                let b = if mirrored { 2 - slot.min(2) } else { slot.min(2) };
                                match b {
                                    0 => self.eq_low[d] = 0.5,
                                    1 => self.eq_mid[d] = 0.5,
                                    _ => self.eq_hi[d] = 0.5,
                                }
                                response.mark_changed();
                            }
                        }
                    }
                }
            }
        }

        // Hover tooltip inspection
        if let Some(pos) = response.hover_pos() {
            let mut tooltip = None;
            if let Some(cheer) = self.active_cheer {
                let elapsed = (now_sec - cheer.start_sec) as f32;
                if elapsed < 1.2 {
                    let sprite = CHEER_SPRITES[cheer.cheer_index % CHEER_SPRITES.len()];
                    tooltip = Some(format!("Cheers: {}", sprite.name));
                }
            }
            if tooltip.is_none() {
                if pos.y >= row_y(4) {
                    tooltip = Some(XFADER_TIP.to_string());
                } else {
                    let mx1 = col_x(layout.main_meter_l) - step_x * 1.0;
                    let mx2 = col_x(layout.main_meter_r) + step_x * 1.0;
                    if pos.x >= mx1 && pos.x <= mx2 {
                        tooltip = Some("Main output level".to_string());
                    } else {
                        for &d in active_channels {
                            if let Some(f_rng) = layout.fader_meter[d] {
                                let fx1 = col_x(f_rng.start) - step_x * 1.5;
                                let fx2 = col_x(f_rng.end) + step_x * 1.5;
                                if pos.x >= fx1 && pos.x <= fx2 {
                                    tooltip = Some(format!(
                                        "Deck {} volume on its level meter. The lamp below is headphone cue.",
                                        DECK_LETTERS[d]
                                    ));
                                    break;
                                }
                            }
                            if let Some(eq_rng) = layout.eq_stem_zone[d] {
                                let zx1 = col_x(eq_rng.start) - step_x * 0.5;
                                let zx2 = col_x(eq_rng.end) + step_x * 0.5;
                                if pos.x >= zx1 && pos.x <= zx2 {
                                    if self.stem_view[d] {
                                        tooltip = Some("Stems: click one to turn it on or off".to_string());
                                    } else {
                                        tooltip = Some("Equaliser: drag a band up or down; click its bottom (or right-click) to cut it".to_string());
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if let Some(tip) = tooltip {
                response = response.on_hover_text(tip);
            }
        }

        // Single continuous lattice grid
        let mut cells = vec![DotCell::Unlit; cols * rows];
        macro_rules! set_cell {
            ($c:expr, $r:expr, $cell:expr) => {{
                let c_val = $c;
                let r_val = $r;
                if c_val < cols && r_val < rows {
                    cells[r_val * cols + c_val] = $cell;
                }
            }};
        }

        // 1. Crossfader Ribbon along bottom rows 0..=3
        let xf_start = layout.xfader_start;
        let xf_end = layout.xfader_end;
        let xf_len = xf_end.saturating_sub(xf_start) + 1;
        let handle_c = xf_start + ((self.crossfader.clamp(0.0, 1.0) * (xf_len.saturating_sub(2)) as f32).round() as usize).min(xf_len.saturating_sub(2));

        let lv_a = if anim { self.channel_peaks[0].clamp(0.0, 1.0) } else { 0.25 };
        let lv_b = if anim { self.channel_peaks[1].clamp(0.0, 1.0) } else { 0.25 };
        let col_a = TraktorTheme::DECK_A();
        let col_b = TraktorTheme::DECK_B();
        let handle_fg = TraktorTheme::TEXT_PRIMARY();

        for c in xf_start..=xf_end {
            for r in 0..4 {
                if c == handle_c || c == handle_c + 1 {
                    set_cell!(c, r, DotCell::Filled(handle_fg));
                } else {
                    // Handle hard left = all A, so the ribbon right of the handle is A's.
                    let is_a = c > handle_c;
                    let lv = if is_a { lv_a } else { lv_b };
                    let on = r == 0 || (r as f32 / 4.0) < lv;
                    if on {
                        let base = if is_a { col_a } else { col_b };
                        let fade = 0.45 + 0.55 * (1.0 - r as f32 / 5.0);
                        set_cell!(c, r, DotCell::Filled(base.gamma_multiply(fade)));
                    }
                }
            }
        }

        painter.text(
            Pos2::new(inner_rect.left() + 6.0, row_y(1)),
            Align2::CENTER_CENTER,
            "A",
            theme::dot_font(12.0),
            col_a,
        );
        painter.text(
            Pos2::new(inner_rect.right() - 6.0, row_y(1)),
            Align2::CENTER_CENTER,
            "B",
            theme::dot_font(12.0),
            col_b,
        );

        // 2. MAIN Stereo Meter (centre)
        let main_l = layout.main_meter_l;
        let main_r = layout.main_meter_r;
        let lit_ml = value_to_lit_rows(if anim { self.main_peaks.0 } else { 0.0 }, track_rows);
        let lit_mr = value_to_lit_rows(if anim { self.main_peaks.1 } else { 0.0 }, track_rows);

        for k in 0..track_rows {
            let r = body_bot_row + k;
            for (c, lit) in [(main_l, lit_ml), (main_r, lit_mr)] {
                if k < lit {
                    let col = if k >= track_rows.saturating_sub(2) {
                        TraktorTheme::METER_RED()
                    } else if k >= track_rows.saturating_sub(4) {
                        TraktorTheme::METER_YELLOW()
                    } else {
                        TraktorTheme::TEXT_PRIMARY()
                    };
                    set_cell!(c, r, DotCell::Filled(col));
                }
            }
        }
        painter.text(
            Pos2::new(col_x(mid), footer_label_y),
            Align2::CENTER_CENTER,
            "MAIN",
            FontId::proportional(8.0),
            TraktorTheme::TEXT_SECONDARY(),
        );

        // 3. Faders-on-Meter for each active channel
        for &d in active_channels {
            let Some(f_rng) = layout.fader_meter[d] else { continue };
            let accent = TraktorTheme::deck_color(d + 1);
            let vol = self.volume[d].clamp(0.0, 1.0);
            let eff_l = if anim { self.channel_peaks[d] } else { 0.0 } * vol;
            let eff_r = if anim { self.channel_peaks[d] * 0.94 } else { 0.0 } * vol;
            let lit_l = value_to_lit_rows(eff_l, track_rows);
            let lit_r = value_to_lit_rows(eff_r, track_rows);
            let fader_cx = (col_x(f_rng.start) + col_x(f_rng.end)) * 0.5;
            let hy = row_y(body_bot_row) - vol * track_h;
            let fader_row_f = body_bot_row as f32 + vol * (track_rows.saturating_sub(1)) as f32;

            for k in 0..track_rows {
                let r = body_bot_row + k;
                let above = r as f32 > fader_row_f + 0.6;
                for (c, lit) in [(f_rng.start, lit_l), (f_rng.end, lit_r)] {
                    if k < lit {
                        let col = if k >= track_rows.saturating_sub(2) {
                            TraktorTheme::METER_RED()
                        } else {
                            accent
                        };
                        let cell = if above { DotCell::Scaled(col, 0.6) } else { DotCell::Filled(col) };
                        set_cell!(c, r, cell);
                    } else if above {
                        set_cell!(c, r, DotCell::Scaled(unlit, 0.6));
                    }
                }
            }

            // Fader bracket
            let bw = (step_x * (f_rng.len() as f32 + 2.5)).clamp(22.0, 30.0);
            let bh = 12.0_f32;
            let bracket = Rect::from_center_size(Pos2::new(fader_cx, hy), Vec2::new(bw, bh));
            let stroke = Stroke::new(1.8_f32, TraktorTheme::TEXT_PRIMARY());
            painter.rect_stroke(bracket, 3.5, stroke);
            painter.line_segment([Pos2::new(bracket.left() - 4.0, hy), bracket.left_center()], stroke);
            painter.line_segment([bracket.right_center(), Pos2::new(bracket.right() + 4.0, hy)], stroke);

            // dB readout at header
            painter.text(
                Pos2::new(fader_cx, header_y),
                Align2::CENTER_CENTER,
                format_db(vol, 0.85),
                theme::dot_font(10.5),
                TraktorTheme::TEXT_PRIMARY(),
            );

            // Headphone cue lamp along footer row
            let is_cue = self.cue[d];
            let cue_col = if is_cue { TraktorTheme::CUE_ACTIVE() } else { TraktorTheme::TEXT_MUTED() };
            if is_cue {
                painter.circle_filled(Pos2::new(fader_cx - 7.0, cue_lamp_y), 2.6, cue_col);
            } else {
                painter.circle_stroke(Pos2::new(fader_cx - 7.0, cue_lamp_y), 2.2, Stroke::new(1.1_f32, cue_col));
            }
            painter.text(
                Pos2::new(fader_cx + 4.5, cue_lamp_y),
                Align2::CENTER_CENTER,
                "CUE",
                FontId::proportional(7.5),
                if is_cue { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_MUTED() },
            );
        }

        // 4. EQ or Stems zone for each active channel
        let mut kill_icons: Vec<(Pos2, &Sprite, i32, SpriteMode, Color32)> = Vec::new();
        for &d in active_channels {
            let Some(zone) = layout.eq_stem_zone[d] else { continue };
            let accent = TraktorTheme::deck_color(d + 1);
            let accent_vis = visible_on_inset(accent);
            let mirrored = d == 1 || d == 3;
            let is_compact = layout.is_compact[d];

            if self.stem_view[d] {
                // Stems mode
                let stem_count = 4usize;
                let sep_count = stem_count.saturating_sub(1);
                let avail_cols = zone.len().saturating_sub(sep_count);
                let stem_w = (avail_cols / stem_count).max(1);
                let names = ["DRM", "BAS", "OTH", "VOC"];

                for slot in 0..stem_count {
                    let s = if mirrored { 3 - slot } else { slot };
                    let slot_start_c = zone.start + slot * (stem_w + 1);
                    let slot_end_c = (slot_start_c + stem_w - 1).min(zone.end);
                    let slot_cx = (col_x(slot_start_c) + col_x(slot_end_c)) * 0.5;

                    let muted = self.stem_muted[d][s];
                    let fx = self.stem_fx_send[d][s];
                    let open_t = theme::fade(ui.ctx(), ("stem_wipe", d, s), !muted);

                    // Stem toggle bounce tracking
                    let stem_id = ui.make_persistent_id(("stem_anim", d, s));
                    let prev_state: Option<(bool, f64)> = ui.ctx().data(|d| d.get_temp(stem_id));
                    let (last_muted, mut anim_start) = prev_state.unwrap_or((muted, -100.0));
                    if muted != last_muted {
                        anim_start = now_sec;
                    }
                    ui.ctx().data_mut(|d| d.insert_temp(stem_id, (muted, anim_start)));
                    let elapsed = (now_sec - anim_start) as f32;
                    let is_animating = anim && elapsed >= 0.0 && elapsed < 0.6;
                    if is_animating {
                        ui.ctx().request_repaint();
                    }

                    // Top state
                    let top_txt = if muted { "OFF" } else if fx { "FX" } else { "ON" };
                    painter.text(
                        Pos2::new(slot_cx, header_y),
                        Align2::CENTER_CENTER,
                        top_txt,
                        theme::dot_font(9.5),
                        if muted { ink.gamma_multiply(0.4) } else if fx { TraktorTheme::ACCENT() } else { ink },
                    );

                    // Bottom label
                    painter.text(
                        Pos2::new(slot_cx, footer_label_y),
                        Align2::CENTER_CENTER,
                        names[s],
                        FontId::proportional(7.5),
                        if muted { ink.gamma_multiply(0.4) } else { ink.gamma_multiply(0.75) },
                    );

                    let live = if anim { self.stem_peaks[d][s].clamp(0.0, 1.0) } else { 0.45 };
                    let active_frac = (0.35 + 0.65 * live) * open_t;
                    let lit_rows = value_to_lit_rows(active_frac, track_rows);
                    let wipe_rows = value_to_lit_rows(open_t, track_rows);

                    for c in slot_start_c..=slot_end_c {
                        let is_border_c = c == slot_start_c || c == slot_end_c;
                        for k in 0..track_rows {
                            let r = body_bot_row + k;
                            let is_border = is_border_c || k == 0 || k == track_rows - 1;
                            if k < lit_rows {
                                let row_t = 0.6 + 0.4 * (1.0 - k as f32 / track_rows.max(1) as f32);
                                set_cell!(c, r, DotCell::Filled(accent_vis.gamma_multiply(row_t)));
                            } else if muted && is_border && k >= wipe_rows {
                                set_cell!(c, r, DotCell::Hollow(accent_vis.gamma_multiply(0.45)));
                            }
                        }
                    }

                    if anim && !is_compact && stem_w >= 4 {
                        let sprite = STEM_SPRITES[s];
                        let sprite_h = sprite.dimensions(0).1 as f32;
                        let floor_row = body_bot_row as f32;
                        let ceil_row = (body_top_row as f32 - sprite_h).max(floor_row);
                        let icon_top = (floor_row + (ceil_row - floor_row) * open_t).round();
                        let icon_cy = row_y(icon_top as usize) - (sprite_h * 0.5) * step_y;

                        let (bounce_dots, mode, col) = if is_animating {
                            let b = bounce_dot_offset(elapsed, 0.6, 5);
                            let pop = ((1.0 - (elapsed / 0.25).min(1.0)) * 6.0).round() as i32;
                            let m = if muted { SpriteMode::Hollow } else { SpriteMode::Filled };
                            let c = if muted { accent_vis.gamma_multiply(0.45) } else { ink };
                            (b - pop, m, c)
                        } else {
                            let m = if muted { SpriteMode::Hollow } else { SpriteMode::Filled };
                            let c = if muted { accent_vis.gamma_multiply(0.38) } else { ink };
                            (0, m, c)
                        };

                        paint_sprite(
                            painter,
                            Pos2::new(slot_cx, icon_cy),
                            Vec2::new(step_x, step_y),
                            dot_radius,
                            sprite,
                            0,
                            bounce_dots,
                            mode,
                            col,
                            unlit,
                            false,
                        );
                    }
                }

                if let Some(pct) = self.stem_progress[d] {
                    let bar_rect = Rect::from_min_max(
                        Pos2::new(col_x(zone.start), inner_rect.top() + 2.0),
                        Pos2::new(col_x(zone.end), inner_rect.top() + 5.0),
                    );
                    painter.rect_filled(bar_rect, 1.5, unlit);
                    let w = bar_rect.width() * pct.clamp(0.0, 1.0);
                    painter.rect_filled(Rect::from_min_size(bar_rect.min, Vec2::new(w, bar_rect.height())), 1.5, TraktorTheme::ACCENT());
                }
            } else {
                // EQ mode
                let band_count = 3usize;
                let sep_count = band_count.saturating_sub(1);
                let avail_cols = zone.len().saturating_sub(sep_count);
                let band_w = (avail_cols / band_count).max(1);
                let labels = ["LOW", "MID", "HI"];
                let vals = [self.eq_low[d], self.eq_mid[d], self.eq_hi[d]];
                let kills = [self.eq_kill_low[d], self.eq_kill_mid[d], self.eq_kill_hi[d]];
                let mut node_centers = [Pos2::ZERO; 3];

                for slot in 0..band_count {
                    let b = if mirrored { 2 - slot } else { slot };
                    let slot_start_c = zone.start + slot * (band_w + 1);
                    let slot_end_c = (slot_start_c + band_w - 1).min(zone.end);
                    let slot_cx = (col_x(slot_start_c) + col_x(slot_end_c)) * 0.5;
                    let val = vals[b].clamp(0.0, 1.0);
                    let killed = kills[b];
                    let live = if anim { self.band_peaks[d][b].clamp(0.0, 1.0) } else { 0.0 };

                    let kill_id = ui.make_persistent_id(("eq_kill_anim", d, b));
                    let prev_kill: Option<(bool, f64)> = ui.ctx().data(|d| d.get_temp(kill_id));
                    let (last_killed, mut anim_start) = prev_kill.unwrap_or((killed, -100.0));
                    if killed != last_killed {
                        anim_start = now_sec;
                    }
                    ui.ctx().data_mut(|d| d.insert_temp(kill_id, (killed, anim_start)));
                    let elapsed = (now_sec - anim_start) as f32;
                    let is_animating = anim && elapsed >= 0.0 && elapsed < 0.6;
                    if is_animating {
                        ui.ctx().request_repaint();
                    }

                    // Top label
                    let lbl = if is_compact { &labels[b][..1] } else { labels[b] };
                    painter.text(
                        Pos2::new(slot_cx, header_y),
                        Align2::CENTER_CENTER,
                        lbl,
                        FontId::proportional(8.0),
                        ink.gamma_multiply(0.68),
                    );

                    // Bottom readout
                    painter.text(
                        Pos2::new(slot_cx, footer_label_y),
                        Align2::CENTER_CENTER,
                        format_eq_db(val, killed),
                        theme::dot_font(10.5),
                        if killed { TraktorTheme::METER_RED() } else { ink },
                    );

                    let lit_rows = value_to_lit_rows(val, track_rows);
                    for c in slot_start_c..=slot_end_c {
                        let sub = if band_w > 1 { (c - slot_start_c) as f32 / (band_w - 1) as f32 } else { 0.5 };
                        let sub_freq = if mirrored { 1.0 - sub } else { sub };
                        let freq_pos = (b as f32 + (sub_freq - 0.5) * 0.8) / 2.0;
                        let flt_gain = filter_band_gain(self.filter[d], self.filter_on[d], freq_pos);

                        for k in 0..track_rows {
                            let r = body_bot_row + k;
                            if k < lit_rows {
                                let level_boost = 0.65 + 0.35 * live;
                                let col = accent_vis.gamma_multiply((level_boost * flt_gain).clamp(0.22, 1.0));
                                let cell = if killed { DotCell::Hollow(col) } else { DotCell::Filled(col) };
                                set_cell!(c, r, cell);
                            }
                        }
                    }

                    if anim && !is_compact && band_w >= 4 {
                        // Kill icon sits at the foot of the band, in a blanked window, painted after the lattice.
                        let icon_pos = Pos2::new(slot_cx, row_y(body_bot_row + 2));
                        let icon = if is_animating {
                            let bounce = bounce_dot_offset(elapsed, 0.6, 5);
                            let drop_in = ((1.0 - (elapsed / 0.25).min(1.0)) * 6.0).round() as i32;
                            let sprite = if killed { &SPRITE_KILL_X } else { &SPRITE_TICK };
                            let col = if killed { TraktorTheme::METER_RED() } else { TraktorTheme::PLAY_ACTIVE() };
                            Some((icon_pos, sprite, bounce + drop_in, SpriteMode::Filled, col))
                        } else if killed {
                            Some((icon_pos, &SPRITE_KILL_X, 0, SpriteMode::Hollow, TraktorTheme::METER_RED()))
                        } else {
                            None
                        };
                        if let Some(icon) = icon {
                            let mid_c = (slot_start_c + slot_end_c) / 2;
                            for c in mid_c.saturating_sub(3).max(slot_start_c)..=(mid_c + 3).min(slot_end_c) {
                                for r in body_bot_row..=body_bot_row + 5 {
                                    set_cell!(c, r, DotCell::Unlit);
                                }
                            }
                            kill_icons.push(icon);
                        }
                    }

                    let node_y = row_y(body_bot_row) - val * track_h;
                    node_centers[slot] = Pos2::new(slot_cx, node_y);
                }

                // Smooth cubic contour curve and circular band nodes
                let left_edge = Pos2::new(col_x(zone.start) - step_x * 0.5, node_centers[0].y);
                let right_edge = Pos2::new(col_x(zone.end) + step_x * 0.5, node_centers[2].y);
                let anchors = [left_edge, node_centers[0], node_centers[1], node_centers[2], right_edge];
                for pair in anchors.windows(2) {
                    let (a, b) = (pair[0], pair[1]);
                    let steps = 10usize;
                    let mut prev = a;
                    for s in 1..=steps {
                        let t = s as f32 / steps as f32;
                        let smooth = t * t * (3.0 - 2.0 * t);
                        let next = Pos2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * smooth);
                        painter.line_segment([prev, next], Stroke::new(1.8_f32, accent_vis));
                        prev = next;
                    }
                }
                for (slot, &center) in node_centers.iter().enumerate() {
                    let b = if mirrored { 2 - slot } else { slot };
                    let killed = kills[b];
                    painter.circle_filled(center, 4.5, bg);
                    painter.circle_stroke(
                        center,
                        4.5,
                        Stroke::new(1.5_f32, if killed { TraktorTheme::METER_RED() } else { ink }),
                    );
                }
            }
        }

        // 5. Whole-canvas choreography:
        // A. Crossfade cheers wave across full canvas width
        let mut cheer_overlay = None;
        if let Some(cheer) = self.active_cheer {
            if anim {
                let elapsed = (now_sec - cheer.start_sec) as f32;
                if elapsed < 1.2 {
                    let cheer_target_d = (cheer.deck).min(3);
                    let target_cx = if let Some(z) = layout.eq_stem_zone[cheer_target_d] {
                        z.center()
                    } else {
                        mid as f32 * 0.5
                    };
                    let cheer_cy_row = (body_bot_row + body_top_row) / 2;
                    let sprite = CHEER_SPRITES[cheer.cheer_index % CHEER_SPRITES.len()];
                    let bounce = if elapsed < 0.6 { bounce_dot_offset(elapsed, 0.6, 3) } else { 0 };
                    let frame_idx = if sprite.frames.len() > 1 {
                        ((elapsed * 4.0) as usize) % sprite.frames.len()
                    } else {
                        0
                    };

                    let cheer_accent = TraktorTheme::deck_color(cheer.deck + 1);

                    // Wave ripple outwards across the other zones
                    for c in 0..cols {
                        let w = cheer_wave_intensity(c, target_cx.round() as usize, elapsed, 1.2, 45.0);
                        if w > 0.0 {
                            let wave_col = cheer_accent.gamma_multiply(w * 0.75);
                            for r in 0..rows {
                                if cells[r * cols + c] == DotCell::Unlit {
                                    cells[r * cols + c] = DotCell::Filled(wave_col);
                                }
                            }
                        }
                    }

                    // Blank the dots under the sprite so it reads in front of the EQ / stem bars.
                    let (sw, sh) = sprite.dimensions(frame_idx);
                    let cc = target_cx.round() as usize;
                    for c in cc.saturating_sub(sw / 2 + 1)..=(cc + sw / 2 + 1).min(cols - 1) {
                        for r in cheer_cy_row.saturating_sub(sh / 2 + 1)..=(cheer_cy_row + sh / 2 + 1 + bounce.max(0) as usize).min(rows - 1) {
                            cells[r * cols + c] = DotCell::Unlit;
                        }
                    }
                    cheer_overlay = Some((sprite, frame_idx, bounce, cheer_accent, col_x(cc), row_y(cheer_cy_row)));
                    ui.ctx().request_repaint();
                }
            }
        }

        // B. Track load column wipe on loaded deck's half
        let track_ids_key = ui.make_persistent_id("continuous_mixer_track_ids");
        let load_starts_key = ui.make_persistent_id("continuous_mixer_load_starts");
        let prev_ids: Option<[Option<usize>; 4]> = ui.ctx().data(|d| d.get_temp(track_ids_key));
        let mut load_starts: [f64; 4] = ui.ctx().data(|d| d.get_temp(load_starts_key)).unwrap_or([-100.0; 4]);

        if prev_ids != Some(self.deck_track_ids) {
            for d in 0..4 {
                if self.deck_track_ids[d].is_some() && prev_ids.map(|p| p[d]) != Some(self.deck_track_ids[d]) {
                    load_starts[d] = now_sec;
                }
            }
            ui.ctx().data_mut(|d| {
                d.insert_temp(track_ids_key, self.deck_track_ids);
                d.insert_temp(load_starts_key, load_starts);
            });
        }

        if anim {
            for &d in active_channels {
                let elapsed = (now_sec - load_starts[d]) as f32;
                if elapsed >= 0.0 && elapsed < 0.7 {
                    let is_left = d == 0 || d == 2;
                    let (sc, ec) = if is_left {
                        (layout.xfader_start, mid.saturating_sub(2))
                    } else {
                        (layout.xfader_end, mid + 2)
                    };
                    let d_accent = TraktorTheme::deck_color(d + 1);
                    for c in sc.min(ec)..=sc.max(ec) {
                        let wipe = track_load_wipe_col(c, sc, ec, elapsed, 0.7);
                        if wipe > 0.0 {
                            let wcol = d_accent.gamma_multiply(wipe * 0.85);
                            for r in body_bot_row..=body_top_row {
                                if cells[r * cols + c] == DotCell::Unlit {
                                    cells[r * cols + c] = DotCell::Filled(wcol);
                                }
                            }
                        }
                    }
                    ui.ctx().request_repaint();
                }
            }
        }

        // C. Sync engaged pulse travelling between master and synced deck
        let sync_state_key = ui.make_persistent_id("continuous_mixer_sync_state");
        let sync_pulse_key = ui.make_persistent_id("continuous_mixer_sync_pulse");
        let prev_sync: Option<[bool; 4]> = ui.ctx().data(|d| d.get_temp(sync_state_key));
        let mut sync_pulse: Option<(usize, usize, f64)> = ui.ctx().data(|d| d.get_temp(sync_pulse_key));

        if prev_sync != Some(self.deck_sync_enabled) {
            for &d in active_channels {
                if self.deck_sync_enabled[d] && prev_sync.is_some_and(|p| !p[d]) {
                    let from_d = if d == 1 { 0 } else { 1 };
                    sync_pulse = Some((from_d, d, now_sec));
                    break;
                }
            }
            ui.ctx().data_mut(|d| {
                d.insert_temp(sync_state_key, self.deck_sync_enabled);
                d.insert_temp(sync_pulse_key, sync_pulse);
            });
        }

        if anim {
            if let Some((from_d, to_d, start_t)) = sync_pulse {
                let elapsed = (now_sec - start_t) as f32;
                if elapsed < 0.6 {
                    let from_c = if from_d == 0 || from_d == 2 {
                        layout.eq_stem_zone[from_d].map(|z| z.center().round() as usize).unwrap_or(mid.saturating_sub(10))
                    } else {
                        layout.eq_stem_zone[from_d].map(|z| z.center().round() as usize).unwrap_or(mid + 10)
                    };
                    let to_c = if to_d == 0 || to_d == 2 {
                        layout.eq_stem_zone[to_d].map(|z| z.center().round() as usize).unwrap_or(mid.saturating_sub(10))
                    } else {
                        layout.eq_stem_zone[to_d].map(|z| z.center().round() as usize).unwrap_or(mid + 10)
                    };
                    let pulse_row = (body_bot_row + body_top_row) / 2;
                    for c in from_c.min(to_c)..=from_c.max(to_c) {
                        let p = sync_pulse_col(c, from_c, to_c, elapsed, 0.6);
                        if p > 0.0 {
                            set_cell!(c, pulse_row, DotCell::Filled(TraktorTheme::ACCENT().gamma_multiply(p)));
                        }
                    }
                    ui.ctx().request_repaint();
                } else {
                    ui.ctx().data_mut(|d| d.remove_temp::<(usize, usize, f64)>(sync_pulse_key));
                }
            }
        }

        // D. Both decks in key: subtle two-dot link glyph near MAIN meter
        if self.in_key {
            let link_r = rows.saturating_sub(3);
            set_cell!(mid.saturating_sub(1), link_r, DotCell::Filled(TraktorTheme::ACCENT()));
            set_cell!(mid + 1, link_r, DotCell::Filled(TraktorTheme::ACCENT()));
            painter.line_segment(
                [dot_pos(mid.saturating_sub(1), link_r), dot_pos(mid + 1, link_r)],
                Stroke::new(1.2_f32, TraktorTheme::ACCENT().gamma_multiply(0.85)),
            );
        }

        // E. Recording active: slow blinking red dot at top centre
        if self.is_recording {
            let lit = recording_blink_lit(now_sec, true);
            let col = if lit { TraktorTheme::METER_RED() } else { unlit };
            set_cell!(mid, rows.saturating_sub(1), DotCell::Filled(col));
            ui.ctx().request_repaint();
        }

        // Paint the entire lattice in one pass
        paint_dot_grid(painter, inner_rect, cols, rows, dot_radius, unlit, |c, r| {
            cells[r * cols + c]
        });

        for (pos, sprite, bounce, mode, col) in kill_icons {
            paint_sprite(painter, pos, Vec2::new(step_x, step_y), dot_radius, sprite, 0, bounce, mode, col, unlit, false);
        }
        // Cheer sprite and caption go on last, over the lattice.
        if let Some((sprite, frame_idx, bounce, col, x, y)) = cheer_overlay {
            paint_sprite(painter, Pos2::new(x, y), Vec2::new(step_x, step_y), dot_radius, sprite, frame_idx, bounce, SpriteMode::Filled, col, unlit, false);
            let caption = painter.layout_no_wrap("★ NICE MIX ★".to_owned(), theme::dot_font(9.5), col);
            let cap_rect = Align2::CENTER_CENTER.anchor_size(Pos2::new(x, inner_rect.top() + 6.0), caption.size());
            painter.rect_filled(cap_rect.expand(2.0), 2.0, bg);
            painter.galley(cap_rect.min, caption, col);
        }

        response
    }
}


/// Pure helper: maps a phase difference in beats (`-0.5..=0.5`) to a column offset (`-half_cols..=half_cols`).
pub fn phase_col_offset(phase_diff: f64, half_cols: isize) -> isize {
    let clamped = phase_diff.clamp(-0.5, 0.5);
    ((clamped * 2.0) * half_cols as f64).round() as isize
}

/// Paints a four-beat bar counter (`1 2 3 4`) in a dot-matrix grid inside `rect`.
/// Sweeps with the beat (`beats_from_grid = (elapsed_sec - grid_offset_sec) * bpm / 60.0`) when `is_playing` is true;
/// renders the current beat as hollow dots when stopped so play/stop reads without colour or motion alone.
pub fn bar_counter(
    painter: &Painter,
    rect: Rect,
    beats_from_grid: f64,
    is_playing: bool,
    accent: Color32,
    ink: Color32,
) {
    let unlit = unlit_on_inset();
    let accent_vis = visible_on_inset(accent);
    let beat_in_bar = (beats_from_grid.floor() as i64).rem_euclid(4) as usize;
    let beat_frac = if theme::animations() && is_playing {
        beats_from_grid.rem_euclid(1.0) as f32
    } else {
        1.0
    };
    let pulse = if theme::animations() && is_playing {
        0.55 + 0.45 * (1.0 - beat_frac).powi(2)
    } else {
        1.0
    };

    let gap = 3.0_f32;
    let block_w = ((rect.width() - 3.0 * gap) / 4.0).max(8.0);
    for b in 0..4usize {
        let bx = rect.left() + b as f32 * (block_w + gap);
        let block_rect = Rect::from_min_max(Pos2::new(bx, rect.top()), Pos2::new(bx + block_w, rect.bottom()));
        let cols = ((block_w / 7.0) as usize).clamp(3, 12);
        let rows = 2usize;
        paint_dot_grid(painter, block_rect, cols, rows, 1.85, unlit, |c, _r| {
            if b == beat_in_bar {
                if is_playing {
                    let col_thresh = ((beat_frac * cols as f32).ceil() as usize).max(1);
                    if c < col_thresh {
                        DotCell::Filled(accent_vis.gamma_multiply(pulse))
                    } else {
                        DotCell::Filled(accent_vis.gamma_multiply(0.45))
                    }
                } else {
                    DotCell::Hollow(ink.gamma_multiply(0.75))
                }
            } else if b < beat_in_bar && is_playing {
                DotCell::Filled(ink.gamma_multiply(0.38))
            } else {
                DotCell::Unlit
            }
        });
    }
}

/// Paints a dot-matrix phase meter showing how far this deck's beat is from the other deck's (`-0.5..=0.5`, centre = locked).
pub fn phase_meter(
    painter: &Painter,
    rect: Rect,
    phase_diff: Option<f64>,
    accent: Color32,
    ink: Color32,
) {
    let cols = ((rect.width() / 7.0) as usize).clamp(11, 41) | 1;
    let rows = 2usize;
    let half = (cols / 2) as isize;
    let center_col = cols / 2;
    let unlit = unlit_on_inset();
    let accent_vis = visible_on_inset(accent);

    // Centre lock tick marks at top and bottom of the phase strip
    let cx = rect.center().x;
    painter.line_segment(
        [Pos2::new(cx, rect.top() - 1.5), Pos2::new(cx, rect.bottom() + 1.5)],
        Stroke::new(1.0_f32, ink.gamma_multiply(0.45)),
    );

    let offset = phase_diff.map(|d| phase_col_offset(d, half));
    paint_dot_grid(painter, rect, cols, rows, 1.75, unlit, |c, _r| {
        let Some(off) = offset else {
            return if c == center_col {
                DotCell::Hollow(ink.gamma_multiply(0.4))
            } else {
                DotCell::Unlit
            };
        };
        let target = (center_col as isize + off).clamp(0, (cols - 1) as isize) as usize;
        let (lo, hi) = (center_col.min(target), center_col.max(target));
        if c >= lo && c <= hi {
            if off == 0 {
                DotCell::Filled(ink)
            } else {
                DotCell::Filled(accent_vis)
            }
        } else {
            DotCell::Unlit
        }
    });
}

/// Paints a horizontal dot-matrix wipe overlay across `rect` when `wipe_progress` is in `0.01..0.99`.
/// Used on the deck display when a track loads or a loop engages.
pub fn paint_dot_wipe(painter: &Painter, rect: Rect, wipe_progress: f32, accent: Color32) {
    if !theme::animations() || wipe_progress <= 0.01 || wipe_progress >= 0.99 {
        return;
    }
    let cols = ((rect.width() / 8.0).floor() as usize).clamp(12, 48);
    let rows = ((rect.height() / 8.0).floor() as usize).clamp(3, 8);
    let accent_vis = visible_on_inset(accent);
    let wave_center = wipe_progress * (cols as f32 + 4.0) - 2.0;
    let step_x = rect.width() / cols as f32;
    let step_y = rect.height() / rows as f32;

    for c in 0..cols {
        let dist = (c as f32 - wave_center).abs();
        if dist < 3.2 {
            let alpha = (1.0 - dist / 3.2).powi(2);
            let col = accent_vis.gamma_multiply(alpha * 0.85);
            let x = rect.left() + (c as f32 + 0.5) * step_x;
            for r in 0..rows {
                let y = rect.top() + (r as f32 + 0.5) * step_y;
                painter.circle_filled(Pos2::new(x, y), 2.2 * alpha.max(0.5), col);
            }
        }
    }
}

/// Pure helper: returns normalized `(deck_a_level, deck_b_level)` (`0.0..=1.0`) across a transition
/// for progress `t` in `0.0..=1.0`.
/// - `0`: Crossfade (linear falling / rising)
/// - `1`: Bass Swap (deck A drops bass sharply around the midpoint, deck B brings bass in)
/// - `2`: Vocal Bridge (deck B instrumental enters early, deck A vocal lingers through the bridge)
pub fn transition_curve_points(preset_idx: usize, progress: f32) -> (f32, f32) {
    let t = progress.clamp(0.0, 1.0);
    match preset_idx {
        0 => (1.0 - t, t),
        1 => {
            if t < 0.45 {
                (1.0, 0.0)
            } else if t > 0.55 {
                (0.0, 1.0)
            } else {
                let frac = (t - 0.45) / 0.10;
                (1.0 - frac, frac)
            }
        }
        _ => {
            let a = if t < 0.6 {
                1.0
            } else {
                (1.0 - (t - 0.6) / 0.4).clamp(0.0, 1.0)
            };
            let b = if t < 0.4 {
                (t / 0.4).clamp(0.0, 1.0)
            } else {
                1.0
            };
            (a, b)
        }
    }
}

/// Compact dot-matrix transition curve display showing:
/// - Deck A falling curve in DECK_A colour
/// - Deck B rising curve in DECK_B colour
/// - Live crossfader cursor tracking `crossfader_pos`
/// - Bar division ticks corresponding to `bars` (8, 16, 32)
/// - Wipe overlay animation when `wipe_progress` is in 0.01..0.99
pub fn dot_transition_display(
    ui: &mut Ui,
    preset_idx: usize,
    bars: u32,
    crossfader_pos: f32,
    wipe_progress: f32,
    size: Vec2,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let bg = TraktorTheme::WAVEFORM_BG();
    painter.rect(rect, 3.0, bg, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let col_a = visible_on_inset(TraktorTheme::DECK_A());
    let col_b = visible_on_inset(TraktorTheme::DECK_B());
    let top_y = rect.top() + 4.0;
    painter.text(Pos2::new(rect.left() + 8.0, top_y), Align2::LEFT_TOP, "A", theme::dot_font(11.0), col_a);
    painter.text(Pos2::new(rect.right() - 8.0, top_y), Align2::RIGHT_TOP, "B", theme::dot_font(11.0), col_b);

    let preset_name = match preset_idx {
        0 => "CROSSFADE",
        1 => "BASS SWAP",
        _ => "VOCAL BRIDGE",
    };
    let center_label = format!("{preset_name}  ·  {bars} BARS");
    painter.text(
        Pos2::new(rect.center().x, top_y + 1.0),
        Align2::CENTER_TOP,
        &center_label,
        FontId::proportional(9.0),
        TraktorTheme::TEXT_MUTED(),
    );

    let grid_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 22.0, rect.top() + 16.0),
        Pos2::new(rect.right() - 22.0, rect.bottom() - 4.0),
    );

    let cols = 32usize;
    let rows = 6usize;
    let unlit = unlit_on_inset();
    let cursor_col = ((crossfader_pos.clamp(0.0, 1.0) * (cols - 1) as f32).round() as usize).min(cols - 1);
    let highlight = TraktorTheme::TEXT_HIGHLIGHT();

    paint_dot_grid(painter, grid_rect, cols, rows, 1.8, unlit, |c, r| {
        let is_cursor = c == cursor_col;

        let t_mid = c as f32 / (cols - 1) as f32;
        let t_lo = (c as f32 - 0.5).max(0.0) / (cols - 1) as f32;
        let t_hi = (c as f32 + 0.5).min((cols - 1) as f32) / (cols - 1) as f32;

        let (a_lo, b_lo) = transition_curve_points(preset_idx, t_lo);
        let (a_mid, b_mid) = transition_curve_points(preset_idx, t_mid);
        let (a_hi, b_hi) = transition_curve_points(preset_idx, t_hi);

        let min_a = ((a_lo.min(a_mid).min(a_hi) * (rows - 1) as f32).round() as usize).min(rows - 1);
        let max_a = ((a_lo.max(a_mid).max(a_hi) * (rows - 1) as f32).round() as usize).min(rows - 1);
        let in_a = r >= min_a && r <= max_a;

        let min_b = ((b_lo.min(b_mid).min(b_hi) * (rows - 1) as f32).round() as usize).min(rows - 1);
        let max_b = ((b_lo.max(b_mid).max(b_hi) * (rows - 1) as f32).round() as usize).min(rows - 1);
        let in_b = r >= min_b && r <= max_b;

        if is_cursor {
            if in_a && in_b {
                DotCell::Filled(highlight)
            } else if in_a {
                DotCell::Filled(col_a)
            } else if in_b {
                DotCell::Filled(col_b)
            } else {
                DotCell::Scaled(TraktorTheme::TEXT_PRIMARY(), 0.75)
            }
        } else if in_a && in_b {
            DotCell::Filled(theme::mix(col_a, col_b, 0.5))
        } else if in_a {
            DotCell::Filled(col_a)
        } else if in_b {
            DotCell::Filled(col_b)
        } else {
            let phrase_step = match bars {
                8 => 4,
                16 => 2,
                _ => 1,
            };
            if r == 0 && c % phrase_step == 0 {
                DotCell::Scaled(TraktorTheme::TEXT_MUTED(), 0.5)
            } else {
                DotCell::Unlit
            }
        }
    });

    paint_dot_wipe(painter, grid_rect, wipe_progress, TraktorTheme::ACCENT());

    response
}

/// Simplified track info for the dot-matrix arc display.
#[derive(Clone, Debug, PartialEq)]
pub struct SetArcTrack {
    pub key: String,
    pub bpm: f64,
    pub energy: u8,
}

/// Dot-matrix "arc" display plotting energy as dot columns per track,
/// with BPM as a dotted line across tracks, the musical key written under each column,
/// and the selected track's column highlighted.
pub fn set_arc_display(
    ui: &mut Ui,
    tracks: &[SetArcTrack],
    selected_idx: Option<usize>,
    size: Vec2,
) -> (Response, Option<usize>) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    if !ui.is_rect_visible(rect) {
        return (response, None);
    }

    let painter = ui.painter();
    let bg = TraktorTheme::WAVEFORM_BG();
    painter.rect(rect, 4.0, bg, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    // Title / legend
    let top_y = rect.top() + 6.0;
    painter.text(
        Pos2::new(rect.left() + 8.0, top_y),
        Align2::LEFT_TOP,
        "SET FLOW & ENERGY ARC",
        theme::dot_font(10.0),
        TraktorTheme::TEXT_MUTED(),
    );
    painter.text(
        Pos2::new(rect.right() - 8.0, top_y + 1.0),
        Align2::RIGHT_TOP,
        "▪ ENERGY  ·  ● BPM",
        FontId::proportional(9.0),
        TraktorTheme::TEXT_MUTED(),
    );

    let grid_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 16.0, rect.top() + 24.0),
        Pos2::new(rect.right() - 16.0, rect.bottom() - 18.0),
    );

    // Each track is two dots wide with a one-dot gap.
    let cols = (tracks.len() * 3).max(24) - 1;
    let rows = 10usize;
    let unlit = unlit_on_inset();

    let bpms: Vec<f64> = tracks.iter().map(|t| t.bpm).filter(|&b| b > 0.0).collect();
    let (min_bpm, max_bpm) = if bpms.is_empty() {
        (110.0, 140.0)
    } else {
        let mut min = bpms.iter().copied().fold(f64::INFINITY, f64::min);
        let mut max = bpms.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if (max - min) < 6.0 {
            min -= 3.0;
            max += 3.0;
        }
        (min, max)
    };

    let dot_radius = 2.0_f32;

    paint_dot_grid(painter, grid_rect, cols, rows, dot_radius, unlit, |c, r| {
        if c % 3 == 2 || c / 3 >= tracks.len() {
            return DotCell::Unlit;
        }
        let t = &tracks[c / 3];
        let is_selected = selected_idx == Some(c / 3);
        let e_lit = (t.energy as usize).min(rows);

        let bpm_ratio = if max_bpm > min_bpm && t.bpm > 0.0 {
            ((t.bpm - min_bpm) / (max_bpm - min_bpm)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let bpm_r = ((bpm_ratio * (rows - 1) as f64).round() as usize).min(rows - 1);

        if r == bpm_r {
            let col = if is_selected {
                TraktorTheme::TEXT_HIGHLIGHT()
            } else {
                TraktorTheme::PLAY_ACTIVE()
            };
            DotCell::Filled(col)
        } else if r < e_lit {
            let col = if is_selected {
                TraktorTheme::TEXT_HIGHLIGHT()
            } else {
                visible_on_inset(TraktorTheme::ACCENT())
            };
            DotCell::Filled(col)
        } else if is_selected {
            DotCell::Scaled(visible_on_inset(TraktorTheme::ACCENT()), 0.4)
        } else {
            DotCell::Unlit
        }
    });

    // Musical keys under each column
    for (c, t) in tracks.iter().enumerate() {
        let col_x = if cols > 1 {
            grid_rect.left() + dot_radius + (c as f32 * 3.0 + 0.5) * ((grid_rect.width() - dot_radius * 2.0) / (cols - 1) as f32)
        } else {
            grid_rect.center().x
        };
        let is_selected = selected_idx == Some(c);
        let key_col = if is_selected {
            TraktorTheme::TEXT_HIGHLIGHT()
        } else {
            TraktorTheme::TEXT_SECONDARY()
        };
        painter.text(
            Pos2::new(col_x, grid_rect.bottom() + 3.0),
            Align2::CENTER_TOP,
            &t.key,
            FontId::monospace(9.0),
            key_col,
        );
    }

    let mut clicked_col = None;
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if grid_rect.contains(pos) && !tracks.is_empty() {
                let step_x = if cols > 1 { (grid_rect.width() - dot_radius * 2.0) / (cols - 1) as f32 } else { 1.0 };
                let col = (((pos.x - (grid_rect.left() + dot_radius)) / step_x).round() as usize / 3).min(tracks.len().saturating_sub(1));
                clicked_col = Some(col);
            }
        }
    }

    (response, clicked_col)
}

/// Paints a short horizontal dot rule (e.g. for section headings in dot-matrix house style).
pub fn paint_dot_rule(painter: &Painter, origin: Pos2, count: usize, spacing: f32, radius: f32, color: Color32) {
    for i in 0..count {
        let p = Pos2::new(origin.x + (i as f32) * spacing, origin.y);
        painter.circle_filled(p, radius, color);
    }
}

/// Interactive dot-matrix two-state toggle switch.
/// Displays an animated sliding dot track (44x24 px) with ON/OFF label in dot font / monospace.
/// Keyboard navigable (Space/Enter) and clickable.
pub fn dot_toggle(ui: &mut Ui, value: &mut bool, accent: Color32, hover: &str) -> Response {
    let size = Vec2::new(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked()
        || (resp.has_focus() && ui.input(|i| i.key_pressed(Key::Space) || i.key_pressed(Key::Enter)))
    {
        *value = !*value;
        resp.mark_changed();
    }
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let anim = theme::animations();
        let t = if anim {
            ui.ctx().animate_bool(resp.id, *value)
        } else if *value {
            1.0
        } else {
            0.0
        };

        let bg = TraktorTheme::CONTROL();
        let border = if resp.has_focus() || resp.hovered() {
            TraktorTheme::BORDER_ACCENT()
        } else {
            TraktorTheme::BORDER_COLOR()
        };
        painter.rect(rect, 12.0, bg, Stroke::new(1.0_f32, border));

        let left_x = rect.left() + 11.0;
        let right_x = rect.right() - 11.0;
        let dot_x = left_x + (right_x - left_x) * t;
        let dot_center = Pos2::new(dot_x, rect.center().y);

        let dot_col = theme::mix(unlit_on_panel(), accent, t);
        painter.circle_filled(dot_center, 4.2, dot_col);
        if t < 0.6 {
            painter.circle_stroke(
                dot_center,
                4.2,
                Stroke::new(1.0_f32, theme::mix(TraktorTheme::TEXT_MUTED(), Color32::TRANSPARENT, t / 0.6)),
            );
        }

        let label_text = if *value { "ON" } else { "OFF" };
        let label_pos = if *value {
            Pos2::new(rect.left() + 12.0, rect.center().y)
        } else {
            Pos2::new(rect.right() - 12.0, rect.center().y)
        };
        let label_col = if *value {
            TraktorTheme::TEXT_PRIMARY()
        } else {
            TraktorTheme::TEXT_MUTED()
        };
        painter.text(label_pos, Align2::CENTER_CENTER, label_text, FontId::monospace(8.0), label_col);

        if resp.has_focus() {
            painter.rect_stroke(rect.expand(1.5), 13.0, Stroke::new(1.0_f32, TraktorTheme::TEXT_SECONDARY()));
        }
    }
    resp.on_hover_text(hover)
}

/// Interactive dot-matrix horizontal level slider (e.g. for sensitivity, resonance, brightness, offset).
/// Shows a bar of dots, numeric readout in dot font, supports mouse drag, click, and arrow keys.
pub fn dot_slider(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    display_text: &str,
    accent: Color32,
) -> Response {
    let size = Vec2::new(170.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let range = (max - min).max(1e-5);

    let bar_w = 90.0_f32;
    let bar_rect = Rect::from_min_size(rect.min + Vec2::new(0.0, 2.0), Vec2::new(bar_w, 20.0));

    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let frac = ((pos.x - bar_rect.left()) / bar_rect.width()).clamp(0.0, 1.0);
            *value = (min + frac * range).clamp(min, max);
            resp.mark_changed();
        }
    }
    if resp.double_clicked() {
        *value = default;
        resp.mark_changed();
    }
    if resp.has_focus() {
        let step = range * 0.05;
        if ui.input(|i| i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::ArrowDown)) {
            *value = (*value - step).clamp(min, max);
            resp.mark_changed();
        }
        if ui.input(|i| i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::ArrowUp)) {
            *value = (*value + step).clamp(min, max);
            resp.mark_changed();
        }
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let num_dots = 12usize;
        let norm = ((*value - min) / range).clamp(0.0, 1.0);
        let lit_count = (norm * num_dots as f32).round() as usize;

        let dot_spacing = (bar_w - 6.0) / (num_dots - 1) as f32;
        let cy = bar_rect.center().y;
        for i in 0..num_dots {
            let cx = bar_rect.left() + 3.0 + i as f32 * dot_spacing;
            let is_lit = i < lit_count;
            let col = if is_lit { accent } else { unlit_on_panel() };
            painter.circle_filled(Pos2::new(cx, cy), if is_lit { 2.1 } else { 1.6 }, col);
        }

        let text_pos = Pos2::new(rect.right() - 2.0, rect.center().y);
        painter.text(
            text_pos,
            Align2::RIGHT_CENTER,
            display_text,
            theme::dot_font(10.5),
            TraktorTheme::TEXT_PRIMARY(),
        );

        if resp.hovered() || resp.has_focus() {
            painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT()));
        }
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn continuous_mixer_pointer_drags() {
        use eframe::egui;
        for two_channel_only in [true, false] {
            let ctx = egui::Context::default();
            crate::theme::install_fonts(&ctx);
            ctx.style_mut(|s| s.interaction.tooltip_delay = 999.0);
            let mut volume = [0.5; 4];
            let mut cue = [false; 4];
            let mut low = [0.5; 4];
            let mut mid = [0.5; 4];
            let mut hi = [0.5; 4];
            let mut kill_low = [false; 4];
            let mut kill_mid = [false; 4];
            let mut kill_hi = [false; 4];
            let mut muted = [[false; 4]; 4];
            let mut sends = [[false; 4]; 4];
            let mut xfader = 0.5;
            let mut cheer = None;
            let mut mixer = ContinuousMixer {
                two_channel_only, volume: &mut volume, cue: &mut cue,
                eq_low: &mut low, eq_mid: &mut mid, eq_hi: &mut hi,
                eq_kill_low: &mut kill_low, eq_kill_mid: &mut kill_mid,
                eq_kill_hi: &mut kill_hi, filter: [0.5; 4], filter_on: [false; 4],
                stem_view: [false; 4], stem_muted: &mut muted, stem_fx_send: &mut sends,
                crossfader: &mut xfader, channel_peaks: [0.0; 4],
                band_peaks: [[0.0; 3]; 4], stem_peaks: [[0.0; 4]; 4],
                main_peaks: (0.0, 0.0), stem_progress: [None; 4],
                active_cheer: &mut cheer, deck_track_ids: [None; 4],
                deck_sync_enabled: [false; 4], in_key: false, is_recording: false,
                size: Vec2::new(500.0, 220.0),
            };
            let mut time = 0.0;
            let mut frame = |mixer: &mut ContinuousMixer<'_>, events| {
                time += 0.1;
                let mut rect = egui::Rect::NOTHING;
                let _ = ctx.run(egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                    time: Some(time), events, ..Default::default()
                }, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| { rect = mixer.show(ui).rect; });
                });
                rect
            };
            let rect = frame(&mut mixer, vec![egui::Event::PointerMoved(Pos2::ZERO)]);
            let cols = 93;
            let layout = compute_mixer_zone_layout(cols, two_channel_only);
            let col_x = |c: usize| rect.left() + 9.35 + c as f32 * (500.0 - 18.7) / (cols - 1) as f32;
            let button = |pos, pressed| egui::Event::PointerButton {
                pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE,
            };
            let channels: &[usize] = if two_channel_only { &[0, 1] } else { &[0, 2, 3, 1] };
            let mut targets = vec![(Pos2::new(rect.center().x, rect.bottom() - 12.0), Vec2::new(50.0, 0.0), None)];
            for &d in channels {
                let zone = layout.fader_meter[d].unwrap();
                targets.push((Pos2::new(col_x((zone.start + zone.end) / 2), rect.center().y), Vec2::new(0.0, -20.0), Some(d)));
            }
            for (start, delta, deck) in targets {
                frame(&mut mixer, vec![egui::Event::PointerMoved(start), button(start, true)]);
                frame(&mut mixer, vec![egui::Event::PointerMoved(start + delta)]);
                frame(&mut mixer, vec![egui::Event::PointerMoved(start + delta * 2.0)]);
                let value = match deck { Some(d) => mixer.volume[d], None => *mixer.crossfader };
                assert!(value > 0.5, "drag failed for {deck:?}, two-channel={two_channel_only}");
                frame(&mut mixer, vec![button(start + delta * 2.0, false)]);
                frame(&mut mixer, vec![egui::Event::PointerMoved(start)]);
                assert_eq!(value, match deck { Some(d) => mixer.volume[d], None => *mixer.crossfader });
            }
            let zone = layout.eq_stem_zone[0].unwrap();
            let start = Pos2::new(col_x(zone.start + 1), rect.center().y);
            frame(&mut mixer, vec![egui::Event::PointerMoved(start), button(start, true)]);
            frame(&mut mixer, vec![egui::Event::PointerMoved(start - Vec2::new(0.0, 20.0))]);
            assert!(mixer.eq_low[0] > 0.5);
            frame(&mut mixer, vec![button(start - Vec2::new(0.0, 20.0), false)]);
            let center = Pos2::new(rect.center().x, rect.bottom() - 12.0);
            frame(&mut mixer, vec![egui::Event::PointerMoved(center), button(center, true)]);
            frame(&mut mixer, vec![button(center, false)]);
            assert!((*mixer.crossfader - 0.5).abs() < 0.01);
            // A single click at the foot of deck A's LOW band cuts it; another restores it.
            let rows = ((208.0_f32 - 2.7) / 5.2).round() as usize;
            let foot = Pos2::new(col_x(zone.start + 1), rect.bottom() - 7.35 - 9.0 * (205.3 / (rows - 1) as f32));
            for want in [true, false] {
                for _ in 0..5 { frame(&mut mixer, vec![egui::Event::PointerMoved(foot)]); }
                frame(&mut mixer, vec![button(foot, true)]);
                frame(&mut mixer, vec![button(foot, false)]);
                assert_eq!(mixer.eq_kill_low[0], want, "two-channel={two_channel_only}");
            }
            // A stationary press/release on a cue lamp must remain a click.
            let fader = layout.fader_meter[0].unwrap();
            let cue_pos = Pos2::new(col_x(fader.start), rect.bottom() - 7.35 - 5.0 * (220.0 - 14.7) / 38.0);
            frame(&mut mixer, vec![egui::Event::PointerMoved(cue_pos), button(cue_pos, true)]);
            frame(&mut mixer, vec![button(cue_pos, false)]);
            assert!(mixer.cue[0]);
        }
    }

    #[test]
    fn ring_dots_follow_bipolar_and_unipolar_values() {
        // Bipolar centered at 0.5: only center dot (index 5 of 11) lit
        for i in 0..11 {
            assert_eq!(ring_dot_lit(i, 11, 0.5, 0.5), i == 5, "center dot at i={i}");
        }
        // Bipolar boosted to 0.8: dots 5..=8 lit
        assert!(!ring_dot_lit(3, 11, 0.8, 0.5));
        assert!(ring_dot_lit(5, 11, 0.8, 0.5));
        assert!(ring_dot_lit(8, 11, 0.8, 0.5));
        assert!(!ring_dot_lit(10, 11, 0.8, 0.5));

        // Bipolar cut to 0.2: dots 2..=5 lit
        assert!(!ring_dot_lit(0, 11, 0.2, 0.5));
        assert!(ring_dot_lit(2, 11, 0.2, 0.5));
        assert!(ring_dot_lit(5, 11, 0.2, 0.5));
        assert!(!ring_dot_lit(7, 11, 0.2, 0.5));

        // Unipolar from 0.0: 0 is unlit, 0.5 lights 0..=5
        assert!(!ring_dot_lit(0, 11, 0.0, 0.0));
        assert!(ring_dot_lit(0, 11, 0.5, 0.0));
        assert!(ring_dot_lit(5, 11, 0.5, 0.0));
        assert!(!ring_dot_lit(7, 11, 0.5, 0.0));
    }

    #[test]
    fn value_and_phase_mappings_are_bounded() {
        assert_eq!(value_to_lit_rows(0.0, 18), 0);
        assert_eq!(value_to_lit_rows(0.5, 18), 9);
        assert_eq!(value_to_lit_rows(1.2, 18), 18);

        assert_eq!(phase_col_offset(0.0, 5), 0);
        assert_eq!(phase_col_offset(0.25, 5), 3);
        assert_eq!(phase_col_offset(-0.5, 5), -5);
        assert_eq!(phase_col_offset(0.9, 5), 5);
    }

    #[test]
    fn filter_sweep_and_db_formatting() {
        // Bypassed or centred filter leaves all bands at 1.0
        assert_eq!(filter_band_gain(0.5, true, 0.0), 1.0);
        assert_eq!(filter_band_gain(0.0, false, 1.0), 1.0);
        // Low-pass (filter < 0.5) attenuates HI (1.0) while leaving LOW (0.0) intact
        assert!(filter_band_gain(0.2, true, 1.0) < 0.5);
        assert_eq!(filter_band_gain(0.2, true, 0.0), 1.0);
        // High-pass (filter > 0.5) attenuates LOW (0.0) while leaving HI (1.0) intact
        assert!(filter_band_gain(0.8, true, 0.0) < 0.5);
        assert_eq!(filter_band_gain(0.8, true, 1.0), 1.0);

        assert_eq!(format_eq_db(0.5, false), "0");
        assert_eq!(format_eq_db(0.5, true), "CUT");
        assert!(format_eq_db(0.75, false).starts_with('+'));
        assert!(format_eq_db(0.25, false).starts_with('-'));
        assert_eq!(format_db(0.0, 0.85), "-∞");
        assert_eq!(format_db(0.85, 0.85), "0");
    }

    #[test]
    fn sprites_and_cheers_are_well_formed() {
        assert!(CHEER_SPRITES_COUNT >= 8);
        assert_eq!(CHEER_SPRITES.len(), CHEER_SPRITES_COUNT);
        for s in CHEER_SPRITES.iter() {
            assert!(!s.name.is_empty());
            assert!(!s.frames.is_empty());
            let (w, h) = s.dimensions(0);
            assert!(w > 0 && h > 0, "sprite {} must have non-zero dimensions", s.name);
        }

        assert_eq!(STEM_SPRITES.len(), 4);
        for s in STEM_SPRITES.iter() {
            let (w, h) = s.dimensions(0);
            assert_eq!(w, 5, "stem sprite {} must be 5 dots wide", s.name);
            assert!(h >= 5 && h <= 6, "stem sprite {} must be 5-6 dots high", s.name);
        }

        let (kw, kh) = SPRITE_KILL_X.dimensions(0);
        assert_eq!(kw, 5);
        assert_eq!(kh, 5);
        let (tw, th) = SPRITE_TICK.dimensions(0);
        assert_eq!(tw, 5);
        assert_eq!(th, 5);
    }

    #[test]
    fn bounce_offset_is_bounded_and_settles() {
        assert_eq!(bounce_dot_offset(0.0, 0.6, 5), 0);
        assert_eq!(bounce_dot_offset(0.6, 0.6, 5), 0);
        assert_eq!(bounce_dot_offset(1.0, 0.6, 5), 0);
        // During the bounce, hops produce positive integer offsets
        let hop1 = bounce_dot_offset(0.12, 0.6, 5);
        assert!(hop1 >= 2 && hop1 <= 5, "hop1 was {hop1}");
        let hop2 = bounce_dot_offset(0.48, 0.6, 5);
        assert!(hop2 >= 0 && hop2 <= 2, "hop2 was {hop2}");
    }

    #[test]
    fn transition_curve_points_follow_dj_presets() {
        // Crossfade
        let (a0, b0) = transition_curve_points(0, 0.0);
        assert!((a0 - 1.0).abs() < 1e-5);
        assert!(b0.abs() < 1e-5);

        let (a_mid, b_mid) = transition_curve_points(0, 0.5);
        assert!((a_mid - 0.5).abs() < 1e-5);
        assert!((b_mid - 0.5).abs() < 1e-5);

        let (a1, b1) = transition_curve_points(0, 1.0);
        assert!(a1.abs() < 1e-5);
        assert!((b1 - 1.0).abs() < 1e-5);

        // Bass swap
        let (a_bs_early, b_bs_early) = transition_curve_points(1, 0.3);
        assert_eq!((a_bs_early, b_bs_early), (1.0, 0.0));
        let (a_bs_mid, b_bs_mid) = transition_curve_points(1, 0.5);
        assert!((a_bs_mid - 0.5).abs() < 1e-5);
        assert!((b_bs_mid - 0.5).abs() < 1e-5);
        let (a_bs_late, b_bs_late) = transition_curve_points(1, 0.7);
        assert_eq!((a_bs_late, b_bs_late), (0.0, 1.0));

        // Vocal bridge
        let (a_vb_early, b_vb_early) = transition_curve_points(2, 0.2);
        assert_eq!(a_vb_early, 1.0);
        assert!((b_vb_early - 0.5).abs() < 1e-5);
        let (a_vb_mid, b_vb_mid) = transition_curve_points(2, 0.5);
        assert_eq!((a_vb_mid, b_vb_mid), (1.0, 1.0)); // Overlap plateau
        let (a_vb_late, b_vb_late) = transition_curve_points(2, 0.8);
        assert!((a_vb_late - 0.5).abs() < 1e-5);
        assert_eq!(b_vb_late, 1.0);
    }

    #[test]
    fn set_arc_track_instantiation() {
        let t = SetArcTrack {
            key: "8m".to_string(),
            bpm: 124.0,
            energy: 7,
        };
        assert_eq!(t.key, "8m");
        assert_eq!(t.bpm, 124.0);
        assert_eq!(t.energy, 7);
    }

    #[test]
    fn cheer_wave_intensity_behavior() {
        // Zero at or after duration
        assert_eq!(cheer_wave_intensity(20, 20, 1.25, 1.2, 45.0), 0.0);
        assert_eq!(cheer_wave_intensity(20, 20, -0.1, 1.2, 45.0), 0.0);

        // At t = 0.2, wave has expanded by r = 0.2 * 45 = 9 cols.
        // Near distance 9 it should have positive intensity
        let near_wave = cheer_wave_intensity(29, 20, 0.2, 1.2, 45.0);
        let far_wave = cheer_wave_intensity(50, 20, 0.2, 1.2, 45.0);
        assert!(near_wave > 0.0, "near_wave should be positive: {near_wave}");
        assert_eq!(far_wave, 0.0, "far away should be 0: {far_wave}");
        assert!(near_wave <= 1.0, "intensity must be <= 1.0");
    }

    #[test]
    fn track_load_wipe_col_behavior() {
        assert_eq!(track_load_wipe_col(10, 0, 20, -0.05, 0.7), 0.0);
        assert_eq!(track_load_wipe_col(10, 0, 20, 0.75, 0.7), 0.0);

        // Halfway through (t = 0.35), wipe head is at 10
        let at_head = track_load_wipe_col(10, 0, 20, 0.35, 0.7);
        let far_away = track_load_wipe_col(0, 0, 20, 0.35, 0.7);
        assert!(at_head > 0.5, "at_head should be bright: {at_head}");
        assert!(far_away < at_head, "far from wipe head should be dimmer");
    }

    #[test]
    fn sync_pulse_col_behavior() {
        assert_eq!(sync_pulse_col(15, 0, 30, -0.1, 0.6), 0.0);
        assert_eq!(sync_pulse_col(15, 0, 30, 0.65, 0.6), 0.0);

        // Around halfway (t = 0.3), pulse should be around mid (15)
        let at_mid = sync_pulse_col(15, 0, 30, 0.3, 0.6);
        let at_end = sync_pulse_col(30, 0, 30, 0.3, 0.6);
        assert!(at_mid > 0.5, "pulse should peak near curr pos: {at_mid}");
        assert_eq!(at_end, 0.0, "far ahead of pulse should be 0: {at_end}");
    }

    #[test]
    fn recording_blink_behavior() {
        assert!(!recording_blink_lit(0.2, false));
        assert!(recording_blink_lit(0.2, true));
        assert!(!recording_blink_lit(0.7, true));
        assert!(recording_blink_lit(1.2, true));
    }

    #[test]
    fn harmonically_compatible_link_dots_behavior() {
        assert!(!harmonically_compatible_link_dots(None, None));
        assert!(!harmonically_compatible_link_dots(Some(""), Some("8m")));
        // 8m (Am) is compatible with 8m (same), 7m (Em), 9m (Dm), 8d (C major)
        assert!(harmonically_compatible_link_dots(Some("8m"), Some("8m")));
        assert!(harmonically_compatible_link_dots(Some("8m"), Some("7m")));
        assert!(harmonically_compatible_link_dots(Some("8m"), Some("8d")));
        // Incompatible key (e.g. 2m vs 8m)
        assert!(!harmonically_compatible_link_dots(Some("8m"), Some("2m")));
    }

    #[test]
    fn compute_mixer_zone_layout_symmetry_and_bounds() {
        for cols in [60, 80, 100, 140, 180] {
            // 2-channel mode
            let l2 = compute_mixer_zone_layout(cols, true);
            assert_eq!(l2.mid, cols / 2);
            assert!(l2.main_meter_l < l2.mid);
            assert!(l2.main_meter_r > l2.mid);
            assert_eq!(l2.mid - l2.main_meter_l, l2.main_meter_r - l2.mid);

            // Mirror symmetry check between Deck A and Deck B
            let a_eq = l2.eq_stem_zone[0].expect("Deck A EQ zone must exist");
            let b_eq = l2.eq_stem_zone[1].expect("Deck B EQ zone must exist");
            let a_fad = l2.fader_meter[0].expect("Deck A fader zone must exist");
            let b_fad = l2.fader_meter[1].expect("Deck B fader zone must exist");

            assert_eq!(a_eq.len(), b_eq.len(), "EQ zones must be symmetric in length");
            assert_eq!(a_fad.len(), b_fad.len(), "Fader zones must be symmetric in length");
            assert_eq!(l2.mid - a_eq.start, b_eq.end - l2.mid, "EQ outer ends must be equidistant from mid");
            assert_eq!(l2.mid - a_fad.start, b_fad.end - l2.mid, "Faders must be equidistant from mid");

            // 4-channel mode
            let l4 = compute_mixer_zone_layout(cols, false);
            assert_eq!(l4.mid, cols / 2);
            for d in 0..4 {
                assert!(l4.eq_stem_zone[d].is_some(), "Deck {d} must have EQ zone in 4-deck");
                assert!(l4.fader_meter[d].is_some(), "Deck {d} must have fader zone in 4-deck");
            }
            // Decks C and D symmetry
            let c_eq = l4.eq_stem_zone[2].unwrap();
            let d_eq = l4.eq_stem_zone[3].unwrap();
            assert_eq!(c_eq.len(), d_eq.len());
            assert_eq!(l4.mid - c_eq.start, d_eq.end - l4.mid);
        }
    }

    #[test]
    fn dot_slider_normalization_and_clamping() {
        let min = -50.0_f32;
        let max = 50.0_f32;
        let range = max - min;
        let norm = |v: f32| ((v - min) / range).clamp(0.0, 1.0);
        assert_eq!(norm(-50.0), 0.0);
        assert_eq!(norm(0.0), 0.5);
        assert_eq!(norm(50.0), 1.0);
        assert_eq!(norm(-100.0), 0.0);
        assert_eq!(norm(100.0), 1.0);
    }
}
