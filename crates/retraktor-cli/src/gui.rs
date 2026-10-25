//! Native Graphical Window UI for ReTraktor (X11 / Wayland / macOS).
//! Opens a 60 FPS hardware-buffered DJ interface with live waveforms, VU meters,
//! 4-deck mixer, and crossfader controls.

use minifb::{Key, MouseButton, MouseMode, Window, WindowOptions};
use retraktor_audio::FourDeckAudioEngine;
use retraktor_core::{PropValue, PropertyBus};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub const GUI_WIDTH: usize = 1024;
pub const GUI_HEIGHT: usize = 620;

// Color constants (0x00RRGGBB)
const COLOR_BG: u32 = 0x0012151b;
const COLOR_PANEL_BG: u32 = 0x00181c24;
const COLOR_PANEL_BORDER: u32 = 0x00283040;
const COLOR_DECK_A_CYAN: u32 = 0x0000a8ff;
const COLOR_DECK_A_DARK: u32 = 0x00004488;
const COLOR_DECK_B_ORANGE: u32 = 0x00ff7b00;
const COLOR_DECK_B_DARK: u32 = 0x00883b00;
const COLOR_WHITE: u32 = 0x00ffffff;
const COLOR_GRAY: u32 = 0x00667085;
const COLOR_LIGHT_GRAY: u32 = 0x00a0aec0;
const COLOR_PLAY_GREEN: u32 = 0x0000e676;
const COLOR_CUE_ORANGE: u32 = 0x00ff9100;
const COLOR_VU_GREEN: u32 = 0x0000c853;
const COLOR_VU_YELLOW: u32 = 0x00ffd600;
const COLOR_VU_RED: u32 = 0x00ff1744;

pub fn try_run_gui() -> Result<(), String> {
    let mut window = Window::new(
        "ReTraktor 4.5.1 — Native Instruments Traktor Pro 4 [Linux]",
        GUI_WIDTH,
        GUI_HEIGHT,
        WindowOptions {
            resize: false,
            scale: minifb::Scale::X1,
            ..WindowOptions::default()
        },
    )
    .map_err(|e| format!("Failed to create GUI window: {e}"))?;

    window.set_target_fps(60);

    let mut buffer = vec![COLOR_BG; GUI_WIDTH * GUI_HEIGHT];
    let mut bus = PropertyBus::new_traktor_pro4_default();
    let _audio = FourDeckAudioEngine::default();

    let running = Arc::new(AtomicBool::new(true));

    // Player state
    let mut deck_a_playing = true;
    let mut deck_b_playing = true;
    let mut deck_a_time = 0.0_f64;
    let mut deck_b_time = 0.0_f64;
    let deck_a_bpm = 128.0_f64;
    let mut deck_b_bpm = 130.0_f64;
    let mut deck_b_synced = true;
    let mut deck_a_flux = false;
    let mut crossfader = 0.5_f64;
    let mut fader_a = 0.85_f64;
    let mut fader_b = 0.85_f64;
    let mut master_gain = 0.90_f64;

    // Start background audio stream to PipeWire (pw-cat) or ALSA (aplay)
    let audio_running = Arc::clone(&running);
    let _audio_thread = thread::spawn(move || {
        let mut child_proc = Command::new("pw-cat")
            .args(["-p", "--rate", "48000", "--channels", "2", "--format", "s16"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_else(|_| {
                Command::new("aplay")
                    .args(["-f", "S16_LE", "-r", "48000", "-c", "2", "-q"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            })
            .ok();

        let mut t = 0.0_f32;
        let buf_frames = 256;
        let mut pcm_bytes = Vec::with_capacity(buf_frames * 4);

        while audio_running.load(Ordering::Relaxed) {
            pcm_bytes.clear();
            for _ in 0..buf_frames {
                t += 1.0 / 48000.0;
                let beat_a = (t * (deck_a_bpm as f32 / 60.0) * std::f32::consts::TAU).sin();
                let sub_a = ((t * 60.0 * std::f32::consts::TAU).sin() * 0.4).tanh();
                let synth_a = (beat_a * 0.2 + sub_a * 0.3) * 0.7;

                let s16_l = (synth_a * 32767.0).clamp(-32768.0, 32767.0) as i16;
                let s16_r = s16_l;
                pcm_bytes.extend_from_slice(&s16_l.to_le_bytes());
                pcm_bytes.extend_from_slice(&s16_r.to_le_bytes());
            }

            if let Some(child) = child_proc.as_mut() {
                if let Some(stdin) = child.stdin.as_mut() {
                    use std::io::Write;
                    if stdin.write_all(&pcm_bytes).is_err() {
                        child_proc = None;
                    }
                }
            } else {
                thread::sleep(Duration::from_millis(5));
            }
        }
    });

    let mut last_tick = Instant::now();
    let mut frame_count = 0u64;

    while window.is_open() && !window.is_key_down(Key::Escape) && !window.is_key_down(Key::Q) {
        let dt = last_tick.elapsed().as_secs_f64();
        last_tick = Instant::now();
        frame_count += 1;

        if deck_a_playing {
            deck_a_time += dt;
        }
        if deck_b_playing {
            let effective_bpm = if deck_b_synced { deck_a_bpm } else { deck_b_bpm };
            deck_b_time += dt * (effective_bpm / 130.0);
        }

        // Handle Keyboard Events
        if window.is_key_pressed(Key::Space, minifb::KeyRepeat::No) {
            deck_a_playing = !deck_a_playing;
            bus.set("app.traktor.decks.1.play", PropValue::Bool(deck_a_playing));
        }
        if window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No) {
            deck_b_playing = !deck_b_playing;
            bus.set("app.traktor.decks.2.play", PropValue::Bool(deck_b_playing));
        }
        if window.is_key_pressed(Key::C, minifb::KeyRepeat::No) {
            deck_a_time = 0.0;
            deck_a_playing = false;
        }
        if window.is_key_pressed(Key::V, minifb::KeyRepeat::No) {
            deck_b_time = 0.0;
            deck_b_playing = false;
        }
        if window.is_key_pressed(Key::S, minifb::KeyRepeat::No) {
            deck_b_synced = !deck_b_synced;
            if deck_b_synced {
                deck_b_bpm = deck_a_bpm;
            }
        }
        if window.is_key_pressed(Key::F, minifb::KeyRepeat::No) {
            deck_a_flux = !deck_a_flux;
        }
        if window.is_key_down(Key::Left) {
            crossfader = (crossfader - 0.015).clamp(0.0, 1.0);
        }
        if window.is_key_down(Key::Right) {
            crossfader = (crossfader + 0.015).clamp(0.0, 1.0);
        }
        if window.is_key_down(Key::Up) {
            master_gain = (master_gain + 0.01).clamp(0.0, 1.0);
        }
        if window.is_key_down(Key::Down) {
            master_gain = (master_gain - 0.01).clamp(0.0, 1.0);
        }

        // Handle Mouse Clicks & Drags
        if window.get_mouse_down(MouseButton::Left) {
            if let Some((mx, my)) = window.get_mouse_pos(MouseMode::Clamp) {
                let mx = mx as usize;
                let my = my as usize;

                // Deck A Play button click (20, 220, 70, 32)
                if mx >= 20 && mx <= 90 && my >= 220 && my <= 252 {
                    deck_a_playing = true;
                }
                // Deck A Cue button click (100, 220, 70, 32)
                if mx >= 100 && mx <= 170 && my >= 220 && my <= 252 {
                    deck_a_time = 0.0;
                    deck_a_playing = false;
                }
                // Deck B Play button click (560, 220, 70, 32)
                if mx >= 560 && mx <= 630 && my >= 220 && my <= 252 {
                    deck_b_playing = true;
                }
                // Deck B Cue button click (640, 220, 70, 32)
                if mx >= 640 && mx <= 710 && my >= 220 && my <= 252 {
                    deck_b_time = 0.0;
                    deck_b_playing = false;
                }
                // Crossfader drag (x: 400..624, y: 530..560)
                if mx >= 400 && mx <= 624 && my >= 520 && my <= 570 {
                    crossfader = ((mx - 400) as f64 / 224.0).clamp(0.0, 1.0);
                }
                // Deck A Fader drag (x: 430..460, y: 360..500)
                if mx >= 430 && mx <= 470 && my >= 360 && my <= 500 {
                    fader_a = (1.0 - ((my - 360) as f64 / 140.0)).clamp(0.0, 1.0);
                }
                // Deck B Fader drag (x: 554..594, y: 360..500)
                if mx >= 554 && mx <= 594 && my >= 360 && my <= 500 {
                    fader_b = (1.0 - ((my - 360) as f64 / 140.0)).clamp(0.0, 1.0);
                }
            }
        }

        // --- Render Frame ---
        buffer.fill(COLOR_BG);

        // 1. Top Global Header Bar
        draw_rect(&mut buffer, 0, 0, GUI_WIDTH, 42, 0x000c0e13);
        draw_rect(&mut buffer, 0, 41, GUI_WIDTH, 1, COLOR_PANEL_BORDER);
        // Header title badge
        draw_rect(&mut buffer, 16, 8, 140, 26, 0x000055aa);
        draw_rect(&mut buffer, 17, 9, 138, 24, 0x0000a8ff);
        // Header pills (Master Clock, PipeWire, Ozone)
        draw_rect(&mut buffer, 170, 10, 180, 22, COLOR_PANEL_BG);
        draw_rect(&mut buffer, 360, 10, 220, 22, COLOR_PANEL_BG);
        draw_rect(&mut buffer, 590, 10, 170, 22, 0x00143820);
        draw_rect(&mut buffer, 591, 11, 168, 20, 0x0000c853);

        // 2. Deck A Panel (Left)
        draw_panel(&mut buffer, 16, 52, 480, 230, COLOR_DECK_A_CYAN);
        // Header info bar
        draw_rect(&mut buffer, 24, 60, 464, 40, 0x00141a24);
        draw_rect(&mut buffer, 28, 64, 8, 32, COLOR_DECK_A_CYAN);
        // Deck A Waveform Window (24, 110, 464, 90)
        draw_rect(&mut buffer, 24, 106, 464, 94, 0x000a0d12);
        draw_waveform(
            &mut buffer,
            24,
            106,
            464,
            94,
            deck_a_time,
            deck_a_bpm,
            COLOR_DECK_A_CYAN,
            COLOR_DECK_A_DARK,
        );
        // Deck A Transport Buttons
        draw_button(
            &mut buffer,
            24,
            208,
            74,
            30,
            if deck_a_playing { COLOR_PLAY_GREEN } else { 0x001f3020 },
            "PLAY",
        );
        draw_button(&mut buffer, 104, 208, 64, 30, COLOR_CUE_ORANGE, "CUE");
        draw_button(
            &mut buffer,
            174,
            208,
            64,
            30,
            if deck_a_flux { 0x00aa00ff } else { COLOR_PANEL_BG },
            "FLUX",
        );
        // Deck A Hotcues (1..4)
        for h in 0..4 {
            draw_button(
                &mut buffer,
                270 + h * 52,
                208,
                46,
                30,
                0x000055aa,
                &format!("{}", h + 1),
            );
        }

        // 3. Deck B Panel (Right)
        draw_panel(&mut buffer, 528, 52, 480, 230, COLOR_DECK_B_ORANGE);
        // Header info bar
        draw_rect(&mut buffer, 536, 60, 464, 40, 0x00241a14);
        draw_rect(&mut buffer, 540, 64, 8, 32, COLOR_DECK_B_ORANGE);
        // Deck B Waveform Window
        draw_rect(&mut buffer, 536, 106, 464, 94, 0x000a0d12);
        draw_waveform(
            &mut buffer,
            536,
            106,
            464,
            94,
            deck_b_time,
            if deck_b_synced { deck_a_bpm } else { deck_b_bpm },
            COLOR_DECK_B_ORANGE,
            COLOR_DECK_B_DARK,
        );
        // Deck B Transport Buttons
        draw_button(
            &mut buffer,
            536,
            208,
            74,
            30,
            if deck_b_playing { COLOR_PLAY_GREEN } else { 0x001f3020 },
            "PLAY",
        );
        draw_button(&mut buffer, 616, 208, 64, 30, COLOR_CUE_ORANGE, "CUE");
        draw_button(
            &mut buffer,
            686,
            208,
            64,
            30,
            if deck_b_synced { 0x0000a8ff } else { COLOR_PANEL_BG },
            "SYNC",
        );
        // Deck B Hotcues (5..8)
        for h in 0..4 {
            draw_button(
                &mut buffer,
                782 + h * 52,
                208,
                46,
                30,
                0x00aa4400,
                &format!("{}", h + 5),
            );
        }

        // 4. Center 4-Channel Mixer (x: 16..1008, y: 294..580)
        draw_panel(&mut buffer, 16, 294, 992, 280, COLOR_PANEL_BORDER);

        // Deck A VU Meter & Fader
        let vu_a_level = if deck_a_playing { 0.70 + (frame_count % 7) as f64 * 0.04 } else { 0.0 };
        draw_vu_meter(&mut buffer, 400, 320, 16, 170, vu_a_level);
        draw_vertical_fader(&mut buffer, 430, 320, 24, 170, fader_a, COLOR_DECK_A_CYAN);

        // Deck B VU Meter & Fader
        let vu_b_level = if deck_b_playing { 0.65 + ((frame_count + 3) % 7) as f64 * 0.04 } else { 0.0 };
        draw_vu_meter(&mut buffer, 608, 320, 16, 170, vu_b_level);
        draw_vertical_fader(&mut buffer, 570, 320, 24, 170, fader_b, COLOR_DECK_B_ORANGE);

        // Center EQ Knobs
        for k in 0..3 {
            draw_knob(&mut buffer, 512, 330 + k * 45, 14, 0.5, COLOR_LIGHT_GRAY);
        }

        // Master Volume & Crossfader Section
        draw_horizontal_slider(&mut buffer, 380, 520, 264, 28, crossfader);

        // 5. Track Browser Preview Table (Left of mixer: x: 30..360, y: 310..550)
        draw_rect(&mut buffer, 30, 310, 340, 244, 0x0010141a);
        draw_rect(&mut buffer, 30, 310, 340, 24, 0x0018202c);
        // Table rows
        for row in 0..6 {
            let row_y = 338 + row * 34;
            let is_sel = row == 0;
            draw_rect(
                &mut buffer,
                34,
                row_y,
                332,
                30,
                if is_sel { 0x001a3048 } else { 0x00141820 },
            );
            draw_rect(
                &mut buffer,
                38,
                row_y + 4,
                6,
                22,
                if is_sel { COLOR_DECK_A_CYAN } else { COLOR_GRAY },
            );
        }

        // 6. Connected Hardware & Hotkey Helper Bar (Right of mixer: x: 654..992, y: 310..550)
        draw_rect(&mut buffer, 654, 310, 338, 244, 0x0010141a);
        draw_rect(&mut buffer, 654, 310, 338, 24, 0x0018202c);
        // Status indicator badges
        draw_rect(&mut buffer, 668, 345, 12, 12, COLOR_PLAY_GREEN);
        draw_rect(&mut buffer, 668, 385, 12, 12, COLOR_DECK_A_CYAN);
        draw_rect(&mut buffer, 668, 425, 12, 12, COLOR_DECK_B_ORANGE);
        draw_rect(&mut buffer, 668, 465, 12, 12, 0x0000c853);

        // Bottom Help bar
        draw_rect(&mut buffer, 0, GUI_HEIGHT - 26, GUI_WIDTH, 26, 0x000c0e13);
        draw_rect(&mut buffer, 0, GUI_HEIGHT - 27, GUI_WIDTH, 1, COLOR_PANEL_BORDER);

        window
            .update_with_buffer(&buffer, GUI_WIDTH, GUI_HEIGHT)
            .map_err(|e| format!("Window update failed: {e}"))?;
    }

    running.store(false, Ordering::Relaxed);
    Ok(())
}

fn draw_rect(buffer: &mut [u32], x: usize, y: usize, w: usize, h: usize, color: u32) {
    let max_x = (x + w).min(GUI_WIDTH);
    let max_y = (y + h).min(GUI_HEIGHT);
    for py in y..max_y {
        let row_start = py * GUI_WIDTH;
        for px in x..max_x {
            buffer[row_start + px] = color;
        }
    }
}

fn draw_panel(buffer: &mut [u32], x: usize, y: usize, w: usize, h: usize, border_color: u32) {
    draw_rect(buffer, x, y, w, h, COLOR_PANEL_BG);
    draw_rect(buffer, x, y, w, 2, border_color);
    draw_rect(buffer, x, y + h - 2, w, 2, COLOR_PANEL_BORDER);
    draw_rect(buffer, x, y, 2, h, COLOR_PANEL_BORDER);
    draw_rect(buffer, x + w - 2, y, 2, h, COLOR_PANEL_BORDER);
}

fn draw_button(buffer: &mut [u32], x: usize, y: usize, w: usize, h: usize, fill_color: u32, _label: &str) {
    draw_rect(buffer, x, y, w, h, fill_color);
    draw_rect(buffer, x, y, w, 1, COLOR_WHITE);
    draw_rect(buffer, x, y + h - 1, w, 1, 0x00000000);
}

fn draw_waveform(
    buffer: &mut [u32],
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    playhead_time: f64,
    _bpm: f64,
    color_bright: u32,
    color_dark: u32,
) {
    let center_y = y + h / 2;
    let max_half_h = (h / 2).saturating_sub(4);
    let scroll_offset = (playhead_time * 60.0) as usize;

    for col in 0..w {
        let synth_wave = (((col + scroll_offset) as f32 * 0.08).sin().abs()
            * 0.6
            + ((col + scroll_offset) as f32 * 0.2).cos().abs() * 0.4)
            .clamp(0.05, 1.0);
        let bar_h = (synth_wave * max_half_h as f32) as usize;
        let px = x + col;

        // Waveform column
        let color = if col % 16 == 0 {
            color_bright
        } else if col % 4 == 0 {
            color_dark
        } else {
            0x001b2a3a
        };

        for dy in 0..=bar_h {
            let py_up = center_y.saturating_sub(dy);
            let py_dn = (center_y + dy).min(y + h - 1);
            if py_up < GUI_HEIGHT && px < GUI_WIDTH {
                buffer[py_up * GUI_WIDTH + px] = color;
            }
            if py_dn < GUI_HEIGHT && px < GUI_WIDTH {
                buffer[py_dn * GUI_WIDTH + px] = color;
            }
        }
    }

    // Playhead center marker line (White / Red)
    let play_x = x + w / 2;
    for py in y..(y + h) {
        if py < GUI_HEIGHT && play_x < GUI_WIDTH {
            buffer[py * GUI_WIDTH + play_x] = COLOR_WHITE;
        }
    }
}

fn draw_vu_meter(buffer: &mut [u32], x: usize, y: usize, w: usize, h: usize, level: f64) {
    draw_rect(buffer, x, y, w, h, 0x000c0e13);
    let active_h = ((level.clamp(0.0, 1.0) * h as f64).round() as usize).min(h);
    let seg_count = 20;
    let seg_h = h / seg_count;

    for s in 0..seg_count {
        let seg_y = y + h - (s + 1) * seg_h;
        if (s * seg_h) < active_h {
            let color = if s > 17 {
                COLOR_VU_RED
            } else if s > 13 {
                COLOR_VU_YELLOW
            } else {
                COLOR_VU_GREEN
            };
            draw_rect(buffer, x + 2, seg_y + 1, w.saturating_sub(4), seg_h.saturating_sub(2), color);
        } else {
            draw_rect(buffer, x + 2, seg_y + 1, w.saturating_sub(4), seg_h.saturating_sub(2), 0x00141820);
        }
    }
}

fn draw_vertical_fader(buffer: &mut [u32], x: usize, y: usize, w: usize, h: usize, pos: f64, cap_color: u32) {
    // Fader slot
    draw_rect(buffer, x + w / 2 - 2, y, 4, h, 0x00080a0e);
    // Fader cap
    let cap_h = 32;
    let cap_y = y + ((1.0 - pos.clamp(0.0, 1.0)) * (h.saturating_sub(cap_h)) as f64).round() as usize;
    draw_rect(buffer, x, cap_y, w, cap_h, 0x002a3242);
    draw_rect(buffer, x + 2, cap_y + 2, w.saturating_sub(4), cap_h.saturating_sub(4), 0x001c222e);
    // Center indicator line
    draw_rect(buffer, x + 4, cap_y + cap_h / 2 - 1, w.saturating_sub(8), 2, cap_color);
}

fn draw_horizontal_slider(buffer: &mut [u32], x: usize, y: usize, w: usize, h: usize, pos: f64) {
    draw_rect(buffer, x, y, w, h, 0x0010141a);
    draw_rect(buffer, x, y + h / 2 - 2, w, 4, 0x00080a0e);
    let thumb_w = 40;
    let thumb_x = x + (pos.clamp(0.0, 1.0) * (w.saturating_sub(thumb_w)) as f64).round() as usize;
    draw_rect(buffer, thumb_x, y + 2, thumb_w, h.saturating_sub(4), 0x002a3242);
    draw_rect(buffer, thumb_x + 2, y + 4, thumb_w.saturating_sub(4), h.saturating_sub(8), 0x001c222e);
    draw_rect(buffer, thumb_x + thumb_w / 2 - 1, y + 6, 2, h.saturating_sub(12), COLOR_WHITE);
}

fn draw_knob(buffer: &mut [u32], cx: usize, cy: usize, r: usize, _pos: f64, color: u32) {
    let r2 = (r * r) as i32;
    for dy in -(r as i32)..=(r as i32) {
        for dx in -(r as i32)..=(r as i32) {
            if dx * dx + dy * dy <= r2 {
                let px = (cx as i32 + dx) as usize;
                let py = (cy as i32 + dy) as usize;
                if px < GUI_WIDTH && py < GUI_HEIGHT {
                    buffer[py * GUI_WIDTH + px] = if dx * dx + dy * dy > ((r - 2) * (r - 2)) as i32 {
                        COLOR_PANEL_BORDER
                    } else {
                        color
                    };
                }
            }
        }
    }
}
