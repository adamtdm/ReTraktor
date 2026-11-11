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

mod app;
mod audio;
mod controller_map;
mod demucs;
mod dot;
mod library_ui;
mod playlist_import;
mod preferences;
mod theme;
mod widgets;

use app::ReTraktorApp;

/// Screenshot mode can test other window sizes via SHOT_SIZE=WxH.
fn shot_window_size() -> Option<Vec2> {
    std::env::var("RETRAKTOR_SHOT_DIR").ok()?;
    let (w, h) = std::env::var("SHOT_SIZE").ok()?.split_once('x').map(|(w, h)| (w.parse().ok(), h.parse().ok()))?;
    Some(Vec2::new(w?, h?))
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ReTraktor")
            .with_inner_size(shot_window_size().unwrap_or(Vec2::new(1280.0, 820.0)))
            .with_min_inner_size(Vec2::new(1024.0, 640.0))
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "ReTraktor",
        native_options,
        Box::new(|cc| {
            theme::install_fonts(&cc.egui_ctx);
            Ok(Box::new(ReTraktorApp::default()))
        }),
    )
}
