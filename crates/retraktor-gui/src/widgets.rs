use crate::audio::PEAKS_PER_SEC;
use crate::theme::TraktorTheme;
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

/// Maps 3-band `[low, mid, high]` amplitudes into the active theme's `WAVEFORM_*` spectrum colors:
/// - Outer silhouette: `WAVEFORM_HIGH` blended with `WAVEFORM_MID`
/// - Inner body: `WAVEFORM_MID` blended with `WAVEFORM_LOW`
/// - Core transients: `WAVEFORM_LOW`
fn spectrum_colors(low: f32, mid: f32, high: f32) -> (Color32, Color32, Color32) {
    let mix = |a: Color32, b: Color32, t: f32| -> Color32 {
        Color32::from(egui::Rgba::from(a) * (1.0 - t) + egui::Rgba::from(b) * t)
    };
    let low_c = TraktorTheme::WAVEFORM_LOW();
    let mid_c = TraktorTheme::WAVEFORM_MID();
    let high_c = TraktorTheme::WAVEFORM_HIGH();

    let h_ratio = (high / (high + mid + 0.05)).clamp(0.0, 1.0);
    let outer_col = mix(mid_c, high_c, 0.5 + 0.5 * h_ratio).gamma_multiply(195.0 / 255.0);

    let m_ratio = (mid / (low + mid + 0.05)).clamp(0.0, 1.0);
    let mid_col = mix(low_c, mid_c, 0.5 + 0.5 * m_ratio).gamma_multiply(220.0 / 255.0);

    let l_boost = low.clamp(0.0, 1.0);
    let core_col = mix(mid_c, low_c, 0.65 + 0.35 * l_boost).gamma_multiply(240.0 / 255.0);

    (outer_col, mid_col, core_col)
}

/// Real-Time 60 FPS Scrolling Traktor Pro 4 "Spectrum" Waveform Display
#[allow(clippy::too_many_arguments)]
pub fn scrolling_waveform(
    ui: &mut Ui,
    elapsed_sec: f64,
    bpm: f64,
    grid_offset_sec: f64,
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
    painter.rect_filled(rect, 1.0, TraktorTheme::WAVEFORM_BG());
    painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

    let center_y = rect.center().y;
    let center_x = rect.center().x;
    let pixels_per_sec = 92.0_f32;

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

    // 2. Multi-Band Traktor Pro 4 "Spectrum" Waveform Columns (1.5px pitch for crisp detail)
    let beat_len_sec = 60.0 / bpm.max(30.0);
    let col_w = 1.5_f32;
    let cols = (width / col_w).ceil() as usize;
    let max_half_h = height * 0.45;

    for i in 0..cols {
        let col_x = rect.left() + (i as f32) * col_w;
        let time_at_col = elapsed_sec + ((col_x - center_x) / pixels_per_sec) as f64;

        if time_at_col < 0.0 {
            continue;
        }

        let (low_amp, mid_amp, high_amp) = if !waveform_peaks.is_empty() {
            let exact_idx = time_at_col * PEAKS_PER_SEC as f64;
            let idx0 = (exact_idx.floor() as usize) % waveform_peaks.len();
            let idx1 = (idx0 + 1) % waveform_peaks.len();
            let frac = exact_idx.fract() as f32;
            let p0 = waveform_peaks[idx0];
            let p1 = waveform_peaks[idx1];
            (
                p0[0] + (p1[0] - p0[0]) * frac,
                p0[1] + (p1[1] - p0[1]) * frac,
                p0[2] + (p1[2] - p0[2]) * frac,
            )
        } else {
            // Empty deck: nothing to draw.
            continue;
        };

        let (outer_col, mid_col, core_col) = spectrum_colors(low_amp, mid_amp, high_amp);
        let total_env = low_amp.max(mid_amp * 0.92).max(high_amp * 0.85);

        // Layer 1: Outer Periwinkle/Sky-Blue silhouette
        let h_outer = (total_env * max_half_h).max(1.0);
        painter.line_segment(
            [
                Pos2::new(col_x, center_y - h_outer),
                Pos2::new(col_x, center_y + h_outer),
            ],
            Stroke::new(col_w, outer_col),
        );

        // Layer 2: Middle Sage/Gold harmonic body
        let h_mid = (mid_amp.max(low_amp * 0.75) * max_half_h * 0.72).max(0.8);
        painter.line_segment(
            [
                Pos2::new(col_x, center_y - h_mid),
                Pos2::new(col_x, center_y + h_mid),
            ],
            Stroke::new(col_w, mid_col),
        );

        // Layer 3: Inner Warm Coral-Orange Bass/Kick core
        let h_core = (low_amp * max_half_h * 0.46).max(0.5);
        painter.line_segment(
            [
                Pos2::new(col_x, center_y - h_core),
                Pos2::new(col_x, center_y + h_core),
            ],
            Stroke::new(col_w, core_col),
        );
    }

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
    bpm: f64,
    grid_offset_sec: f64,
    cues: &[(u8, f64)],
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
    let pixels_per_sec = 92.0_f32;

    // Authentic Traktor Pro 4 Stem Lane Colors (from Traktor Pro 4 Deck B screenshot)
    let stem_core_colors = [
        Color32::from_rgb(218, 102, 54), // 0: DRUMS (Warm Coral-Orange core)
        Color32::from_rgb(214, 118, 42), // 1: BASS  (Deep Warm Amber-Orange)
        Color32::from_rgb(86, 162, 188), // 2: OTHER (Soft Steel-Teal / Blue)
        Color32::from_rgb(98, 182, 118), // 3: VOCALS (Soft Emerald-Sage Green)
    ];
    let stem_outer_colors = [
        Color32::from_rgba_unmultiplied(128, 114, 206, 185), // Drums Highs (Periwinkle)
        Color32::from_rgba_unmultiplied(186, 82, 48, 185),   // Bass Outer
        Color32::from_rgba_unmultiplied(112, 134, 204, 185), // Other Outer
        Color32::from_rgba_unmultiplied(88, 164, 192, 185),  // Vocals Outer
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

    // 2. Draw 4 Stem Waveform Ribbons
    let col_w = 1.5_f32;
    let cols = (wave_rect.width() / col_w).ceil() as usize;
    let max_lane_half = (lane_h * 0.44).max(2.0);

    for i in 0..cols {
        let col_x = wave_rect.left() + (i as f32) * col_w;
        let time_at_col = elapsed_sec + ((col_x - center_x) / pixels_per_sec) as f64;
        if time_at_col < 0.0 {
            continue;
        }

        let amps = if !stem_peaks.is_empty() {
            let exact_idx = time_at_col * PEAKS_PER_SEC as f64;
            let idx0 = (exact_idx.floor() as usize) % stem_peaks.len();
            let idx1 = (idx0 + 1) % stem_peaks.len();
            let frac = exact_idx.fract() as f32;
            let p0 = stem_peaks[idx0];
            let p1 = stem_peaks[idx1];
            [
                p0[0] + (p1[0] - p0[0]) * frac,
                p0[1] + (p1[1] - p0[1]) * frac,
                p0[2] + (p1[2] - p0[2]) * frac,
                p0[3] + (p1[3] - p0[3]) * frac,
            ]
        } else {
            continue;
        };

        for lane in 0..4 {
            let lane_cy = wave_rect.top() + (lane as f32 + 0.5) * lane_h;
            let vol = if stem_muted[lane] {
                0.15
            } else {
                stem_volumes[lane].clamp(0.1, 1.0)
            };
            let h_outer = amps[lane] * vol * max_lane_half;
            let h_core = h_outer * 0.62;

            if stem_muted[lane] {
                painter.line_segment(
                    [Pos2::new(col_x, lane_cy - h_outer), Pos2::new(col_x, lane_cy + h_outer)],
                    Stroke::new(col_w, TraktorTheme::WAVEFORM_GRID().gamma_multiply(0.4)),
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

/// Full-Track Stripe Overview Minimap Widget (`PRTS` Style with spectrum ribbon & playhead window)
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
    _accent: Color32,
) -> Response {
    let width = ui.available_width().max(160.0);
    let height = 22.0;
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
        painter.rect_filled(rect, 1.0, TraktorTheme::WAVEFORM_BG());
        painter.rect_stroke(rect, 1.0, Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()));

        let norm_pos = (*elapsed_sec / total).clamp(0.0, 1.0) as f32;
        let scrub_x = rect.left() + norm_pos * rect.width();
        let cy = rect.center().y;

        // Overview spectrum waveform columns (high density)
        let cols = (rect.width() / 1.5).ceil() as usize;
        for c in 0..cols {
            let x = rect.left() + (c as f32) * 1.5;
            let frac = c as f32 / cols.max(1) as f32;
            let (low, mid, high) = if !waveform_peaks.is_empty() {
                let idx = ((frac * waveform_peaks.len() as f32) as usize)
                    .min(waveform_peaks.len() - 1);
                let p = waveform_peaks[idx];
                (p[0], p[1], p[2])
            } else {
                continue;
            };

            let (outer_col, mid_col, core_col) = spectrum_colors(low, mid, high);
            let h_outer = ((low.max(mid).max(high)) * (height - 6.0) * 0.5).max(1.0);
            let h_core = (low * (height - 6.0) * 0.28).max(0.5);

            painter.line_segment(
                [Pos2::new(x, cy - h_outer), Pos2::new(x, cy + h_outer)],
                Stroke::new(1.2_f32, outer_col),
            );
            painter.line_segment(
                [Pos2::new(x, cy - h_core), Pos2::new(x, cy + h_core)],
                Stroke::new(1.2_f32, if low > 0.55 { core_col } else { mid_col }),
            );
        }

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
) -> Option<TransportAction> {
    let mut action = None;
    let active_fg = if crate::theme::palette().is_dark() {
        TraktorTheme::BG_WINDOW()
    } else {
        TraktorTheme::TEXT_HIGHLIGHT()
    };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);

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
        if icon_button(ui, play_icon, "", play_color, play_text_color, Vec2::new(34.0, 22.0))
            .on_hover_text(if is_playing { "Pause (Space)" } else { "Play (Space)" })
            .clicked()
        {
            action = Some(TransportAction::TogglePlay);
        }

        if ui
            .add(
                egui::Button::new(
                    RichText::new("CUE")
                        .color(TraktorTheme::TEXT_PRIMARY())
                        .font(egui::FontId::monospace(9.5))
                        .strong(),
                )
                .fill(TraktorTheme::CUE_INACTIVE())
                .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR()))
                .min_size(Vec2::new(34.0, 22.0)),
            )
            .on_hover_text("Jump to Cue 1")
            .clicked()
        {
            action = Some(TransportAction::Cue);
        }

        if !compact && ui
            .add(
                egui::Button::new(
                    RichText::new("CUP")
                        .color(TraktorTheme::TEXT_SECONDARY())
                        .font(egui::FontId::monospace(9.0)),
                )
                .fill(TraktorTheme::CUE_INACTIVE())
                .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR()))
                .min_size(Vec2::new(32.0, 22.0)),
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
                        .font(egui::FontId::monospace(9.0))
                        .strong(),
                )
                .fill(if flux_active {
                    TraktorTheme::FLUX_ACTIVE()
                } else {
                    TraktorTheme::CUE_INACTIVE()
                })
                .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR()))
                .min_size(Vec2::new(30.0, 22.0)),
            )
            .on_hover_text("Toggle Flux / Slip Mode")
            .clicked()
        {
            action = Some(TransportAction::ToggleFlux);
        }

        ui.separator();

        let chevron_fill = TraktorTheme::CONTROL();
        if icon_button(ui, Icon::Prev, "", chevron_fill, TraktorTheme::TEXT_SECONDARY(), Vec2::new(18.0, 22.0))
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
                        RichText::new(format!("{beats}"))
                            .color(fg)
                            .font(egui::FontId::monospace(9.0))
                            .strong(),
                    )
                    .fill(fill)
                    .min_size(Vec2::new(20.0, 22.0)),
                )
                .on_hover_text(format!("Loop {beats} beats (L)"))
                .clicked()
            {
                action = Some(TransportAction::SelectLoop(beats));
            }
        }

        if icon_button(ui, Icon::Next, "", chevron_fill, TraktorTheme::TEXT_SECONDARY(), Vec2::new(18.0, 22.0))
            .on_hover_text("Double loop length")
            .clicked()
        {
            action = Some(TransportAction::DoubleLoop);
        }
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
                            .color(TraktorTheme::TEXT_MUTED())
                            .font(egui::FontId::proportional(11.0)),
                    );
                });
            });
            ui.add_space(8.0);
            ui.separator();
            ui.add_space(8.0);

            ui.columns(3, |cols| {
                // Column 1: Deck A & Deck B Transports
                cols[0].vertical(|ui| {
                    ui.label(RichText::new("DECK A (Primary Left)").color(TraktorTheme::DECK_A()).strong());
                    ui.add_space(4.0);
                    let key_row = |ui: &mut Ui, key: &str, desc: &str| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[ {key} ]"))
                                    .color(TraktorTheme::PLAY_ACTIVE())
                                    .font(egui::FontId::monospace(11.0))
                                    .strong(),
                            );
                            ui.label(RichText::new(desc).color(TraktorTheme::TEXT_PRIMARY()).font(egui::FontId::proportional(11.5)));
                        });
                    };
                    key_row(ui, "Space", "Play / Pause Deck A");
                    key_row(ui, "C", "Cue Deck A");
                    key_row(ui, "S", "Sync Deck A BPM");
                    key_row(ui, "1..4", "Hotcues 1 to 4");

                    ui.add_space(12.0);
                    ui.label(RichText::new("DECK B (Primary Right)").color(TraktorTheme::DECK_B()).strong());
                    ui.add_space(4.0);
                    key_row(ui, "Shift+Space", "Play / Pause Deck B");
                    key_row(ui, "Shift+C", "Cue Deck B");
                    key_row(ui, "Shift+S", "Sync Deck B BPM");
                    key_row(ui, "5..8", "Hotcues 1 to 4");
                });

                // Column 2: Workspace Layouts
                cols[1].vertical(|ui| {
                    ui.label(RichText::new("WORKSPACE LAYOUTS").color(TraktorTheme::TEXT_PRIMARY()).strong());
                    ui.add_space(4.0);
                    let layout_row = |ui: &mut Ui, key: &str, desc: &str| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[ {key} ]"))
                                    .color(TraktorTheme::ACCENT())
                                    .font(egui::FontId::monospace(11.0))
                                    .strong(),
                            );
                            ui.label(RichText::new(desc).color(TraktorTheme::TEXT_PRIMARY()).font(egui::FontId::proportional(11.5)));
                        });
                    };
                    layout_row(ui, "F1", "Parallel Full Waveforms");
                    layout_row(ui, "F2", "Essential Compact");
                    layout_row(ui, "F3", "Extended Mixer & FX");
                    layout_row(ui, "F4", "Four-Deck Full Mix");
                    layout_row(ui, "F5", "4-Stem Isolation View");
                    layout_row(ui, "F6", "Full Music Browser");
                });

                // Column 3: Mixer, FX & Stems
                cols[2].vertical(|ui| {
                    ui.label(RichText::new("SYSTEM & STEMS").color(TraktorTheme::TEXT_PRIMARY()).strong());
                    ui.add_space(4.0);
                    let sys_row = |ui: &mut Ui, key: &str, desc: &str| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("[ {key} ]"))
                                    .color(TraktorTheme::ACCENT())
                                    .font(egui::FontId::monospace(11.0))
                                    .strong(),
                            );
                            ui.label(RichText::new(desc).color(TraktorTheme::TEXT_PRIMARY()).font(egui::FontId::proportional(11.5)));
                        });
                    };
                    sys_row(ui, "Cmd / Ctrl + ,", "Preferences & Setup");
                    sys_row(ui, "?", "Toggle Shortcuts HUD");
                    sys_row(ui, "Esc", "Dismiss HUD / Modals");
                    sys_row(ui, "Click Platter", "Vinyl Scratch / Stop");
                    sys_row(ui, "◆ Stems", "Separate Audio via AI");
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("💡 Pro Tip: Hardware USB DJ controllers (Traktor Kontrol / MX2) operate at 1000 Hz alongside keyboard shortcuts.")
                        .color(TraktorTheme::TEXT_MUTED())
                        .font(egui::FontId::proportional(11.0)),
                );
            });
        });
}
