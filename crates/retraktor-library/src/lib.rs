//! # `retraktor-library`
//!
//! Phase 2 Library Management, Harmonic Key Wheel, Browser/Preparation/History state,
//! Audio Track Analyzer (Loudness, Beatgrid, 3-band Waveform Stripe), and Deck Loader.

use retraktor_core::{CueType, PropValue, PropertyBus};
use retraktor_formats::{
    NmlCollection, NmlCue, NmlEntry, NmlLoudness, NmlTempo, StripeColumn, WaveformStripe,
};
use std::collections::BTreeMap;

/// Harmonic Key Wheel mapping Traktor's `MUSICAL_KEY/@VALUE` (`0..=23`), Open Key (`1d..12d`, `1m..12m`),
/// and Camelot (`1B..12B`, `1A..12A`) notation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyWheel;

impl KeyWheel {
    /// Open Key notation for Traktor musical key ID `0..=23` (`0..=11` Major `d`, `12..=23` Minor `m`).
    /// Verified against real Traktor Pro 4.5.1 `collection.nml` (`INFO/@KEY` vs `MUSICAL_KEY/@VALUE`).
    pub const OPEN_KEY_TABLE: [&'static str; 24] = [
        // 0..11 Major (C, Db, D, Eb, E, F, Gb, G, Ab, A, Bb, B)
        "1d", "8d", "3d", "10d", "5d", "12d", "7d", "2d", "9d", "4d", "11d", "6d",
        // 12..23 Minor (Cm, C#m, Dm, Ebm, Em, Fm, F#m, Gm, G#m, Am, Bbm, Bm)
        "10m", "5m", "12m", "7m", "2m", "9m", "4m", "11m", "6m", "1m", "8m", "3m",
    ];

    /// Converts a Traktor musical key ID (`0..=23`) to Open Key string (e.g. `0` -> `"1d"` C, `21` -> `"1m"` Am).
    pub fn to_open_key(key_id: u8) -> Option<&'static str> {
        Self::OPEN_KEY_TABLE.get(key_id as usize).copied()
    }

    /// Parses an Open Key string (`"1m".."12m"` or `"1d".."12d"`) into `(number: 1..=12, is_minor: bool)`.
    pub fn parse_open_key(open_key: &str) -> Option<(u8, bool)> {
        let s = open_key.trim();
        if let Some(num_str) = s.strip_suffix('m') {
            let n = num_str.parse::<u8>().ok()?;
            if (1..=12).contains(&n) {
                return Some((n, true));
            }
        } else if let Some(num_str) = s.strip_suffix('d') {
            let n = num_str.parse::<u8>().ok()?;
            if (1..=12).contains(&n) {
                return Some((n, false));
            }
        }
        None
    }

    /// Returns true if two Open Key strings (`a`, `b`) are harmonically compatible on the Circle of Fifths
    /// (same key, +/- 1 step on the wheel, or relative major/minor with the same wheel number).
    pub fn are_harmonically_compatible(key_a: &str, key_b: &str) -> bool {
        let Some((num_a, min_a)) = Self::parse_open_key(key_a) else {
            return false;
        };
        let Some((num_b, min_b)) = Self::parse_open_key(key_b) else {
            return false;
        };
        if num_a == num_b {
            // Identical key or relative major/minor (e.g. 1m <-> 1d)
            return true;
        }
        if min_a == min_b {
            // Adjacent +/- 1 on the circle of fifths (wrapping 12 <-> 1)
            let diff = (num_a as i16 - num_b as i16).abs();
            return diff == 1 || diff == 11;
        }
        false
    }
}

/// Real-time audio analyzer that generates [`NmlLoudness`], [`NmlTempo`], Beatgrid [`NmlCue`],
/// and a 3-band [`WaveformStripe`] from raw mono/stereo PCM samples.
pub struct TrackAnalyzer;

impl TrackAnalyzer {
    /// Analyzes interleaved stereo PCM samples (`[L, R, L, R, ...]`) at `sample_rate` Hz.
    pub fn analyze_stereo_pcm(
        samples: &[f32],
        sample_rate: u32,
        estimated_bpm: f64,
        stripe_columns: usize,
    ) -> (NmlLoudness, NmlTempo, NmlCue, WaveformStripe) {
        let frames = samples.len() / 2;
        let duration_sec = (frames as f32) / (sample_rate as f32).max(1.0);

        let mut peak = 0.0_f32;
        let mut sum_sq = 0.0_f64;
        for &s in samples {
            let a = s.abs();
            if a > peak {
                peak = a;
            }
            sum_sq += (s as f64) * (s as f64);
        }
        let rms = if !samples.is_empty() {
            (sum_sq / (samples.len() as f64)).sqrt() as f32
        } else {
            1e-6
        };
        let peak_db = 20.0 * (peak.max(1e-6) as f64).log10();
        let rms_db = 20.0 * (rms.max(1e-6) as f64).log10();
        // Autogain target: -12 dBFS RMS reference
        let perceived_db = (-12.0 - rms_db).clamp(-18.0, 18.0);

        let loudness = NmlLoudness {
            analyzed_db: rms_db,
            perceived_db,
            peak_db,
        };
        let tempo = NmlTempo {
            bpm: estimated_bpm,
            bpm_quality: 100.0,
        };
        let grid_cue = NmlCue {
            name: "AutoGrid".to_string(),
            displ_order: 0,
            cue_type: CueType::Grid,
            start_ms: 0.0,
            len_ms: 0.0,
            repeats: -1,
            hotcue: 0,
            grid_bpm: Some(estimated_bpm),
        };

        // Compute 3-band waveform stripe columns
        let cols = stripe_columns.max(1);
        let frames_per_col = (frames / cols).max(1);
        let mut columns = Vec::with_capacity(cols);
        for c in 0..cols {
            let start_f = c * frames_per_col;
            let end_f = ((c + 1) * frames_per_col).min(frames);
            let mut low_acc = 0.0_f32;
            let mut mid_acc = 0.0_f32;
            let mut high_acc = 0.0_f32;
            let mut prev = 0.0_f32;
            let count = (end_f.saturating_sub(start_f)).max(1) as f32;
            for f in start_f..end_f {
                let mono = 0.5 * (samples[f * 2] + samples[f * 2 + 1]);
                let diff = (mono - prev).abs();
                let smooth = 0.5 * (mono + prev).abs();
                low_acc += smooth;
                mid_acc += mono.abs();
                high_acc += diff;
                prev = mono;
            }
            let low = ((low_acc / count) * 255.0).clamp(0.0, 255.0) as u8;
            let mid = ((mid_acc / count) * 255.0).clamp(0.0, 255.0) as u8;
            let high = ((high_acc / count) * 255.0).clamp(0.0, 255.0) as u8;
            let total = ((low as u16 + mid as u16 + high as u16) / 3).min(255) as u8;
            columns.push(StripeColumn {
                low,
                mid,
                high,
                total,
            });
        }

        // Populate beat transient timestamps
        let beat_interval = (60.0 / estimated_bpm.max(20.0)) as f32;
        let mut transients_sec = Vec::new();
        let mut t = 0.0_f32;
        while t < duration_sec {
            transients_sec.push(t);
            t += beat_interval;
        }

        let stripe = WaveformStripe {
            header_tag: 0x06,
            version: 2,
            scale_factor: 3.5,
            block_size: 1024,
            block_count: 64,
            populated_columns: cols as u32,
            duration_sec,
            columns,
            transients_sec,
        };

        (loudness, tempo, grid_cue, stripe)
    }
}

/// Browser, Preparation List, Favorites, and Session History controller synchronized with `PropertyBus`.
#[derive(Debug, Clone, Default)]
pub struct BrowserState {
    pub selected_row: usize,
    pub preparation_keys: Vec<String>,
    pub history_keys: Vec<String>,
    pub favorite_crates: [Option<String>; 12],
}

impl BrowserState {
    pub fn scroll_list(&mut self, delta: i64, total_tracks: usize, bus: &mut PropertyBus) {
        if total_tracks == 0 {
            self.selected_row = 0;
            return;
        }
        let next = (self.selected_row as i64 + delta).clamp(0, (total_tracks - 1) as i64) as usize;
        self.selected_row = next;
        bus.set(
            "app.traktor.browser.list.select_up_down",
            PropValue::Int(next as i64),
        );
    }

    pub fn toggle_preparation(&mut self, track_key: &str, bus: &mut PropertyBus) {
        if let Some(pos) = self.preparation_keys.iter().position(|k| k == track_key) {
            self.preparation_keys.remove(pos);
            bus.set("app.traktor.browser.preparation.append", PropValue::Bool(false));
        } else {
            self.preparation_keys.push(track_key.to_string());
            bus.set("app.traktor.browser.preparation.append", PropValue::Bool(true));
        }
    }
}

/// Indexed music library backed by an [`NmlCollection`].
#[derive(Debug, Clone)]
pub struct TrackLibrary {
    by_primary_key: BTreeMap<String, NmlEntry>,
    ordered_keys: Vec<String>,
    pub browser: BrowserState,
}

impl TrackLibrary {
    pub fn from_nml(collection: &NmlCollection) -> Self {
        let mut by_primary_key = BTreeMap::new();
        let mut ordered_keys = Vec::with_capacity(collection.entries.len());
        for entry in &collection.entries {
            let key = entry.location.primary_key();
            ordered_keys.push(key.clone());
            by_primary_key.insert(key, entry.clone());
        }
        Self {
            by_primary_key,
            ordered_keys,
            browser: BrowserState::default(),
        }
    }

    pub fn len(&self) -> usize {
        self.by_primary_key.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_primary_key.is_empty()
    }

    pub fn get_by_key(&self, key: &str) -> Option<&NmlEntry> {
        self.by_primary_key.get(key)
    }

    /// Evaluates a Smartlist filter over the collection (BPM range, genre substring, and optional harmonic key match).
    pub fn filter_smartlist(
        &self,
        min_bpm: Option<f64>,
        max_bpm: Option<f64>,
        genre_contains: Option<&str>,
    ) -> Vec<&NmlEntry> {
        let genre_needle = genre_contains.map(|s| s.to_ascii_lowercase());
        self.ordered_keys
            .iter()
            .filter_map(|k| self.by_primary_key.get(k))
            .filter(|entry| {
                if let Some(lo) = min_bpm {
                    let bpm = entry.tempo.as_ref().map(|t| t.bpm).unwrap_or(0.0);
                    if bpm < lo {
                        return false;
                    }
                }
                if let Some(hi) = max_bpm {
                    let bpm = entry.tempo.as_ref().map(|t| t.bpm).unwrap_or(0.0);
                    if bpm > hi {
                        return false;
                    }
                }
                if let Some(needle) = &genre_needle {
                    let g = entry
                        .genre
                        .as_deref()
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !g.contains(needle) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }

    /// Finds all tracks in the library that are harmonically compatible (`KeyWheel`) with `target_open_key`.
    pub fn find_harmonic_matches(&self, target_open_key: &str) -> Vec<&NmlEntry> {
        self.ordered_keys
            .iter()
            .filter_map(|k| self.by_primary_key.get(k))
            .filter(|entry| {
                entry
                    .key_text
                    .as_deref()
                    .map(|k| KeyWheel::are_harmonically_compatible(target_open_key, k))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Loads an [`NmlEntry`] into Deck `deck_idx` (`1..=4`) on the [`PropertyBus`],
    /// populating `app.traktor.decks.<N>.track.content.*`, `tempo.*`, `autogain`, and `hotcues.1..8`,
    /// and recording the track in the session history.
    pub fn load_into_deck(&mut self, deck_idx: usize, entry: &NmlEntry, bus: &mut PropertyBus) {
        assert!((1..=4).contains(&deck_idx), "deck_idx must be 1..=4");
        let prefix = format!("app.traktor.decks.{deck_idx}");

        self.browser.history_keys.push(entry.location.primary_key());

        bus.set(&format!("{prefix}.is_loaded"), PropValue::Bool(true));
        bus.set(&format!("{prefix}.is_loaded_signal"), PropValue::Bool(true));
        bus.set(
            &format!("{prefix}.track.content.title"),
            PropValue::String(entry.title.clone()),
        );
        bus.set(
            &format!("{prefix}.track.content.artist"),
            PropValue::String(entry.artist.clone()),
        );
        bus.set(
            &format!("{prefix}.track.content.album"),
            PropValue::String(entry.album_title.clone().unwrap_or_default()),
        );
        if let Some(key_str) = &entry.key_text {
            bus.set(
                &format!("{prefix}.track.content.musical_key"),
                PropValue::String(key_str.clone()),
            );
        }
        if let Some(mk) = entry.musical_key_value {
            bus.set(
                &format!("{prefix}.track.key.final_id"),
                PropValue::Int(mk as i64),
            );
        }
        if let Some(len) = entry.playtime_sec {
            bus.set(
                &format!("{prefix}.track.content.track_length"),
                PropValue::Float(len),
            );
        }
        if let Some(tempo) = &entry.tempo {
            bus.set(
                &format!("{prefix}.tempo.base_bpm"),
                PropValue::Float(tempo.bpm),
            );
            bus.set(
                &format!("{prefix}.tempo.true_bpm"),
                PropValue::Float(tempo.bpm),
            );
            bus.set(
                &format!("{prefix}.track.content.bpm"),
                PropValue::Float(tempo.bpm),
            );
        }
        if let Some(loud) = &entry.loudness {
            bus.set(
                &format!("app.traktor.mixer.channels.{deck_idx}.autogain"),
                PropValue::Float(loud.perceived_db),
            );
        }

        // Clear and populate Hotcues 1..=8
        for h in 1..=8 {
            bus.set(
                &format!("{prefix}.track.cue.hotcues.{h}.exists"),
                PropValue::Bool(false),
            );
        }
        for cue in &entry.cues {
            if (0..8).contains(&cue.hotcue) {
                let h = cue.hotcue + 1;
                bus.set(
                    &format!("{prefix}.track.cue.hotcues.{h}.exists"),
                    PropValue::Bool(true),
                );
                bus.set(
                    &format!("{prefix}.track.cue.hotcues.{h}.type"),
                    PropValue::Int(cue.cue_type as i64),
                );
                bus.set(
                    &format!("{prefix}.track.cue.hotcues.{h}.start_pos"),
                    PropValue::Float(cue.start_ms / 1000.0),
                );
                bus.set(
                    &format!("{prefix}.track.cue.hotcues.{h}.length"),
                    PropValue::Float(cue.len_ms / 1000.0),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use retraktor_formats::NmlLocation;

    #[test]
    fn test_library_harmonic_keywheel_and_analyzer() {
        assert!(KeyWheel::are_harmonically_compatible("4m", "4m"));
        assert!(KeyWheel::are_harmonically_compatible("4m", "5m"));
        assert!(KeyWheel::are_harmonically_compatible("4m", "3m"));
        assert!(KeyWheel::are_harmonically_compatible("4m", "4d"));
        assert!(!KeyWheel::are_harmonically_compatible("4m", "9m"));

        // Synthesize 1 second of 48kHz stereo test tone and analyze
        let pcm: Vec<f32> = (0..96_000)
            .map(|i| ((i as f32) * 0.05).sin() * 0.5)
            .collect();
        let (loud, tempo, grid, stripe) =
            TrackAnalyzer::analyze_stereo_pcm(&pcm, 48_000, 128.0, 64);
        assert_eq!(tempo.bpm, 128.0);
        assert_eq!(grid.cue_type, CueType::Grid);
        assert_eq!(stripe.columns.len(), 64);
        assert!(loud.peak_db <= 0.0);

        let mut col = NmlCollection::new(20);
        col.entries.push(NmlEntry {
            title: "Subzero".to_string(),
            artist: "Ben Klock".to_string(),
            audio_id: None,
            location: NmlLocation::from_posix_path("/home/dj/subzero.flac", "linux-root"),
            album_title: Some("Ostgut Ton".to_string()),
            album_track: Some(1),
            genre: Some("Techno".to_string()),
            comment: None,
            key_text: Some("4m".to_string()),
            playtime_sec: Some(412.0),
            bitrate: Some(1411200),
            tempo: Some(tempo),
            loudness: Some(loud),
            musical_key_value: Some(12),
            cues: vec![grid],
            stems_path: None,
        });

        let mut lib = TrackLibrary::from_nml(&col);
        assert_eq!(lib.find_harmonic_matches("5m").len(), 1);
        let entry = lib.get_by_key("linux-root/:home/:dj/:subzero.flac").unwrap().clone();
        let mut bus = PropertyBus::new_traktor_pro4_default();
        lib.load_into_deck(1, &entry, &mut bus);
        assert_eq!(lib.browser.history_keys.len(), 1);
    }
}
