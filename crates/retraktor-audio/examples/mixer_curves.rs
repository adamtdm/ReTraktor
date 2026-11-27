//! Measures ReTraktor's mixer as a DJ hears it, for comparison with the same measurements of Traktor:
//! sine tones through a channel's filter and EQ (the code `FourDeckAudioEngine::process_frame` runs), and
//! the crossfader's gain law. Writes CSVs (gains in dB) to the directory given, default `mixer-curves/`.
//!
//! `cargo run -p retraktor-audio --example mixer_curves -- out/`

use retraktor_audio::FourDeckAudioEngine;
use retraktor_dsp::{BipolarFilter, MixerEqModel, ThreeBandIsolator};
use std::fmt::Write;

/// Traktor's Mix Recorder rate, so the two sets of curves line up to the last bin.
const SR: u32 = 44_100;
/// Log-spaced test tones, 20 Hz–20 kHz, three per octave.
fn freqs() -> Vec<f32> {
    (0..=30).map(|i| 20.0 * 2f32.powf(i as f32 / 3.0)).collect()
}
/// Knob positions 0.00, 0.05 … 1.00.
fn knobs() -> Vec<f32> {
    (0..=20).map(|i| i as f32 / 20.0).collect()
}

/// Gain in dB of a sine at `hz` through the channel's filter and EQ, after a second to settle.
fn gain_db(hz: f32, filter_knob: f32, eq: [f32; 3]) -> f32 {
    let mut filter = BipolarFilter::default();
    (filter.knob, filter.enabled) = (filter_knob, true);
    let model = MixerEqModel { low: eq[0], mid: eq[1], high: eq[2], ..Default::default() };
    let g = [model.low, model.mid, model.high].map(|k| model.knob_to_linear(k));
    let mut iso = ThreeBandIsolator::default();
    let (settle, measure) = (SR as usize, SR as usize / 2);
    let (mut in_sq, mut out_sq) = (0.0f64, 0.0f64);
    for n in 0..settle + measure {
        let x = 0.25 * (std::f32::consts::TAU * hz * n as f32 / SR as f32).sin();
        let (fl, fr) = filter.process_at(x, x, SR as f32, 120.0);
        let (y, _) = iso.process_stereo(SR, fl, fr, g[0], g[1], g[2]);
        if n >= settle {
            in_sq += (x * x) as f64;
            out_sq += (y * y) as f64;
        }
    }
    (10.0 * (out_sq.max(1e-20) / in_sq).log10()) as f32
}

fn main() -> std::io::Result<()> {
    let out = std::path::PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "mixer-curves".into()));
    std::fs::create_dir_all(&out)?;
    let fs = freqs();
    let header = |first: &str| format!("{first},{}\n", fs.iter().map(|f| format!("{f:.0}")).collect::<Vec<_>>().join(","));

    // Each EQ band's knob swept with the others at unity: rows are knob positions, columns frequencies.
    for (band, name) in ["low", "mid", "high"].iter().enumerate() {
        let mut csv = header("knob");
        for k in knobs() {
            let mut eq = [0.5; 3];
            eq[band] = k;
            let row: Vec<String> = fs.iter().map(|&f| format!("{:.2}", gain_db(f, 0.5, eq))).collect();
            writeln!(csv, "{k:.2},{}", row.join(",")).unwrap();
        }
        std::fs::write(out.join(format!("eq_{name}.csv")), csv)?;
    }

    // The filter knob: below 0.5 low-pass, above high-pass.
    let mut csv = header("knob");
    for k in knobs() {
        let row: Vec<String> = fs.iter().map(|&f| format!("{:.2}", gain_db(f, k, [0.5; 3]))).collect();
        writeln!(csv, "{k:.2},{}", row.join(",")).unwrap();
    }
    std::fs::write(out.join("filter.csv"), csv)?;

    // Crossfader: each side's gain and the two summed (as two uncorrelated tracks add in power) per curve.
    let mut csv = String::from("curve,position,left_db,right_db,sum_power_db\n");
    for curve in [0.0, 0.126, 0.252, 0.378, 0.504, 0.63, 0.748, 0.874, 1.0] {
        for i in 0..=40 {
            let mut e = FourDeckAudioEngine::default();
            e.crossfader = i as f32 / 40.0;
            e.crossfader_curve = curve;
            let (l, r) = e.crossfader_gains();
            let db = |g: f32| 20.0 * g.max(1e-6).log10();
            writeln!(csv, "{curve:.3},{:.3},{:.2},{:.2},{:.2}", e.crossfader, db(l), db(r), 10.0 * (l * l + r * r).max(1e-12).log10()).unwrap();
        }
    }
    std::fs::write(out.join("crossfader.csv"), csv)?;

    let mut csv = String::from("knob,gain_db\n");
    for k in knobs() {
        writeln!(csv, "{k:.2},{:.2}", (20.0 * FourDeckAudioEngine::fader_gain(k).max(1e-6).log10()).max(-90.0)).unwrap();
    }
    std::fs::write(out.join("fader.csv"), csv)?;
    eprintln!("wrote eq_low/mid/high.csv, filter.csv, crossfader.csv, fader.csv to {}", out.display());
    Ok(())
}
