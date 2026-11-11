//! Set list workflow: Build → Perform → Record.
//!
//! - **Build**: Sequence tracks, check harmonic flow warnings (key clashes, tempo jumps, energy drops),
//!   greedy reorder for optimal flow, and visualize the set flow in the dot-matrix arc display.
//! - **Perform**: Next Up strip on the Mix view, tracking played tracks (≥ 30s playback) and
//!   one-key loading (`Shift+N`) to the idle deck.
//! - **Record**: Timestamped recordings (`ReTraktor_YYYY-MM-DD_HH-MM.wav`), logging audible tracks
//!   and generating `.cue` and `.txt` tracklists alongside the audio.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2};
use retraktor_library::KeyWheel;

use crate::dot::{self, SetArcTrack};
use crate::library_ui::{LibraryManager, UiTrackEntry};
use crate::theme::{self, TraktorTheme};

const HIT: f32 = 24.0;

fn panel(fill: Color32) -> egui::Frame {
    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR()))
        .rounding(4.0)
        .inner_margin(egui::Margin::symmetric(8.0, 6.0))
}

/// Set list persistent model. Saved to `~/.config/retraktor/set.txt`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetList {
    pub name: String,
    pub track_ids: Vec<usize>,
}

impl Default for SetList {
    fn default() -> Self {
        Self {
            name: "My Set".to_string(),
            track_ids: Vec::new(),
        }
    }
}

pub fn default_set_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".config/retraktor/set.txt")
}

impl SetList {
    /// Loads the set list from `~/.config/retraktor/set.txt` or returns a default instance.
    pub fn load() -> Self {
        let path = default_set_path();
        if let Ok(text) = fs::read_to_string(&path) {
            Self::load_from_str(&text)
        } else {
            Self::default()
        }
    }

    /// Parses set list format:
    /// First line: set name (optional prefix `name:` or `#`).
    /// Subsequent lines: one track id (usize) per line.
    /// Tolerate missing, blank, or non-numeric lines.
    pub fn load_from_str(text: &str) -> Self {
        let mut lines = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
        let mut name = "My Set".to_string();
        let mut track_ids = Vec::new();

        if let Some(first) = lines.next() {
            let stripped = first
                .strip_prefix("name:")
                .or_else(|| first.strip_prefix("NAME:"))
                .or_else(|| first.strip_prefix('#'))
                .map(|s| s.trim())
                .unwrap_or(first);

            if let Ok(id) = stripped.parse::<usize>() {
                track_ids.push(id);
            } else if !stripped.is_empty() {
                name = stripped.to_string();
            }
        }

        for line in lines {
            if line.starts_with('#') {
                continue;
            }
            if let Ok(id) = line.parse::<usize>() {
                track_ids.push(id);
            }
        }

        Self { name, track_ids }
    }

    /// Serializes to the standard set file format.
    pub fn to_text(&self) -> String {
        let mut out = format!("{}\n", self.name);
        for id in &self.track_ids {
            out.push_str(&format!("{id}\n"));
        }
        out
    }

    /// Saves the set list to disk, creating parent directories if needed.
    pub fn save(&self) -> std::io::Result<()> {
        let path = default_set_path();
        self.save_to_path(&path)
    }

    pub fn save_to_path(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, self.to_text())
    }
}

/// Transition flow warning between two consecutive tracks in a set list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowWarning {
    /// Incompatible harmonic keys according to the Open Key / KeyWheel circle of fifths.
    KeyClash,
    /// Tempo jump exceeding 6% relative difference (after half/double tempo folding).
    TempoJump,
    /// Sudden energy drop of 3 or more (e.g. 8 down to 5).
    EnergyDrop,
}

pub type Warning = FlowWarning;

impl FlowWarning {
    pub fn label(self) -> &'static str {
        match self {
            Self::KeyClash => "Key clash",
            Self::TempoJump => "Tempo jump (>6%)",
            Self::EnergyDrop => "Energy drop (≥3)",
        }
    }
}

/// Pure helper: relative tempo difference after folding half/double tempo (e.g. 70 bpm ≈ 140 bpm).
pub fn folded_bpm_diff(bpm_a: f64, bpm_b: f64) -> Option<f64> {
    if bpm_a <= 0.0 || bpm_b <= 0.0 {
        return None;
    }
    let mut ratio = bpm_b / bpm_a;
    while ratio < 0.75 {
        ratio *= 2.0;
    }
    while ratio > 1.5 {
        ratio /= 2.0;
    }
    Some((ratio - 1.0).abs())
}

/// Computes harmonic and flow warnings for transitions between consecutive tracks.
/// Returns a list of `(transition_index, warning)` tuples, where `transition_index`
/// corresponds to the track transitioning to `transition_index + 1`.
pub fn flow_warnings(tracks: &[UiTrackEntry]) -> Vec<(usize, FlowWarning)> {
    let mut warnings = Vec::new();
    if tracks.len() < 2 {
        return warnings;
    }

    for i in 0..tracks.len() - 1 {
        let a = &tracks[i];
        let b = &tracks[i + 1];

        // 1. Key clash: only check if both keys are known / non-empty
        if !a.musical_key.is_empty() && !b.musical_key.is_empty() {
            let key_a = KeyWheel::normalize(&a.musical_key)
                .and_then(KeyWheel::to_open_key)
                .unwrap_or(&a.musical_key);
            let key_b = KeyWheel::normalize(&b.musical_key)
                .and_then(KeyWheel::to_open_key)
                .unwrap_or(&b.musical_key);

            if !KeyWheel::are_harmonically_compatible(key_a, key_b) {
                warnings.push((i, FlowWarning::KeyClash));
            }
        }

        // 2. Tempo jump over 6% after folding half/double tempo
        if let Some(diff) = folded_bpm_diff(a.bpm, b.bpm) {
            if diff > 0.06 {
                warnings.push((i, FlowWarning::TempoJump));
            }
        }

        // 3. Energy drop of 3 or more (when both tracks have energy ratings > 0)
        if a.energy > 0 && b.energy > 0 && a.energy >= b.energy + 3 {
            warnings.push((i, FlowWarning::EnergyDrop));
        }
    }

    warnings
}

/// Transition cost heuristic from track `a` to `b` (lower is smoother).
fn transition_cost(a: &UiTrackEntry, b: &UiTrackEntry) -> f64 {
    let mut cost = 0.0;

    // Harmonic key cost
    if !a.musical_key.is_empty() && !b.musical_key.is_empty() {
        let key_a = KeyWheel::normalize(&a.musical_key)
            .and_then(KeyWheel::to_open_key)
            .unwrap_or(&a.musical_key);
        let key_b = KeyWheel::normalize(&b.musical_key)
            .and_then(KeyWheel::to_open_key)
            .unwrap_or(&b.musical_key);

        if KeyWheel::are_harmonically_compatible(key_a, key_b) {
            cost += 0.0;
        } else {
            cost += 2.5;
        }
    } else {
        cost += 0.5;
    }

    // Tempo cost
    if let Some(diff) = folded_bpm_diff(a.bpm, b.bpm) {
        cost += (diff / 0.06).min(3.0) * 1.5;
    }

    // Energy cost: bias towards gently rising energy (+0 to +2)
    if a.energy > 0 && b.energy > 0 {
        let diff = b.energy as f64 - a.energy as f64;
        if diff < 0.0 {
            // Energy drop penalty
            cost += (-diff) * 1.5;
        } else if diff > 3.0 {
            // Sudden large jump penalty
            cost += (diff - 2.0) * 0.8;
        } else {
            // Gentle rise: optimal
            cost += 0.0;
        }
    }

    cost
}

/// Greedy reorder keeping the first track in place, minimising key/tempo/energy
/// transition cost between neighbours with a preference for gently rising energy.
/// Returns the reordered track IDs (or track slice indices if all IDs are 0).
pub fn order_for_flow(tracks: &[UiTrackEntry]) -> Vec<usize> {
    if tracks.is_empty() {
        return Vec::new();
    }
    if tracks.len() == 1 {
        let all_zero = tracks[0].id == 0;
        return vec![if all_zero { 0 } else { tracks[0].id }];
    }

    let all_zero = tracks.iter().all(|t| t.id == 0);
    let mut order_indices = Vec::with_capacity(tracks.len());
    let mut remaining: Vec<usize> = (1..tracks.len()).collect();

    order_indices.push(0);

    while !remaining.is_empty() {
        let current_idx = *order_indices.last().unwrap();
        let current = &tracks[current_idx];

        let mut best_pos = 0;
        let mut best_cost = f64::INFINITY;

        for (pos, &cand_idx) in remaining.iter().enumerate() {
            let cand = &tracks[cand_idx];
            let c = transition_cost(current, cand);
            if c < best_cost {
                best_cost = c;
                best_pos = pos;
            }
        }

        let next_idx = remaining.remove(best_pos);
        order_indices.push(next_idx);
    }

    if all_zero {
        order_indices
    } else {
        order_indices.iter().map(|&idx| tracks[idx].id).collect()
    }
}

/// Total playback duration in seconds for a list of tracks.
pub fn total_duration(tracks: &[UiTrackEntry]) -> f64 {
    tracks.iter().map(|t| t.duration_sec).sum()
}

/// Formats duration into human-readable `H:MM:SS` or `MM:SS`.
pub fn format_duration(sec: f64) -> String {
    let s = sec.round().max(0.0) as u64;
    let h = s / 3600;
    let m = (s % 3600) / 60;
    let rem_s = s % 60;
    if h > 0 {
        format!("{h}:{m:02}:{rem_s:02}")
    } else {
        format!("{m:02}:{rem_s:02}")
    }
}

/// Standard CD CUE sheet generation at 75 frames per second.
/// Format:
/// ```text
/// FILE "filename.wav" WAVE
///   TRACK 01 AUDIO
///     TITLE "Track Title"
///     PERFORMER "Artist Name"
///     INDEX 01 mm:ss:ff
/// ```
pub fn cue_sheet(entries: &[(f64, &str, &str)], wav_file_name: &str) -> String {
    let wav_base = Path::new(wav_file_name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(wav_file_name);

    let mut out = format!("FILE \"{wav_base}\" WAVE\n");
    for (i, &(start_sec, artist, title)) in entries.iter().enumerate() {
        let s = start_sec.max(0.0);
        let min = (s / 60.0).floor() as u32;
        let sec = (s % 60.0).floor() as u32;
        let frames = (((s % 1.0) * 75.0).round() as u32).min(74);

        out.push_str(&format!("  TRACK {:02} AUDIO\n", i + 1));
        out.push_str(&format!("    TITLE \"{title}\"\n"));
        out.push_str(&format!("    PERFORMER \"{artist}\"\n"));
        out.push_str(&format!("    INDEX 01 {min:02}:{sec:02}:{frames:02}\n"));
    }
    out
}

/// Plain text tracklist companion file.
pub fn plain_tracklist(entries: &[(f64, &str, &str)]) -> String {
    let mut out = String::new();
    for (i, &(start_sec, artist, title)) in entries.iter().enumerate() {
        let s = start_sec.max(0.0);
        let min = (s / 60.0).floor() as u32;
        let sec = (s % 60.0).floor() as u32;
        out.push_str(&format!("{:02}. [{min:02}:{sec:02}] {artist} — {title}\n", i + 1));
    }
    out
}

/// Computes (year, month, day, hour, minute) in UTC from Unix epoch seconds.
/// Howard Hinnant's Euclidean affine civil date algorithm (standard in C++20 / libc).
/// UTC is used to avoid external datetime dependencies.
pub fn civil_datetime_from_secs(secs: u64) -> (i32, u32, u32, u32, u32) {
    let sec_in_day = secs % 86400;
    let hour = (sec_in_day / 3600) as u32;
    let minute = ((sec_in_day % 3600) / 60) as u32;

    let days = (secs / 86400) as i64;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d, hour, minute)
}

/// Formats UTC SystemTime as `YYYY-MM-DD_HH-MM`.
pub fn format_recording_timestamp(time: SystemTime) -> String {
    let secs = time.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let (y, m, d, h, min) = civil_datetime_from_secs(secs);
    format!("{y:04}-{m:02}-{d:02}_{h:02}-{min:02}")
}

/// Finds non-colliding recording paths for WAV, CUE, and TXT files.
/// Never overwrites existing files.
pub fn unique_recording_paths(dir: &Path, time: SystemTime) -> (PathBuf, PathBuf, PathBuf) {
    let base_name = format!("ReTraktor_{}", format_recording_timestamp(time));
    let mut candidate = base_name.clone();
    let mut suffix = 1;

    while dir.join(format!("{candidate}.wav")).exists()
        || dir.join(format!("{candidate}.cue")).exists()
        || dir.join(format!("{candidate}.txt")).exists()
    {
        candidate = format!("{base_name}_{suffix}");
        suffix += 1;
    }

    (
        dir.join(format!("{candidate}.wav")),
        dir.join(format!("{candidate}.cue")),
        dir.join(format!("{candidate}.txt")),
    )
}

/// Live interactive state for the Set list workflow.
pub struct SetUiState {
    pub set_list: SetList,
    pub selected_index: Option<usize>,
    pub undo_order: Option<Vec<usize>>,
    pub played_track_ids: HashSet<usize>,
    pub deck_loaded_track_id: [Option<usize>; 4],
    pub deck_play_duration: [f32; 4],
    pub recorded_audible_tracks: Vec<(f64, String, String)>,
    pub recording_start: Option<std::time::Instant>,
    pub recording_samples_start_len: usize,
}

impl Default for SetUiState {
    fn default() -> Self {
        Self {
            set_list: SetList::load(),
            selected_index: None,
            undo_order: None,
            played_track_ids: HashSet::new(),
            deck_loaded_track_id: [None; 4],
            deck_play_duration: [0.0; 4],
            recorded_audible_tracks: Vec::new(),
            recording_start: None,
            recording_samples_start_len: 0,
        }
    }
}

impl SetUiState {
    /// Resets deck play duration when a new track is loaded.
    pub fn on_track_loaded(&mut self, deck_idx: usize, track_id: usize) {
        if deck_idx < 4 {
            if self.deck_loaded_track_id[deck_idx] != Some(track_id) {
                self.deck_loaded_track_id[deck_idx] = Some(track_id);
                self.deck_play_duration[deck_idx] = 0.0;
            }
        }
    }

    /// Updates playback duration per deck and marks tracks as played after 30 seconds.
    pub fn update_playback(&mut self, deck_idx: usize, is_playing: bool, dt: f32) {
        if deck_idx < 4 && is_playing {
            self.deck_play_duration[deck_idx] += dt;
            if self.deck_play_duration[deck_idx] >= 30.0 {
                if let Some(id) = self.deck_loaded_track_id[deck_idx] {
                    self.played_track_ids.insert(id);
                }
            }
        }
    }

    /// Resolves `track_ids` to existing `UiTrackEntry` references in the library.
    pub fn resolved_tracks<'a>(&self, library: &'a LibraryManager) -> Vec<UiTrackEntry> {
        self.set_list
            .track_ids
            .iter()
            .filter_map(|&id| library.track(id).cloned())
            .collect()
    }

    /// Finds the next unplayed track in the set list.
    /// Returns `(1-based index, total_count, track)`.
    pub fn next_unplayed_track(&self, library: &LibraryManager) -> Option<(usize, usize, UiTrackEntry)> {
        let total = self.set_list.track_ids.len();
        if total == 0 {
            return None;
        }

        for (i, &id) in self.set_list.track_ids.iter().enumerate() {
            if !self.played_track_ids.contains(&id) {
                if let Some(t) = library.track(id) {
                    return Some((i + 1, total, t.clone()));
                }
            }
        }
        None
    }

    /// Logs a track if it becomes audible during recording.
    pub fn check_audible_track(
        &mut self,
        deck_idx: usize,
        is_playing: bool,
        channel_vol: f32,
        crossfader_pos: f32,
        artist: &str,
        title: &str,
        offset_sec: f64,
    ) {
        if !is_playing || channel_vol <= 0.05 || artist.is_empty() && title.is_empty() {
            return;
        }

        // Crossfader check: 0.0 is full Deck A, 1.0 is full Deck B
        let audible = match deck_idx {
            0 => crossfader_pos < 0.95,
            1 => crossfader_pos > 0.05,
            _ => true,
        };

        if audible {
            // Avoid duplicate consecutive logging
            let is_duplicate = self
                .recorded_audible_tracks
                .last()
                .map(|(_, a, t)| a == artist && t == title)
                .unwrap_or(false);

            if !is_duplicate {
                self.recorded_audible_tracks.push((
                    offset_sec.max(0.0),
                    artist.to_string(),
                    title.to_string(),
                ));
            }
        }
    }

    /// Adds track IDs to the set list.
    pub fn add_tracks(&mut self, ids: &[usize]) {
        for &id in ids {
            if !self.set_list.track_ids.contains(&id) {
                self.set_list.track_ids.push(id);
            }
        }
        let _ = self.set_list.save();
    }

    /// Handles keyboard navigation and shortcuts for the SET layout.
    pub fn handle_keys(&mut self, ctx: &egui::Context, library: &LibraryManager, idle_deck: usize) -> Option<(usize, usize)> {
        let mut load_action = None;
        if ctx.wants_keyboard_input() {
            return None;
        }

        let len = self.set_list.track_ids.len();
        if len == 0 {
            return None;
        }

        let (up, down, alt, del, key_o, ctrl_z, enter) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
                i.modifiers.alt,
                i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                i.key_pressed(egui::Key::O),
                i.modifiers.command && i.key_pressed(egui::Key::Z),
                i.key_pressed(egui::Key::Enter),
            )
        });

        if let Some(idx) = self.selected_index {
            if alt && up && idx > 0 {
                self.set_list.track_ids.swap(idx, idx - 1);
                self.selected_index = Some(idx - 1);
                let _ = self.set_list.save();
            } else if alt && down && idx + 1 < len {
                self.set_list.track_ids.swap(idx, idx + 1);
                self.selected_index = Some(idx + 1);
                let _ = self.set_list.save();
            } else if !alt && up {
                self.selected_index = Some(idx.saturating_sub(1));
            } else if !alt && down {
                self.selected_index = Some((idx + 1).min(len - 1));
            } else if del {
                self.set_list.track_ids.remove(idx);
                self.selected_index = if self.set_list.track_ids.is_empty() {
                    None
                } else {
                    Some(idx.min(self.set_list.track_ids.len() - 1))
                };
                let _ = self.set_list.save();
            } else if enter {
                if let Some(&id) = self.set_list.track_ids.get(idx) {
                    load_action = Some((idle_deck, id));
                }
            }
        } else if down || up {
            self.selected_index = Some(0);
        }

        // O = order for best flow (or toggle undo)
        if key_o {
            if let Some(prev) = self.undo_order.take() {
                self.set_list.track_ids = prev;
                let _ = self.set_list.save();
            } else {
                let resolved = self.resolved_tracks(library);
                self.undo_order = Some(self.set_list.track_ids.clone());
                self.set_list.track_ids = order_for_flow(&resolved);
                let _ = self.set_list.save();
            }
        } else if ctrl_z {
            if let Some(prev) = self.undo_order.take() {
                self.set_list.track_ids = prev;
                let _ = self.set_list.save();
            }
        }

        load_action
    }

    /// Renders the PERFORM "NEXT UP" slim strip on the Mix view.
    pub fn show_next_up_strip(&mut self, ui: &mut Ui, library: &mut LibraryManager, idle_deck: usize) {
        if self.set_list.track_ids.is_empty() {
            return;
        }

        let next_track = self.next_unplayed_track(library);

        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            ui.set_height(24.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);

                if let Some((idx, total, t)) = next_track {
                    let next_label = format!("NEXT {:02}/{:02}", idx, total);
                    ui.label(
                        RichText::new(&next_label)
                            .color(TraktorTheme::ACCENT())
                            .font(theme::dot_font(11.0))
                            .strong(),
                    );

                    let title_text = format!("{} — {}", t.artist, t.title);
                    ui.label(
                        RichText::new(title_text)
                            .color(TraktorTheme::TEXT_PRIMARY())
                            .font(theme::dot_font(11.0)),
                    );

                    let bpm_str = if t.bpm > 0.0 { format!("{:.1}", t.bpm) } else { "--.-".to_string() };
                    ui.label(
                        RichText::new(bpm_str)
                            .color(TraktorTheme::PLAY_ACTIVE())
                            .font(theme::dot_font(11.0)),
                    );

                    if !t.musical_key.is_empty() {
                        ui.label(
                            RichText::new(&t.musical_key)
                                .color(TraktorTheme::TEXT_HIGHLIGHT())
                                .font(theme::dot_font(11.0)),
                        );
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let deck_letter = if idle_deck == 1 { "A" } else { "B" };
                        let btn_label = format!("▶ Load Deck {deck_letter} (Shift+N)");
                        if ui.add(
                            egui::Button::new(
                                RichText::new(btn_label)
                                    .color(TraktorTheme::TEXT_PRIMARY())
                                    .size(11.0),
                            )
                            .fill(TraktorTheme::CONTROL())
                            .min_size(Vec2::new(0.0, 20.0)),
                        )
                        .on_hover_text(format!("Load {} to Deck {} (Shift+N)", t.title, deck_letter))
                        .clicked()
                        {
                            library.load_to_deck(idle_deck, t.id, [false; 4]);
                        }
                    });
                } else {
                    ui.label(
                        RichText::new("SET COMPLETE · ALL TRACKS PLAYED")
                            .color(TraktorTheme::TEXT_MUTED())
                            .font(theme::dot_font(11.0)),
                    );
                }
            });
        });
    }

    /// Renders the complete BUILD workspace view (Set list + Arc display + Running time).
    pub fn show_set_view(&mut self, ui: &mut Ui, library: &mut LibraryManager, idle_deck: usize) {
        let avail_h = ui.available_height().max(240.0);
        let top_h = (avail_h * 0.48).clamp(180.0, 260.0);

        let resolved = self.resolved_tracks(library);
        let warnings = flow_warnings(&resolved);
        let total_sec = total_duration(&resolved);

        panel(TraktorTheme::BG_PANEL()).show(ui, |ui| {
            ui.set_height(top_h);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 0.0);
                let list_w = (ui.available_width() * 0.52).clamp(320.0, 680.0);
                let arc_w = (ui.available_width() - list_w - 24.0).max(200.0);

                // --- Left Column: Set List Rows ---
                ui.allocate_ui_with_layout(Vec2::new(list_w, top_h), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(list_w);

                    // Header & Toolbar
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&self.set_list.name)
                                .color(TraktorTheme::TEXT_PRIMARY())
                                .font(theme::dot_font(13.0))
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format!("({} tracks)", self.set_list.track_ids.len()))
                                .color(TraktorTheme::TEXT_MUTED())
                                .font(egui::FontId::proportional(11.0)),
                        );

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(
                                    RichText::new("Clear")
                                        .color(TraktorTheme::TEXT_MUTED())
                                        .size(11.0),
                                )
                                .fill(TraktorTheme::CONTROL())
                                .min_size(Vec2::new(0.0, HIT)),
                            )
                            .on_hover_text("Clear entire set list")
                            .clicked()
                            {
                                self.set_list.track_ids.clear();
                                self.selected_index = None;
                                let _ = self.set_list.save();
                            }

                            let order_label = if self.undo_order.is_some() { "Undo Order (O)" } else { "Order Flow (O)" };
                            if ui.add(
                                egui::Button::new(
                                    RichText::new(order_label)
                                        .color(TraktorTheme::ACCENT())
                                        .size(11.0),
                                )
                                .fill(TraktorTheme::CONTROL())
                                .min_size(Vec2::new(0.0, HIT)),
                            )
                            .on_hover_text("Greedy reorder keeping first track, minimising key/tempo/energy clash (O)")
                            .clicked()
                            {
                                if let Some(prev) = self.undo_order.take() {
                                    self.set_list.track_ids = prev;
                                    let _ = self.set_list.save();
                                } else {
                                    self.undo_order = Some(self.set_list.track_ids.clone());
                                    self.set_list.track_ids = order_for_flow(&resolved);
                                    let _ = self.set_list.save();
                                }
                            }
                        });
                    });

                    ui.add_space(4.0);
                    ui.separator();

                    // Scrollable Track Rows
                    let table_h = top_h - 44.0;
                    egui::ScrollArea::vertical()
                        .id_salt("setlist_track_rows")
                        .max_height(table_h)
                        .show(ui, |ui| {
                            if resolved.is_empty() {
                                ui.add_space(24.0);
                                ui.vertical_centered(|ui| {
                                    ui.label(
                                        RichText::new("Set list is empty")
                                            .color(TraktorTheme::TEXT_MUTED())
                                            .font(theme::dot_font(12.0)),
                                    );
                                    ui.label(
                                        RichText::new("Select tracks in the library below and press Shift+A or right-click to add")
                                            .color(TraktorTheme::TEXT_SECONDARY())
                                            .font(egui::FontId::proportional(11.0)),
                                    );
                                });
                                return;
                            }

                            for (i, track) in resolved.iter().enumerate() {
                                let is_sel = self.selected_index == Some(i);
                                let is_played = self.played_track_ids.contains(&track.id);

                                let row_bg = if is_sel {
                                    TraktorTheme::ROW_SELECTED()
                                } else if is_played {
                                    TraktorTheme::BG_PANEL_ALT()
                                } else {
                                    Color32::TRANSPARENT
                                };

                                let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
                                if ui.is_rect_visible(rect) {
                                    let painter = ui.painter();
                                    if row_bg != Color32::TRANSPARENT {
                                        painter.rect_filled(rect, 2.0, row_bg);
                                    }

                                    // Columns: # | Title | Artist | BPM | Key | Energy | Time | Warn
                                    let num_str = format!("{:02}", i + 1);
                                    let text_col = if is_sel {
                                        TraktorTheme::TEXT_HIGHLIGHT()
                                    } else if is_played {
                                        TraktorTheme::TEXT_MUTED()
                                    } else {
                                        TraktorTheme::TEXT_PRIMARY()
                                    };

                                    let y = rect.center().y;
                                    painter.text(Pos2::new(rect.left() + 4.0, y), Align2::LEFT_CENTER, &num_str, theme::dot_font(10.0), text_col);

                                    let title_w = (list_w * 0.36).max(110.0);
                                    let artist_w = (list_w * 0.22).max(80.0);

                                    let title_rect = Rect::from_min_size(Pos2::new(rect.left() + 28.0, rect.top()), Vec2::new(title_w, rect.height()));
                                    painter.text(Pos2::new(title_rect.left(), y), Align2::LEFT_CENTER, &track.title, egui::FontId::proportional(11.0), text_col);

                                    let artist_rect = Rect::from_min_size(Pos2::new(title_rect.right() + 4.0, rect.top()), Vec2::new(artist_w, rect.height()));
                                    painter.text(Pos2::new(artist_rect.left(), y), Align2::LEFT_CENTER, &track.artist, egui::FontId::proportional(11.0), TraktorTheme::TEXT_SECONDARY());

                                    let bpm_str = if track.bpm > 0.0 { format!("{:.1}", track.bpm) } else { "-".into() };
                                    painter.text(Pos2::new(artist_rect.right() + 6.0, y), Align2::LEFT_CENTER, &bpm_str, theme::dot_font(10.0), TraktorTheme::PLAY_ACTIVE());

                                    let key_str = if !track.musical_key.is_empty() { &track.musical_key } else { "-" };
                                    painter.text(Pos2::new(artist_rect.right() + 52.0, y), Align2::LEFT_CENTER, key_str, theme::dot_font(10.0), TraktorTheme::TEXT_HIGHLIGHT());

                                    let energy_str = if track.energy > 0 { format!("E{}", track.energy) } else { "-".into() };
                                    painter.text(Pos2::new(artist_rect.right() + 84.0, y), Align2::LEFT_CENTER, &energy_str, FontId::monospace(10.0), TraktorTheme::ACCENT());

                                    let dur_str = format_duration(track.duration_sec);
                                    painter.text(Pos2::new(rect.right() - 28.0, y), Align2::RIGHT_CENTER, &dur_str, FontId::monospace(10.0), TraktorTheme::TEXT_MUTED());

                                    // Warning icon indicator if join to next track has issues
                                    let row_warnings: Vec<_> = warnings.iter().filter(|(idx, _)| *idx == i).map(|(_, w)| *w).collect();
                                    if !row_warnings.is_empty() {
                                        painter.text(Pos2::new(rect.right() - 8.0, y), Align2::CENTER_CENTER, "⚠", egui::FontId::proportional(12.0), TraktorTheme::METER_RED());
                                    }
                                }

                                if resp.clicked() {
                                    self.selected_index = Some(i);
                                }
                                if resp.double_clicked() {
                                    library.load_to_deck(idle_deck, track.id, [false; 4]);
                                }

                                // Warning tooltip
                                let row_warnings: Vec<_> = warnings.iter().filter(|(idx, _)| *idx == i).map(|(_, w)| *w).collect();
                                if !row_warnings.is_empty() {
                                    let tip = row_warnings
                                        .iter()
                                        .map(|w| format!("⚠ {}", w.label()))
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    resp.on_hover_text(tip);
                                }
                            }
                        });
                });

                ui.separator();

                // --- Right Column: Dot-Matrix Arc Display & Running Time ---
                ui.allocate_ui_with_layout(Vec2::new(arc_w, top_h), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(arc_w);

                    let arc_tracks: Vec<SetArcTrack> = resolved
                        .iter()
                        .map(|t| SetArcTrack {
                            key: t.musical_key.clone(),
                            bpm: t.bpm,
                            energy: t.energy,
                        })
                        .collect();

                    let arc_h = top_h - 48.0;
                    let (_, clicked) = dot::set_arc_display(ui, &arc_tracks, self.selected_index, Vec2::new(arc_w - 8.0, arc_h));
                    if let Some(c) = clicked {
                        self.selected_index = Some(c);
                    }

                    ui.add_space(4.0);

                    // Under it: Total Running Time in dot font
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::new(12.0, 0.0);
                        ui.label(
                            RichText::new("RUNNING TIME")
                                .color(TraktorTheme::TEXT_MUTED())
                                .font(egui::FontId::monospace(10.5))
                                .strong(),
                        );
                        ui.label(
                            RichText::new(format_duration(total_sec))
                                .color(TraktorTheme::PLAY_ACTIVE())
                                .font(theme::dot_font(14.0))
                                .strong(),
                        );

                        if !warnings.is_empty() {
                            ui.label(
                                RichText::new(format!("⚠ {} flow warnings", warnings.len()))
                                    .color(TraktorTheme::METER_RED())
                                    .font(egui::FontId::proportional(11.0)),
                            );
                        } else if !resolved.is_empty() {
                            ui.label(
                                RichText::new("✔ Harmonically compatible")
                                    .color(TraktorTheme::PLAY_ACTIVE())
                                    .font(egui::FontId::proportional(11.0)),
                            );
                        }
                    });
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_track(id: usize, bpm: f64, key: &str, energy: u8, duration_sec: f64) -> UiTrackEntry {
        UiTrackEntry {
            id,
            title: format!("Title {id}"),
            artist: format!("Artist {id}"),
            bpm,
            musical_key: key.to_string(),
            energy,
            duration_sec,
            ..Default::default()
        }
    }

    #[test]
    fn set_list_load_save_tolerate_missing() {
        let text = "Friday Warm-up\n101\n# Comment\n102\ninvalid_id\n103\n";
        let set = SetList::load_from_str(text);
        assert_eq!(set.name, "Friday Warm-up");
        assert_eq!(set.track_ids, vec![101, 102, 103]);

        let out = set.to_text();
        assert!(out.starts_with("Friday Warm-up\n"));
        assert!(out.contains("101\n"));
        assert!(out.contains("102\n"));
        assert!(out.contains("103\n"));
    }

    #[test]
    fn flow_warnings_detect_key_clash_tempo_and_energy() {
        // Track 0: 8m, 124 bpm, energy 7
        // Track 1: 3d (incompatible with 8m), 125 bpm, energy 7 -> KeyClash
        // Track 2: 3d, 136 bpm (>6% jump from 125), energy 7 -> TempoJump
        // Track 3: 3d, 137 bpm, energy 4 (drop of 3 from 7) -> EnergyDrop
        let tracks = vec![
            make_test_track(1, 124.0, "8m", 7, 180.0),
            make_test_track(2, 125.0, "3d", 7, 200.0),
            make_test_track(3, 136.0, "3d", 7, 210.0),
            make_test_track(4, 137.0, "3d", 4, 190.0),
        ];

        let w = flow_warnings(&tracks);
        assert!(w.contains(&(0, FlowWarning::KeyClash)));
        assert!(w.contains(&(1, FlowWarning::TempoJump)));
        assert!(w.contains(&(2, FlowWarning::EnergyDrop)));
    }

    #[test]
    fn flow_warnings_folded_tempo_half_double() {
        // Dubstep 70 bpm to Drum & Bass 140 bpm should not trigger tempo jump (exact 2x)
        let tracks = vec![
            make_test_track(1, 70.0, "8m", 6, 180.0),
            make_test_track(2, 140.0, "8m", 6, 180.0),
        ];
        let w = flow_warnings(&tracks);
        assert!(!w.iter().any(|(_, warn)| *warn == FlowWarning::TempoJump));

        // 70 to 160 is >6% even after folding (80 vs 70 -> ~14% diff)
        let tracks2 = vec![
            make_test_track(1, 70.0, "8m", 6, 180.0),
            make_test_track(2, 160.0, "8m", 6, 180.0),
        ];
        let w2 = flow_warnings(&tracks2);
        assert!(w2.contains(&(0, FlowWarning::TempoJump)));
    }

    #[test]
    fn order_for_flow_keeps_first_and_minimises_cost() {
        let t1 = make_test_track(1, 120.0, "1d", 4, 180.0);
        let t2 = make_test_track(2, 128.0, "8d", 8, 200.0);
        let t3 = make_test_track(3, 121.0, "2d", 5, 210.0);

        let ordered = order_for_flow(&[t1, t2, t3]);
        // Track 1 must stay first; Track 3 is compatible with 1d and has 121 bpm and energy 5; Track 2 follows
        assert_eq!(ordered, vec![1, 3, 2]);
    }

    #[test]
    fn cue_sheet_and_duration_format() {
        let tracks = vec![
            make_test_track(1, 120.0, "1d", 5, 125.5),
            make_test_track(2, 122.0, "1d", 6, 200.0),
        ];
        let total = total_duration(&tracks);
        assert!((total - 325.5).abs() < 1e-6);
        assert_eq!(format_duration(total), "05:26");

        let entries = &[
            (0.0, "Artist A", "Title A"),
            (125.5, "Artist B", "Title B"),
        ];
        let cue = cue_sheet(entries, "MyMix.wav");
        assert!(cue.contains("FILE \"MyMix.wav\" WAVE"));
        assert!(cue.contains("TRACK 01 AUDIO"));
        assert!(cue.contains("INDEX 01 00:00:00"));
        assert!(cue.contains("TRACK 02 AUDIO"));
        assert!(cue.contains("INDEX 01 02:05:38")); // 125.5s = 2m 5s, 0.5 * 75 = 38 frames
    }

    #[test]
    fn civil_datetime_computation() {
        // Known epoch 0 -> 1970-01-01 00:00
        let (y, m, d, h, min) = civil_datetime_from_secs(0);
        assert_eq!((y, m, d, h, min), (1970, 1, 1, 0, 0));

        // 2026-11-10 23:45
        // 1794354300 seconds from epoch
        let (y2, m2, d2, h2, min2) = civil_datetime_from_secs(1794354300);
        assert_eq!((y2, m2, d2, h2, min2), (2026, 11, 10, 23, 45));
    }
}
