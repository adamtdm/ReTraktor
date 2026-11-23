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

/// The UI context, for threads that receive controller input to wake a throttled UI.
static UI_CTX: std::sync::OnceLock<egui::Context> = std::sync::OnceLock::new();

/// Asks for a redraw now; a no-op before the window exists (and in tests).
pub fn wake_ui() {
    if let Some(ctx) = UI_CTX.get() {
        ctx.request_repaint();
    }
}

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

/// Extracts `.nml` paths from CLI arguments, accepting plain paths and `file://` URIs from file managers.
fn nml_paths_from_args(args: impl IntoIterator<Item = String>) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for raw in args {
        if raw.starts_with('-') {
            continue;
        }
        let path_str = match raw.strip_prefix("file://") {
            Some(rest) => {
                let rest = rest.strip_prefix("localhost").unwrap_or(rest);
                percent_decode_path(rest)
            }
            None => raw,
        };
        let p = std::path::PathBuf::from(path_str);
        if p.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("nml")) {
            out.push(p);
        }
    }
    out
}

fn percent_decode_path(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = std::str::from_utf8(&bytes[i + 1..i + 3]) {
                if let Ok(val) = u8::from_str_radix(hex, 16) {
                    out.push(val);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn main() -> eframe::Result<()> {
    // Before any thread starts: PipeWire must play our channels as sent (main on 1–2, headphones on 3–4). Left to
    // remix, it folds the cue pair into the speakers on a stereo device and copies the main mix into the headphones
    // on a 4-out one, so both decks are heard whatever the crossfader says. A user's own PIPEWIRE_PROPS wins.
    if std::env::var_os("PIPEWIRE_PROPS").is_none() {
        std::env::set_var("PIPEWIRE_PROPS", "{ channelmix.disable = true stream.dont-remix = true }");
    }
    let nml_files = nml_paths_from_args(std::env::args().skip(1));
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
        Box::new(move |cc| {
            let _ = UI_CTX.set(cc.egui_ctx.clone());
            theme::install_fonts(&cc.egui_ctx);
            if let Err(e) = theme::remember_first_overview_look() {
                eprintln!("retraktor: could not save the overview look: {e}");
            }
            // egui moves 40 pt per wheel notch (under 2 library rows); desktop apps move about 3 lines.
            cc.egui_ctx.options_mut(|o| o.line_scroll_speed = 3.0 * 22.0);
            let mut app = ReTraktorApp::default();
            app.open_nml_files(nml_files);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn nml_paths_from_args_handles_plain_uppercase_and_file_uris() {
        let got = nml_paths_from_args([
            "--flag".into(),
            "/tmp/other.mp3".into(),
            "/music/collection.nml".into(),
            "/music/SET.NML".into(),
            "file:///home/dj/My%20Set%20%231.nml".into(),
            "file://localhost/tmp/cafe%CC%81.nml".into(),
        ]);
        assert_eq!(
            got,
            vec![
                std::path::PathBuf::from("/music/collection.nml"),
                std::path::PathBuf::from("/music/SET.NML"),
                std::path::PathBuf::from("/home/dj/My Set #1.nml"),
                std::path::PathBuf::from("/tmp/cafe\u{0301}.nml"),
            ]
        );
    }
}
