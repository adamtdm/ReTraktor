use crate::audio::PEAKS_PER_SEC;
use crate::theme::TraktorTheme;
use retraktor_formats::StripeColumn;
use eframe::egui::{self, Color32, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};
use std::f32::consts::PI;

/// Custom Rotary Knob Widget (270° sweep, center detent, dB/percentage labels, mouse drag)
pub fn rotary_knob(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    label: &str,
    accent: Color32,
) -> Response {
    rotary_knob_sized(ui, value, min, max, default, label, accent, 36.0)
}

/// Compact Rotary Knob Widget with authentic Traktor Pro 4 matte dark grey styling
pub fn rotary_knob_sized(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    default: f32,
    label: &str,
    accent: Color32,
    box_size: f32,
) -> Response {
    let size = Vec2::new(box_size, box_size + 5.0);
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
        let center = rect.center() + Vec2::new(0.0, -4.5);
        let radius = (box_size * 0.35).max(9.5);

        // Outer ring slot
        painter.circle_stroke(
            center,
            radius + 1.2,
            Stroke::new(1.2_f32, TraktorTheme::BORDER_COLOR()),
        );

        // Matte charcoal knob cap (matching Traktor Pro 4 knobs)
        painter.circle_filled(center, radius, TraktorTheme::CONTROL());
        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT()),
        );

        // Angular calculations (-135° to +135°)
        let norm = (*value - min) / (max - min).max(1e-5);
        let default_norm = (default - min) / (max - min).max(1e-5);
        let angle_start = -135.0_f32.to_radians() - PI / 2.0;
        let angle_end = 135.0_f32.to_radians() - PI / 2.0;
        let current_angle = angle_start + norm * (angle_end - angle_start);
        let origin_angle = if (default_norm - 0.5).abs() < 0.05 {
            -PI / 2.0 // bipolar knob sweeps from 12 o'clock
        } else {
            angle_start // unipolar knob sweeps from min
        };

        // Subtle value arc along outer perimeter
        let sweep = current_angle - origin_angle;
        if sweep.abs() > 0.04 {
            let points = 16;
            let track_radius = radius + 0.5;
            let mut arc_pts = Vec::with_capacity(points + 1);
            for i in 0..=points {
                let a = origin_angle + (i as f32 / points as f32) * sweep;
                arc_pts.push(center + Vec2::new(a.cos(), a.sin()) * track_radius);
            }
            for w in arc_pts.windows(2) {
                painter.line_segment([w[0], w[1]], Stroke::new(1.8_f32, accent));
            }
        }

        // Pointer indicator line (clean light grey/silver)
        let ptr_inner = center + Vec2::new(current_angle.cos(), current_angle.sin()) * 2.5;
        let ptr_outer =
            center + Vec2::new(current_angle.cos(), current_angle.sin()) * (radius - 1.2);
        painter.line_segment(
            [ptr_inner, ptr_outer],
            Stroke::new(1.5_f32, TraktorTheme::TEXT_PRIMARY()),
        );

        // Knob label underneath
        let label_pos = Pos2::new(rect.center().x, rect.bottom() - 4.0);
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(8.5),
            TraktorTheme::TEXT_SECONDARY(),
        );
    }

    response
}

/// Custom Vertical Volume Fader Widget with smooth track, matte cap, and tick markers
pub fn vertical_fader(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    _accent: Color32,
    height: f32,
) -> Response {
    let size = Vec2::new(24.0, height);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if response.dragged() {
        let delta = -response.drag_delta().y / (rect.height() - 16.0).max(20.0) * (max - min);
        *value = (*value + delta).clamp(min, max);
        response.mark_changed();
    }

    if response.double_clicked() {
        *value = 0.85;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let track_x = rect.center().x;
        let track_top = rect.top() + 6.0;
        let track_bot = rect.bottom() - 6.0;
        let track_h = (track_bot - track_top).max(10.0);

        // dB Tick marks on left and right (like Traktor Pro 4 pitch/volume faders)
        for i in 0..=10 {
            let norm = i as f32 / 10.0;
            let y = track_bot - norm * track_h;
            let tick_w = if i == 5 || i == 0 || i == 10 {
                6.5
            } else {
                4.0
            };
            painter.line_segment(
                [Pos2::new(track_x - tick_w - 2.0, y), Pos2::new(track_x - 2.5, y)],
                Stroke::new(0.8_f32, TraktorTheme::BORDER_ACCENT()),
            );
            painter.line_segment(
                [Pos2::new(track_x + 2.5, y), Pos2::new(track_x + tick_w + 2.0, y)],
                Stroke::new(0.8_f32, TraktorTheme::BORDER_ACCENT()),
            );
        }

        // Fader rail / slot
        let slot_rect = Rect::from_min_max(
            Pos2::new(track_x - 1.5, track_top),
            Pos2::new(track_x + 1.5, track_bot),
        );
        painter.rect_filled(slot_rect, 0.5, TraktorTheme::BG_INPUT());

        let norm_val = (*value - min) / (max - min).max(1e-5);
        let cap_y = track_bot - norm_val * track_h;

        // Traktor-style flat matte fader cap with crisp light grey center line
        let cap_w = 18.0;
        let cap_h = 9.0;
        let cap_rect = Rect::from_center_size(Pos2::new(track_x, cap_y), Vec2::new(cap_w, cap_h));

        painter.rect_filled(cap_rect, 1.5, TraktorTheme::CONTROL_HOVER());
        painter.rect_stroke(
            cap_rect,
            1.5,
            Stroke::new(0.8_f32, TraktorTheme::BORDER_COLOR()),
        );
        painter.line_segment(
            [
                Pos2::new(cap_rect.left() + 2.0, cap_y),
                Pos2::new(cap_rect.right() - 2.0, cap_y),
            ],
            Stroke::new(1.5_f32, TraktorTheme::TEXT_PRIMARY()),
        );
    }

    response
}

/// Custom Horizontal Crossfader Widget
pub fn crossfader(ui: &mut Ui, value: &mut f32, width: f32) -> Response {
    let size = Vec2::new(width, 20.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if response.dragged() {
        let delta = response.drag_delta().x / (rect.width() - 20.0).max(20.0);
        *value = (*value + delta).clamp(0.0, 1.0);
        response.mark_changed();
    }

    if response.double_clicked() {
        *value = 0.5;
        response.mark_changed();
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let slot_left = rect.left() + 8.0;
        let slot_right = rect.right() - 8.0;
        let slot_w = (slot_right - slot_left).max(10.0);
        let slot_y = rect.center().y;

        // Horizontal rail
        let slot_rect = Rect::from_min_max(
            Pos2::new(slot_left, slot_y - 1.5),
            Pos2::new(slot_right, slot_y + 1.5),
        );
        painter.rect_filled(slot_rect, 1.0, TraktorTheme::BG_INPUT());

        // Center marker
        painter.line_segment(
            [
                Pos2::new(rect.center().x, slot_y - 5.0),
                Pos2::new(rect.center().x, slot_y + 5.0),
            ],
            Stroke::new(1.0_f32, TraktorTheme::TEXT_MUTED()),
        );

        // Crossfader Cap
        let cap_x = slot_left + *value * slot_w;
        let cap_rect = Rect::from_center_size(Pos2::new(cap_x, slot_y), Vec2::new(12.0, 15.0));

        painter.rect_filled(cap_rect, 1.5, TraktorTheme::CONTROL_HOVER());
        painter.rect_stroke(
            cap_rect,
            1.5,
            Stroke::new(0.8_f32, TraktorTheme::BORDER_COLOR()),
        );
        painter.line_segment(
            [
                Pos2::new(cap_x, cap_rect.top() + 2.0),
                Pos2::new(cap_x, cap_rect.bottom() - 2.0),
            ],
            Stroke::new(1.5_f32, TraktorTheme::TEXT_PRIMARY()),
        );
    }

    response
}

/// Stereo Segmented LED VU Meter Tower Widget (Toned-down Traktor Pro 4 style)
pub fn vu_meter(ui: &mut Ui, level_l: f32, level_r: f32, height: f32) {
    let width = 10.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, TraktorTheme::METER_BG());

    let segments = 16;
    let seg_w = (width - 3.0) / 2.0;
    let seg_h = ((height - 4.0) / segments as f32 - 1.0).max(1.2);

    let draw_channel = |ch_x: f32, level: f32| {
        let lit_count = ((level.clamp(0.0, 1.0) * segments as f32).round() as usize).min(segments);
        for seg_idx in 0..segments {
            let y = rect.bottom() - 2.0 - (seg_idx as f32 + 1.0) * (seg_h + 1.0);
            let seg_rect = Rect::from_min_size(Pos2::new(ch_x, y), Vec2::new(seg_w, seg_h));

            let color = if seg_idx < lit_count {
                if seg_idx >= segments - 2 {
                    TraktorTheme::METER_RED()
                } else if seg_idx >= segments - 4 {
                    TraktorTheme::METER_YELLOW()
                } else {
                    TraktorTheme::METER_GREEN()
                }
            } else {
                TraktorTheme::METER_GREEN().gamma_multiply(0.15)
            };
            painter.rect_filled(seg_rect, 0.0, color);
        }
    };

    draw_channel(rect.left() + 1.0, level_l);
    draw_channel(rect.left() + 1.0 + seg_w + 1.0, level_r);
}

/// Maps 3-band `[low, mid, high]` amplitudes into the waveform colours `bands` ([`crate::theme::waveform_bands`]):
/// - Outer silhouette: highs blended with mids
/// - Inner body: mids blended with bass
/// - Core transients: bass
fn spectrum_colors([low_c, mid_c, high_c]: [Color32; 3], low: f32, mid: f32, high: f32) -> (Color32, Color32, Color32) {
    let mix = |a: Color32, b: Color32, t: f32| -> Color32 {
        Color32::from(egui::Rgba::from(a) * (1.0 - t) + egui::Rgba::from(b) * t)
    };

    let h_ratio = (high / (high + mid + 0.05)).clamp(0.0, 1.0);
    let outer_col = mix(mid_c, high_c, 0.5 + 0.5 * h_ratio).gamma_multiply(195.0 / 255.0);

    let m_ratio = (mid / (low + mid + 0.05)).clamp(0.0, 1.0);
    let mid_col = mix(low_c, mid_c, 0.5 + 0.5 * m_ratio).gamma_multiply(220.0 / 255.0);

    let l_boost = low.clamp(0.0, 1.0);
    let core_col = mix(mid_c, low_c, 0.65 + 0.35 * l_boost).gamma_multiply(240.0 / 255.0);

    (outer_col, mid_col, core_col)
}

/// Computes the quantized dot-matrix grid layout (`(cols, half_rows, pitch_x, pitch_y, radius)`)
/// for `rect`, keeping the total circle count well under a few thousand shapes per frame.
pub(crate) fn dot_grid_metrics(width: f32, height: f32, max_cols: usize, max_half_rows: usize) -> (usize, usize, f32, f32, f32) {
    let pitch_x = (width / max_cols.max(1) as f32).max(5.5);
    let cols = ((width / pitch_x).floor() as usize).clamp(1, max_cols.max(1));
    let step_x = width / cols as f32;
    let half_h = (height * 0.45).max(3.0);
    let pitch_y = (half_h / max_half_rows.max(1) as f32).max(4.5);
    let half_rows = ((half_h / pitch_y).floor() as usize).clamp(1, max_half_rows.max(1));
    let step_y = half_h / half_rows as f32;
    let radius = (step_x.min(step_y) * 0.34).clamp(1.1, 2.5);
    (cols, half_rows, step_x, step_y, radius)
}

/// The waveform analysis floors digital silence at or under this (0.04 for separated stems), so a
/// dot lane this low stays dark while real low-level content lights a row.
const WAVEFORM_SILENCE: f32 = 0.04;
/// Deck waveforms' vertical zoom above the silence floor, through typical levels.
const WAVEFORM_ZOOM: f32 = 2.2;
/// Slope of the headroom ramp that takes over from the zoom near the top and reaches full height at 1.0.
const WAVEFORM_HEADROOM_SLOPE: f32 = 0.48;

/// Deck waveforms' fixed vertical zoom, the same for every deck and track, applied to heights only.
/// Silence is unchanged; above it amplitudes stretch by `WAVEFORM_ZOOM` up to a knee (about 0.33 in,
/// 0.68 out), then follow a gentler ramp to 1.0, so hot peaks stay in order instead of flattening.
fn zoom_waveform(amp: f32) -> f32 {
    if amp <= WAVEFORM_SILENCE {
        return amp;
    }
    let zoomed = WAVEFORM_SILENCE + (amp - WAVEFORM_SILENCE) * WAVEFORM_ZOOM;
    zoomed.min(1.0 - (1.0 - amp) * WAVEFORM_HEADROOM_SLOPE)
}

fn zoom_bands((low, mid, high): (f32, f32, f32)) -> (f32, f32, f32) {
    (zoom_waveform(low), zoom_waveform(mid), zoom_waveform(high))
}

/// Quantises a `[0, 1]` amplitude into how many dot pairs (`0..=half_rows`) should light up.
pub(crate) fn quantize_dot_rows(amp: f32, half_rows: usize) -> usize {
    if half_rows == 0 || amp <= WAVEFORM_SILENCE {
        return 0;
    }
    ((amp.clamp(0.0, 1.0) * half_rows as f32).ceil() as usize).clamp(1, half_rows)
}

/// Samples interpolated `[low, mid, high]` peak amplitudes at `time_sec` from `peaks`.
fn sample_scrolling_3band(peaks: &[[f32; 3]], time_sec: f64) -> Option<(f32, f32, f32)> {
    if peaks.is_empty() || time_sec < 0.0 {
        return None;
    }
    let exact_idx = time_sec * PEAKS_PER_SEC as f64;
    let idx0 = (exact_idx.floor() as usize) % peaks.len();
    let idx1 = (idx0 + 1) % peaks.len();
    let frac = exact_idx.fract() as f32;
    let p0 = peaks[idx0];
    let p1 = peaks[idx1];
    Some((
        p0[0] + (p1[0] - p0[0]) * frac,
        p0[1] + (p1[1] - p0[1]) * frac,
        p0[2] + (p1[2] - p0[2]) * frac,
    ))
}

/// Draws a 3-band waveform inside `rect` in the given `style`, in the waveform colours `bands`.
/// `sample_at_x` returns `Some((played, tone))` for a given `x` coordinate in `rect`: `(low, mid, high)`
/// at the level the deck plays, for heights, and as analysed, for colours and the bass-core cues.
#[allow(clippy::too_many_arguments)]
fn paint_3band_waveform(
    painter: &egui::Painter,
    rect: Rect,
    playhead_x: f32,
    style: crate::theme::WaveformStyle,
    bands: [Color32; 3],
    max_dot_cols: usize,
    max_dot_half_rows: usize,
    scroll_px: f64,
    sample_at_x: impl Fn(f32) -> Option<((f32, f32, f32), (f32, f32, f32))>,
) {
    use crate::theme::WaveformStyle;
    // Columns sit on fixed track positions and glide with the audio; screen-fixed columns shimmer.
    let shift = |pitch: f32| scroll_px.rem_euclid(pitch as f64) as f32;
    let painter = &painter.with_clip_rect(rect);
    let center_y = rect.center().y;
    let max_half_h = rect.height() * 0.45;

    match style {
        WaveformStyle::Classic => {
            // One plain mesh: abutting columns need no edge smoothing, and it is half the points of stroked lines.
            let col_w = if crate::preferences::low_power() { 3.0_f32 } else { 1.5 };
            let cols = (rect.width() / col_w).ceil() as usize;
            let mut mesh = egui::Mesh::default();
            mesh.reserve_triangles((cols + 2) * 6);
            mesh.reserve_vertices((cols + 2) * 12);
            let mut bar = |x: f32, h: f32, c: Color32| {
                let r = Rect::from_min_max(Pos2::new(x - col_w * 0.5, center_y - h), Pos2::new(x + col_w * 0.5, center_y + h));
                mesh.add_colored_rect(r, c);
            };
            for i in 0..cols + 2 {
                let col_x = rect.left() + (i as f32) * col_w - shift(col_w);
                let Some(((low_amp, mid_amp, high_amp), tone)) = sample_at_x(col_x) else { continue };
                let (outer_col, mid_col, core_col) = spectrum_colors(bands, tone.0, tone.1, tone.2);
                let total_env = low_amp.max(mid_amp * 0.92).max(high_amp * 0.85);
                bar(col_x, (total_env * max_half_h).max(1.0), outer_col);
                bar(col_x, (mid_amp.max(low_amp * 0.75) * max_half_h * 0.72).max(0.8), mid_col);
                bar(col_x, (low_amp * max_half_h * 0.46).max(0.5), core_col);
            }
            painter.add(egui::Shape::mesh(mesh));
        }
        WaveformStyle::Dots => {
            let (cols, half_rows, step_x, step_y, radius) =
                dot_grid_metrics(rect.width(), rect.height(), max_dot_cols, max_dot_half_rows);
            let unlit = crate::theme::mix(TraktorTheme::WAVEFORM_BG(), TraktorTheme::WAVEFORM_GRID(), 0.22);
            let glow_w = (step_x * 4.5).max(18.0);
            for c in 0..cols + 2 {
                let col_x = rect.left() + (c as f32 + 0.5) * step_x - shift(step_x);
                let near = (1.0 - ((col_x - playhead_x).abs() / glow_w)).clamp(0.0, 1.0);
                let sampled = sample_at_x(col_x);
                let (lit_outer, lit_mid, lit_core, outer_col, mid_col, core_col) =
                    if let Some(((low, mid, high), tone)) = sampled {
                        let (oc, mc, cc) = spectrum_colors(bands, tone.0, tone.1, tone.2);
                        let env = low.max(mid * 0.92).max(high * 0.85);
                        let lo = quantize_dot_rows(env, half_rows);
                        let lm = quantize_dot_rows(mid.max(low * 0.75) * 0.75, half_rows).min(lo);
                        let lc = quantize_dot_rows(low * 0.52, half_rows).min(lm);
                        (lo, lm, lc, oc, mc, cc)
                    } else {
                        (0, 0, 0, unlit, unlit, unlit)
                    };
                for r in 0..half_rows {
                    let dy = (r as f32 + 0.5) * step_y;
                    let col = if r < lit_core {
                        core_col
                    } else if r < lit_mid {
                        mid_col
                    } else if r < lit_outer {
                        outer_col
                    } else {
                        unlit
                    };
                    let col = if r < lit_outer && near > 0.0 {
                        crate::theme::mix(col, TraktorTheme::TEXT_HIGHLIGHT(), near * 0.42)
                    } else if r >= lit_outer && near > 0.0 {
                        crate::theme::mix(col, TraktorTheme::WAVEFORM_GRID(), near * 0.25)
                    } else {
                        col
                    };
                    let rad = if r < lit_outer && near > 0.0 { radius * (1.0 + 0.12 * near) } else { radius };
                    painter.circle_filled(Pos2::new(col_x, center_y - dy), rad, col);
                    painter.circle_filled(Pos2::new(col_x, center_y + dy), rad, col);
                }
            }
        }
        WaveformStyle::Bars => {
            let pitch = if rect.height() <= 26.0 { 4.0_f32 } else { 5.0_f32 };
            let bar_w = if rect.height() <= 26.0 { 2.4_f32 } else { 3.2_f32 };
            let rounding = bar_w * 0.5;
            let cols = (rect.width() / pitch).floor() as usize;
            let glow_w = pitch * 4.0;
            for c in 0..cols + 2 {
                let col_x = rect.left() + (c as f32 + 0.5) * pitch - shift(pitch);
                let Some(((low, mid, high), tone)) = sample_at_x(col_x) else { continue };
                let (outer_col, mid_col, core_col) = spectrum_colors(bands, tone.0, tone.1, tone.2);
                let near = (1.0 - ((col_x - playhead_x).abs() / glow_w)).clamp(0.0, 1.0);
                let boost = |col: Color32| {
                    if near > 0.0 {
                        crate::theme::mix(col, TraktorTheme::TEXT_HIGHLIGHT(), near * 0.35)
                    } else {
                        col
                    }
                };
                let env = low.max(mid * 0.92).max(high * 0.85);
                let h_outer = (env * max_half_h).max(1.5);
                painter.rect_filled(
                    Rect::from_center_size(Pos2::new(col_x, center_y), Vec2::new(bar_w, h_outer * 2.0)),
                    rounding,
                    boost(outer_col),
                );
                let h_mid = (mid.max(low * 0.75) * max_half_h * 0.68).max(1.0).min(h_outer);
                painter.rect_filled(
                    Rect::from_center_size(Pos2::new(col_x, center_y), Vec2::new(bar_w, h_mid * 2.0)),
                    rounding,
                    boost(mid_col),
                );
                if tone.0 > 0.12 {
                    let h_core = (low * max_half_h * 0.42).max(0.8).min(h_mid);
                    painter.rect_filled(
                        Rect::from_center_size(Pos2::new(col_x, center_y), Vec2::new(bar_w, h_core * 2.0)),
                        rounding,
                        boost(core_col),
                    );
                }
            }
        }
        WaveformStyle::Line => {
            let step = 2.0_f32;
            let cols = (rect.width() / step).ceil() as usize;
            let mut prev: Option<(Pos2, Pos2, Color32)> = None;
            for c in 0..=cols + 1 {
                let col_x = (rect.left() + (c as f32) * step - shift(step)).min(rect.right());
                if let Some(((low, mid, high), tone)) = sample_at_x(col_x) {
                    let (outer_col, mid_col, core_col) = spectrum_colors(bands, tone.0, tone.1, tone.2);
                    let env = low.max(mid * 0.92).max(high * 0.85);
                    let h = (env * max_half_h).max(0.8);
                    let top = Pos2::new(col_x, center_y - h);
                    let bot = Pos2::new(col_x, center_y + h);
                    let stroke_col = if tone.0 > 0.55 {
                        core_col
                    } else if tone.1 >= tone.2 {
                        mid_col
                    } else {
                        outer_col
                    };
                    if let Some((p_top, p_bot, p_col)) = prev {
                        let seg_col = crate::theme::mix(p_col, stroke_col, 0.5);
                        painter.line_segment([p_top, top], Stroke::new(1.5_f32, seg_col));
                        painter.line_segment([p_bot, bot], Stroke::new(1.5_f32, seg_col));
                    }
                    prev = Some((top, bot, stroke_col));
                } else {
                    prev = None;
                }
            }
        }
    }
}

/// Paints a small live preview of `style` inside `rect` for the Preferences > Look style cards.
pub fn paint_waveform_style_preview(
    painter: &egui::Painter,
    rect: Rect,
    style: crate::theme::WaveformStyle,
) {
    painter.rect_filled(rect, 4.0, TraktorTheme::WAVEFORM_BG());
    painter.rect_stroke(rect, 4.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));
    let cy = rect.center().y;
    let cx = rect.center().x;
    painter.line_segment(
        [Pos2::new(rect.left(), cy), Pos2::new(rect.right(), cy)],
        Stroke::new(0.5_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.35)),
    );
    let sample_at_x = |x: f32| -> Option<((f32, f32, f32), (f32, f32, f32))> {
        let t = ((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0);
        // Synthetic 4-beat phrase with kick transients, mid chord body, and hi-hat offbeats
        let beat_phase = (t * 4.0).fract();
        let kick = (1.0 - beat_phase * 3.2).max(0.0).powi(2);
        let offbeat = (1.0 - ((beat_phase - 0.5).abs() * 4.5)).max(0.0);
        let phrase = 0.35 + 0.65 * (t * PI).sin();
        let low = (kick * 0.92 + 0.12 * phrase).clamp(0.08, 0.98);
        let mid = ((0.35 + 0.45 * (t * PI * 3.0).sin().abs()) * phrase + kick * 0.25).clamp(0.12, 0.92);
        let high = (offbeat * 0.78 + 0.22 * (t * PI * 7.0).cos().abs()).clamp(0.08, 0.90);
        Some(((low, mid, high), (low, mid, high)))
    };
    paint_3band_waveform(painter, rect, cx, style, crate::theme::waveform_bands(), 22, 4, 0.0, sample_at_x);
    for b in 1..4 {
        let bx = rect.left() + (b as f32 / 4.0) * rect.width();
        painter.line_segment(
            [Pos2::new(bx, rect.top()), Pos2::new(bx, rect.bottom())],
            Stroke::new(0.6_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.35)),
        );
    }
    painter.line_segment(
        [Pos2::new(cx, rect.top()), Pos2::new(cx, rect.bottom())],
        Stroke::new(1.3_f32, TraktorTheme::WAVEFORM_PLAYHEAD()),
    );
}

/// Real-Time 60 FPS Scrolling Traktor Pro 4 "Spectrum" Waveform Display
#[allow(clippy::too_many_arguments)]
pub fn scrolling_waveform(
    ui: &mut Ui,
    elapsed_sec: f64,
    tempo: f64,
    bpm: f64,
    grid_offset_sec: f64,
    cues: &[(u8, f64)],
    phrases: &[retraktor_library::Phrase],
    waveform_peaks: &[[f32; 3]],
    played_peaks: &[[f32; 3]],
    loop_active: bool,
    loop_start_sec: f64,
    loop_len_sec: f64,
    deck_badge: Option<(&str, Color32)>,
    height: f32,
) -> Response {
    let width = ui.available_width().max(180.0);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, TraktorTheme::WAVEFORM_BG());
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let center_y = rect.center().y;
    let center_x = rect.center().x;
    // Drawn in heard time, so synced decks' beat lines scroll at the same speed and spacing.
    let pixels_per_sec = 92.0 / tempo.clamp(0.25, 4.0) as f32;

    // Subtle horizontal zero-crossing baseline
    painter.line_segment(
        [Pos2::new(rect.left(), center_y), Pos2::new(rect.right(), center_y)],
        Stroke::new(0.5_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.35)),
    );

    // 1. Active Loop Overlay
    if loop_active && loop_len_sec > 0.01 {
        let l_start_x = (center_x + ((loop_start_sec - elapsed_sec) as f32) * pixels_per_sec)
            .clamp(rect.left(), rect.right());
        let l_end_x = (center_x
            + ((loop_start_sec + loop_len_sec - elapsed_sec) as f32) * pixels_per_sec)
            .clamp(rect.left(), rect.right());
        if l_end_x > l_start_x {
            let loop_rect =
                Rect::from_min_max(Pos2::new(l_start_x, rect.top()), Pos2::new(l_end_x, rect.bottom()));
            painter.rect_filled(
                loop_rect,
                0.0,
                TraktorTheme::LOOP_ACTIVE().gamma_multiply(0.11),
            );
            painter.rect_stroke(loop_rect, 0.0, Stroke::new(1.0_f32, TraktorTheme::LOOP_ACTIVE()));
        }
    }

    // 2. Waveform body in active style
    let beat_len_sec = 60.0 / bpm.max(30.0);
    let style = crate::theme::waveform_style();
    let scroll_px = (elapsed_sec + ((rect.left() - center_x) / pixels_per_sec) as f64) * pixels_per_sec as f64;
    paint_3band_waveform(painter, rect, center_x, style, crate::theme::waveform_bands(), 120, 7, scroll_px, |col_x| {
        let time_at_col = elapsed_sec + ((col_x - center_x) / pixels_per_sec) as f64;
        Some((zoom_bands(sample_scrolling_3band(played_peaks, time_at_col)?), sample_scrolling_3band(waveform_peaks, time_at_col)?))
    });

    // 3. Subtle Beatgrid Lines (drawn cleanly over the waveform like Traktor Pro 4)
    let visible_sec_half = (width / 2.0 / pixels_per_sec) as f64;
    let min_sec = (elapsed_sec - visible_sec_half).max(0.0);
    let max_sec = elapsed_sec + visible_sec_half;
    let start_beat = ((min_sec - grid_offset_sec) / beat_len_sec).floor() as i64;
    let end_beat = ((max_sec - grid_offset_sec) / beat_len_sec).ceil() as i64;

    for b in start_beat..=end_beat {
        let beat_time = grid_offset_sec + b as f64 * beat_len_sec;
        let x = center_x + ((beat_time - elapsed_sec) as f32) * pixels_per_sec;
        if x < rect.left() || x > rect.right() {
            continue;
        }

        let is_bar_start = b % 4 == 0;
        let stroke = if is_bar_start {
            Stroke::new(1.0_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.5))
        } else {
            Stroke::new(0.6_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.2))
        };
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], stroke);
        // Bar numbers along the bottom edge; every 4th bar when bars are too close to label.
        let bar_px = 4.0 * beat_len_sec as f32 * pixels_per_sec;
        let every = if bar_px < 30.0 { 16 } else { 4 };
        if b >= 0 && b % every == 0 && rect.height() > 30.0 {
            painter.text(Pos2::new(x + 2.0, rect.bottom() - 1.0), egui::Align2::LEFT_BOTTOM, format!("{}", b / 4 + 1), egui::FontId::proportional(9.0), TraktorTheme::TEXT_SECONDARY());
        }
    }
    if rect.height() > 30.0 {
        painter.text(rect.right_bottom() - Vec2::new(3.0, 1.0), egui::Align2::RIGHT_BOTTOM, format!("{:.0} s", 2.0 * visible_sec_half), egui::FontId::proportional(9.0), TraktorTheme::TEXT_MUTED());
    }

    // 4. Hotcue Markers
    for &(cue_id, cue_sec) in cues {
        let cue_x = center_x + ((cue_sec - elapsed_sec) as f32) * pixels_per_sec;
        if cue_x >= rect.left() && cue_x <= rect.right() {
            let color = TraktorTheme::hotcue_color(cue_id as usize);
            painter.line_segment(
                [Pos2::new(cue_x, rect.top()), Pos2::new(cue_x, rect.bottom())],
                Stroke::new(1.2_f32, color),
            );
        }
    }

    // 4a. Phrase Boundaries (faint vertical line with 1-2 letter tag in dot font)
    for phrase in phrases {
        let px = center_x + ((phrase.start_sec - elapsed_sec) as f32) * pixels_per_sec;
        if px >= rect.left() && px <= rect.right() {
            painter.line_segment(
                [Pos2::new(px, rect.top()), Pos2::new(px, rect.bottom())],
                Stroke::new(0.8_f32, TraktorTheme::TEXT_MUTED().gamma_multiply(0.35)),
            );
            painter.text(
                Pos2::new(px + 3.0, rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                phrase.kind.tag(),
                crate::theme::dot_font(8.5),
                TraktorTheme::TEXT_MUTED().gamma_multiply(0.75),
            );
        }
    }

    // 5. Center Playhead Line (Traktor Red Vertical Hairline)
    painter.line_segment(
        [
            Pos2::new(center_x, rect.top()),
            Pos2::new(center_x, rect.bottom()),
        ],
        Stroke::new(1.5_f32, TraktorTheme::WAVEFORM_PLAYHEAD()),
    );

    // Optional Subtle Deck Letter on Left Edge (used in Parallel layout)
    if let Some((badge_txt, _badge_col)) = deck_badge {
        let badge_rect =
            Rect::from_min_size(Pos2::new(rect.left() + 5.0, rect.top() + 5.0), Vec2::new(18.0, 16.0));
        painter.rect_filled(badge_rect, 2.0, TraktorTheme::BG_PANEL().gamma_multiply(0.85));
        painter.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            badge_txt,
            egui::FontId::proportional(11.0),
            TraktorTheme::TEXT_PRIMARY(),
        );
    }

    response
}

/// Real-Time 4-Lane Stacked Stem Waveforms Display matching Traktor Pro 4 Deck B screenshot:
/// Integrated left header (`DRUMS`, `BASS`, `OTHER`, `VOCALS`) with mute & knob indicators + 4 stem lanes.
#[allow(clippy::too_many_arguments)]
pub fn scrolling_stem_waveforms(
    ui: &mut Ui,
    elapsed_sec: f64,
    tempo: f64,
    bpm: f64,
    grid_offset_sec: f64,
    cues: &[(u8, f64)],
    phrases: &[retraktor_library::Phrase],
    stem_peaks: &[[f32; 4]],
    stem_volumes: &[f32; 4],
    stem_muted: &mut [bool; 4],
    loop_active: bool,
    loop_start_sec: f64,
    loop_len_sec: f64,
    _deck_badge: Option<(&str, Color32)>,
    height: f32,
) -> Response {
    let width = ui.available_width().max(220.0);
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let header_w = 106.0_f32.min(width * 0.28);
    let wave_rect = Rect::from_min_max(Pos2::new(rect.left() + header_w, rect.top()), rect.max);
    let header_rect = Rect::from_min_max(rect.min, Pos2::new(rect.left() + header_w, rect.bottom()));
    let lane_h = height / 4.0;

    // Allow clicking on the left stem header column to toggle individual stem mute
    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if header_rect.contains(pos) {
                let lane = (((pos.y - rect.top()) / lane_h).floor() as usize).min(3);
                stem_muted[lane] = !stem_muted[lane];
                response.mark_changed();
            }
        }
    }

    let painter = ui.painter();
    painter.rect_filled(wave_rect, 1.0, TraktorTheme::WAVEFORM_BG());
    painter.rect_filled(header_rect, 1.0, TraktorTheme::BG_SECTION());
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let center_x = wave_rect.center().x;
    // Drawn in heard time, so synced decks' beat lines scroll at the same speed and spacing.
    let pixels_per_sec = 92.0 / tempo.clamp(0.25, 4.0) as f32;

    // Stem lane colours derived from theme tokens
    let stem_core_colors = [
        TraktorTheme::WAVEFORM_LOW(),
        crate::theme::mix(TraktorTheme::WAVEFORM_LOW(), TraktorTheme::WAVEFORM_MID(), 0.45),
        TraktorTheme::WAVEFORM_HIGH(),
        TraktorTheme::LOOP_ACTIVE(),
    ];
    let stem_outer_colors = [
        crate::theme::mix(TraktorTheme::WAVEFORM_LOW(), TraktorTheme::WAVEFORM_HIGH(), 0.5).gamma_multiply(0.72),
        TraktorTheme::WAVEFORM_LOW().gamma_multiply(0.72),
        TraktorTheme::WAVEFORM_HIGH().gamma_multiply(0.72),
        crate::theme::mix(TraktorTheme::LOOP_ACTIVE(), TraktorTheme::WAVEFORM_HIGH(), 0.4).gamma_multiply(0.72),
    ];
    let stem_labels = ["DRUMS", "BASS", "OTHER", "VOCALS"];

    // 1. Active Loop Overlay in Waveform Area
    if loop_active && loop_len_sec > 0.01 {
        let l_start_x = (center_x + ((loop_start_sec - elapsed_sec) as f32) * pixels_per_sec)
            .clamp(wave_rect.left(), wave_rect.right());
        let l_end_x = (center_x
            + ((loop_start_sec + loop_len_sec - elapsed_sec) as f32) * pixels_per_sec)
            .clamp(wave_rect.left(), wave_rect.right());
        if l_end_x > l_start_x {
            let loop_rect = Rect::from_min_max(
                Pos2::new(l_start_x, wave_rect.top()),
                Pos2::new(l_end_x, wave_rect.bottom()),
            );
            painter.rect_filled(
                loop_rect,
                0.0,
                TraktorTheme::LOOP_ACTIVE().gamma_multiply(0.1),
            );
            painter.rect_stroke(loop_rect, 0.0, Stroke::new(1.0_f32, TraktorTheme::LOOP_ACTIVE()));
        }
    }

    // 2. Draw 4 Stem Waveform Ribbons in active style
    let max_lane_half = (lane_h * 0.44).max(2.0);
    let sample_stems_at_x = |col_x: f32| -> Option<[f32; 4]> {
        let time_at_col = elapsed_sec + ((col_x - center_x) / pixels_per_sec) as f64;
        if time_at_col < 0.0 || stem_peaks.is_empty() {
            return None;
        }
        let exact_idx = time_at_col * PEAKS_PER_SEC as f64;
        let idx0 = (exact_idx.floor() as usize) % stem_peaks.len();
        let idx1 = (idx0 + 1) % stem_peaks.len();
        let frac = exact_idx.fract() as f32;
        let p0 = stem_peaks[idx0];
        let p1 = stem_peaks[idx1];
        Some([
            p0[0] + (p1[0] - p0[0]) * frac,
            p0[1] + (p1[1] - p0[1]) * frac,
            p0[2] + (p1[2] - p0[2]) * frac,
            p0[3] + (p1[3] - p0[3]) * frac,
        ].map(zoom_waveform))
    };

    let muted_col = TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.4);
    let scroll_px = (elapsed_sec + ((wave_rect.left() - center_x) / pixels_per_sec) as f64) * pixels_per_sec as f64;
    let shift = |pitch: f32| scroll_px.rem_euclid(pitch as f64) as f32;
    let full_painter = painter;
    let painter = &full_painter.with_clip_rect(wave_rect);
    match crate::theme::waveform_style() {
        crate::theme::WaveformStyle::Classic => {
            let col_w = 1.5_f32;
            let cols = (wave_rect.width() / col_w).ceil() as usize;
            for i in 0..cols + 2 {
                let col_x = wave_rect.left() + (i as f32) * col_w - shift(col_w);
                let Some(amps) = sample_stems_at_x(col_x) else { continue };
                for lane in 0..4 {
                    let lane_cy = wave_rect.top() + (lane as f32 + 0.5) * lane_h;
                    let vol = if stem_muted[lane] { 0.15 } else { stem_volumes[lane].clamp(0.1, 1.0) };
                    let h_outer = amps[lane] * vol * max_lane_half;
                    let h_core = h_outer * 0.62;
                    if stem_muted[lane] {
                        painter.line_segment(
                            [Pos2::new(col_x, lane_cy - h_outer), Pos2::new(col_x, lane_cy + h_outer)],
                            Stroke::new(col_w, muted_col),
                        );
                    } else {
                        painter.line_segment(
                            [Pos2::new(col_x, lane_cy - h_outer), Pos2::new(col_x, lane_cy + h_outer)],
                            Stroke::new(col_w, stem_outer_colors[lane]),
                        );
                        painter.line_segment(
                            [Pos2::new(col_x, lane_cy - h_core), Pos2::new(col_x, lane_cy + h_core)],
                            Stroke::new(col_w, stem_core_colors[lane]),
                        );
                    }
                }
            }
        }
        crate::theme::WaveformStyle::Dots => {
            let (cols, half_rows, step_x, step_y, radius) =
                dot_grid_metrics(wave_rect.width(), lane_h, 96, 3);
            let unlit = crate::theme::mix(TraktorTheme::WAVEFORM_BG(), TraktorTheme::WAVEFORM_GRID(), 0.22);
            let glow_w = (step_x * 4.5).max(18.0);
            for c in 0..cols + 2 {
                let col_x = wave_rect.left() + (c as f32 + 0.5) * step_x - shift(step_x);
                let near = (1.0 - ((col_x - center_x).abs() / glow_w)).clamp(0.0, 1.0);
                let amps = sample_stems_at_x(col_x);
                for lane in 0..4 {
                    let lane_cy = wave_rect.top() + (lane as f32 + 0.5) * lane_h;
                    let vol = if stem_muted[lane] { 0.15 } else { stem_volumes[lane].clamp(0.1, 1.0) };
                    let lit = amps.map(|a| quantize_dot_rows(a[lane] * vol, half_rows)).unwrap_or(0);
                    let base_col = if stem_muted[lane] { muted_col } else { stem_core_colors[lane] };
                    for r in 0..half_rows {
                        let dy = (r as f32 + 0.5) * step_y;
                        let col = if r < lit {
                            if near > 0.0 {
                                crate::theme::mix(base_col, TraktorTheme::TEXT_HIGHLIGHT(), near * 0.42)
                            } else {
                                base_col
                            }
                        } else if near > 0.0 {
                            crate::theme::mix(unlit, TraktorTheme::WAVEFORM_GRID(), near * 0.25)
                        } else {
                            unlit
                        };
                        painter.circle_filled(Pos2::new(col_x, lane_cy - dy), radius, col);
                        painter.circle_filled(Pos2::new(col_x, lane_cy + dy), radius, col);
                    }
                }
            }
        }
        crate::theme::WaveformStyle::Bars => {
            let pitch = 5.0_f32;
            let bar_w = 3.2_f32;
            let rounding = bar_w * 0.5;
            let cols = (wave_rect.width() / pitch).floor() as usize;
            let glow_w = pitch * 4.0;
            for c in 0..cols + 2 {
                let col_x = wave_rect.left() + (c as f32 + 0.5) * pitch - shift(pitch);
                let Some(amps) = sample_stems_at_x(col_x) else { continue };
                let near = (1.0 - ((col_x - center_x).abs() / glow_w)).clamp(0.0, 1.0);
                for lane in 0..4 {
                    let lane_cy = wave_rect.top() + (lane as f32 + 0.5) * lane_h;
                    let vol = if stem_muted[lane] { 0.15 } else { stem_volumes[lane].clamp(0.1, 1.0) };
                    let h_outer = (amps[lane] * vol * max_lane_half).max(1.0);
                    let col = if stem_muted[lane] {
                        muted_col
                    } else if near > 0.0 {
                        crate::theme::mix(stem_core_colors[lane], TraktorTheme::TEXT_HIGHLIGHT(), near * 0.35)
                    } else {
                        stem_core_colors[lane]
                    };
                    painter.rect_filled(
                        Rect::from_center_size(Pos2::new(col_x, lane_cy), Vec2::new(bar_w, h_outer * 2.0)),
                        rounding,
                        col,
                    );
                }
            }
        }
        crate::theme::WaveformStyle::Line => {
            let step = 2.0_f32;
            let cols = (wave_rect.width() / step).ceil() as usize;
            let mut prev: [Option<(Pos2, Pos2)>; 4] = [None; 4];
            for c in 0..=cols + 1 {
                let col_x = (wave_rect.left() + (c as f32) * step - shift(step)).min(wave_rect.right());
                if let Some(amps) = sample_stems_at_x(col_x) {
                    for lane in 0..4 {
                        let lane_cy = wave_rect.top() + (lane as f32 + 0.5) * lane_h;
                        let vol = if stem_muted[lane] { 0.15 } else { stem_volumes[lane].clamp(0.1, 1.0) };
                        let h = (amps[lane] * vol * max_lane_half).max(0.6);
                        let top = Pos2::new(col_x, lane_cy - h);
                        let bot = Pos2::new(col_x, lane_cy + h);
                        let col = if stem_muted[lane] { muted_col } else { stem_core_colors[lane] };
                        if let Some((p_top, p_bot)) = prev[lane] {
                            painter.line_segment([p_top, top], Stroke::new(1.4_f32, col));
                            painter.line_segment([p_bot, bot], Stroke::new(1.4_f32, col));
                        }
                        prev[lane] = Some((top, bot));
                    }
                } else {
                    prev = [None; 4];
                }
            }
        }
    }
    let painter = full_painter;

    // 3. Beatgrid Lines in Waveform Area
    let beat_len_sec = 60.0 / bpm.max(30.0);
    let visible_sec_half = (wave_rect.width() / 2.0 / pixels_per_sec) as f64;
    let min_sec = (elapsed_sec - visible_sec_half).max(0.0);
    let max_sec = elapsed_sec + visible_sec_half;
    let start_beat = ((min_sec - grid_offset_sec) / beat_len_sec).floor() as i64;
    let end_beat = ((max_sec - grid_offset_sec) / beat_len_sec).ceil() as i64;

    for b in start_beat..=end_beat {
        let beat_time = grid_offset_sec + b as f64 * beat_len_sec;
        let x = center_x + ((beat_time - elapsed_sec) as f32) * pixels_per_sec;
        if x < wave_rect.left() || x > wave_rect.right() {
            continue;
        }
        let is_bar_start = b % 4 == 0;
        let stroke = if is_bar_start {
            Stroke::new(1.0_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.45))
        } else {
            Stroke::new(0.5_f32, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.18))
        };
        painter.line_segment(
            [Pos2::new(x, wave_rect.top()), Pos2::new(x, wave_rect.bottom())],
            stroke,
        );
    }

    // 4. Hotcues & Red Playhead Line
    for &(cue_id, cue_sec) in cues {
        let cue_x = center_x + ((cue_sec - elapsed_sec) as f32) * pixels_per_sec;
        if cue_x >= wave_rect.left() && cue_x <= wave_rect.right() {
            let color = TraktorTheme::hotcue_color(cue_id as usize);
            painter.line_segment(
                [Pos2::new(cue_x, wave_rect.top()), Pos2::new(cue_x, wave_rect.bottom())],
                Stroke::new(1.2_f32, color),
            );
        }
    }

    // 4a. Phrase Boundaries
    for phrase in phrases {
        let px = center_x + ((phrase.start_sec - elapsed_sec) as f32) * pixels_per_sec;
        if px >= wave_rect.left() && px <= wave_rect.right() {
            painter.line_segment(
                [Pos2::new(px, wave_rect.top()), Pos2::new(px, wave_rect.bottom())],
                Stroke::new(0.8_f32, TraktorTheme::TEXT_MUTED().gamma_multiply(0.35)),
            );
            painter.text(
                Pos2::new(px + 3.0, wave_rect.top() + 3.0),
                egui::Align2::LEFT_TOP,
                phrase.kind.tag(),
                crate::theme::dot_font(8.5),
                TraktorTheme::TEXT_MUTED().gamma_multiply(0.75),
            );
        }
    }

    painter.line_segment(
        [
            Pos2::new(center_x, wave_rect.top()),
            Pos2::new(center_x, wave_rect.bottom()),
        ],
        Stroke::new(1.5_f32, TraktorTheme::WAVEFORM_PLAYHEAD()),
    );

    // 5. Left Stem Header Column (DRUMS / BASS / OTHER / VOCALS + Mini Knob/Mute Indicators)
    painter.line_segment(
        [
            Pos2::new(header_rect.right(), header_rect.top()),
            Pos2::new(header_rect.right(), header_rect.bottom()),
        ],
        Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()),
    );

    for lane in 0..4 {
        let top_y = rect.top() + (lane as f32) * lane_h;
        let lane_cy = top_y + lane_h * 0.5;
        if lane > 0 {
            painter.line_segment(
                [Pos2::new(rect.left(), top_y), Pos2::new(rect.right(), top_y)],
                Stroke::new(0.8_f32, TraktorTheme::BORDER_COLOR()),
            );
        }

        let muted = stem_muted[lane];
        let text_col = if muted {
            TraktorTheme::TEXT_MUTED()
        } else {
            TraktorTheme::TEXT_PRIMARY()
        };
        painter.text(
            Pos2::new(header_rect.left() + 6.0, lane_cy),
            egui::Align2::LEFT_CENTER,
            stem_labels[lane],
            egui::FontId::proportional(8.5),
            text_col,
        );

        // Mini Volume Ring + Mute Dot + FX Send Ring (matching Traktor Pro 4 Stem Header)
        let ring1_c = Pos2::new(header_rect.right() - 34.0, lane_cy);
        let ring2_c = Pos2::new(header_rect.right() - 14.0, lane_cy);
        let ring_r = (lane_h * 0.26).clamp(3.0, 5.0);

        painter.circle_stroke(
            ring1_c,
            ring_r,
            Stroke::new(1.1_f32, text_col),
        );
        painter.circle_filled(
            Pos2::new(ring1_c.x + ring_r + 3.0, lane_cy + 1.5),
            1.6,
            text_col,
        );

        painter.circle_stroke(
            ring2_c,
            ring_r,
            Stroke::new(1.1_f32, TraktorTheme::DECK_B()),
        );
    }

    response
}

/// The overview columns under fractions `from..to` of the track: the mean bands (`0..=1`) of those drawn and their
/// loudest total, or `None` where nothing is decoded yet. At least one column, so a narrow span samples one.
pub(crate) fn overview_span(overview: &[StripeColumn], from: f32, to: f32) -> Option<([f32; 3], f32)> {
    let n = overview.len();
    if n == 0 {
        return None;
    }
    let first = ((from.clamp(0.0, 1.0) * n as f32) as usize).min(n - 1);
    let end = ((to.clamp(0.0, 1.0) * n as f32).ceil() as usize).clamp(first + 1, n);
    let (mut sum, mut count, mut total) = ([0f32; 3], 0, 0u8);
    for c in overview[first..end].iter().filter(|c| **c != StripeColumn::default()) {
        sum = [sum[0] + c.low as f32, sum[1] + c.mid as f32, sum[2] + c.high as f32];
        count += 1;
        total = total.max(c.total);
    }
    (count > 0).then(|| (sum.map(|v| v / count as f32 / 255.0), total as f32 / 255.0))
}

/// The background and whole-track overview under a deck, also drawn in the browser rows, in the chosen
/// overview look and waveform colours. Columns not decoded yet are left blank; `playhead_x` lights the waveform
/// styles that glow.
pub fn paint_overview_waveform(painter: &egui::Painter, rect: Rect, overview: &[StripeColumn], playhead_x: f32) {
    paint_overview_look(painter, rect, overview, None, playhead_x, crate::theme::overview_look(), crate::theme::waveform_bands());
}

/// Deck overview: colours come from `overview` (analysed frequency balance), heights from `played_overview`
/// with the deck's shared vertical zoom applied.
pub fn paint_deck_overview_waveform(
    painter: &egui::Painter,
    rect: Rect,
    overview: &[StripeColumn],
    played_overview: &[StripeColumn],
    playhead_x: f32,
) {
    paint_overview_look(painter, rect, overview, Some(played_overview), playhead_x, crate::theme::overview_look(), crate::theme::waveform_bands());
}

/// `overview` in the shape of `look`, in the waveform colours `bands` (`[bass, mids, highs]`), whatever the look.
fn paint_overview_look(
    painter: &egui::Painter,
    rect: Rect,
    overview: &[StripeColumn],
    played_overview: Option<&[StripeColumn]>,
    playhead_x: f32,
    look: crate::theme::OverviewLook,
    bands: [Color32; 3],
) {
    use crate::theme::OverviewLook;
    let background = TraktorTheme::WAVEFORM_BG(); // once: each call reads the theme
    painter.rect_filled(rect, 1.0, background);
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));
    if overview.is_empty() {
        return;
    }
    let frac = |x: f32| (x - rect.left()) / rect.width().max(1.0);
    // Native samples one column where each mark falls; the others take every column under a pixel.
    let sampled_pair = |x: f32| -> Option<((f32, f32, f32), (f32, f32, f32))> {
        let f = frac(x);
        let ([tl, tm, th], _) = overview_span(overview, f, f)?;
        let tone = (tl, tm, th);
        let played = match played_overview {
            Some(p) => {
                let ([pl, pm, ph], _) = overview_span(p, f, f)?;
                zoom_bands((pl, pm, ph))
            }
            None => tone,
        };
        Some((played, tone))
    };
    let pixel = |x: f32| -> Option<([f32; 3], [f32; 3], f32)> {
        let (f0, f1) = (frac(x), frac(x + 1.0));
        let (tone, tone_loud) = overview_span(overview, f0, f1)?;
        match played_overview {
            Some(p) => {
                let (played, played_loud) = overview_span(p, f0, f1)?;
                Some((tone, played.map(zoom_waveform), zoom_waveform(played_loud)))
            }
            None => Some((tone, tone, tone_loud)),
        }
    };
    let painter = &painter.with_clip_rect(rect.shrink(1.0));
    let vline = |x: f32, top: f32, bottom: f32, colour: Color32| painter.line_segment([Pos2::new(x, top), Pos2::new(x, bottom)], Stroke::new(1.0_f32, colour));
    let style = crate::theme::waveform_style();
    match look {
        OverviewLook::Native if style == crate::theme::WaveformStyle::Classic => {
            let cy = rect.center().y;
            let cols = (rect.width() / 1.5).ceil() as usize;
            for c in 0..cols {
                let x = rect.left() + (c as f32) * 1.5;
                let Some(((low, mid, high), tone)) = sampled_pair(rect.left() + c as f32 / cols.max(1) as f32 * rect.width()) else { continue };
                let (outer_col, mid_col, core_col) = spectrum_colors(bands, tone.0, tone.1, tone.2);
                let h_outer = ((low.max(mid).max(high)) * (rect.height() - 6.0) * 0.5).max(1.0);
                let h_core = (low * (rect.height() - 6.0) * 0.28).max(0.5);
                painter.line_segment([Pos2::new(x, cy - h_outer), Pos2::new(x, cy + h_outer)], Stroke::new(1.2_f32, outer_col));
                painter.line_segment(
                    [Pos2::new(x, cy - h_core), Pos2::new(x, cy + h_core)],
                    Stroke::new(1.2_f32, if tone.0 > 0.55 { core_col } else { mid_col }),
                );
            }
        }
        OverviewLook::Native => paint_3band_waveform(painter, rect, playhead_x, style, bands, 64, 2, 0.0, sampled_pair),
        OverviewLook::Traktor => {
            let (cy, half) = (rect.center().y, rect.height() * 0.5 - 1.5);
            let [low_c, mid_c, high_c] = bands.map(egui::Rgba::from);
            for px in 0..rect.width().ceil() as usize {
                let x = rect.left() + px as f32;
                let Some(([tl, tm, th], [low, mid, high], loudest)) = pixel(x) else { continue };
                // The band colours mixed by how much of each there is: bass alone is the bass colour.
                let sum = (tl + tm + th).max(1e-3);
                let body_col = Color32::from(low_c * (tl / sum) + mid_c * (tm / sum) + high_c * (th / sum));
                let x = x + 0.5;
                let body = low.max(mid).max(high) * half;
                // A faint halo out to the loudest hit, then the body in its own colour, darkening from the centre out.
                vline(x, cy - loudest * half, cy + loudest * half, body_col.gamma_multiply(0.22));
                vline(x, cy - body, cy + body, crate::theme::mix(background, body_col, 0.55));
                vline(x, cy - body * 0.6, cy + body * 0.6, crate::theme::mix(background, body_col, 0.8));
                vline(x, cy - body * 0.25, cy + body * 0.25, body_col);
            }
        }
        OverviewLook::Rekordbox => {
            let (bottom, height) = (rect.bottom() - 1.5, rect.height() - 3.0);
            for px in 0..rect.width().ceil() as usize {
                let x = rect.left() + px as f32;
                let Some(([tl, tm, th], [low, mid, high], loudest)) = pixel(x) else { continue };
                let x = x + 0.5;
                // Layers stacked from the bottom, bass, mids, then highs. The height blends the body with the loudest
                // hit (not floored at it, which flattens mastered tracks into a wall). Each band gets its share of
                // that height, so a bass hit alone stays all bass rather than gaining a cap of highs.
                let played_sum = low * 0.6 + mid * 0.25 + high * 0.25;
                let share = [tl * 0.6, tm * 0.25, th * 0.25];
                let sum = share.iter().sum::<f32>().max(1e-3);
                let top = (0.55 * played_sum + 0.45 * loudest).min(1.0);
                let bass_top = top * share[0] / sum;
                let mids_top = bass_top + top * share[1] / sum;
                vline(x, bottom - top * height, bottom, bands[2]);
                vline(x, bottom - mids_top * height, bottom, bands[1]);
                vline(x, bottom - bass_top * height, bottom, bands[0]);
            }
        }
    }
}

/// A small sample of `look` for the Preferences > Look cards: a track with an intro, a drop, a quiet breakdown and
/// a second drop, drawn as the decks and rows draw it.
pub fn paint_overview_look_preview(painter: &egui::Painter, rect: Rect, look: crate::theme::OverviewLook) {
    let columns: Vec<StripeColumn> = (0..128)
        .map(|i| {
            let t = i as f32 / 128.0;
            let energy = if t < 0.15 { 0.45 } else if (0.5..0.65).contains(&t) { 0.18 } else { 0.85 };
            let kick = if (i % 4 == 0) && !(0.5..0.65).contains(&t) { 1.0 } else { 0.7 };
            let band = |v: f32| (v.clamp(0.0, 1.0) * 255.0) as u8;
            let (low, mid, high) = (band(energy * kick), band(energy * 0.8), band(0.25 + energy * 0.35));
            StripeColumn { low, mid, high, total: low.max(mid).max(high) }
        })
        .collect();
    paint_overview_look(painter, rect, &columns, None, rect.center().x, look, crate::theme::waveform_bands());
}

/// Full-Track Stripe Overview Minimap Widget (`PRTS` Style with spectrum ribbon & playhead window)
#[allow(clippy::too_many_arguments)]
pub fn stripe_overview(
    ui: &mut Ui,
    elapsed_sec: &mut f64,
    total_len_sec: f64,
    overview: &[StripeColumn],
    played_overview: &[StripeColumn],
    cues: &[(u8, f64)],
    phrases: &[retraktor_library::Phrase],
    loop_active: bool,
    loop_start_sec: f64,
    loop_len_sec: f64,
    _accent: Color32,
) -> Response {
    let width = ui.available_width().max(160.0);
    let height = 28.0;
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    let total = total_len_sec.max(1.0);

    if response.dragged() || response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            let norm = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
            *elapsed_sec = (norm as f64) * total;
            response.mark_changed();
        }
    }

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let norm_pos = (*elapsed_sec / total).clamp(0.0, 1.0) as f32;
        let scrub_x = rect.left() + norm_pos * rect.width();
        paint_deck_overview_waveform(painter, rect, overview, played_overview, scrub_x);

        // Minute markers along top of stripe (like Traktor Pro 4)
        let total_mins = (total / 60.0).floor() as usize;
        for m in 1..=total_mins {
            let mx = rect.left() + ((m as f64 * 60.0 / total) as f32) * rect.width();
            painter.line_segment(
                [Pos2::new(mx, rect.top()), Pos2::new(mx, rect.top() + 4.0)],
                Stroke::new(1.0_f32, TraktorTheme::WAVEFORM_GRID()),
            );
        }

        // Hotcue tick marks on Stripe
        for &(cue_id, cue_sec) in cues {
            let cx = rect.left() + ((cue_sec / total).clamp(0.0, 1.0) as f32) * rect.width();
            let col = TraktorTheme::hotcue_color(cue_id as usize);
            painter.line_segment(
                [Pos2::new(cx, rect.top()), Pos2::new(cx, rect.bottom())],
                Stroke::new(1.0_f32, col),
            );
        }

        // Phrase boundaries on Stripe: thin ticks with 1-2 letter tag in dot font
        for phrase in phrases {
            let px = rect.left() + ((phrase.start_sec / total).clamp(0.0, 1.0) as f32) * rect.width();
            painter.line_segment(
                [Pos2::new(px, rect.top()), Pos2::new(px, rect.top() + 7.0)],
                Stroke::new(0.8_f32, TraktorTheme::TEXT_MUTED().gamma_multiply(0.70)),
            );
            painter.text(
                Pos2::new(px + 2.0, rect.top() + 1.0),
                egui::Align2::LEFT_TOP,
                phrase.kind.tag(),
                crate::theme::dot_font(7.5),
                TraktorTheme::TEXT_MUTED().gamma_multiply(0.85),
            );
        }

        // Loop region highlight
        if loop_active && loop_len_sec > 0.0 {
            let loop_start_x = rect.left() + (loop_start_sec / total) as f32 * rect.width();
            let loop_end_x =
                rect.left() + ((loop_start_sec + loop_len_sec) / total) as f32 * rect.width();
            let loop_rect = Rect::from_min_max(
                Pos2::new(loop_start_x.clamp(rect.left(), rect.right()), rect.top()),
                Pos2::new(loop_end_x.clamp(rect.left(), rect.right()), rect.bottom()),
            );
            painter.rect_filled(
                loop_rect,
                1.0,
                TraktorTheme::LOOP_ACTIVE().gamma_multiply(0.14),
            );
            painter.rect_stroke(loop_rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::LOOP_ACTIVE()));
        }

        // Translucent Playhead Window Box + Red Center Hairline (matching Traktor Pro 4 stripe)
        let win_half_w = 12.0;
        let win_rect = Rect::from_min_max(
            Pos2::new((scrub_x - win_half_w).max(rect.left()), rect.top() + 1.0),
            Pos2::new((scrub_x + win_half_w).min(rect.right()), rect.bottom() - 1.0),
        );
        painter.rect_filled(
            win_rect,
            1.0,
            TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.15),
        );
        painter.line_segment(
            [Pos2::new(scrub_x, rect.top()), Pos2::new(scrub_x, rect.bottom())],
            Stroke::new(1.5_f32, TraktorTheme::WAVEFORM_PLAYHEAD()),
        );
    }

    response
}

/// Hotcue Pad Widget (`1`..`8`) — Understated matte grey Traktor Pro 4 hotcue bar
pub fn cue_pad(ui: &mut Ui, index: usize, has_cue: bool, active: bool, width: f32) -> Response {
    let size = Vec2::new(width, 18.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let bg = if active {
            TraktorTheme::DECK_A()
        } else if has_cue {
            TraktorTheme::CONTROL_HOVER()
        } else {
            TraktorTheme::HOTCUE_EMPTY()
        };

        painter.rect_filled(rect, 1.0, bg);
        painter.rect_stroke(rect, 1.0, Stroke::new(0.8_f32, TraktorTheme::BORDER_COLOR()));

        // Subtle bottom accent bar when a cue/state is stored
        if has_cue {
            let cue_col = TraktorTheme::hotcue_color(index);
            let bar_rect = Rect::from_min_max(
                Pos2::new(rect.left() + 1.0, rect.bottom() - 2.5),
                Pos2::new(rect.right() - 1.0, rect.bottom() - 0.5),
            );
            painter.rect_filled(bar_rect, 0.5, cue_col);
        }

        let text_color = if active {
            TraktorTheme::BG_INPUT()
        } else if has_cue {
            TraktorTheme::TEXT_PRIMARY()
        } else {
            TraktorTheme::TEXT_MUTED()
        };

        painter.text(
            Pos2::new(rect.left() + 5.0, rect.center().y - 0.5),
            egui::Align2::LEFT_CENTER,
            index.to_string(),
            egui::FontId::proportional(9.5),
            text_color,
        );
    }

    response
}

/// Small vector icons drawn with the painter, so they render identically on every system font.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Play,
    Pause,
    Prev,
    Next,
    Dot,
}

/// Paints `icon` centred in `rect`.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.3;
    let tri = |pts: [Pos2; 3]| egui::Shape::convex_polygon(pts.to_vec(), color, Stroke::NONE);
    match icon {
        Icon::Play => {
            painter.add(tri([Pos2::new(c.x - r * 0.75, c.y - r), Pos2::new(c.x + r, c.y), Pos2::new(c.x - r * 0.75, c.y + r)]));
        }
        Icon::Pause => {
            for dx in [-r * 0.5, r * 0.5] {
                painter.rect_filled(Rect::from_center_size(Pos2::new(c.x + dx, c.y), Vec2::new(r * 0.55, r * 2.0)), 1.0, color);
            }
        }
        Icon::Prev => {
            painter.add(tri([Pos2::new(c.x + r * 0.6, c.y - r * 0.8), Pos2::new(c.x - r * 0.6, c.y), Pos2::new(c.x + r * 0.6, c.y + r * 0.8)]));
        }
        Icon::Next => {
            painter.add(tri([Pos2::new(c.x - r * 0.6, c.y - r * 0.8), Pos2::new(c.x + r * 0.6, c.y), Pos2::new(c.x - r * 0.6, c.y + r * 0.8)]));
        }
        Icon::Dot => {
            painter.circle_filled(c, r * 0.6, color);
        }
    }
}

/// Button showing a painted icon, optionally followed by a label.
pub fn icon_button(ui: &mut Ui, icon: Icon, label: &str, fill: Color32, fg: Color32, min_size: Vec2) -> Response {
    // Leading spaces reserve room for the icon when there's a label.
    let text = if label.is_empty() { String::new() } else { format!("    {label}") };
    let resp = ui.add(egui::Button::new(RichText::new(text).color(fg).size(11.5)).fill(fill).min_size(min_size));
    let icon_rect = if label.is_empty() {
        resp.rect
    } else {
        Rect::from_min_size(resp.rect.min + Vec2::new(4.0, 0.0), Vec2::new(resp.rect.height() * 0.8, resp.rect.height()))
    };
    paint_icon(ui.painter(), icon_rect, icon, fg);
    resp
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportAction {
    TogglePlay,
    Cue,
    Cup,
    ToggleFlux,
    HalveLoop,
    DoubleLoop,
    SelectLoop(u32),
}

/// Accessible, high-contrast Traktor Pro 4 deck transport bar
pub fn deck_transport_bar(
    ui: &mut Ui,
    is_playing: bool,
    flux_active: bool,
    loop_active: bool,
    current_loop_beats: u32,
    compact: bool,
    // Keyboard prefix for this deck's shortcuts: "" (A), "Shift+" (B), None (no keys).
    keys: Option<&str>,
) -> Option<TransportAction> {
    let key = |k: &str| keys.map(|p| format!(" ({p}{k})")).unwrap_or_default();
    const H: f32 = 30.0;
    let mut action = None;
    let active_fg = if crate::theme::palette().is_dark() {
        TraktorTheme::BG_WINDOW()
    } else {
        TraktorTheme::TEXT_HIGHLIGHT()
    };
    // Wraps on a narrow deck (Tools open at 1024 px) instead of pushing the next panel off screen.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(3.0, 3.0);

        let play_color = if is_playing {
            TraktorTheme::PLAY_ACTIVE()
        } else {
            TraktorTheme::PLAY_INACTIVE()
        };
        let play_text_color = if is_playing {
            active_fg
        } else {
            TraktorTheme::TEXT_PRIMARY()
        };
        let play_icon = if is_playing { Icon::Pause } else { Icon::Play };
        if icon_button(ui, play_icon, "", play_color, play_text_color, Vec2::new(46.0, H))
            .on_hover_text(format!("{}{}", if is_playing { "Pause" } else { "Play" }, key("Space")))
            .clicked()
        {
            action = Some(TransportAction::TogglePlay);
        }

        if ui
            .add(
                egui::Button::new(
                    RichText::new("CUE")
                        .color(TraktorTheme::TEXT_PRIMARY())
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                )
                .fill(TraktorTheme::CUE_INACTIVE())
                .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR()))
                .min_size(Vec2::new(46.0, H)),
            )
            .on_hover_text(format!("Jump to cue 1{}", key("C")))
            .clicked()
        {
            action = Some(TransportAction::Cue);
        }

        if !compact && ui
            .add(
                egui::Button::new(
                    RichText::new("CUP")
                        .color(TraktorTheme::TEXT_SECONDARY())
                        .font(egui::FontId::monospace(10.5)),
                )
                .fill(TraktorTheme::CUE_INACTIVE())
                .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR()))
                .min_size(Vec2::new(34.0, H)),
            )
            .on_hover_text("Cue Play (Jump to Cue 1 & Play)")
            .clicked()
        {
            action = Some(TransportAction::Cup);
        }

        if !compact && ui
            .add(
                egui::Button::new(
                    RichText::new("FLX")
                        .color(if flux_active {
                            active_fg
                        } else {
                            TraktorTheme::TEXT_SECONDARY()
                        })
                        .font(egui::FontId::monospace(10.5))
                        .strong(),
                )
                .fill(if flux_active {
                    TraktorTheme::FLUX_ACTIVE()
                } else {
                    TraktorTheme::CUE_INACTIVE()
                })
                .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR()))
                .min_size(Vec2::new(34.0, H)),
            )
            .on_hover_text("Toggle Flux / Slip Mode")
            .clicked()
        {
            action = Some(TransportAction::ToggleFlux);
        }

        ui.separator();
        // The loop group wraps as one unit (caption, chevrons and sizes), never leaving a chevron alone.
        // Its width comes from the previous frame, as egui lays out before it can measure.
        let group_id = ui.id().with("loop_group");
        let group_w: f32 = ui.data(|d| d.get_temp(group_id)).unwrap_or(0.0);
        if ui.available_size_before_wrap().x < group_w {
            ui.end_row();
        }
        let group = ui.horizontal(|ui| {
            ui.set_min_height(H); // the caption centres on the buttons, not on egui's default row height
            ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
            if !compact {
                ui.label(RichText::new("LOOP").font(egui::FontId::proportional(10.5)).color(if loop_active { TraktorTheme::LOOP_ACTIVE() } else { TraktorTheme::TEXT_SECONDARY() }));
            }
            let chevron_fill = TraktorTheme::CONTROL();
            if icon_button(ui, Icon::Prev, "", chevron_fill, TraktorTheme::TEXT_SECONDARY(), Vec2::new(22.0, H))
                .on_hover_text("Halve loop length")
                .clicked()
            {
                action = Some(TransportAction::HalveLoop);
            }

            // Compact decks show only the current loop length; it toggles the loop.
            let sizes: &[u32] = if compact { std::slice::from_ref(&current_loop_beats) } else { &[1, 2, 4, 8, 16] };
            for &beats in sizes {
                let is_selected_size = current_loop_beats == beats;
                let is_active_loop = loop_active && is_selected_size;
                let fill = if is_active_loop {
                    TraktorTheme::LOOP_ACTIVE()
                } else if is_selected_size {
                    TraktorTheme::CONTROL_HOVER()
                } else {
                    TraktorTheme::CONTROL()
                };
                let fg = if is_active_loop {
                    active_fg
                } else if is_selected_size {
                    TraktorTheme::LOOP_ACTIVE()
                } else {
                    TraktorTheme::TEXT_SECONDARY()
                };
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(if compact { format!("LOOP {beats}") } else { format!("{beats}") })
                                .color(fg)
                                .font(egui::FontId::monospace(10.5))
                                .strong(),
                        )
                        .fill(fill)
                        .min_size(Vec2::new(24.0, H)),
                    )
                    .on_hover_text(format!("Loop {beats} beats: click to set and toggle the loop{}", key("L")))
                    .clicked()
                {
                    action = Some(TransportAction::SelectLoop(beats));
                }
            }

            if icon_button(ui, Icon::Next, "", chevron_fill, TraktorTheme::TEXT_SECONDARY(), Vec2::new(22.0, H))
                .on_hover_text("Double loop length")
                .clicked()
            {
                action = Some(TransportAction::DoubleLoop);
            }
        });
        ui.data_mut(|d| d.insert_temp(group_id, group.response.rect.width()));
    });
    action
}

/// Renders a high-contrast Heads-Up Display (HUD) keyboard shortcut cheat-sheet.
pub fn render_shortcuts_hud(ctx: &egui::Context, is_open: &mut bool) {
    if !*is_open {
        return;
    }

    egui::Window::new("Keyboard shortcuts")
        .open(is_open)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
        .default_width(680.0)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(TraktorTheme::BG_PANEL())
                .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT()))
                .rounding(6.0)
                .inner_margin(16.0),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("LIVE PERFORMANCE SHORTCUTS")
                        .color(TraktorTheme::TEXT_PRIMARY())
                        .font(egui::FontId::monospace(14.0))
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("Press Esc or ? to close")
                            .color(TraktorTheme::TEXT_SECONDARY())
                            .font(egui::FontId::proportional(11.0)),
                    );
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            ui.columns(3, |cols| {
                let key_row = |ui: &mut Ui, key: &str, desc: &str, col: Color32| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("[ {key} ]"))
                                .color(col)
                                .font(egui::FontId::monospace(10.5))
                                .strong(),
                        );
                        ui.label(RichText::new(desc).color(TraktorTheme::TEXT_PRIMARY()).font(egui::FontId::proportional(11.0)));
                    });
                };

                // Column 1: Transport & Stems
                cols[0].vertical(|ui| {
                    ui.label(RichText::new("TRANSPORT & STEMS (Shift = Deck B)").color(TraktorTheme::DECK_A()).strong());
                    ui.add_space(4.0);
                    let c = TraktorTheme::PLAY_ACTIVE();
                    key_row(ui, "Space / Shift+Space", "Play / Pause A / B", c);
                    key_row(ui, "C / Shift+C", "Cue 1 Deck A / B", c);
                    key_row(ui, "S / Shift+S", "Sync BPM & Phase A / B", c);
                    key_row(ui, "L / Shift+L", "Loop On / Off A / B", c);
                    key_row(ui, "1..4 / 5..8", "Hotcues 1..4 A / B", c);
                    key_row(ui, "K / Shift+K", "Auto Cues Deck A / B", c);
                    key_row(ui, "T / Shift+T", "EQ / Stems View A / B", c);
                    key_row(ui, "G / Shift+G", "Create Demucs AI Stems A / B", c);
                    key_row(ui, "Q W E R", "Mute Stems 1..4 (Deck A)", c);
                    key_row(ui, "Shift+Q W E R", "Mute Stems 1..4 (Deck B)", c);
                });

                // Column 2: Mixer, EQ, Filter & FX
                cols[1].vertical(|ui| {
                    ui.label(RichText::new("MIXER, EQ, FILTER & FX").color(TraktorTheme::DECK_B()).strong());
                    ui.add_space(4.0);
                    let c = TraktorTheme::ACCENT();
                    key_row(ui, "Z X V", "EQ Kill Low / Mid / Hi (A)", c);
                    key_row(ui, "Shift+Z X V", "EQ Kill Low / Mid / Hi (B)", c);
                    key_row(ui, "H / Shift+H", "Headphone Cue On / Off A / B", c);
                    key_row(ui, "F / Shift+F", "Filter On / Off A / B", c);
                    key_row(ui, "D / Shift+D", "FX Unit On / Off A / B", c);
                    key_row(ui, "- / =", "Channel Fader Down / Up (A)", c);
                    key_row(ui, "Shift+- / +", "Channel Fader Down / Up (B)", c);
                    key_row(ui, "[ / \\ / ]", "Crossfader Left / Centre / Right", c);
                });

                // Column 3: Layouts & System
                cols[2].vertical(|ui| {
                    ui.label(RichText::new("LAYOUTS & SYSTEM").color(TraktorTheme::TEXT_PRIMARY()).strong());
                    ui.add_space(4.0);
                    let c = TraktorTheme::ACCENT();
                    key_row(ui, "F1", "Mix (Parallel Waveforms)", c);
                    key_row(ui, "F2", "4-Deck Full Mix", c);
                    key_row(ui, "F3", "Browser (Full Library)", c);
                    key_row(ui, "F4", "Timeline Studio", c);
                    key_row(ui, "F5", "Set Workflow (Build)", c);
                    ui.add_space(6.0);
                    ui.label(RichText::new("LIBRARY").color(TraktorTheme::DECK_A()).strong());
                    ui.add_space(4.0);
                    key_row(ui, "Alt+click", "Edit a Cell in Place", c);
                    key_row(ui, "Ctrl+I", "Edit Selected Tracks", c);
                    ui.add_space(6.0);
                    ui.label(RichText::new("SET & PERFORM").color(TraktorTheme::DECK_B()).strong());
                    ui.add_space(4.0);
                    key_row(ui, "Shift+A", "Add Selected to Set", c);
                    key_row(ui, "Shift+N", "Load Next Up to Deck", c);
                    key_row(ui, "O / Ctrl+Z", "Order Flow / Undo", c);
                    key_row(ui, "Alt+↑ / ↓", "Reorder Track in Set", c);
                    key_row(ui, "Delete", "Remove Track from Set", c);
                    ui.add_space(6.0);
                    ui.label(RichText::new("TIMELINE STUDIO").color(TraktorTheme::DECK_A()).strong());
                    ui.add_space(4.0);
                    key_row(ui, "U", "Cycle Transition Curve", c);
                    key_row(ui, "B", "Cycle Bar Length (8/16/32)", c);
                    key_row(ui, "A", "Sync + play both", c);
                    ui.add_space(6.0);
                    key_row(ui, "N", "Suggest Next Track (idle deck)", c);
                    key_row(ui, "Cmd / Ctrl + ,", "Preferences & Setup", c);
                    key_row(ui, "Ctrl+1..8", "Jump Prefs Group (in dialog)", c);
                    key_row(ui, "?", "Toggle Shortcuts HUD", c);
                    key_row(ui, "Esc", "Dismiss HUD / Modals", c);
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("The MX2 and MIDI controllers work alongside these keys; Shift picks deck B.")
                        .color(TraktorTheme::TEXT_SECONDARY())
                        .font(egui::FontId::proportional(11.0)),
                );
            });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A hit three quarters through a 3:51 track is drawn three quarters along the overview at a row's width and
    /// a deck's, where their playheads put 0.75 of the track, and nowhere else nearby. A half-decoded track is
    /// drawn up to where it is decoded and blank after.
    #[test]
    fn overview_columns_sit_at_their_place_in_the_track() {
        let n = 13_842;
        let peaks: Vec<[f32; 3]> = (0..n).map(|j| if j == n * 3 / 4 { [1.0; 3] } else { [0.2; 3] }).collect();
        let overview = crate::overview::from_peaks(&peaks);
        for width in [150.0f32, 900.0] {
            let hit = |px: f32| overview_span(&overview, px / width, (px + 1.0) / width).unwrap().1 == 1.0;
            let at = (0.75 * width).floor();
            assert!(hit(at), "{width} px: no hit at {at}");
            assert!(!hit(at - 3.0) && !hit(at + 3.0), "{width} px: the hit is smeared");
        }
        let half: Vec<[f32; 3]> = (0..1000).map(|j| if j < 500 { [0.3; 3] } else { [0.0; 3] }).collect();
        let half = crate::overview::from_peaks(&half);
        assert!(overview_span(&half, 0.2, 0.21).is_some() && overview_span(&half, 0.8, 0.81).is_none());
        assert!(overview_span(&[], 0.0, 1.0).is_none());
    }

    /// The lines, boxes and dots `look` paints for `columns` in `bands` at 120 x 40 px, with their colours.
    fn render(columns: &[StripeColumn], look: crate::theme::OverviewLook, bands: [Color32; 3]) -> Vec<(egui::Shape, Option<Color32>)> {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(120.0, 40.0));
        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            paint_overview_look(&ctx.layer_painter(egui::LayerId::background()), rect, columns, None, 60.0, look, bands);
        });
        out.shapes
            .into_iter()
            .map(|clipped| {
                let colour = match &clipped.shape {
                    egui::Shape::LineSegment { stroke, .. } => match stroke.color {
                        egui::epaint::ColorMode::Solid(c) => Some(c),
                        _ => None,
                    },
                    egui::Shape::Rect(r) => Some(r.fill),
                    egui::Shape::Circle(c) => Some(c.fill),
                    _ => None,
                };
                (clipped.shape, colour)
            })
            .collect()
    }

    /// The same stripe in two sets of waveform colours: every look (Native in whatever waveform style is set)
    /// paints exactly the same shapes, only in other colours. In the layered look a bass hit alone is bass from
    /// the bottom to the top, not capped by the mids' or highs' colour.
    #[test]
    fn overview_looks_are_shapes_in_the_waveform_colours() {
        use crate::theme::OverviewLook;
        let column = |low: u8, mid: u8, high: u8| StripeColumn { low, mid, high, total: low.max(mid).max(high) };
        let music: Vec<StripeColumn> = (0..256u32).map(|i| column((40 + i * 7 % 200) as u8, (30 + i * 13 % 180) as u8, (20 + i * 29 % 160) as u8)).collect();
        let spectrum = [Color32::from_rgb(232, 48, 52), Color32::from_rgb(210, 210, 210), Color32::from_rgb(120, 120, 120)];
        let ultraviolet = [Color32::from_rgb(150, 90, 255), Color32::from_rgb(255, 70, 200), Color32::from_rgb(150, 230, 255)];
        for look in OverviewLook::ALL {
            let (a, b) = (render(&music, look, spectrum), render(&music, look, ultraviolet));
            assert!(a.len() > 100 && a.len() == b.len(), "{look:?}: {} and {} shapes", a.len(), b.len());
            let geometry = |(shape, _): &(egui::Shape, Option<Color32>)| shape.visual_bounding_rect();
            assert!(a.iter().map(geometry).eq(b.iter().map(geometry)), "{look:?} changed shape with the colours");
            let recoloured = a.iter().zip(&b).filter(|(x, y)| x.1 != y.1).count();
            assert!(recoloured > a.len() / 2, "{look:?}: only {recoloured} of {} shapes took the new colours", a.len());
        }

        // A kick with the floors a decode leaves in the other bands.
        let bass_hit = vec![column(255, 5, 3); 64];
        let mut stacks = std::collections::BTreeMap::<i32, (f32, f32)>::new(); // x: (top of everything, top of bass)
        for (shape, colour) in render(&bass_hit, OverviewLook::Rekordbox, ultraviolet) {
            if let egui::Shape::LineSegment { points, .. } = shape {
                let stack = stacks.entry(points[0].x as i32).or_insert((f32::MAX, f32::MAX));
                stack.0 = stack.0.min(points[0].y);
                if colour == Some(ultraviolet[0]) {
                    stack.1 = stack.1.min(points[0].y);
                }
            }
        }
        let bottom = 40.0 - 1.5;
        assert!(stacks.len() > 100, "{} columns drawn", stacks.len());
        for (x, (top, bass_top)) in stacks {
            assert!(bottom - bass_top >= 0.95 * (bottom - top), "x {x}: bass up to {bass_top}, the stack up to {top}");
        }

        // A mastered passage: every column has a full-scale hit, but the body under it rises and falls with the beat.
        let mastered: Vec<StripeColumn> = (0..120u32).map(|i| StripeColumn { total: 255, ..column((60 + i * 37 % 180) as u8, 90, 60) }).collect();
        let mut tops = std::collections::BTreeMap::<i32, f32>::new();
        for (shape, _) in render(&mastered, OverviewLook::Rekordbox, ultraviolet) {
            if let egui::Shape::LineSegment { points, .. } = shape {
                let top = tops.entry(points[0].x as i32).or_insert(f32::MAX);
                *top = top.min(points[0].y);
            }
        }
        let (lowest, highest) = tops.values().fold((f32::MIN, f32::MAX), |(lo, hi), &y| (lo.max(y), hi.min(y)));
        assert!(lowest - highest > 4.0, "the beat envelope is flattened: tops span {highest}..{lowest} px");
    }

    #[test]
    fn dot_grid_quantisation_bounds_and_shape_budget() {
        let (cols, half_rows, step_x, step_y, radius) = dot_grid_metrics(1920.0, 120.0, 120, 7);
        assert!(cols <= 120 && cols > 0);
        assert!(half_rows <= 7 && half_rows > 0);
        assert!(cols * half_rows * 2 <= 2000, "dot waveform stays under 2000 circles");
        assert!(step_x >= 5.5 && step_y >= 4.5);
        assert!((1.1..=2.5).contains(&radius));

        assert_eq!(quantize_dot_rows(0.0, half_rows), 0);
        assert_eq!(quantize_dot_rows(0.01, half_rows), 0);
        assert_eq!(quantize_dot_rows(0.05, half_rows), 1);
        assert_eq!(quantize_dot_rows(0.5, 6), 3);
        assert_eq!(quantize_dot_rows(1.0, half_rows), half_rows);
        assert_eq!(quantize_dot_rows(1.5, half_rows), half_rows);
    }

    /// The deck waveform zoom roughly doubles typical matched columns, keeps every silence floor
    /// dark, and keeps hot peaks in order under full height.
    #[test]
    fn waveform_zoom_doubles_typical_levels_keeps_silence_dark_and_hot_peaks_apart() {
        // Classic column height in the Mix deck at 1280x820 (an 89.2 px lane, 45% each side).
        let column_px = |amp: f32| amp * 0.9 * 89.2;
        // Today's matched median and 90th-percentile columns, 20 and 30 px.
        let (median, p90) = (20.0 / column_px(1.0), 30.0 / column_px(1.0));
        let px = column_px(zoom_waveform(median));
        assert!((38.0..=42.0).contains(&px), "median column {px} px");
        let px = column_px(zoom_waveform(p90));
        assert!((54.0..=60.0).contains(&px), "90th-percentile column {px} px");
        // Quiet detail just above the floor gets the full zoom.
        assert!((zoom_waveform(0.1) - (WAVEFORM_SILENCE + 0.06 * WAVEFORM_ZOOM)).abs() < 1e-6);

        // The analysis' own silence floors (3-band, estimated stems, separated stems) stay put and dark.
        let (silent_bands, silent_stems) = crate::audio::LoadedTrackAudio::compute_waveform_peaks(&[0.0; 96_000], 48_000);
        for floor in silent_bands[0].into_iter().chain(silent_stems[0]).chain([0.04]) {
            assert_eq!(zoom_waveform(floor), floor);
            assert_eq!(quantize_dot_rows(zoom_waveform(floor), 7), 0, "silence at {floor} lit a dot");
        }

        // Rising everywhere with no jump at the floor or the knee, full height only at 1.0.
        let curve: Vec<f32> = (0..=1000).map(|i| zoom_waveform(i as f32 / 1000.0)).collect();
        assert!(curve.windows(2).all(|w| w[1] > w[0] && w[1] - w[0] <= WAVEFORM_ZOOM / 1000.0 + 1e-6));
        assert_eq!(zoom_waveform(1.0), 1.0);
        // Hot peaks keep at least the ramp's share of their spacing instead of flattening at a cap.
        let hot = [0.5, 0.6, 0.75, 0.9, 0.96].map(zoom_waveform);
        assert!(hot.windows(2).all(|w| w[1] - w[0] >= 0.06 * WAVEFORM_HEADROOM_SLOPE * 0.99), "{hot:?}");
        assert!(hot[4] < 1.0, "{hot:?}");
    }
}
