//! # `retraktor-gui`
//!
//! Native hardware-accelerated desktop graphical user interface for ReTraktor
//! (clean-room Linux reimplementation of Traktor Pro 4).
//!
//! Provides 60 FPS parallel scrolling frequency waveforms, 4-channel DJ mixer,
//! rotary knobs, vertical faders, crossfader, harmonic KeyWheel browser,
//! Ozone Maximizer master limiter, and real-time audio playback via PipeWire / ALSA.

#![allow(dead_code)]

use eframe::egui::{self, Vec2};

mod ai;
mod app;
mod audio;
mod controller_map;
mod rules_file;
mod demucs;
mod dot;
mod keylock;
mod library_ui;
mod overview;
mod playlist_import;
pub mod midi;
mod preferences;
mod recorder;
pub mod setlist;
mod theme;
mod widgets;

use app::ReTraktorApp;

/// Screenshot mode can test other window sizes via SHOT_SIZE=WxH.
fn shot_window_size() -> Option<Vec2> {
    std::env::var("RETRAKTOR_SHOT_DIR").ok()?;
    let (w, h) = std::env::var("SHOT_SIZE").ok()?.split_once('x').map(|(w, h)| (w.parse().ok(), h.parse().ok()))?;
    Some(Vec2::new(w?, h?))
}

/// Decodes the embedded 256×256 PNG icon for the window/dock icon on X11 & Wayland.
fn load_app_icon() -> Option<egui::IconData> {
    let bytes: &[u8] = include_bytes!("../../../assets/retraktor.png");
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::ALPHA);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some(egui::IconData { rgba: buf, width: info.width, height: info.height })
}

fn main() -> eframe::Result<()> {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("ReTraktor")
        .with_app_id("retraktor")
        .with_inner_size(shot_window_size().unwrap_or(Vec2::new(1280.0, 820.0)))
        .with_min_inner_size(Vec2::new(1024.0, 640.0))
        .with_active(true);
    if let Some(icon) = load_app_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let native_options = eframe::NativeOptions { viewport, ..Default::default() };

    eframe::run_native(
        "ReTraktor",
        native_options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            if let Err(e) = theme::remember_first_overview_look() {
                eprintln!("retraktor: could not save the overview look: {e}");
            }
            // egui moves 40 pt per wheel notch (under 2 library rows); desktop apps move about 3 lines.
            cc.egui_ctx.options_mut(|o| o.line_scroll_speed = 3.0 * 22.0);
            Ok(Box::new(ReTraktorApp::default()))
        }),
    )
}
