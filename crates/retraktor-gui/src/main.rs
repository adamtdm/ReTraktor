//! # `retraktor-gui`
//!
//! Native hardware-accelerated desktop graphical user interface for ReTraktor
//! (clean-room Linux reimplementation of Native Instruments Traktor Pro 4).
//!
//! Provides 60 FPS parallel scrolling frequency waveforms, 4-channel DJ mixer,
//! rotary knobs, vertical faders, crossfader, harmonic KeyWheel browser,
//! Ozone Maximizer master limiter, and real-time audio playback via PipeWire / ALSA.

#![allow(dead_code)]

use eframe::egui::{self, Vec2};

mod app;
mod audio;
mod library_ui;
mod preferences;
mod theme;
mod widgets;

use app::ReTraktorApp;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("ReTraktor 4.5.1 — Native Instruments Traktor Pro 4 on Linux")
            .with_inner_size(Vec2::new(1280.0, 820.0))
            .with_min_inner_size(Vec2::new(1024.0, 640.0))
            .with_active(true),
        ..Default::default()
    };

    eframe::run_native(
        "ReTraktor Pro 4",
        native_options,
        Box::new(|_cc| Ok(Box::new(ReTraktorApp::default()))),
    )
}
