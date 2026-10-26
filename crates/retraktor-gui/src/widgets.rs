use crate::audio::PEAKS_PER_SEC;
use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
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

/// Compact Rotary Knob Widget with configurable box size
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
    let size = Vec2::new(box_size, box_size + 4.0);
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
        let center = rect.center() + Vec2::new(0.0, -4.0);
        let radius = (box_size * 0.34).max(10.0);

        // Background circle
        painter.circle_filled(center, radius, Color32::from_rgb(26, 28, 34));
        painter.circle_stroke(center, radius, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR));

        // Center detent tick (top center)
        let detent_top = center + Vec2::new(0.0, -radius - 2.5);
        let detent_bot = center + Vec2::new(0.0, -radius);
        painter.line_segment(
            [detent_top, detent_bot],
            Stroke::new(1.0_f32, TraktorTheme::TEXT_MUTED),
        );

        // Angular calculations (-135° to +135°)
        let norm = (*value - min) / (max - min).max(1e-5);
        let angle_start = -135.0_f32.to_radians() - PI / 2.0;
        let angle_end = 135.0_f32.to_radians() - PI / 2.0;
        let current_angle = angle_start + norm * (angle_end - angle_start);

        // Value arc track
        let track_radius = radius - 2.0;
        let points = 20;
        let sweep = current_angle - angle_start;
        if sweep.abs() > 0.05 {
            let mut arc_pts = Vec::with_capacity(points + 1);
            for i in 0..=points {
                let a = angle_start + (i as f32 / points as f32) * sweep;
                arc_pts.push(center + Vec2::new(a.cos(), a.sin()) * track_radius);
            }
            for w in arc_pts.windows(2) {
                painter.line_segment([w[0], w[1]], Stroke::new(2.2_f32, accent));
            }
        }

        // Pointer indicator line
        let ptr_inner = center + Vec2::new(current_angle.cos(), current_angle.sin()) * 3.0;
        let ptr_outer =
            center + Vec2::new(current_angle.cos(), current_angle.sin()) * (radius - 1.5);
        painter.line_segment([ptr_inner, ptr_outer], Stroke::new(1.8_f32, Color32::WHITE));

        // Center dot
        painter.circle_filled(center, 2.5, Color32::from_rgb(45, 48, 56));

        // Knob label underneath
        let label_pos = Pos2::new(rect.center().x, rect.bottom() - 4.0);
        painter.text(
            label_pos,
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(8.5),
            TraktorTheme::TEXT_SECONDARY,
        );
    }

    response
}

/// Custom Vertical Volume Fader Widget with smooth track, metallic cap, and dB markers
pub fn vertical_fader(
    ui: &mut Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    accent: Color32,
    height: f32,
) -> Response {
    let size = Vec2::new(26.0, height);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click_and_drag());

    if response.dragged() {
        let delta = -response.drag_delta().y / (rect.height() - 18.0).max(20.0) * (max - min);
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
        let track_top = rect.top() + 8.0;
        let track_bot = rect.bottom() - 8.0;
        let track_h = (track_bot - track_top).max(10.0);

        // Fader rail / slot
        let slot_rect = Rect::from_min_max(
            Pos2::new(track_x - 2.0, track_top),
            Pos2::new(track_x + 2.0, track_bot),
        );
        painter.rect_filled(slot_rect, 1.0, Color32::from_rgb(14, 15, 18));
        painter.rect_stroke(slot_rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR));

        // dB Tick marks on left and right
        let ticks = [1.00_f32, 0.85, 0.70, 0.50, 0.25, 0.00];
        for &norm in &ticks {
            let y = track_bot - norm * track_h;
            painter.line_segment(
                [Pos2::new(track_x - 9.0, y), Pos2::new(track_x - 4.0, y)],
                Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT),
            );
            painter.line_segment(
                [Pos2::new(track_x + 4.0, y), Pos2::new(track_x + 9.0, y)],
                Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT),
            );
        }

        let norm_val = (*value - min) / (max - min).max(1e-5);
        let cap_y = track_bot - norm_val * track_h;

        // Active level glow line up to cap
        painter.line_segment(
            [Pos2::new(track_x, track_bot), Pos2::new(track_x, cap_y)],
            Stroke::new(2.0_f32, accent.gamma_multiply(0.5)),
        );

        // Fader Cap (Traktor-style metallic handle with colored center line)
        let cap_w = 22.0;
        let cap_h = 12.0;
        let cap_rect = Rect::from_center_size(Pos2::new(track_x, cap_y), Vec2::new(cap_w, cap_h));

        painter.rect_filled(cap_rect, 2.0, Color32::from_rgb(50, 54, 64));
        painter.rect_stroke(
            cap_rect,
            2.0,
            Stroke::new(1.0_f32, Color32::from_rgb(85, 90, 105)),
        );
        painter.line_segment(
            [
                Pos2::new(cap_rect.left() + 2.0, cap_y),
                Pos2::new(cap_rect.right() - 2.0, cap_y),
            ],
            Stroke::new(2.0_f32, accent),
        );
    }

    response
}

/// Custom Horizontal Crossfader Widget
pub fn crossfader(ui: &mut Ui, value: &mut f32, width: f32) -> Response {
    let size = Vec2::new(width, 22.0);
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
        let slot_left = rect.left() + 10.0;
        let slot_right = rect.right() - 10.0;
        let slot_w = (slot_right - slot_left).max(10.0);
        let slot_y = rect.center().y;

        // Horizontal rail
        let slot_rect = Rect::from_min_max(
            Pos2::new(slot_left, slot_y - 2.0),
            Pos2::new(slot_right, slot_y + 2.0),
        );
        painter.rect_filled(slot_rect, 1.0, Color32::from_rgb(14, 15, 18));
        painter.rect_stroke(slot_rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR));

        // Center marker
        painter.line_segment(
            [
                Pos2::new(rect.center().x, slot_y - 6.0),
                Pos2::new(rect.center().x, slot_y + 6.0),
            ],
            Stroke::new(1.5_f32, TraktorTheme::TEXT_MUTED),
        );

        // Crossfader Cap
        let cap_x = slot_left + *value * slot_w;
        let cap_rect = Rect::from_center_size(Pos2::new(cap_x, slot_y), Vec2::new(16.0, 18.0));

        painter.rect_filled(cap_rect, 2.0, Color32::from_rgb(52, 56, 68));
        painter.rect_stroke(
            cap_rect,
            2.0,
            Stroke::new(1.0_f32, Color32::from_rgb(90, 95, 110)),
        );
        painter.line_segment(
            [
                Pos2::new(cap_x, cap_rect.top() + 2.0),
                Pos2::new(cap_x, cap_rect.bottom() - 2.0),
            ],
            Stroke::new(2.0_f32, Color32::WHITE),
        );
    }

    response
}

/// Stereo Segmented LED VU Meter Tower Widget
pub fn vu_meter(ui: &mut Ui, level_l: f32, level_r: f32, height: f32) {
    let width = 12.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());

    if !ui.is_rect_visible(rect) {
        return;
    }

    let painter = ui.painter();
    painter.rect_filled(rect, 1.0, TraktorTheme::METER_BG);
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR));

    let segments = 16;
    let seg_w = (width - 4.0) / 2.0;
    let seg_h = ((height - 6.0) / segments as f32 - 1.0).max(1.5);

    let draw_channel = |ch_x: f32, level: f32| {
        let lit_count = ((level.clamp(0.0, 1.0) * segments as f32).round() as usize).min(segments);
        for seg_idx in 0..segments {
            let y = rect.bottom() - 3.0 - (seg_idx as f32 + 1.0) * (seg_h + 1.0);
            let seg_rect = Rect::from_min_size(Pos2::new(ch_x, y), Vec2::new(seg_w, seg_h));

            let color = if seg_idx < lit_count {
                if seg_idx >= segments - 2 {
                    TraktorTheme::METER_RED // Clip (>0dB)
                } else if seg_idx >= segments - 5 {
                    TraktorTheme::METER_YELLOW // -6dB to 0dB
                } else {
                    TraktorTheme::METER_GREEN // Normal
                }
            } else {
                Color32::from_rgb(25, 27, 32) // Unlit
            };
            painter.rect_filled(seg_rect, 0.5, color);
        }
    };

    draw_channel(rect.left() + 2.0, level_l);
    draw_channel(rect.left() + 2.0 + seg_w + 1.0, level_r);
}

/// Real-Time 60 FPS Scrolling 3-Band Frequency Waveform Display
#[allow(clippy::too_many_arguments)]
pub fn scrolling_waveform(
    ui: &mut Ui,
    elapsed_sec: f64,
    bpm: f64,
    cues: &[(u8, f64)],
    waveform_peaks: &[[f32; 3]],
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
    painter.rect_filled(rect, 2.0, TraktorTheme::WAVEFORM_BG);
    painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR));

    let center_y = rect.center().y;
    let center_x = rect.center().x;
    let pixels_per_sec = 85.0_f32; // Waveform zoom scale

    // 1. Draw Active Loop Overlay if engaged
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
                Color32::from_rgba_unmultiplied(118, 255, 3, 28),
            );
            painter.rect_stroke(loop_rect, 0.0, Stroke::new(1.0_f32, TraktorTheme::LOOP_ACTIVE));
        }
    }

    // 2. Draw Beatgrid Lines
    let beat_len_sec = 60.0 / bpm.max(30.0);
    let visible_sec_half = (width / 2.0 / pixels_per_sec) as f64;
    let min_sec = (elapsed_sec - visible_sec_half).max(0.0);
    let max_sec = elapsed_sec + visible_sec_half;

    let start_beat = (min_sec / beat_len_sec).floor() as i64;
    let end_beat = (max_sec / beat_len_sec).ceil() as i64;

    for b in start_beat..=end_beat {
        let beat_time = b as f64 * beat_len_sec;
        let x = center_x + ((beat_time - elapsed_sec) as f32) * pixels_per_sec;
        if x < rect.left() || x > rect.right() {
            continue;
        }

        let is_bar_start = b % 4 == 0;
        let stroke = if is_bar_start {
            Stroke::new(1.4_f32, TraktorTheme::WAVEFORM_GRID)
        } else {
            Stroke::new(0.7_f32, Color32::from_rgba_unmultiplied(255, 255, 255, 45))
        };
        painter.line_segment([Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())], stroke);

        if is_bar_start {
            let bar_num = b / 4 + 1;
            painter.text(
                Pos2::new(x + 3.0, rect.top() + 2.0),
                egui::Align2::LEFT_TOP,
                format!("{bar_num}.1"),
                egui::FontId::monospace(8.5),
                TraktorTheme::TEXT_SECONDARY,
            );
        }
    }

    // 3. Draw Multi-Band Frequency Waveform Ribbons (Lows, Mids, Highs)
    let col_w = 2.0_f32;
    let cols = (width / col_w).ceil() as usize;
    let max_half_h = height * 0.44;

    for i in 0..cols {
        let col_x = rect.left() + (i as f32) * col_w;
        let time_at_col = elapsed_sec + ((col_x - center_x) / pixels_per_sec) as f64;

        if time_at_col < 0.0 {
            continue;
        }

        let (low_amp, mid_amp, high_amp) = if !waveform_peaks.is_empty() {
            let bucket = ((time_at_col * PEAKS_PER_SEC as f64) as usize) % waveform_peaks.len();
            let p = waveform_peaks[bucket];
            (p[0], p[1], p[2])
        } else {
            let phase = time_at_col * (bpm / 60.0) * 2.0 * PI as f64;
            let beat_frac = (time_at_col / beat_len_sec).fract();
            let kick_env = (-beat_frac * 8.0).exp() as f32;
            let low = (kick_env * 0.8 + 0.15 * (phase * 0.5).sin().abs() as f32).clamp(0.05, 1.0);
            let mid = ((phase * 2.0).sin().abs() * 0.5 + 0.25).clamp(0.05, 0.8) as f32;
            let hat_phase = (time_at_col / (beat_len_sec * 0.5)).fract();
            let high = ((-hat_phase * 12.0).exp() * 0.6 + 0.1) as f32;
            (low, mid, high)
        };

        // Highs (outermost Cyan ribbon)
        let h_high = high_amp * max_half_h;
        painter.line_segment(
            [
                Pos2::new(col_x, center_y - h_high),
                Pos2::new(col_x, center_y + h_high),
            ],
            Stroke::new(col_w, TraktorTheme::WAVEFORM_HIGH),
        );

        // Mids (middle Amber ribbon)
        let h_mid = mid_amp * max_half_h * 0.82;
        painter.line_segment(
            [
                Pos2::new(col_x, center_y - h_mid),
                Pos2::new(col_x, center_y + h_mid),
            ],
            Stroke::new(col_w, TraktorTheme::WAVEFORM_MID),
        );

        // Lows (core Red bass ribbon)
        let h_low = low_amp * max_half_h * 0.62;
        painter.line_segment(
            [
                Pos2::new(col_x, center_y - h_low),
                Pos2::new(col_x, center_y + h_low),
            ],
            Stroke::new(col_w, TraktorTheme::WAVEFORM_LOW),
        );
    }

    // 4. Draw Hotcue Markers on Waveform
    for &(cue_id, cue_sec) in cues {
        let cue_x = center_x + ((cue_sec - elapsed_sec) as f32) * pixels_per_sec;
        if cue_x >= rect.left() && cue_x <= rect.right() {
            let color = TraktorTheme::hotcue_color(cue_id as usize);
            painter.line_segment(
                [Pos2::new(cue_x, rect.top()), Pos2::new(cue_x, rect.bottom())],
                Stroke::new(1.5_f32, color),
            );
            let badge_rect =
                Rect::from_min_size(Pos2::new(cue_x - 6.0, rect.top() + 2.0), Vec2::new(12.0, 11.0));
            painter.rect_filled(badge_rect, 2.0, color);
            painter.text(
                badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                cue_id.to_string(),
                egui::FontId::monospace(8.0),
                Color32::BLACK,
            );
        }
    }

    // 5. Center Playhead Needle
    painter.line_segment(
        [
            Pos2::new(center_x, rect.top()),
            Pos2::new(center_x, rect.bottom()),
        ],
        Stroke::new(2.0_f32, TraktorTheme::WAVEFORM_PLAYHEAD),
    );
    let tri_size = 4.0;
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(center_x - tri_size, rect.top()),
            Pos2::new(center_x + tri_size, rect.top()),
            Pos2::new(center_x, rect.top() + tri_size * 1.5),
        ],
        TraktorTheme::WAVEFORM_PLAYHEAD,
        Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            Pos2::new(center_x - tri_size, rect.bottom()),
            Pos2::new(center_x + tri_size, rect.bottom()),
            Pos2::new(center_x, rect.bottom() - tri_size * 1.5),
        ],
        TraktorTheme::WAVEFORM_PLAYHEAD,
        Stroke::NONE,
    ));

    // Optional Deck Overlay Badge on Left Edge (used in Parallel Full-Width Waveform Mode)
    if let Some((badge_txt, badge_col)) = deck_badge {
        let badge_rect =
            Rect::from_min_size(Pos2::new(rect.left() + 6.0, rect.top() + 6.0), Vec2::new(22.0, 18.0));
        painter.rect_filled(badge_rect, 2.0, badge_col);
        painter.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            badge_txt,
            egui::FontId::monospace(11.0),
            Color32::BLACK,
        );
    }

    response
}

/// Full-Track Stripe Overview Minimap Widget (`PRTS` Style with scrubber)
#[allow(clippy::too_many_arguments)]
pub fn stripe_overview(
    ui: &mut Ui,
    elapsed_sec: &mut f64,
    total_len_sec: f64,
    waveform_peaks: &[[f32; 3]],
    cues: &[(u8, f64)],
    loop_active: bool,
    loop_start_sec: f64,
    loop_len_sec: f64,
    accent: Color32,
) -> Response {
    let width = ui.available_width().max(160.0);
    let height = 20.0;
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
        painter.rect_filled(rect, 2.0, Color32::from_rgb(14, 15, 18));
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR));

        let norm_pos = (*elapsed_sec / total).clamp(0.0, 1.0) as f32;
        let scrub_x = rect.left() + norm_pos * rect.width();

        // Overview waveform columns
        let cols = 120;
        let step = rect.width() / cols as f32;
        for c in 0..cols {
            let x = rect.left() + c as f32 * step;
            let frac = c as f32 / cols as f32;
            let amp = if !waveform_peaks.is_empty() {
                let idx = ((frac * waveform_peaks.len() as f32) as usize)
                    .min(waveform_peaks.len() - 1);
                let p = waveform_peaks[idx];
                ((p[0] + p[1] + p[2]) * 0.42).clamp(0.15, 0.95)
            } else {
                (frac * 6.0 * PI).sin().abs() * 0.6 + 0.3
            };
            let h = amp * (height - 4.0);
            let color = if x <= scrub_x {
                Color32::from_rgb(80, 90, 105)
            } else {
                accent
            };
            painter.line_segment(
                [
                    Pos2::new(x, rect.center().y - h / 2.0),
                    Pos2::new(x, rect.center().y + h / 2.0),
                ],
                Stroke::new(1.2_f32, color),
            );
        }

        // Hotcue tick marks on Stripe
        for &(cue_id, cue_sec) in cues {
            let cx = rect.left() + ((cue_sec / total).clamp(0.0, 1.0) as f32) * rect.width();
            let col = TraktorTheme::hotcue_color(cue_id as usize);
            painter.line_segment(
                [Pos2::new(cx, rect.top()), Pos2::new(cx, rect.bottom())],
                Stroke::new(1.2_f32, col),
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
                Color32::from_rgba_unmultiplied(118, 255, 3, 40),
            );
            painter.rect_stroke(loop_rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::LOOP_ACTIVE));
        }

        // Draggable Playhead Scrubber needle
        painter.line_segment(
            [Pos2::new(scrub_x, rect.top()), Pos2::new(scrub_x, rect.bottom())],
            Stroke::new(2.0_f32, Color32::WHITE),
        );
    }

    response
}

/// Hotcue Pad Widget (`[1]`..`[8]`)
pub fn cue_pad(ui: &mut Ui, index: usize, has_cue: bool, active: bool, width: f32) -> Response {
    let size = Vec2::new(width, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let base_color = if has_cue {
            TraktorTheme::hotcue_color(index)
        } else {
            TraktorTheme::HOTCUE_EMPTY
        };

        let bg = if active {
            Color32::WHITE
        } else if has_cue {
            base_color
        } else {
            TraktorTheme::HOTCUE_EMPTY
        };

        painter.rect_filled(rect, 2.0, bg);
        painter.rect_stroke(rect, 2.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT));

        let text_color = if has_cue || active {
            Color32::BLACK
        } else {
            TraktorTheme::TEXT_MUTED
        };

        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            index.to_string(),
            egui::FontId::monospace(9.5),
            text_color,
        );
    }

    response
}
