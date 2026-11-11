//! Reusable dot-matrix display primitives for the Mix view and companion screens.
//!
//! All colours come from [`crate::theme::TraktorTheme`] and [`crate::theme`] helpers;
//! all motion is gated by [`crate::theme::animations`].

use crate::theme::{self, TraktorTheme};
use eframe::egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

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

/// Two-column dot meter with an interactive fader bracket, dB readout in `dot_font`, and headphone cue lamp.
/// Dragging vertically moves `volume`; double-clicking resets `volume` to `0.85`; clicking the bottom
/// headphone lamp toggles `cue`.
#[allow(clippy::too_many_arguments)]
pub fn fader_meter(
    ui: &mut Ui,
    volume: &mut f32,
    level_l: f32,
    level_r: f32,
    cue: &mut bool,
    accent: Color32,
    height: f32,
    width: f32,
) -> Response {
    let w = width.max(32.0);
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(w, height), Sense::click_and_drag());

    let footer_h = 34.0;
    let meter_top = rect.top() + 6.0;
    let meter_bot = (rect.bottom() - footer_h).max(meter_top + 36.0);
    let track_h = (meter_bot - meter_top).max(20.0);
    let cue_rect = Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 22.0), rect.max);

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if cue_rect.contains(pos) {
                *cue = !*cue;
                response.mark_changed();
            }
        }
    }
    if response.dragged() {
        let delta = -response.drag_delta().y / track_h;
        *volume = (*volume + delta).clamp(0.0, 1.0);
        response.mark_changed();
    }
    if response.double_clicked() {
        *volume = 0.85;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let cx = rect.center().x;
        let rows = ((track_h / 8.2).floor() as usize).clamp(10, 18);
        let step_y = if rows > 1 { track_h / (rows - 1) as f32 } else { track_h };
        let hy = meter_top + (1.0 - volume.clamp(0.0, 1.0)) * track_h;
        let unlit_col = unlit_on_panel();
        let anim = theme::animations();
        let eff_l = if anim { level_l } else { 0.0 } * *volume;
        let eff_r = if anim { level_r } else { 0.0 } * *volume;
        let lit_l = value_to_lit_rows(eff_l, rows);
        let lit_r = value_to_lit_rows(eff_r, rows);

        for k in 0..rows {
            let yy = meter_bot - k as f32 * step_y;
            let above = yy < hy - 4.0;
            let r = if above { 1.55 } else { 2.65 };
            for (dx, lit_rows) in [(-4.5_f32, lit_l), (4.5_f32, lit_r)] {
                let col = if k < lit_rows {
                    if k >= rows.saturating_sub(2) {
                        TraktorTheme::METER_RED()
                    } else {
                        accent
                    }
                } else {
                    unlit_col
                };
                painter.circle_filled(Pos2::new(cx + dx, yy), r, col);
            }
        }

        // Fader bracket with side wings (from mixer-v2.png)
        let bw = (w - 8.0).clamp(24.0, 32.0);
        let bh = 13.0;
        let bracket = Rect::from_center_size(Pos2::new(cx, hy), Vec2::new(bw, bh));
        let stroke = Stroke::new(1.8_f32, TraktorTheme::TEXT_PRIMARY());
        painter.rect_stroke(bracket, 4.0, stroke);
        painter.line_segment([Pos2::new(bracket.left() - 4.5, hy), Pos2::new(bracket.left(), hy)], stroke);
        painter.line_segment([Pos2::new(bracket.right(), hy), Pos2::new(bracket.right() + 4.5, hy)], stroke);

        // dB readout in dot font
        let db_str = format_db(*volume, 0.85);
        painter.text(
            Pos2::new(cx, meter_bot + 10.0),
            Align2::CENTER_CENTER,
            db_str,
            theme::dot_font(11.5),
            TraktorTheme::TEXT_PRIMARY(),
        );

        // Small headphone cue lamp below the dB readout
        let lamp_y = rect.bottom() - 8.0;
        let cue_col = if *cue { TraktorTheme::CUE_ACTIVE() } else { TraktorTheme::TEXT_MUTED() };
        if *cue {
            painter.circle_filled(Pos2::new(cx - 8.0, lamp_y), 3.2, cue_col);
        } else {
            painter.circle_stroke(Pos2::new(cx - 8.0, lamp_y), 2.8, Stroke::new(1.1_f32, cue_col));
        }
        painter.text(
            Pos2::new(cx + 4.0, lamp_y),
            Align2::CENTER_CENTER,
            "CUE",
            FontId::proportional(7.5),
            if *cue { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_MUTED() },
        );
    }

    response
}

/// Centre stereo two-column dot meter (for `MAIN`) in the same dot-matrix language as [`fader_meter`].
pub fn dot_meter(ui: &mut Ui, level_l: f32, level_r: f32, label: &str, height: f32, width: f32) -> Response {
    let w = width.max(26.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(w, height), Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let cx = rect.center().x;
    let footer_h = 34.0;
    let meter_top = rect.top() + 6.0;
    let meter_bot = (rect.bottom() - footer_h).max(meter_top + 36.0);
    let track_h = (meter_bot - meter_top).max(20.0);
    let rows = ((track_h / 8.2).floor() as usize).clamp(10, 18);
    let step_y = if rows > 1 { track_h / (rows - 1) as f32 } else { track_h };
    let unlit_col = unlit_on_panel();
    let anim = theme::animations();
    let lit_l = value_to_lit_rows(if anim { level_l } else { 0.0 }, rows);
    let lit_r = value_to_lit_rows(if anim { level_r } else { 0.0 }, rows);

    for k in 0..rows {
        let yy = meter_bot - k as f32 * step_y;
        for (dx, lit_rows) in [(-4.0_f32, lit_l), (4.0_f32, lit_r)] {
            let col = if k < lit_rows {
                if k >= rows.saturating_sub(2) {
                    TraktorTheme::METER_RED()
                } else if k >= rows.saturating_sub(4) {
                    TraktorTheme::METER_YELLOW()
                } else {
                    TraktorTheme::TEXT_PRIMARY()
                }
            } else {
                unlit_col
            };
            painter.circle_filled(Pos2::new(cx + dx, yy), 2.25, col);
        }
    }

    painter.text(
        Pos2::new(cx, meter_bot + 10.0),
        Align2::CENTER_CENTER,
        label,
        FontId::proportional(8.5),
        TraktorTheme::TEXT_SECONDARY(),
    );

    response
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

/// Per-deck dot-matrix EQ display (dark inset): three band columns/blocks `LOW / MID / HI`
/// (mirrored on deck B so `LOW` is on the outer edge for both) whose lit height is the EQ knob value,
/// brightness follows that band's live level, filter sweeps across it, and killed bands render as hollow dots.
/// Also draws the smooth EQ contour curve from `mixer-v2.png`.
/// Drag vertically on any band to adjust its EQ knob; double-click to reset to 0 dB; right-click or click
/// the top dB readout to toggle EQ kill.
#[allow(clippy::too_many_arguments)]
pub fn eq_display(
    ui: &mut Ui,
    eq_low: &mut f32,
    eq_mid: &mut f32,
    eq_hi: &mut f32,
    kill_low: &mut bool,
    kill_mid: &mut bool,
    kill_hi: &mut bool,
    band_levels: [f32; 3],
    filter: f32,
    filter_on: bool,
    mirrored: bool,
    accent: Color32,
    size: Vec2,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let band_w = rect.width() / 3.0;

    // Band order left-to-right: A = [LOW(0), MID(1), HI(2)], B (mirrored) = [HI(2), MID(1), LOW(0)]
    let band_at_slot = |slot: usize| -> usize {
        if mirrored { 2 - slot.min(2) } else { slot.min(2) }
    };

    if let Some(pos) = response.interact_pointer_pos() {
        let slot = (((pos.x - rect.left()) / band_w).floor() as usize).min(2);
        let b = band_at_slot(slot);
        if response.dragged() {
            let delta = -response.drag_delta().y / (rect.height() * 0.65).max(30.0);
            let val = match b {
                0 => &mut *eq_low,
                1 => &mut *eq_mid,
                _ => &mut *eq_hi,
            };
            *val = (*val + delta).clamp(0.0, 1.0);
            response.mark_changed();
        }
        if response.double_clicked() {
            match b {
                0 => *eq_low = 0.5,
                1 => *eq_mid = 0.5,
                _ => *eq_hi = 0.5,
            }
            response.mark_changed();
        }
        if response.secondary_clicked() || (response.clicked() && pos.y < rect.top() + 24.0) {
            match b {
                0 => *kill_low = !*kill_low,
                1 => *kill_mid = !*kill_mid,
                _ => *kill_hi = !*kill_hi,
            }
            response.mark_changed();
        }
    }

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let bg = TraktorTheme::WAVEFORM_BG();
    let ink = theme::on(bg);
    let accent_vis = visible_on_inset(accent);
    let unlit = unlit_on_inset();
    painter.rect_filled(rect, 6.0, bg);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let pad_top = 22.0;
    let pad_bot = 18.0;
    let body_top = rect.top() + pad_top;
    let body_bot = (rect.bottom() - pad_bot).max(body_top + 24.0);
    let body_h = body_bot - body_top;
    let mid_y = (body_top + body_bot) * 0.5;

    // Subtle 0 dB center reference line
    let dash_step = 6.0_f32;
    let mut dx = rect.left() + 8.0;
    while dx < rect.right() - 8.0 {
        painter.circle_filled(Pos2::new(dx, mid_y), 1.0, unlit);
        dx += dash_step;
    }

    let vals = [*eq_low, *eq_mid, *eq_hi];
    let kills = [*kill_low, *kill_mid, *kill_hi];
    let labels = ["LOW", "MID", "HI"];
    let anim = theme::animations();

    let rows = ((body_h / 4.8).floor() as usize).clamp(16, 32);
    let cols_per_band = ((band_w / 4.6).floor() as usize).clamp(5, 12);
    let dot_radius = 1.3_f32;
    let mut node_centers = [Pos2::ZERO; 3];

    for slot in 0..3 {
        let b = band_at_slot(slot);
        let slot_left = rect.left() + slot as f32 * band_w;
        let slot_cx = slot_left + band_w * 0.5;
        let val = vals[b].clamp(0.0, 1.0);
        let killed = kills[b];
        let live = if anim { band_levels[b].clamp(0.0, 1.0) } else { 0.0 };

        // Detect kill state change between frames via egui temp memory
        let kill_id = ui.make_persistent_id(("eq_kill_anim", mirrored, b));
        let now = ui.input(|i| i.time);
        let prev_kill: Option<(bool, f64)> = ui.ctx().data(|d| d.get_temp(kill_id));
        let (last_killed, mut anim_start) = prev_kill.unwrap_or((killed, -100.0));
        if killed != last_killed {
            anim_start = now;
            ui.ctx().data_mut(|d| d.insert_temp(kill_id, (killed, now)));
        }
        let elapsed = (now - anim_start) as f32;
        let is_animating = anim && elapsed >= 0.0 && elapsed < 0.6;
        if is_animating {
            ui.ctx().request_repaint();
        }

        // Top value readout in dot font
        painter.text(
            Pos2::new(slot_cx, rect.top() + 11.0),
            Align2::CENTER_CENTER,
            format_eq_db(val, killed),
            theme::dot_font(11.0),
            if killed { TraktorTheme::METER_RED() } else { ink },
        );

        // Bottom band label
        painter.text(
            Pos2::new(slot_cx, rect.bottom() - 8.0),
            Align2::CENTER_CENTER,
            labels[b],
            FontId::proportional(8.5),
            ink.gamma_multiply(0.68),
        );

        // Dot-matrix column block for this band
        let col_rect = Rect::from_min_max(
            Pos2::new(slot_left + 4.0, body_top + 2.0),
            Pos2::new(slot_left + band_w - 4.0, body_bot - 2.0),
        );
        let lit_rows = value_to_lit_rows(val, rows);
        paint_dot_grid(painter, col_rect, cols_per_band, rows, dot_radius, unlit, |c, r| {
            // Frequency position 0.0 (LOW) .. 1.0 (HI) for the filter sweep
            let sub = if cols_per_band > 1 { c as f32 / (cols_per_band - 1) as f32 } else { 0.5 };
            let sub_freq = if mirrored { 1.0 - sub } else { sub };
            let freq_pos = (b as f32 + (sub_freq - 0.5) * 0.8) / 2.0;
            let flt_gain = filter_band_gain(filter, filter_on, freq_pos);

            if r < lit_rows {
                let level_boost = 0.65 + 0.35 * live;
                let col = accent_vis.gamma_multiply((level_boost * flt_gain).clamp(0.22, 1.0));
                if killed {
                    DotCell::Hollow(col)
                } else {
                    DotCell::Filled(col)
                }
            } else {
                DotCell::Unlit
            }
        });

        if anim {
            let step_x = if cols_per_band > 1 { (col_rect.width() - dot_radius * 2.0).max(1.0) / (cols_per_band - 1) as f32 } else { 0.0 };
            let step_y = if rows > 1 { (col_rect.height() - dot_radius * 2.0).max(1.0) / (rows - 1) as f32 } else { 0.0 };
            let top_dot_y = col_rect.top() + dot_radius;
            let icon_cy = top_dot_y + 2.0 * step_y;

            if is_animating {
                let bounce = bounce_dot_offset(elapsed, 0.6, 5);
                let pop_down = ((1.0 - (elapsed / 0.25).min(1.0)) * 6.0).round() as i32;
                let eff_bounce = bounce - pop_down;
                let sprite = if killed { &SPRITE_KILL_X } else { &SPRITE_TICK };
                let col = if killed { TraktorTheme::METER_RED() } else { TraktorTheme::PLAY_ACTIVE() };
                paint_sprite(
                    painter,
                    Pos2::new(slot_cx, icon_cy),
                    Vec2::new(step_x, step_y),
                    dot_radius,
                    sprite,
                    0,
                    eff_bounce,
                    SpriteMode::Filled,
                    col,
                    unlit,
                    false,
                );
            } else if killed {
                paint_sprite(
                    painter,
                    Pos2::new(slot_cx, icon_cy),
                    Vec2::new(step_x, step_y),
                    dot_radius,
                    &SPRITE_KILL_X,
                    0,
                    0,
                    SpriteMode::Hollow,
                    TraktorTheme::METER_RED(),
                    unlit,
                    false,
                );
            }
        }

        let node_y = body_bot - val * body_h;
        node_centers[slot] = Pos2::new(slot_cx, node_y);
    }

    // Smooth cubic EQ contour curve & circular band nodes (as in mixer-v2.png)
    let left_edge = Pos2::new(rect.left() + 6.0, node_centers[0].y);
    let right_edge = Pos2::new(rect.right() - 6.0, node_centers[2].y);
    let anchors = [left_edge, node_centers[0], node_centers[1], node_centers[2], right_edge];
    for pair in anchors.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let steps = 12usize;
        let mut prev = a;
        for s in 1..=steps {
            let t = s as f32 / steps as f32;
            // Smoothstep cubic interpolation
            let smooth = t * t * (3.0 - 2.0 * t);
            let next = Pos2::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * smooth);
            painter.line_segment([prev, next], Stroke::new(2.0_f32, accent_vis));
            prev = next;
        }
    }
    for (slot, &center) in node_centers.iter().enumerate() {
        let b = band_at_slot(slot);
        let killed = kills[b];
        painter.circle_filled(center, 5.5, bg);
        painter.circle_stroke(
            center,
            5.5,
            Stroke::new(1.8_f32, if killed { TraktorTheme::METER_RED() } else { ink }),
        );
    }

    response
}

/// Per-deck dot-matrix STEMS display (dark inset): four blocks `DRUMS / BASS / OTHER / VOCALS`,
/// each animated from that stem's live level. Clicking a block toggles mute (`stem_muted`);
/// right-clicking toggles FX send (`stem_fx_send`). Muted stems collapse to a dim hollow outline
/// with a staged dot wipe animation.
#[allow(clippy::too_many_arguments)]
pub fn stem_display(
    ui: &mut Ui,
    deck_idx: usize,
    stem_muted: &mut [bool; 4],
    stem_fx_send: &mut [bool; 4],
    stem_levels: [f32; 4],
    stem_progress: Option<f32>,
    mirrored: bool,
    accent: Color32,
    size: Vec2,
) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    let slot_w = rect.width() / 4.0;
    let stem_at_slot = |slot: usize| -> usize {
        if mirrored { 3 - slot.min(3) } else { slot.min(3) }
    };

    if let Some(pos) = response.interact_pointer_pos() {
        let slot = (((pos.x - rect.left()) / slot_w).floor() as usize).min(3);
        let s = stem_at_slot(slot);
        if response.clicked() {
            stem_muted[s] = !stem_muted[s];
            response.mark_changed();
        }
        if response.secondary_clicked() {
            stem_fx_send[s] = !stem_fx_send[s];
            response.mark_changed();
        }
    }

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let bg = TraktorTheme::WAVEFORM_BG();
    let ink = theme::on(bg);
    let accent_vis = visible_on_inset(accent);
    let unlit = unlit_on_inset();
    painter.rect_filled(rect, 6.0, bg);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let names = if rect.width() < 150.0 {
        ["DRM", "BAS", "OTH", "VOC"]
    } else {
        ["DRUMS", "BASS", "OTHER", "VOCALS"]
    };

    let pad_top = 22.0;
    let pad_bot = 18.0;
    let body_top = rect.top() + pad_top;
    let body_bot = (rect.bottom() - pad_bot).max(body_top + 24.0);
    let body_h = body_bot - body_top;
    let rows = ((body_h / 4.8).floor() as usize).clamp(16, 32);
    let cols = ((slot_w / 4.6).floor() as usize).clamp(5, 12);
    let dot_radius = 1.3_f32;
    let anim = theme::animations();

    for slot in 0..4 {
        let s = stem_at_slot(slot);
        let slot_left = rect.left() + slot as f32 * slot_w;
        let slot_cx = slot_left + slot_w * 0.5;
        let muted = stem_muted[s];
        let fx = stem_fx_send[s];
        let open_t = theme::fade(ui.ctx(), ("stem_wipe", deck_idx, s), !muted);

        // Detect toggle of stem_muted between frames via egui temp memory
        let stem_id = ui.make_persistent_id(("stem_anim", deck_idx, s));
        let now = ui.input(|i| i.time);
        let prev_state: Option<(bool, f64)> = ui.ctx().data(|d| d.get_temp(stem_id));
        let (last_muted, mut anim_start) = prev_state.unwrap_or((muted, -100.0));
        if muted != last_muted {
            anim_start = now;
        }
        // Stored every frame: a first toggle has nothing to compare against otherwise.
        ui.ctx().data_mut(|d| d.insert_temp(stem_id, (muted, anim_start)));
        let elapsed = (now - anim_start) as f32;
        let is_animating = anim && elapsed >= 0.0 && elapsed < 0.6;
        if is_animating {
            ui.ctx().request_repaint();
        }

        // Top state readout in dot font
        let top_txt = if muted {
            "OFF"
        } else if fx {
            "FX"
        } else {
            "ON"
        };
        painter.text(
            Pos2::new(slot_cx, rect.top() + 11.0),
            Align2::CENTER_CENTER,
            top_txt,
            theme::dot_font(10.0),
            if muted { ink.gamma_multiply(0.4) } else if fx { TraktorTheme::ACCENT() } else { ink },
        );

        // Bottom stem label
        painter.text(
            Pos2::new(slot_cx, rect.bottom() - 8.0),
            Align2::CENTER_CENTER,
            names[s],
            FontId::proportional(8.0),
            if muted { ink.gamma_multiply(0.4) } else { ink.gamma_multiply(0.75) },
        );

        let block_rect = Rect::from_min_max(
            Pos2::new(slot_left + 3.0, body_top + 2.0),
            Pos2::new(slot_left + slot_w - 3.0, body_bot - 2.0),
        );
        let live = if anim { stem_levels[s].clamp(0.0, 1.0) } else { 0.45 };
        // Active stems always show at least 35% height so on/off state is unmistakable when paused
        let active_frac = (0.35 + 0.65 * live) * open_t;
        let lit_rows = value_to_lit_rows(active_frac, rows);
        let wipe_rows = value_to_lit_rows(open_t, rows);

        paint_dot_grid(painter, block_rect, cols, rows, dot_radius, unlit, |c, r| {
            let is_border = c == 0 || c == cols - 1 || r == 0 || r == rows - 1;
            if r < lit_rows {
                let row_t = 0.6 + 0.4 * (1.0 - r as f32 / rows.max(1) as f32);
                DotCell::Filled(accent_vis.gamma_multiply(row_t))
            } else if muted && is_border && r >= wipe_rows {
                DotCell::Hollow(accent_vis.gamma_multiply(0.45))
            } else {
                DotCell::Unlit
            }
        });

        if anim {
            let step_x = if cols > 1 { (block_rect.width() - dot_radius * 2.0).max(1.0) / (cols - 1) as f32 } else { 0.0 };
            let step_y = if rows > 1 { (block_rect.height() - dot_radius * 2.0).max(1.0) / (rows - 1) as f32 } else { 0.0 };
            let top_dot_y = block_rect.top() + dot_radius;
            let sprite = STEM_SPRITES[s];
            // The icon rides the stem: one row from the top when on, resting on the floor when muted.
            let sprite_h = sprite.dimensions(0).1 as f32;
            let floor_row = rows as f32 - 1.0 - sprite_h;
            let icon_top = (floor_row + (1.0 - floor_row) * open_t).round();
            let icon_cy = top_dot_y + (icon_top + (sprite_h - 1.0) * 0.5) * step_y;

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

    if let Some(pct) = stem_progress {
        let bar_rect = Rect::from_min_max(Pos2::new(rect.left() + 6.0, rect.top() + 2.0), Pos2::new(rect.right() - 6.0, rect.top() + 5.0));
        painter.rect_filled(bar_rect, 1.5, unlit);
        let w = bar_rect.width() * pct.clamp(0.0, 1.0);
        painter.rect_filled(Rect::from_min_size(bar_rect.min, Vec2::new(w, bar_rect.height())), 1.5, TraktorTheme::ACCENT());
    }

    response
}

/// Paints the cheer overlay animation on the deck display.
/// Temporarily replaces the EQ/stem dots on that display and wipes in/out over 1.5 seconds.
pub fn cheer_display(
    ui: &mut Ui,
    cheer_index: usize,
    elapsed_sec: f32,
    accent: Color32,
    size: Vec2,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    let bg = TraktorTheme::WAVEFORM_BG();
    let accent_vis = visible_on_inset(accent);
    let unlit = unlit_on_inset();
    painter.rect_filled(rect, 6.0, bg);
    painter.rect_stroke(rect, 6.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let pad_top = 18.0;
    let pad_bot = 16.0;
    let body_top = rect.top() + pad_top;
    let body_bot = (rect.bottom() - pad_bot).max(body_top + 24.0);
    let body_h = body_bot - body_top;
    let body_w = (rect.width() - 16.0).max(20.0);
    let grid_rect = Rect::from_center_size(rect.center(), Vec2::new(body_w, body_h));

    let rows = ((body_h / 4.8).floor() as usize).clamp(16, 32);
    let cols = ((body_w / 4.6).floor() as usize).clamp(16, 36);
    let dot_radius = 1.3_f32;

    let sprite = CHEER_SPRITES[cheer_index % CHEER_SPRITES.len()];
    let frame_count = sprite.frames.len();
    let frame_idx = if frame_count > 1 {
        ((elapsed_sec * 4.0) as usize) % frame_count
    } else {
        0
    };

    let step_x = if cols > 1 { (grid_rect.width() - dot_radius * 2.0).max(1.0) / (cols - 1) as f32 } else { 0.0 };
    let step_y = if rows > 1 { (grid_rect.height() - dot_radius * 2.0).max(1.0) / (rows - 1) as f32 } else { 0.0 };

    let wipe_prog = if elapsed_sec < 0.25 {
        (elapsed_sec / 0.25).clamp(0.0, 1.0)
    } else if elapsed_sec > 1.25 {
        ((1.5 - elapsed_sec) / 0.25).clamp(0.0, 1.0)
    } else {
        1.0
    };

    // Base unlit grid
    paint_dot_grid(painter, grid_rect, cols, rows, dot_radius, unlit, |_c, _r| DotCell::Unlit);

    let bounce_dots = if elapsed_sec < 0.6 {
        bounce_dot_offset(elapsed_sec, 0.6, 3)
    } else {
        0
    };

    let (sw, _sh) = sprite.dimensions(frame_idx);
    let eff_step_x = if sw as f32 * step_x > body_w - 4.0 {
        (body_w - 4.0) / sw as f32
    } else {
        step_x
    };

    if wipe_prog > 0.05 {
        paint_sprite(
            painter,
            grid_rect.center(),
            Vec2::new(eff_step_x, step_y),
            dot_radius,
            sprite,
            frame_idx,
            bounce_dots,
            SpriteMode::Filled,
            accent_vis,
            unlit,
            false,
        );
    }

    let alpha_t = wipe_prog.clamp(0.0, 1.0);
    painter.text(
        Pos2::new(rect.center().x, rect.top() + 10.0),
        Align2::CENTER_CENTER,
        "★ NICE MIX ★",
        theme::dot_font(9.5),
        accent_vis.gamma_multiply(alpha_t),
    );

    ui.ctx().request_repaint();
    response.on_hover_text(format!("Cheers: {}", sprite.name))
}

/// Interactive 4-row dot-ribbon crossfader across the bottom of the mixer panel.
/// Left of the 2-dot-wide handle is Deck A colour (`level_a` height); right is Deck B colour (`level_b` height).
/// Dragging moves `value`; double-clicking centres (`0.5`).
pub fn dot_crossfader(
    ui: &mut Ui,
    value: &mut f32,
    level_a: f32,
    level_b: f32,
    width: f32,
) -> Response {
    let height = 42.0;
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(width.max(120.0), height), Sense::click_and_drag());

    let pad_side = 22.0;
    let ribbon_left = rect.left() + pad_side;
    let ribbon_right = (rect.right() - pad_side).max(ribbon_left + 40.0);
    let ribbon_w = ribbon_right - ribbon_left;

    if response.dragged() {
        let delta = response.drag_delta().x / ribbon_w.max(20.0);
        *value = (*value + delta).clamp(0.0, 1.0);
        response.mark_changed();
    } else if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if pos.x >= ribbon_left && pos.x <= ribbon_right {
                *value = ((pos.x - ribbon_left) / ribbon_w).clamp(0.0, 1.0);
                response.mark_changed();
            }
        }
    }
    if response.double_clicked() {
        *value = 0.5;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.text(
            Pos2::new(rect.center().x, rect.top() + 5.0),
            Align2::CENTER_CENTER,
            "CROSSFADER",
            FontId::proportional(8.0),
            TraktorTheme::TEXT_SECONDARY(),
        );

        let ribbon_top = rect.top() + 14.0;
        let ribbon_bot = rect.bottom() - 3.0;
        let ribbon_rect = Rect::from_min_max(Pos2::new(ribbon_left, ribbon_top), Pos2::new(ribbon_right, ribbon_bot));
        let ribbon_cy = ribbon_rect.center().y;

        painter.text(
            Pos2::new(rect.left() + 9.0, ribbon_cy),
            Align2::CENTER_CENTER,
            "A",
            theme::dot_font(14.0),
            TraktorTheme::DECK_A(),
        );
        painter.text(
            Pos2::new(rect.right() - 9.0, ribbon_cy),
            Align2::CENTER_CENTER,
            "B",
            theme::dot_font(14.0),
            TraktorTheme::DECK_B(),
        );

        let cols = ((ribbon_w / 7.5).floor() as usize).clamp(16, 60);
        let rows = 4usize;
        let handle_col = ((value.clamp(0.0, 1.0) * (cols.saturating_sub(2)) as f32).round() as usize)
            .min(cols.saturating_sub(2));
        let anim = theme::animations();
        let lv_a = if anim { level_a.clamp(0.0, 1.0) } else { 0.25 };
        let lv_b = if anim { level_b.clamp(0.0, 1.0) } else { 0.25 };
        let col_a = TraktorTheme::DECK_A();
        let col_b = TraktorTheme::DECK_B();
        let handle_fg = TraktorTheme::TEXT_PRIMARY();
        let unlit = unlit_on_panel();

        paint_dot_grid(painter, ribbon_rect, cols, rows, 2.2, unlit, |c, r| {
            if c == handle_col || c == handle_col + 1 {
                DotCell::Filled(handle_fg)
            } else {
                let is_left = c < handle_col;
                let lv = if is_left { lv_a } else { lv_b };
                let on = r == 0 || (r as f32 / rows as f32) < lv;
                if on {
                    let base = if is_left { col_a } else { col_b };
                    let fade = 0.45 + 0.55 * (1.0 - r as f32 / (rows as f32 + 1.0));
                    DotCell::Filled(base.gamma_multiply(fade))
                } else {
                    DotCell::Unlit
                }
            }
        });
    }

    response
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

    let cols = tracks.len().max(8);
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
        if c >= tracks.len() {
            return DotCell::Unlit;
        }
        let t = &tracks[c];
        let is_selected = selected_idx == Some(c);
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
            grid_rect.left() + dot_radius + c as f32 * ((grid_rect.width() - dot_radius * 2.0) / (cols - 1) as f32)
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
                let col = (((pos.x - (grid_rect.left() + dot_radius)) / step_x).round() as usize).min(tracks.len().saturating_sub(1));
                clicked_col = Some(col);
            }
        }
    }

    (response, clicked_col)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
