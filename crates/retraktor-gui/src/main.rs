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
mod demucs;
mod library_ui;
mod preferences;
mod theme;
mod widgets;

use app::ReTraktorApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ReTraktor")
            .with_inner_size(Vec2::new(1280.0, 820.0))
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
