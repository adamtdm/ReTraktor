//! Music library: collection, playlists, tag reading, background scanning/relocation,
//! autosaved Traktor `.nml` persistence, and the virtualized browser table.

use crate::demucs::DemucsManager;
use crate::preferences::LibraryColumnsConfig;
use crate::theme::TraktorTheme;
use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, Stroke, Ui};
use egui_extras::{Column, TableBuilder};
use retraktor_formats::nml::{NmlCue, NmlEntry, NmlLocation, NmlLoudness, NmlPlaylistNode, NmlTempo};
use retraktor_formats::NmlCollection;
use retraktor_library::{analyze_mono, KeyWheel, TrackAnalysis};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

pub const DECK_NAMES: [&str; 4] = ["A", "B", "C", "D"];
const AUDIO_EXTS: [&str; 10] = ["mp3", "flac", "wav", "ogg", "m4a", "aac", "mp4", "aiff", "aif", "opus"];
const ROW_H: f32 = 22.0;
const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, Default)]
pub struct UiTrackEntry {
    pub id: usize,
    pub title: String,
    pub artist: String,
    pub album: String,
    /// 0.0 = not analysed yet.
    pub bpm: f64,
    /// Open Key (`8m`); empty = unknown.
    pub musical_key: String,
    pub duration_sec: f64,
    pub genre: String,
    /// 0..=5 stars.
    pub rating: u8,
    /// 1..=10, 0 = not analysed yet.
    pub energy: u8,
    pub comment: String,
    pub remixer: String,
    pub label: String,
    pub year: String,
    pub bitrate_kbps: u32,
    pub format: String,
    pub track_number: u32,
    pub play_count: u32,
    pub file_path: Option<PathBuf>,
    pub stems_ready: bool,
    pub missing: bool,
    // Traktor data we don't edit but must keep on save.
    pub cues: Vec<NmlCue>,
    pub loudness: Option<NmlLoudness>,
    pub audio_id: Option<String>,
    pub release_date: Option<String>,
    pub phrases: Vec<retraktor_library::Phrase>,
}

impl UiTrackEntry {
    fn from_nml(e: &NmlEntry) -> Self {
        let path = PathBuf::from(e.location.to_posix_path());
        let key = e
            .musical_key_value
            .or_else(|| e.key_text.as_deref().and_then(KeyWheel::normalize))
            .and_then(KeyWheel::to_open_key)
            .unwrap_or_default();
        Self {
            title: e.title.clone(),
            artist: e.artist.clone(),
            album: e.album_title.clone().unwrap_or_default(),
            bpm: e.tempo.as_ref().map_or(0.0, |t| t.bpm),
            musical_key: key.to_string(),
            duration_sec: e.playtime_sec.unwrap_or(0.0),
            genre: e.genre.clone().unwrap_or_default(),
            rating: e.ranking.map_or(0, |r| ((r as u16 + 25) / 51) as u8),
            energy: e.energy.unwrap_or(0),
            comment: e.comment.clone().unwrap_or_default(),
            remixer: e.remixer.clone().unwrap_or_default(),
            label: e.label.clone().unwrap_or_default(),
            year: e.release_date.as_deref().unwrap_or("").chars().take(4).collect(),
            bitrate_kbps: e.bitrate.map_or(0, |b| if b > 10_000 { b / 1000 } else { b }),
            format: ext_upper(&path),
            track_number: e.album_track.unwrap_or(0),
            play_count: e.play_count.unwrap_or(0),
            file_path: Some(path),
            stems_ready: false,
            missing: false,
            cues: e.cues.clone(),
            loudness: e.loudness.clone(),
            audio_id: e.audio_id.clone(),
            release_date: e.release_date.clone(),
            ..Default::default()
        }
    }

    fn to_nml(&self) -> Option<NmlEntry> {
        let path = self.file_path.as_ref()?;
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
        let key_value = KeyWheel::normalize(&self.musical_key);
        // Keep Traktor's full release date unless the user edited the year.
        let release_date = match &self.release_date {
            Some(d) if d.starts_with(&self.year) => Some(d.clone()),
            _ => opt(&self.year),
        };
        Some(NmlEntry {
            title: self.title.clone(),
            artist: self.artist.clone(),
            audio_id: self.audio_id.clone(),
            location: NmlLocation::from_posix_path(&path.to_string_lossy(), "linux-root"),
            album_title: opt(&self.album),
            album_track: (self.track_number > 0).then_some(self.track_number),
            genre: opt(&self.genre),
            comment: opt(&self.comment),
            key_text: opt(&self.musical_key),
            playtime_sec: (self.duration_sec > 0.0).then_some(self.duration_sec),
            bitrate: (self.bitrate_kbps > 0).then_some(self.bitrate_kbps * 1000),
            tempo: (self.bpm > 0.0).then(|| NmlTempo { bpm: self.bpm, bpm_quality: 100.0 }),
            loudness: self.loudness.clone(),
            musical_key_value: key_value,
            cues: self.cues.clone(),
            stems_path: None,
            ranking: (self.rating > 0).then_some(self.rating.min(5) * 51),
            label: opt(&self.label),
            remixer: opt(&self.remixer),
            release_date,
            play_count: (self.play_count > 0).then_some(self.play_count),
            energy: (self.energy > 0).then_some(self.energy),
        })
    }

    /// The filled hotcue pads as (pad 1..=8, seconds), in the track's cue order.
    pub fn hotcue_slots(&self) -> Vec<(u8, f64)> {
        self.cues.iter().filter(|c| (0..8).contains(&c.hotcue)).map(|c| ((c.hotcue + 1) as u8, c.start_ms / 1000.0)).collect()
    }

    /// Applies detected values. Without `overwrite` only unknown values are filled.
    /// A beatgrid is only added to a track with none; auto hot cues only with `hot_cues`, and only to a track with no
    /// cue of any kind in a hot cue slot.
    pub fn apply_analysis(&mut self, a: &TrackAnalysis, overwrite: bool, hot_cues: bool) {
        if overwrite || self.bpm <= 0.0 {
            self.bpm = a.bpm;
        }
        if overwrite || self.musical_key.is_empty() {
            self.musical_key = a.key_open.clone();
        }
        if overwrite || self.energy == 0 {
            self.energy = a.energy;
        }
        if overwrite || self.loudness.is_none() {
            self.loudness = Some(NmlLoudness { analyzed_db: a.loudness_db as f64, perceived_db: a.loudness_db as f64, peak_db: a.peak_db as f64 });
        }
        let cue = |name: String, t: f64, kind, hotcue, grid| NmlCue {
            name,
            displ_order: 0,
            cue_type: kind,
            start_ms: t * 1000.0,
            len_ms: 0.0,
            repeats: -1,
            hotcue,
            grid_bpm: grid,
        };
        use retraktor_core::CueType;
        if !self.cues.iter().any(|c| c.cue_type == CueType::Grid) {
            self.cues.push(cue("AutoGrid".into(), a.beatgrid_offset_sec, CueType::Grid, -1, Some(a.bpm)));
        }
        if hot_cues && !self.cues.iter().any(|c| c.hotcue >= 0) {
            for (i, t) in a.cue_points_sec.iter().take(8).enumerate() {
                self.cues.push(cue(format!("Auto {}", i + 1), *t, CueType::Cue, i as i64, None));
            }
        }
        if overwrite || self.phrases.is_empty() {
            self.phrases = a.phrases.clone();
        }
    }

    /// Copies values from `tags` into fields that are still empty; never overwrites.
    /// Takes another library's data for this track without overwriting anything already here:
    /// empty tags, rating, and each kind of cue (beatgrid, hotcues, memory cues) only if this track has none.
    fn adopt_from(&mut self, other: UiTrackEntry) {
        use retraktor_core::CueType;
        let kinds: [fn(&NmlCue) -> bool; 3] = [
            |c| c.cue_type == CueType::Grid,
            |c| c.cue_type != CueType::Grid && c.hotcue >= 0,
            |c| c.cue_type != CueType::Grid && c.hotcue < 0,
        ];
        for is_kind in kinds {
            if !self.cues.iter().any(is_kind) {
                self.cues.extend(other.cues.iter().filter(|c| is_kind(c)).cloned());
            }
        }
        if self.rating == 0 {
            self.rating = other.rating;
        }
        self.fill_empty_from(other);
    }

    fn fill_empty_from(&mut self, tags: UiTrackEntry) {
        for (dst, src) in [
            (&mut self.title, tags.title),
            (&mut self.artist, tags.artist),
            (&mut self.album, tags.album),
            (&mut self.genre, tags.genre),
            (&mut self.comment, tags.comment),
            (&mut self.remixer, tags.remixer),
            (&mut self.label, tags.label),
            (&mut self.year, tags.year),
            (&mut self.musical_key, tags.musical_key),
        ] {
            if dst.is_empty() {
                *dst = src;
            }
        }
        if self.bpm <= 0.0 {
            self.bpm = tags.bpm;
        }
        if self.duration_sec <= 0.0 {
            self.duration_sec = tags.duration_sec;
        }
        if self.bitrate_kbps == 0 {
            self.bitrate_kbps = tags.bitrate_kbps;
        }
        if self.track_number == 0 {
            self.track_number = tags.track_number;
        }
        self.stems_ready |= tags.stems_ready;
    }

    fn adopt_serato_cues(&mut self, cues: Vec<NmlCue>) {
        if self.cues.is_empty() {
            if let Some(bpm) = cues.iter().find_map(|cue| cue.grid_bpm) {
                self.bpm = bpm;
            }
            self.cues = cues;
        }
    }

    fn search_blob(&self) -> String {
        format!(
            "{} {} {} {} {} {} {} {} {:.0}",
            self.title, self.artist, self.album, self.genre, self.comment, self.remixer, self.label,
            self.musical_key, self.bpm
        )
        .to_lowercase()
    }
}

fn ext_upper(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_uppercase()
}

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Reads a file's tags and audio properties. Missing tags stay empty — never invented.
pub fn read_track_file(path: &Path) -> UiTrackEntry {
    let (mut track, cues) = read_track_tags(path);
    track.adopt_serato_cues(cues);
    track
}

fn read_track_tags(path: &Path) -> (UiTrackEntry, Vec<NmlCue>) {
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::{Accessor, ItemKey, TagType};

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").trim();
    let (artist, title) = match stem.split_once(" - ") {
        Some((a, t)) => (a.trim().to_string(), t.trim().to_string()),
        None => (String::new(), stem.to_string()),
    };
    let mut t = UiTrackEntry {
        title,
        artist,
        format: ext_upper(path),
        file_path: Some(path.to_path_buf()),
        stems_ready: DemucsManager::get_cached_stem_dir(path).is_some(),
        ..Default::default()
    };
    let Ok(mut tagged) = lofty::read_from_path(path) else {
        return (t, Vec::new());
    };
    let props = tagged.properties();
    t.duration_sec = props.duration().as_secs_f64();
    t.bitrate_kbps = props.audio_bitrate().or(props.overall_bitrate()).unwrap_or(0);
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return (t, Vec::new());
    };
    let set = |dst: &mut String, v: Option<std::borrow::Cow<str>>| {
        if let Some(v) = v.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()) {
            *dst = v;
        }
    };
    set(&mut t.title, tag.title());
    set(&mut t.artist, tag.artist());
    set(&mut t.album, tag.album());
    set(&mut t.genre, tag.genre());
    set(&mut t.comment, tag.comment());
    let text = |k: &ItemKey| tag.get_string(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    t.remixer = text(&ItemKey::Remixer).unwrap_or_default();
    t.label = text(&ItemKey::Label).or_else(|| text(&ItemKey::Publisher)).unwrap_or_default();
    t.year = tag.year().map(|y| y.to_string()).unwrap_or_default();
    t.track_number = tag.track().unwrap_or(0);
    t.bpm = text(&ItemKey::Bpm)
        .or_else(|| text(&ItemKey::IntegerBpm))
        .and_then(|b| b.replace(',', ".").parse::<f64>().ok())
        .filter(|b| (40.0..=250.0).contains(b))
        .unwrap_or(0.0);
    t.musical_key = text(&ItemKey::InitialKey)
        .as_deref()
        .and_then(KeyWheel::normalize)
        .and_then(KeyWheel::to_open_key)
        .unwrap_or("")
        .to_string();
    let tags = [TagType::Id3v2, TagType::VorbisComments, TagType::Mp4Ilst]
        .into_iter()
        .filter_map(|kind| tagged.remove(kind));
    (t, retraktor_formats::serato::read_cues(tags))
}

/// Writes the editable fields back into the audio file's own tags.
/// Writes `fields` of `t` into its file's tags; a field left empty removes its tag. Only these fields are
/// touched, so tags ReTraktor never read survive. Fields with no tag here (year, track no., rating, energy) are skipped.
fn write_track_tags(t: &UiTrackEntry, fields: &[SortColumn]) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::file::TaggedFileExt;
    use lofty::tag::{ItemKey, Tag, TagExt};

    let tags: Vec<(ItemKey, String)> = fields
        .iter()
        .filter_map(|&c| {
            let key = match c {
                SortColumn::Title => ItemKey::TrackTitle,
                SortColumn::Artist => ItemKey::TrackArtist,
                SortColumn::Album => ItemKey::AlbumTitle,
                SortColumn::Genre => ItemKey::Genre,
                SortColumn::Comment => ItemKey::Comment,
                SortColumn::Remixer => ItemKey::Remixer,
                SortColumn::Label => ItemKey::Label,
                SortColumn::Key => ItemKey::InitialKey,
                SortColumn::Bpm => ItemKey::Bpm,
                _ => return None,
            };
            Some((key, field_text(t, c)))
        })
        .collect();
    if tags.is_empty() {
        return Ok(());
    }

    let path = t.file_path.as_ref().ok_or("track has no file")?;
    let mut tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    if tagged.primary_tag().is_none() {
        let tag_type = tagged.primary_tag_type();
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged.primary_tag_mut().ok_or("cannot create tag")?;
    for (key, val) in tags {
        if val.is_empty() {
            tag.remove_key(&key);
        } else {
            tag.insert_text(key, val);
        }
    }
    tag.save_to_path(path, WriteOptions::default()).map_err(|e| e.to_string())
}

/// Breadth-first search for `collection.nml` under `roots`, bounded by depth and folders visited.
fn find_collections(roots: &[PathBuf], max_depth: usize, max_dirs: usize) -> Vec<PathBuf> {
    let mut queue: std::collections::VecDeque<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    let (mut found, mut seen, mut visited) = (Vec::new(), HashSet::new(), 0);
    while let Some((dir, depth)) = queue.pop_front() {
        if visited >= max_dirs || !seen.insert(dir.clone()) {
            continue;
        }
        visited += 1;
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_file() && name.eq_ignore_ascii_case("collection.nml") {
                found.push(e.path());
            } else if ft.is_dir() && depth < max_depth && !name.starts_with('.') {
                queue.push_back((e.path(), depth + 1));
            }
        }
    }
    found.sort();
    found.dedup();
    found
}

/// `lo-hi`, `>x`, `>=x`, `<x`, `<=x` or an exact number.
fn in_range(v: f64, spec: &str) -> bool {
    let num = |t: &str| t.trim().parse::<f64>().ok();
    if let Some((lo, hi)) = spec.split_once('-') {
        return matches!((num(lo), num(hi)), (Some(a), Some(b)) if v >= a && v <= b);
    }
    for (op, f) in [(">=", (|a: f64, b: f64| a >= b) as fn(f64, f64) -> bool), ("<=", |a, b| a <= b), (">", |a, b| a > b), ("<", |a, b| a < b)] {
        if let Some(rest) = spec.strip_prefix(op) {
            return num(rest).is_some_and(|b| f(v, b));
        }
    }
    num(spec).is_some_and(|b| (v - b).abs() < 0.5)
}

/// Every term must match. Plain words search text; `bpm:`, `key:` (`~` = compatible),
/// `energy:`, `rating:`, `year:`, `genre:`, `artist:` filter fields.
fn matches_query(t: &UiTrackEntry, terms: &[String]) -> bool {
    let blob = t.search_blob();
    terms.iter().all(|term| match term.split_once(':') {
        Some(("bpm", spec)) => t.bpm > 0.0 && (in_range(t.bpm, spec) || in_range(t.bpm * 2.0, spec) || in_range(t.bpm / 2.0, spec)),
        Some(("key", spec)) => match spec.strip_prefix('~') {
            Some(k) => KeyWheel::normalize(k).and_then(KeyWheel::to_open_key).is_some_and(|k| KeyWheel::are_harmonically_compatible(k, &t.musical_key)),
            None => KeyWheel::normalize(spec).and_then(KeyWheel::to_open_key).is_some_and(|k| k == t.musical_key),
        },
        Some(("energy", spec)) => t.energy > 0 && in_range(t.energy as f64, spec),
        Some(("rating", spec)) => in_range(t.rating as f64, spec),
        Some(("year", spec)) => t.year.parse::<f64>().is_ok_and(|y| in_range(y, spec)),
        Some(("genre", w)) => t.genre.to_lowercase().contains(w),
        Some(("artist", w)) => t.artist.to_lowercase().contains(w),
        _ => blob.contains(term.as_str()),
    })
}

/// Tick-box list of optional library columns (title and artist are always shown).
fn column_menu(ui: &mut Ui, c: &mut LibraryColumnsConfig) {
    ui.label(RichText::new("Show columns").size(11.0).color(TraktorTheme::TEXT_MUTED()));
    for (on, name) in [
        (&mut c.bpm, "BPM"),
        (&mut c.key, "Key"),
        (&mut c.energy, "Energy"),
        (&mut c.duration, "Time"),
        (&mut c.genre, "Genre"),
        (&mut c.album, "Album"),
        (&mut c.rating, "Rating"),
        (&mut c.remixer, "Remixer"),
        (&mut c.label, "Label"),
        (&mut c.year, "Year"),
        (&mut c.comment, "Comment"),
        (&mut c.bitrate, "Bitrate"),
        (&mut c.format, "Format"),
        (&mut c.track_number, "Track number"),
        (&mut c.stems, "Stems"),
    ] {
        ui.checkbox(on, name);
    }
    ui.separator();
    if ui.button("Default columns").clicked() {
        *c = LibraryColumnsConfig::default();
    }
}

fn plural(n: usize) -> String {
    if n == 1 { "1 track".to_string() } else { format!("{n} tracks") }
}

/// The fields a DJ can type into, in the editor's order. Alt+click edits these in the table too.
const EDITABLE: [SortColumn; 13] = [
    SortColumn::Title,
    SortColumn::Artist,
    SortColumn::Album,
    SortColumn::Genre,
    SortColumn::Remixer,
    SortColumn::Label,
    SortColumn::Year,
    SortColumn::TrackNum,
    SortColumn::Key,
    SortColumn::Bpm,
    SortColumn::Rating,
    SortColumn::Energy,
    SortColumn::Comment,
];

/// A track's field as the text a DJ edits; empty for an unset number.
fn field_text(t: &UiTrackEntry, c: SortColumn) -> String {
    let n = |v: u32| if v > 0 { v.to_string() } else { String::new() };
    match c {
        SortColumn::Title => t.title.clone(),
        SortColumn::Artist => t.artist.clone(),
        SortColumn::Album => t.album.clone(),
        SortColumn::Genre => t.genre.clone(),
        SortColumn::Remixer => t.remixer.clone(),
        SortColumn::Label => t.label.clone(),
        SortColumn::Year => t.year.clone(),
        SortColumn::Comment => t.comment.clone(),
        SortColumn::Key => t.musical_key.clone(),
        SortColumn::Bpm if t.bpm > 0.0 => format!("{:.2}", t.bpm),
        SortColumn::TrackNum => n(t.track_number),
        SortColumn::Rating => n(t.rating.into()),
        SortColumn::Energy => n(t.energy.into()),
        _ => String::new(),
    }
}

/// Sets a field from typed text, checking it first; the error says what to type instead.
fn set_field(t: &mut UiTrackEntry, c: SortColumn, text: &str) -> Result<(), String> {
    let v = text.trim();
    let num = |max: f64, what: &str| -> Result<f64, String> {
        if v.is_empty() {
            return Ok(0.0);
        }
        v.parse::<f64>().ok().filter(|n| (0.0..=max).contains(n)).ok_or_else(|| format!("{what} must be a number from 0 to {max}, not '{v}'"))
    };
    match c {
        SortColumn::Title => t.title = v.into(),
        SortColumn::Artist => t.artist = v.into(),
        SortColumn::Album => t.album = v.into(),
        SortColumn::Genre => t.genre = v.into(),
        SortColumn::Remixer => t.remixer = v.into(),
        SortColumn::Label => t.label = v.into(),
        SortColumn::Year => t.year = v.into(),
        SortColumn::Comment => t.comment = v.into(),
        SortColumn::Key if v.is_empty() => t.musical_key.clear(),
        SortColumn::Key => {
            let k = KeyWheel::normalize(v).and_then(KeyWheel::to_open_key).ok_or_else(|| format!("'{v}' isn't a key: use e.g. 8m, 8A or Am"))?;
            t.musical_key = k.to_string();
        }
        SortColumn::Bpm => t.bpm = num(250.0, "BPM")?,
        SortColumn::TrackNum => t.track_number = num(9999.0, "Track number")?.round() as u32,
        SortColumn::Rating => t.rating = num(5.0, "Rating")?.round() as u8,
        SortColumn::Energy => t.energy = num(10.0, "Energy")?.round() as u8,
        _ => return Err(format!("{} can't be edited", c.header())),
    }
    Ok(())
}

fn field_name(c: SortColumn) -> &'static str {
    match c {
        SortColumn::TrackNum => "Track no.",
        _ => c.header(),
    }
}

/// The track editor, for one track or many. Each field keeps the tracks' shared value (`None` where they
/// differ) and what's typed now; only the fields that change are written, to every track.
struct Editor {
    ids: Vec<usize>,
    fields: Vec<(SortColumn, Option<String>, String)>,
    /// ×½ / ×2 pressed while BPMs differ: each track's own BPM is scaled.
    bpm_scale: f64,
    error: Option<String>,
}

impl Editor {
    fn new(tracks: &[&UiTrackEntry]) -> Self {
        let fields = EDITABLE
            .iter()
            .map(|&c| {
                let first = field_text(tracks[0], c);
                let shared = tracks.iter().all(|t| field_text(t, c) == first).then_some(first);
                (c, shared.clone(), shared.unwrap_or_default())
            })
            .collect();
        Self { ids: tracks.iter().map(|t| t.id).collect(), fields, bpm_scale: 1.0, error: None }
    }

    /// Changed from the shared value, or typed into where the tracks differ.
    fn is_changed(was: &Option<String>, now: &str) -> bool {
        was.as_ref().map_or(!now.trim().is_empty(), |w| w.trim() != now.trim())
    }

    /// The fields to write.
    fn changes(&self) -> Vec<(SortColumn, String)> {
        self.fields.iter().filter(|(_, was, now)| Self::is_changed(was, now)).map(|(c, _, now)| (*c, now.clone())).collect()
    }

    /// The fields a save writes, BPM included when only scaled.
    fn changed_fields(&self) -> Vec<SortColumn> {
        let mut cols: Vec<SortColumn> = self.changes().into_iter().map(|(c, _)| c).collect();
        if self.bpm_scale != 1.0 && !cols.contains(&SortColumn::Bpm) {
            cols.push(SortColumn::Bpm);
        }
        cols
    }
}

pub fn stereo_to_mono(pcm: &[f32]) -> Vec<f32> {
    pcm.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])).collect()
}

/// Reads tags for `items` and sends them in batches.
fn read_tags_into(tx: &Sender<Msg>, items: &[(usize, PathBuf)]) {
    for (n, chunk) in items.chunks(100).enumerate() {
        let batch = chunk.iter().map(|(id, p)| {
            let (tags, cues) = read_track_tags(p);
            (*id, tags, cues)
        }).collect();
        let _ = tx.send(Msg::Tags(batch));
        let _ = tx.send(Msg::Status(format!("Reading tags: {} of {}…", n * 100 + chunk.len(), items.len())));
    }
}

/// Iteratively walks `root` calling `f` for every audio file (hidden entries and symlinks skipped).
fn walk_audio(root: &Path, f: &mut impl FnMut(PathBuf)) {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let Ok(ft) = entry.file_type() else { continue };
            let path = entry.path();
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() && is_audio_file(&path) {
                f(path);
            }
        }
    }
}

/// Writes through a temp file and a rename, so a crash or a full disk never leaves a half-written file.
/// Each call gets its own temp name: two saves of one file at once (autosave and close) can't mix their bytes.
pub(crate) fn write_atomic(path: &Path, contents: &str) -> std::io::Result<()> {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp{}-{}", std::process::id(), SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)));
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, contents).and_then(|_| fs::rename(&tmp, path)).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Genre similarity in `0.0..=1.0`, or `None` when either track has no genre tag.
pub fn genre_score(a: &str, b: &str) -> Option<f32> {
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() || b.is_empty() {
        return None;
    }
    if a.eq_ignore_ascii_case(b) {
        return Some(1.0);
    }
    let (al, bl) = (a.to_ascii_lowercase(), b.to_ascii_lowercase());
    if al.contains(&bl) || bl.contains(&al) {
        return Some(0.7);
    }
    // Same root genre word (e.g. "Deep House" vs "Tech House", but not "Deep House" vs "Deep Techno").
    fn root(s: &str) -> Option<&str> {
        s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() >= 3).last()
    }
    Some(if root(&al).is_some() && root(&al) == root(&bl) { 0.5 } else { 0.0 })
}

/// Harmonic + tempo (+ energy, genre when known) compatibility of `t` with `r`, `0.0..=1.0`.
pub fn match_score(r: &UiTrackEntry, t: &UiTrackEntry) -> f32 {
    let (ref_key, ref_bpm) = (r.musical_key.as_str(), r.bpm);
    let key_s = match (KeyWheel::parse_open_key(ref_key), KeyWheel::parse_open_key(&t.musical_key)) {
        (Some(a), Some(b)) if a == b => 1.0,
        (Some(_), Some(_)) if KeyWheel::are_harmonically_compatible(ref_key, &t.musical_key) => 0.8,
        // +2 on the wheel within the same mode: the classic "energy boost" mix
        (Some((na, ma)), Some((nb, mb))) if ma == mb && (nb + 12 - na) % 12 == 2 => 0.5,
        _ => 0.0,
    };
    let bpm_s = if ref_bpm > 0.0 && t.bpm > 0.0 {
        let mut ratio = t.bpm / ref_bpm;
        if ratio < 0.75 {
            ratio *= 2.0;
        } else if ratio > 1.5 {
            ratio /= 2.0;
        }
        (1.0 - ((ratio - 1.0).abs() / 0.08)).max(0.0) as f32
    } else {
        0.0
    };
    let energy_s = (r.energy > 0 && t.energy > 0)
        .then(|| (1.0 - (t.energy as f32 - r.energy as f32).abs() / 4.0).max(0.0));
    let genre_s = genre_score(&r.genre, &t.genre);
    match (energy_s, genre_s) {
        (Some(e), Some(g)) => 0.45 * key_s + 0.35 * bpm_s + 0.10 * e + 0.10 * g,
        (Some(e), None) => 0.50 * key_s + 0.40 * bpm_s + 0.10 * e,
        (None, Some(g)) => 0.50 * key_s + 0.40 * bpm_s + 0.10 * g,
        (None, None) => 0.55 * key_s + 0.45 * bpm_s,
    }
}

#[derive(Clone, Debug)]
pub struct UserPlaylist {
    pub name: String,
    pub track_ids: Vec<usize>,
    pub missing: Vec<crate::playlist_import::Wanted>,
}

/// Drag-and-drop payload: track ids.
#[derive(Clone, Debug)]
pub struct DragTracks(pub Vec<usize>);

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum NavItem {
    Collection,
    Missing,
    Stems,
    Suggestions,
    History,
    Explorer,
    Playlist(usize),
    /// Tracks from a linked library (index into `libraries`).
    Library(usize),
}

/// Another DJ app's library that ReTraktor reads and keeps in step with.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LibKind {
    Traktor,
    Rekordbox,
}

impl LibKind {
    pub fn label(self) -> &'static str {
        match self {
            LibKind::Traktor => "Traktor",
            LibKind::Rekordbox => "rekordbox",
        }
    }

    fn parse(self, text: &str) -> Result<NmlCollection, String> {
        match self {
            LibKind::Traktor => NmlCollection::parse_xml(text),
            LibKind::Rekordbox => retraktor_formats::from_rekordbox_xml(text),
        }
    }
}

/// Traktor saves paths from the root of the volume they're on (a share is `/Volumes/Music` on a Mac), so on
/// another machine they don't exist. The volume is then the nearest folder above the .nml (usually the one
/// holding its `Traktor` folder) under which most of a sample of the missing paths do exist.
fn volume_root(col: &NmlCollection, nml: &Path, files: &HashSet<PathBuf>) -> Option<PathBuf> {
    let missing: Vec<PathBuf> = col.entries.iter().map(|e| PathBuf::from(e.location.to_posix_path())).filter(|p| !files.contains(p)).collect();
    let sample: Vec<&PathBuf> = missing.iter().step_by((missing.len() / 20).max(1)).take(20).collect();
    if sample.is_empty() {
        return None;
    }
    nml.parent()?
        .ancestors()
        .filter(|a| a.parent().is_some())
        .find(|a| sample.iter().filter(|p| on_volume(a, p).exists()).count() * 2 > sample.len())
        .map(Path::to_path_buf)
}

/// Every entry in the folders holding `paths`: one listing per folder, where a check per file is a network
/// round trip each on a NAS (16k tracks took minutes over SMB).
fn listed_files<'a>(paths: impl Iterator<Item = &'a Path>) -> HashSet<PathBuf> {
    let dirs: HashSet<&Path> = paths.filter_map(Path::parent).collect();
    dirs.into_iter().filter_map(|d| fs::read_dir(d).ok()).flatten().flatten().map(|e| e.path()).collect()
}

fn on_volume(root: &Path, saved: &Path) -> PathBuf {
    root.join(saved.strip_prefix("/").unwrap_or(saved))
}

pub struct LinkedLibrary {
    pub kind: LibKind,
    pub path: PathBuf,
    /// Collection tracks this library holds, from the last read.
    pub track_ids: Vec<usize>,
    /// Last read result in words.
    pub note: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SortColumn {
    Order,
    /// The row's waveform: click to preview from there. Not sortable.
    Preview,
    TrackNum,
    Title,
    Artist,
    Album,
    Bpm,
    Key,
    Energy,
    Duration,
    Genre,
    Rating,
    Comment,
    Remixer,
    Label,
    Year,
    Bitrate,
    Format,
    Stems,
    Match,
}

impl SortColumn {
    fn header(self) -> &'static str {
        match self {
            Self::Order => "#",
            Self::Preview => "Preview",
            Self::TrackNum => "Trk",
            Self::Title => "Title",
            Self::Artist => "Artist",
            Self::Album => "Album",
            Self::Bpm => "BPM",
            Self::Key => "Key",
            Self::Energy => "Energy",
            Self::Duration => "Time",
            Self::Genre => "Genre",
            Self::Rating => "Rating",
            Self::Comment => "Comment",
            Self::Remixer => "Remixer",
            Self::Label => "Label",
            Self::Year => "Year",
            Self::Bitrate => "Bitrate",
            Self::Format => "Format",
            Self::Stems => "Stems",
            Self::Match => "Match",
        }
    }

    fn width(self) -> f32 {
        match self {
            Self::Order | Self::Stems | Self::TrackNum => 34.0,
            Self::Preview => 150.0,
            Self::Title => 240.0,
            Self::Artist | Self::Album => 170.0,
            Self::Genre | Self::Label | Self::Remixer | Self::Comment => 120.0,
            Self::Rating => 64.0,
            _ => 52.0,
        }
    }
}

/// What a native file dialog was opened for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Collection,
    Rekordbox,
    MusicFolder,
    RelocateFolder,
    TrackList,
}

pub enum Msg {
    Picked(Pick, PathBuf),
    Detected(Vec<PathBuf>),
    Status(String),
    Scanned(Vec<UiTrackEntry>),
    Checked(Vec<(usize, bool, bool)>),
    Relocated(Vec<(usize, PathBuf)>),
    Tags(Vec<(usize, UiTrackEntry, Vec<NmlCue>)>),
    /// (track id, analysis, overwrite existing values)
    Analyzed(usize, TrackAnalysis, bool),
    Saved(Result<usize, String>),
    /// The collection, its volume root, and the files that exist where it says (see [`listed_files`]).
    LibraryRead(PathBuf, Result<(NmlCollection, Option<PathBuf>, HashSet<PathBuf>), String>),
    Done(String),
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct QuickFilter {
    pub bpm_center: Option<f64>,
    pub key_center: Option<String>,
    pub energy_center: Option<u8>,
    pub genre_exact: Option<String>,
}

impl QuickFilter {
    pub fn is_empty(&self) -> bool {
        self.bpm_center.is_none() && self.key_center.is_none() && self.energy_center.is_none() && self.genre_exact.is_none()
    }

    pub fn active_count(&self) -> usize {
        usize::from(self.bpm_center.is_some())
            + usize::from(self.key_center.is_some())
            + usize::from(self.energy_center.is_some())
            + usize::from(self.genre_exact.is_some())
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn matches(&self, t: &UiTrackEntry) -> bool {
        if let Some(center) = self.bpm_center {
            if !(t.bpm > 0.0 && (t.bpm - center).abs() <= 10.0) {
                return false;
            }
        }
        if let Some(ref center) = self.key_center {
            if !KeyWheel::are_harmonically_compatible(center, &t.musical_key) {
                return false;
            }
        }
        if let Some(center) = self.energy_center {
            if !(t.energy > 0 && t.energy.abs_diff(center) <= 1) {
                return false;
            }
        }
        if let Some(ref genre) = self.genre_exact {
            if t.genre.is_empty() || !t.genre.eq_ignore_ascii_case(genre) {
                return false;
            }
        }
        true
    }

    pub fn toggle_bpm(&mut self, bpm: f64) {
        if bpm > 0.0 && bpm.is_finite() {
            self.bpm_center = if self.bpm_center == Some(bpm) { None } else { Some(bpm) };
        }
    }

    pub fn toggle_key(&mut self, key: &str) {
        if KeyWheel::parse_open_key(key).is_some() {
            self.key_center = if self.key_center.as_deref().is_some_and(|k| k.eq_ignore_ascii_case(key)) {
                None
            } else {
                Some(key.to_string())
            };
        }
    }

    pub fn toggle_energy(&mut self, energy: u8) {
        if energy > 0 {
            self.energy_center = if self.energy_center == Some(energy) { None } else { Some(energy) };
        }
    }

    pub fn toggle_genre(&mut self, genre: &str) {
        if !genre.is_empty() {
            self.genre_exact = if self.genre_exact.as_deref().is_some_and(|g| g.eq_ignore_ascii_case(genre)) {
                None
            } else {
                Some(genre.to_string())
            };
        }
    }
}

fn cmp_ignore_case(a: &str, b: &str) -> std::cmp::Ordering {
    a.chars().flat_map(char::to_lowercase).cmp(b.chars().flat_map(char::to_lowercase))
}

enum QuickFilterAction {
    Bpm(f64),
    Key(String),
    Energy(u8),
    Genre(String),
}

enum Action {
    Select(usize, egui::Modifiers),
    Load(usize, usize),
    LoadAuto(usize),
    AddToPlaylist(Vec<usize>, Option<usize>),
    RemoveFromPlaylist(Vec<usize>),
    Edit(Vec<usize>),
    FillTags(Vec<usize>),
    Analyze(Vec<usize>),
    Stems(usize),
    Reveal(PathBuf),
    RemoveFromCollection(Vec<usize>),
    Sort(SortColumn),
    AddToSet(Vec<usize>),
    QuickFilter(QuickFilterAction),
}

/// A browser row's small whole-track waveform, made by the app from a decode of the file at `path` (never a deck's),
/// or from its stripe in the cache.
pub struct RowWaveform {
    pub path: PathBuf,
    /// The track's overview (blank where not decoded yet) and its length once known, or why the file can't be read.
    pub peaks: Result<(crate::overview::Overview, Option<f64>), String>,
    /// While the decode it comes from is unfinished: that decode and how far it had got (address, seconds decoded,
    /// length). `None` once finished (complete or failed): nothing more to make.
    pub made_from: Option<(usize, usize, Option<u64>)>,
    /// The file as that decode found it (modified time, byte size): a later decode that found it changed (replaced
    /// or repaired) replaces even a finished waveform or error.
    pub identity: Option<(std::time::SystemTime, u64)>,
}

pub struct LibraryManager {
    pub tracks: Vec<UiTrackEntry>,
    pub playlists: Vec<UserPlaylist>,
    pub libraries: Vec<LinkedLibrary>,
    /// A library read added tracks: look for moved files in the music folders.
    relocate_linked: bool,
    pub history: Vec<usize>,
    pub search_query: String,
    pub selected_nav: NavItem,
    pub selected_track_id: Option<usize>,
    pub add_to_set_requested: Option<Vec<usize>>,
    selection: BTreeSet<usize>,
    pub sort_col: SortColumn,
    pub sort_asc: bool,
    pub hide_missing_files: bool,
    pub quick_filter: QuickFilter,

    version: u64,
    view: Vec<usize>,
    view_scores: Vec<f32>,
    view_stamp: Option<(u64, NavItem, String, SortColumn, bool, String, bool, QuickFilter)>,
    id_index: HashMap<usize, usize>,
    /// How many times each track's hot cues were set or cleared by hand this session (not saved): a deck's late
    /// analysis adds auto hot cues only if this hasn't moved since its load.
    cue_revisions: HashMap<usize, u64>,
    scroll_to_selected: bool,

    collection_path: PathBuf,
    /// Files the user removed from the collection; linked libraries must not bring them back on the next read.
    removed: HashSet<PathBuf>,
    /// Lowercased basenames of `removed` for O(1) fallback lookup when a linked library has another folder path.
    removed_names: HashSet<String>,
    dirty_since: Option<Instant>,
    saving: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    jobs: usize,
    pub job_status: String,
    relocate_after_check: Option<Vec<PathBuf>>,
    /// Set by the app from preferences: analyse tracks found by a scan.
    pub auto_analyze: bool,
    scanned_ids: Vec<usize>,
    /// Library track currently on each deck.
    deck_tracks: [Option<usize>; 4],
    /// Lowest id `push_track` may hand out next. Ids are never reused within a session, so a deck or a
    /// pending load that still holds a removed track's id can never reach a different track.
    next_track_id: usize,
    /// Last destructive change: (description, tracks, playlists) before it happened.
    undo: Option<(String, Vec<UiTrackEntry>, Vec<UserPlaylist>)>,

    renaming: Option<(usize, String)>,
    editing: Option<Editor>,
    /// The table cell being typed into (Alt+click): track, field, text, and whether it opened this frame.
    cell_edit: Option<(usize, SortColumn, String, bool)>,
    /// Traktor collections found on this computer; `None` while still searching.
    detected: Option<Vec<PathBuf>>,
    picked: Vec<(Pick, PathBuf)>,
    typed_path: Option<String>,
    explorer_dir: PathBuf,
    explorer_path_input: String,
    explorer_entries: Vec<(PathBuf, bool)>,

    // App integration
    pub pending_loads: Vec<(usize, UiTrackEntry)>,
    /// A Preview press for the app: `Some(id)` toggles that track's headphone preview, `None` stops it.
    pub preview_requested: Option<Option<usize>>,
    /// The track in the headphone preview and whether it is still loading; set by the app before drawing.
    pub previewing: Option<(usize, bool)>,
    /// Where that preview is, as a fraction of its track, once it has found its place; set by the app before drawing.
    pub preview_position: Option<f64>,
    /// Row waveforms by track, kept by the app; one made from another path (a relinked track) isn't drawn.
    pub row_waveforms: HashMap<usize, RowWaveform>,
    /// The tracks whose rows were drawn last, top to bottom: the app prepares their waveforms.
    pub visible_rows: Vec<usize>,
    /// A plain click on a row's waveform for the app: (track, where as a fraction of the track).
    pub overview_clicked: Option<(usize, f64)>,
    /// Set by the app while help, the picker or Preferences is on top: browser keys stay inert.
    pub keys_blocked: bool,
    /// A refused load, shown on the deck itself for a few seconds.
    pub deck_refused: [Option<std::time::Instant>; 4],
    pub status_message: String,
    pub stem_job_progress: Option<(usize, f32)>,
    /// Tracks waiting for Demucs, run one at a time (two at once would exhaust RAM on a laptop).
    pub stem_queue: std::collections::VecDeque<usize>,
    pub last_stemmed_track_id: Option<usize>,
    pub last_loaded_deck: Option<(usize, bool)>,
    pub open_prefs_requested: bool,
    pub open_prefs_file_management: bool,
    pub open_prefs_browser_columns: bool,
    pub open_prefs_ai: bool,
    pub show_ai_playlist_panel: bool,
    pub ai_prompt_input: String,
    pub ai_target_duration_mins: u32,
    pub ai_playlist_runner: Option<crate::ai::AiPlaylistRunner>,
    pub ai_playlist_status: Option<String>,
    pub ai_playlist_sent_info: Option<(usize, usize)>,
    ai_settings_cache: Option<crate::ai::AiSettings>,
}

impl Default for LibraryManager {
    fn default() -> Self {
        Self::new(Self::default_nml_path())
    }
}

impl LibraryManager {
    pub fn new(collection_path: PathBuf) -> Self {
        let (tx, rx) = channel();
        let home = env::var("HOME").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from("/"));
        let mut mgr = Self {
            tracks: Vec::new(),
            playlists: Vec::new(),
            libraries: Vec::new(),
            relocate_linked: false,
            history: Vec::new(),
            search_query: String::new(),
            selected_nav: NavItem::Collection,
            selected_track_id: None,
            add_to_set_requested: None,
            selection: BTreeSet::new(),
            sort_col: SortColumn::Order,
            sort_asc: true,
            hide_missing_files: false,
            quick_filter: QuickFilter::default(),
            version: 0,
            view: Vec::new(),
            view_scores: Vec::new(),
            view_stamp: None,
            id_index: HashMap::new(),
            cue_revisions: HashMap::new(),
            scroll_to_selected: false,
            removed: HashSet::new(),
            removed_names: HashSet::new(),
            collection_path,
            dirty_since: None,
            saving: false,
            tx,
            rx,
            jobs: 0,
            job_status: String::new(),
            relocate_after_check: None,
            auto_analyze: false,
            scanned_ids: Vec::new(),
            deck_tracks: [None; 4],
            next_track_id: 1,
            undo: None,
            renaming: None,
            editing: None,
            cell_edit: None,
            detected: None,
            picked: Vec::new(),
            typed_path: None,
            explorer_path_input: home.display().to_string(),
            explorer_dir: home,
            explorer_entries: Vec::new(),
            pending_loads: Vec::new(),
            preview_requested: None,
            previewing: None,
            preview_position: None,
            row_waveforms: HashMap::new(),
            visible_rows: Vec::new(),
            overview_clicked: None,
            keys_blocked: false,
            deck_refused: [None; 4],
            status_message: String::new(),
            stem_job_progress: None,
            stem_queue: Default::default(),
            last_stemmed_track_id: None,
            last_loaded_deck: None,
            open_prefs_requested: false,
            open_prefs_file_management: false,
            open_prefs_browser_columns: false,
            open_prefs_ai: false,
            show_ai_playlist_panel: false,
            ai_prompt_input: String::new(),
            ai_target_duration_mins: 45,
            ai_playlist_runner: None,
            ai_playlist_status: None,
            ai_playlist_sent_info: None,
            ai_settings_cache: None,
        };
        let path = mgr.collection_path.clone();
        if path.is_file() {
            crate::diag::stage("startup: reading your collection .nml");
            let t0 = std::time::Instant::now();
            let read = mgr.import_nml_file(&path);
            crate::diag::log(&format!("read your collection in {} ms: {} tracks", t0.elapsed().as_millis(), mgr.tracks.len()));
            match read {
                Ok(_) => mgr.status_message.clear(),
                Err(e) => mgr.status_message = format!("Could not read {}: {e}", path.display()),
            }
            mgr.dirty_since = None;
        }
        crate::diag::stage("startup: missing list, removed list");
        mgr.load_missing_sidecar();
        mgr.removed = fs::read_to_string(mgr.removed_file()).unwrap_or_default().lines().map(PathBuf::from).collect();
        mgr.rebuild_removed_names();
        // Linked libraries are re-read on every start so their playlists stay current.
        for line in fs::read_to_string(mgr.libraries_file()).unwrap_or_default().lines() {
            let kind = match line.split_once('\t') {
                Some(("traktor", p)) => Some((LibKind::Traktor, p)),
                Some(("rekordbox", p)) => Some((LibKind::Rekordbox, p)),
                _ => None,
            };
            if let Some((kind, p)) = kind {
                mgr.libraries.push(LinkedLibrary { kind, path: PathBuf::from(p), track_ids: Vec::new(), note: "Reading…".into() });
            }
        }
        for i in 0..mgr.libraries.len() {
            mgr.read_library(i);
        }
        mgr
    }

    fn libraries_file(&self) -> PathBuf {
        self.collection_path.with_file_name("libraries.tsv")
    }

    fn removed_file(&self) -> PathBuf {
        self.collection_path.with_file_name("removed.txt")
    }

    fn rebuild_removed_names(&mut self) {
        self.removed_names = self
            .removed
            .iter()
            .filter_map(|r| r.file_name())
            .map(|n| n.to_string_lossy().to_ascii_lowercase())
            .collect();
    }

    fn save_removed(&mut self) {
        self.rebuild_removed_names();
        let text: String = self.removed.iter().map(|p| format!("{}\n", p.display())).collect();
        if let Err(e) = write_atomic(&self.removed_file(), &text) {
            self.status_message = format!("Could not save the list of removed tracks: {e}");
        }
    }

    /// Whether a linked library's entry is a track the user removed (also when the library knows it by another folder).
    fn was_removed(&self, p: &Path, files: &HashSet<PathBuf>) -> bool {
        self.removed.contains(p)
            || p.file_name().is_some_and(|n| self.removed_names.contains(&n.to_string_lossy().to_ascii_lowercase())) && !files.contains(p)
    }

    fn save_libraries(&mut self) {
        let text: String = self
            .libraries
            .iter()
            .map(|l| format!("{}\t{}\n", if l.kind == LibKind::Traktor { "traktor" } else { "rekordbox" }, l.path.display()))
            .collect();
        if let Err(e) = write_atomic(&self.libraries_file(), &text) {
            self.status_message = format!("Could not save the list of linked libraries: {e}");
        }
    }

    /// "rekordbox", or "rekordbox 2" for a second library of the same kind; also its playlist folder.
    pub fn library_name(&self, i: usize) -> String {
        let kind = self.libraries[i].kind;
        let n = self.libraries[..i].iter().filter(|l| l.kind == kind).count();
        if n == 0 { kind.label().to_string() } else { format!("{} {}", kind.label(), n + 1) }
    }

    /// Links (or re-reads) another app's library and merges it in the background.
    pub fn link_library(&mut self, kind: LibKind, path: PathBuf) {
        let i = match self.libraries.iter().position(|l| l.path == path) {
            Some(i) => i,
            None => {
                self.libraries.push(LinkedLibrary { kind, path, track_ids: Vec::new(), note: "Reading…".into() });
                self.save_libraries();
                self.libraries.len() - 1
            }
        };
        self.selected_nav = NavItem::Library(i);
        self.read_library(i);
    }

    pub fn read_library(&mut self, i: usize) {
        let Some(lib) = self.libraries.get_mut(i) else { return };
        lib.note = "Reading…".into();
        let (kind, path) = (lib.kind, lib.path.clone());
        self.spawn_job(move |tx| {
            let read = fs::read_to_string(&path).map_err(|e| e.to_string()).and_then(|t| kind.parse(&t)).map(|col| {
                let saved: Vec<PathBuf> = col.entries.iter().map(|e| PathBuf::from(e.location.to_posix_path())).collect();
                let mut files = listed_files(saved.iter().map(PathBuf::as_path));
                let root = volume_root(&col, &path, &files);
                if let Some(r) = &root {
                    let mapped: Vec<PathBuf> = saved.iter().map(|p| on_volume(r, p)).collect();
                    files.extend(listed_files(mapped.iter().map(PathBuf::as_path)));
                }
                (col, root, files)
            });
            let _ = tx.send(Msg::LibraryRead(path, read));
            String::new()
        });
    }

    /// Stops following a library: its playlist folder goes, its tracks stay in the collection.
    pub fn unlink_library(&mut self, i: usize) {
        if i >= self.libraries.len() {
            return;
        }
        let prefix = format!("{} / ", self.library_name(i));
        self.playlists.retain(|p| !p.name.starts_with(&prefix));
        let lib = self.libraries.remove(i);
        self.selected_nav = NavItem::Collection;
        self.save_libraries();
        self.save_missing_sidecar();
        self.touch();
        self.status_message = format!("Unlinked {}. Its tracks stay in your collection.", lib.path.display());
    }

    pub fn default_nml_path() -> PathBuf {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        PathBuf::from(home).join(".local/share/retraktor/collection.nml")
    }

    pub fn default_m3u8_path() -> PathBuf {
        Self::default_nml_path().with_extension("m3u8")
    }

    /// Looks for Traktor `collection.nml` files in the usual places, a few folders deep.
    pub fn start_detect(&mut self, music_folders: &[PathBuf]) {
        let home = env::var("HOME").map(PathBuf::from).unwrap_or_default();
        let user = env::var("USER").unwrap_or_default();
        let mut roots = vec![home.join("Documents/Native Instruments"), home.join("Music")];
        roots.extend(music_folders.iter().cloned());
        for base in [PathBuf::from("/media").join(&user), PathBuf::from("/run/media").join(&user), PathBuf::from("/mnt"), home.join(".wine/drive_c/users")] {
            roots.extend(fs::read_dir(base).into_iter().flatten().flatten().map(|e| e.path()));
        }
        self.spawn_job(move |tx| {
            let found = find_collections(&roots, 4, 4000);
            let n = found.len();
            let _ = tx.send(Msg::Detected(found));
            if n == 0 { String::new() } else { format!("Found {n} Traktor collection(s)") }
        });
    }

    /// Opens the desktop's own file/folder chooser without blocking the UI.
    pub fn pick(&self, kind: Pick) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let home = env::var("HOME").map(PathBuf::from).unwrap_or_default();
            let dialog = rfd::FileDialog::new().set_directory(&home);
            let picked = match kind {
                Pick::Collection => dialog.set_title("Choose your Traktor collection.nml").add_filter("Traktor collection", &["nml"]).pick_file(),
                Pick::Rekordbox => dialog
                    .set_title("Choose a rekordbox XML export (rekordbox: File > Export Collection in xml format)")
                    .add_filter("rekordbox XML", &["xml"])
                    .pick_file(),
                Pick::MusicFolder => dialog.set_title("Choose a music folder").pick_folder(),
                Pick::RelocateFolder => dialog.set_title("Choose the folder your missing tracks moved to").pick_folder(),
                Pick::TrackList => dialog
                    .set_title("Import track list")
                    .add_filter("Track list (*.csv, *.txt, *.m3u, *.m3u8)", &["csv", "txt", "m3u", "m3u8"])
                    .pick_file(),
            };
            if let Some(p) = picked {
                let _ = tx.send(Msg::Picked(kind, p));
            }
        });
    }

    /// Queues a `.nml` collection file (e.g. passed on the command line or opened from a file manager) to be linked.
    pub fn queue_nml(&mut self, path: PathBuf) {
        self.picked.push((Pick::Collection, path));
    }

    /// Processes background results and chosen files; call every frame, even while preferences are open.
    pub fn apply_picks(&mut self, music_folders: &mut Vec<PathBuf>) {
        crate::diag::stage("library: background results");
        self.poll();
        crate::diag::stage("library: picked files");
        if std::mem::take(&mut self.relocate_linked) && !music_folders.is_empty() {
            self.relocate_after_check = Some(music_folders.clone());
        }
        for (kind, path) in std::mem::take(&mut self.picked) {
            self.handle_pick(kind, path, music_folders);
        }
    }

    /// Acts on a chosen file or folder.
    fn handle_pick(&mut self, kind: Pick, path: PathBuf, music_folders: &mut Vec<PathBuf>) {
        match kind {
            Pick::Collection => self.link_library(LibKind::Traktor, path),
            Pick::Rekordbox => self.link_library(LibKind::Rekordbox, path),
            Pick::TrackList => {
                self.import_track_list_file(&path);
            }
            Pick::MusicFolder | Pick::RelocateFolder => {
                if !music_folders.contains(&path) {
                    music_folders.push(path.clone());
                }
                if kind == Pick::MusicFolder {
                    self.start_scan(vec![path]);
                } else {
                    self.start_relocate(vec![path]);
                }
            }
        }
    }

    /// Stores a deck-side beatgrid or tempo correction on the track so it survives a reload.
    pub fn set_grid(&mut self, id: usize, offset_sec: f64, bpm: f64) {
        let Some(t) = self.tracks.iter_mut().find(|t| t.id == id) else { return };
        t.bpm = bpm;
        if let Some(grid) = t.cues.iter_mut().find(|c| c.cue_type == retraktor_core::CueType::Grid) {
            grid.start_ms = offset_sec * 1000.0;
            grid.grid_bpm = Some(bpm);
        }
        self.touch();
    }

    /// A deck's analysis of track `id`, done after the deck became playable: fills what is still unknown, as any
    /// analysis does; auto hot cues only with `hot_cues` (no hand edit of its hot cues since the load).
    pub fn apply_late_analysis(&mut self, id: usize, a: &TrackAnalysis, hot_cues: bool) {
        if let Some(t) = self.track_mut(id) {
            t.apply_analysis(a, false, hot_cues);
            self.touch();
        }
    }

    /// See `cue_revisions`.
    pub fn cue_revision(&self, id: usize) -> u64 {
        self.cue_revisions.get(&id).copied().unwrap_or(0)
    }

    /// Sets an empty hotcue pad (1..=8) on the track so it survives a reload and autosaves to .nml.
    /// A pad that already holds a cue, loop or grid is left exactly as it is.
    pub fn add_hotcue(&mut self, id: usize, slot: u8, sec: f64) {
        if !(1..=8).contains(&slot) {
            return;
        }
        let Some(t) = self.track_mut(id).filter(|t| t.id == id) else { return };
        let hotcue = i64::from(slot) - 1;
        if t.cues.iter().any(|c| c.hotcue == hotcue) {
            return;
        }
        t.cues.push(NmlCue {
            name: format!("Hotcue {slot}"),
            displ_order: 0,
            cue_type: retraktor_core::CueType::Cue,
            start_ms: sec * 1000.0,
            len_ms: 0.0,
            repeats: -1,
            hotcue,
            grid_bpm: None,
        });
        *self.cue_revisions.entry(id).or_default() += 1;
        self.touch();
    }

    /// Clears one hotcue pad (1..=8), leaving every other cue untouched. A grid marker on the pad
    /// only leaves the pad, so the track keeps its beatgrid; a cue or loop on it is deleted.
    pub fn clear_hotcue(&mut self, id: usize, slot: u8) {
        if !(1..=8).contains(&slot) {
            return;
        }
        let Some(t) = self.track_mut(id).filter(|t| t.id == id) else { return };
        let hotcue = i64::from(slot) - 1;
        if !t.cues.iter().any(|c| c.hotcue == hotcue) {
            return;
        }
        for grid in t.cues.iter_mut().filter(|c| c.hotcue == hotcue && c.cue_type == retraktor_core::CueType::Grid) {
            grid.hotcue = -1;
        }
        t.cues.retain(|c| c.hotcue != hotcue);
        *self.cue_revisions.entry(id).or_default() += 1;
        self.touch();
    }

    fn touch(&mut self) {
        self.version += 1;
        self.dirty_since = Some(Instant::now());
    }

    pub(crate) fn rebuild_index(&mut self) {
        self.id_index = self.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
        self.next_track_id = self.tracks.iter().map(|t| t.id + 1).fold(self.next_track_id, usize::max);
    }

    pub fn track(&self, id: usize) -> Option<&UiTrackEntry> {
        self.id_index.get(&id).and_then(|&i| self.tracks.get(i))
    }

    fn track_mut(&mut self, id: usize) -> Option<&mut UiTrackEntry> {
        self.id_index.get(&id).and_then(|&i| self.tracks.get_mut(i))
    }

    fn next_id(&mut self) -> usize {
        // Entries pushed with their own ids (demo, tests) skip the index; rebuilding it raises the mark past them.
        // Only a stale index pays for that scan: one per added track made linking a big collection quadratic.
        if self.id_index.len() != self.tracks.len() {
            self.rebuild_index();
        }
        let id = self.next_track_id;
        self.next_track_id = id + 1;
        id
    }

    fn push_track(&mut self, mut t: UiTrackEntry) -> usize {
        // Adding a file again, any way, undoes an earlier removal.
        if t.file_path.as_ref().is_some_and(|p| self.removed.remove(p)) {
            self.save_removed();
        }
        t.id = self.next_id();
        let id = t.id;
        self.id_index.insert(id, self.tracks.len());
        self.tracks.push(t);
        id
    }

    pub fn missing_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.missing).count()
    }

    pub fn is_busy(&self) -> bool {
        self.jobs > 0
    }

    // ---------------------------------------------------------------- persistence

    fn snapshot(&self) -> NmlCollection {
        let mut col = NmlCollection::new(20);
        col.program = "ReTraktor".to_string();
        let mut keys: HashMap<usize, String> = HashMap::new();
        for t in &self.tracks {
            if let Some(e) = t.to_nml() {
                keys.insert(t.id, e.location.primary_key());
                col.entries.push(e);
            }
        }
        let nodes = self
            .playlists
            .iter()
            .enumerate()
            .map(|(i, pl)| NmlPlaylistNode::Playlist {
                name: pl.name.clone(),
                uuid: format!("retraktor-{i}"),
                track_keys: pl.track_ids.iter().filter_map(|id| keys.get(id).cloned()).collect(),
            })
            .collect();
        col.playlists.push(NmlPlaylistNode::Folder { name: "$ROOT".to_string(), subnodes: nodes });
        col
    }

    /// Saves the collection and playlists as Traktor `.nml` v20 (atomic write).
    pub fn save_collection_nml(&mut self, path: &Path) -> Result<usize, String> {
        let col = self.snapshot();
        write_atomic(path, &col.to_xml()).map_err(|e| e.to_string())?;
        if path == self.collection_path {
            self.dirty_since = None;
        }
        self.save_missing_sidecar();
        Ok(col.entries.len())
    }

    /// Exports the whole collection, or one playlist, as extended `.m3u8`.
    pub fn save_collection_m3u8(&self, path: &Path) -> Result<usize, String> {
        self.write_m3u8(path, "ReTraktor Collection", self.tracks.iter().map(|t| t.id).collect())
    }

    fn write_m3u8(&self, path: &Path, name: &str, ids: Vec<usize>) -> Result<usize, String> {
        let mut out = format!("#EXTM3U\n#PLAYLIST:{name}\n");
        let mut n = 0;
        for t in ids.iter().filter_map(|id| self.track(*id)) {
            if let Some(p) = &t.file_path {
                out.push_str(&format!("#EXTINF:{},{} - {}\n{}\n", t.duration_sec.round() as i64, t.artist, t.title, p.display()));
                n += 1;
            }
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        fs::write(path, out).map_err(|e| e.to_string())?;
        Ok(n)
    }

    fn autosave(&mut self) {
        let due = self.dirty_since.is_some_and(|t| t.elapsed() >= AUTOSAVE_DELAY);
        if !due || self.saving {
            return;
        }
        self.saving = true;
        self.dirty_since = None;
        let col = self.snapshot();
        let path = self.collection_path.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let n = col.entries.len();
            let _ = tx.send(Msg::Saved(write_atomic(&path, &col.to_xml()).map(|_| n).map_err(|e| e.to_string())));
        });
    }

    /// Merges a Traktor `.nml` (tracks, cues, grids, playlists) into the collection.
    pub fn import_nml_file(&mut self, path: &Path) -> Result<usize, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let col = NmlCollection::parse_xml(&content)?;
        let (added, _) = self.merge_collection(&col, None, None, &HashSet::new());
        self.status_message = format!("Imported {added} new tracks and {} playlists from {}", self.playlists.len(), path.display());
        Ok(col.entries.len())
    }

    /// Merges tracks and playlists into the collection; returns (new tracks, ids of every merged track).
    /// With `library`, its playlists live in a "<library> / …" folder that mirrors the source,
    /// files from another computer match yours by file name, and your tracks only gain what they lack.
    /// With `root`, saved paths that don't exist here are read under it (see [`volume_root`]).
    /// `files` says which of a linked library's paths exist, so the UI thread never asks the disk per track.
    fn merge_collection(&mut self, col: &NmlCollection, library: Option<&str>, root: Option<&Path>, files: &HashSet<PathBuf>) -> (usize, Vec<usize>) {
        let mut by_path: HashMap<PathBuf, usize> = self
            .tracks
            .iter()
            .filter_map(|t| Some((t.file_path.clone()?, t.id)))
            .collect();
        let file_key = |p: &Path| p.file_name().map(|n| n.to_string_lossy().to_lowercase());
        let mut by_name: HashMap<String, Vec<usize>> = HashMap::new();
        if library.is_some() {
            for t in &self.tracks {
                if let Some(k) = t.file_path.as_deref().and_then(file_key) {
                    by_name.entry(k).or_default().push(t.id);
                }
            }
        }
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let mut ids = Vec::with_capacity(col.entries.len());
        let mut added = 0;
        for e in &col.entries {
            let mut t = UiTrackEntry::from_nml(e);
            let saved = t.file_path.clone().unwrap_or_default();
            if let Some(r) = root.filter(|_| !files.contains(&saved)) {
                t.file_path = Some(on_volume(r, &saved));
            }
            let p = t.file_path.clone().unwrap_or_default();
            if library.is_some() && self.was_removed(&p, files) {
                continue;
            }
            let moved = || file_key(&p).and_then(|k| by_name.get(&k)).filter(|v| v.len() == 1 && !files.contains(&p)).map(|v| v[0]);
            // A track read before the volume was known sits at its saved path: move it onto the volume.
            let at_saved = || (p != saved).then(|| by_path.get(&saved).copied()).flatten();
            let id = match by_path.get(&p).copied().or_else(|| library.and_then(|_| at_saved().or_else(moved))) {
                Some(id) => {
                    if let Some(mine) = self.track_mut(id).filter(|m| p != saved && m.file_path.as_deref() == Some(saved.as_path())) {
                        mine.file_path = Some(p.clone());
                        by_path.insert(p.clone(), id);
                    }
                    if library.is_some() {
                        if let Some(mine) = self.track_mut(id) {
                            mine.adopt_from(t);
                        }
                    }
                    id
                }
                None => {
                    added += 1;
                    let id = self.push_track(t);
                    by_path.insert(p, id);
                    id
                }
            };
            ids.push(id);
            by_key.insert(e.location.primary_key(), id);
        }
        fn walk(node: &NmlPlaylistNode, prefix: &str, by_key: &HashMap<String, usize>, out: &mut Vec<UserPlaylist>) {
            match node {
                NmlPlaylistNode::Folder { name, subnodes } => {
                    let p = if name == "$ROOT" || prefix.is_empty() && name.is_empty() {
                        prefix.to_string()
                    } else if prefix.is_empty() {
                        name.clone()
                    } else {
                        format!("{prefix} / {name}")
                    };
                    subnodes.iter().for_each(|n| walk(n, &p, by_key, out));
                }
                NmlPlaylistNode::Playlist { name, track_keys, .. } => out.push(UserPlaylist {
                    name: if prefix.is_empty() { name.clone() } else { format!("{prefix} / {name}") },
                    track_ids: track_keys.iter().filter_map(|k| by_key.get(k).copied()).collect(),
                    missing: Vec::new(),
                }),
                NmlPlaylistNode::Smartlist { .. } => {}
            }
        }
        let mut imported = Vec::new();
        col.playlists.iter().for_each(|n| walk(n, "", &by_key, &mut imported));
        if let Some(lib) = library {
            let prefix = format!("{lib} / ");
            let names: HashSet<String> = imported.iter().map(|p| format!("{prefix}{}", p.name)).collect();
            let before = self.playlists.len();
            let selected = match self.selected_nav {
                NavItem::Playlist(i) => self.playlists.get(i).map(|p| p.name.clone()),
                _ => None,
            };
            self.playlists.retain(|p| !p.name.starts_with(&prefix) || names.contains(&p.name));
            for mut pl in imported {
                pl.name = format!("{prefix}{}", pl.name);
                match self.playlists.iter_mut().find(|p| p.name == pl.name) {
                    // Tracks added here are the DJ's work: a re-read (every start) only adds the library's new ones.
                    // shortcut: a track removed in the other app stays here; remove it here too, or track the last read if that matters.
                    Some(existing) => {
                        for id in pl.track_ids {
                            if !existing.track_ids.contains(&id) {
                                existing.track_ids.push(id);
                            }
                        }
                    }
                    None => self.playlists.push(pl),
                }
            }
            if self.playlists.len() != before || selected.is_some() {
                self.selected_nav = selected
                    .and_then(|n| self.playlists.iter().position(|p| p.name == n))
                    .map_or(if matches!(self.selected_nav, NavItem::Playlist(_)) { NavItem::Collection } else { self.selected_nav }, NavItem::Playlist);
            }
            self.touch();
            self.start_check();
            return (added, ids);
        }
        for pl in imported {
            match self.playlists.iter_mut().find(|p| p.name == pl.name) {
                Some(existing) => {
                    for id in pl.track_ids {
                        if !existing.track_ids.contains(&id) {
                            existing.track_ids.push(id);
                        }
                    }
                }
                None => self.playlists.push(pl),
            }
        }
        self.touch();
        self.start_check();
        (added, ids)
    }

    /// Imports a playlist from a track list file (.csv, .txt, .m3u, .m3u8).
    pub fn import_track_list_file(&mut self, path: &Path) {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.status_message = format!("Couldn't read {}: {e}", path.display());
                return;
            }
        };
        let wanted = crate::playlist_import::parse_track_list(&text);
        if wanted.is_empty() {
            self.status_message = format!("No tracks found in {}", path.display());
            return;
        }
        let (matched_ids, missing) = crate::playlist_import::match_tracks(&wanted, &self.tracks);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "Imported Playlist".to_string());

        let pl_idx = self.playlists.len();
        self.playlists.push(UserPlaylist {
            name,
            track_ids: matched_ids.clone(),
            missing,
        });
        self.selected_nav = NavItem::Playlist(pl_idx);
        let missing_count = self.playlists[pl_idx].missing.len();
        self.status_message = format!(
            "Imported {} of {} tracks · {} not in your library",
            matched_ids.len(),
            wanted.len(),
            missing_count
        );
        self.save_missing_sidecar();
        self.touch();
        self.ensure_view();
    }

    /// Bundled demo import for screenshot / visual check.
    pub fn demo_import_playlist(&mut self) {
        if self.tracks.is_empty() {
            self.tracks.push(UiTrackEntry {
                id: 1,
                title: "One More Time".to_string(),
                artist: "Daft Punk".to_string(),
                album: "Discovery".to_string(),
                bpm: 123.0,
                musical_key: "11d".to_string(),
                duration_sec: 320.0,
                genre: "House".to_string(),
                bitrate_kbps: 320,
                format: "FLAC".to_string(),
                file_path: Some(PathBuf::from("/music/daft_punk_one_more_time.flac")),
                missing: false,
                ..Default::default()
            });
            self.tracks.push(UiTrackEntry {
                id: 2,
                title: "Glue".to_string(),
                artist: "Bicep".to_string(),
                album: "Bicep".to_string(),
                bpm: 130.0,
                musical_key: "8m".to_string(),
                duration_sec: 269.0,
                genre: "Electronic".to_string(),
                bitrate_kbps: 320,
                format: "FLAC".to_string(),
                file_path: Some(PathBuf::from("/music/bicep_glue.flac")),
                missing: false,
                ..Default::default()
            });
            self.rebuild_index();
        }
        let test_list = "\
# Festival Warmup Track List
1. Daft Punk - One More Time
2. Bicep – Glue
3. Burial - Archangel
4. Four Tet - Baby
";
        let wanted = crate::playlist_import::parse_track_list(test_list);
        let (matched_ids, missing) = crate::playlist_import::match_tracks(&wanted, &self.tracks);
        let pl_idx = self.playlists.len();
        self.playlists.push(UserPlaylist {
            name: "Festival Warmup".to_string(),
            track_ids: matched_ids.clone(),
            missing: missing.clone(),
        });
        self.selected_nav = NavItem::Playlist(pl_idx);
        self.status_message = format!(
            "Imported {} of {} tracks · {} not in your library",
            matched_ids.len(),
            wanted.len(),
            missing.len()
        );
        self.save_missing_sidecar();
        self.touch();
        self.ensure_view();
    }

    fn missing_sidecar_path() -> PathBuf {
        let home = env::var("HOME").unwrap_or_else(|_| "/tmp".into());
        PathBuf::from(home).join(".config/retraktor/playlist-missing.tsv")
    }

    fn save_missing_sidecar(&self) {
        let mut out = String::new();
        for pl in &self.playlists {
            for m in &pl.missing {
                out.push_str(&format!("{}\t{}\t{}\n", pl.name, m.artist, m.title));
            }
        }
        let _ = write_atomic(&Self::missing_sidecar_path(), &out);
    }

    fn load_missing_sidecar(&mut self) {
        let path = Self::missing_sidecar_path();
        let Ok(text) = fs::read_to_string(path) else { return };
        for line in text.lines() {
            let mut parts = line.split('\t');
            let (Some(pl_name), Some(artist), Some(title)) = (parts.next(), parts.next(), parts.next()) else { continue };
            if let Some(pl) = self.playlists.iter_mut().find(|p| p.name == pl_name) {
                let w = crate::playlist_import::Wanted {
                    artist: artist.to_string(),
                    title: title.to_string(),
                };
                if !pl.missing.contains(&w) {
                    pl.missing.push(w);
                }
            }
        }
    }

    // ---------------------------------------------------------------- background jobs

    fn spawn_job(&mut self, f: impl FnOnce(&Sender<Msg>) -> String + Send + 'static) {
        self.jobs += 1;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let done = f(&tx);
            let _ = tx.send(Msg::Done(done));
        });
    }

    /// Finds new audio files under `roots` and reads their tags in the background.
    pub fn start_scan(&mut self, roots: Vec<PathBuf>) {
        let known: HashSet<PathBuf> = self.tracks.iter().filter_map(|t| t.file_path.clone()).collect();
        self.spawn_job(move |tx| {
            let mut batch = Vec::new();
            let mut total = 0usize;
            for root in &roots {
                walk_audio(root, &mut |p| {
                    if known.contains(&p) {
                        return;
                    }
                    batch.push(read_track_file(&p));
                    total += 1;
                    if batch.len() >= 100 {
                        let _ = tx.send(Msg::Scanned(std::mem::take(&mut batch)));
                        let _ = tx.send(Msg::Status(format!("Scanning: {total} new tracks…")));
                    }
                });
            }
            let _ = tx.send(Msg::Scanned(batch));
            format!("Scan finished: {total} new tracks added")
        });
    }

    /// Checks which files still exist and which have cached stems.
    fn start_check(&mut self) {
        let items: Vec<(usize, PathBuf)> = self.tracks.iter().filter_map(|t| Some((t.id, t.file_path.clone()?))).collect();
        self.spawn_job(move |tx| {
            let mut missing = 0;
            for chunk in items.chunks(500) {
                let res: Vec<_> = chunk
                    .iter()
                    .map(|(id, p)| {
                        let gone = !p.is_file();
                        missing += gone as usize;
                        (*id, gone, !gone && DemucsManager::get_cached_stem_dir(p).is_some())
                    })
                    .collect();
                let _ = tx.send(Msg::Checked(res));
            }
            if missing > 0 {
                format!("{missing} tracks not found at their saved location — use Relocate")
            } else {
                "All files found".to_string()
            }
        });
    }

    /// Re-links missing tracks by finding files with the same name under `roots`.
    pub fn start_relocate(&mut self, roots: Vec<PathBuf>) {
        let missing: Vec<(usize, String, String)> = self
            .tracks
            .iter()
            .filter(|t| t.missing)
            .filter_map(|t| {
                let p = t.file_path.as_ref()?;
                let name = p.file_name()?.to_string_lossy().to_lowercase();
                let parent = p.parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().to_lowercase()).unwrap_or_default();
                Some((t.id, name, parent))
            })
            .collect();
        if missing.is_empty() {
            self.status_message = "No missing tracks".to_string();
            return;
        }
        self.status_message = format!("Searching your music folders for {} missing tracks…", missing.len());
        self.spawn_job(move |tx| {
            let wanted: HashSet<&str> = missing.iter().map(|(_, n, _)| n.as_str()).collect();
            let mut found: HashMap<String, Vec<PathBuf>> = HashMap::new();
            let mut seen = 0usize;
            for root in &roots {
                walk_audio(root, &mut |p| {
                    seen += 1;
                    if seen % 2000 == 0 {
                        let _ = tx.send(Msg::Status(format!("Relocating: searched {seen} files…")));
                    }
                    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
                    if wanted.contains(name.as_str()) {
                        found.entry(name).or_default().push(p);
                    }
                });
            }
            // Overlapping roots (a folder and one inside it) find the same file twice.
            for cands in found.values_mut() {
                cands.sort();
                cands.dedup();
            }
            let mut ambiguous = 0usize;
            let fixed: Vec<(usize, PathBuf)> = missing
                .iter()
                .filter_map(|(id, name, parent)| {
                    let cands = found.get(name)?;
                    if let [only] = cands.as_slice() {
                        return Some((*id, only.clone()));
                    }
                    // Several files share the name: take the one in a folder named like the original, if exactly one is.
                    let mut same_folder = cands.iter().filter(|c| c.parent().and_then(|d| d.file_name()).is_some_and(|d| d.to_string_lossy().to_lowercase() == *parent));
                    match (same_folder.next(), same_folder.next()) {
                        (Some(best), None) => Some((*id, best.clone())),
                        _ => {
                            ambiguous += 1;
                            None
                        }
                    }
                })
                .collect();
            let n = fixed.len();
            let _ = tx.send(Msg::Relocated(fixed.clone()));
            read_tags_into(tx, &fixed);
            let mut done = format!("Relocated {n} of {} missing tracks", missing.len());
            if ambiguous > 0 {
                done += &format!(". Multiple matches for {ambiguous}. Use Search another folder…");
            }
            done
        });
    }

    /// Tracks with a file whose BPM, key or energy is still unknown.
    pub fn unanalyzed_ids(&self) -> Vec<usize> {
        self.tracks
            .iter()
            .filter(|t| !t.missing && t.file_path.is_some() && (t.bpm <= 0.0 || t.musical_key.is_empty() || t.energy == 0))
            .map(|t| t.id)
            .collect()
    }

    /// Decodes and analyses tracks on a few low-priority worker threads.
    pub fn start_analysis(&mut self, ids: Vec<usize>, overwrite: bool) {
        let items: Vec<(usize, PathBuf)> = ids.iter().filter_map(|id| Some((*id, self.track(*id)?.file_path.clone()?))).collect();
        let total = items.len();
        // shortcut: ~100 MB of PCM per worker for a 6-minute track; cap at 2 workers for 8 GB machines
        let workers = thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, 2));
        for chunk in items.chunks(total.div_ceil(workers).max(1)) {
            let chunk = chunk.to_vec();
            self.spawn_job(move |tx| {
                let mut ok = 0;
                for (n, (id, path)) in chunk.iter().enumerate() {
                    if let Ok((pcm, sr)) = crate::audio::LoadedTrackAudio::decode_file_to_pcm(path) {
                        let _ = tx.send(Msg::Analyzed(*id, analyze_mono(&stereo_to_mono(&pcm), sr), overwrite));
                        ok += 1;
                    }
                    if n % 5 == 0 {
                        let _ = tx.send(Msg::Status(format!("Analyzing: {} of {} in this batch…", n + 1, chunk.len())));
                    }
                }
                format!("Analyzed {ok} of {} tracks", chunk.len())
            });
        }
        if total == 0 {
            self.status_message = "Nothing to analyze".to_string();
        }
    }

    /// Fills empty fields of `ids` from their files' tags in the background.
    fn start_tag_fill(&mut self, ids: &[usize]) {
        let items: Vec<(usize, PathBuf)> = ids.iter().filter_map(|id| Some((*id, self.track(*id)?.file_path.clone()?))).collect();
        let n = items.len();
        self.spawn_job(move |tx| {
            read_tags_into(tx, &items);
            format!("Read tags from {n} files")
        });
    }

    /// Applies results from background jobs and triggers autosave. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                Msg::Picked(kind, p) => self.picked.push((kind, p)),
                Msg::Detected(found) => self.detected = Some(found),
                Msg::Status(s) => self.job_status = s,
                Msg::Scanned(batch) => {
                    crate::diag::stage("library: adding scanned tracks");
                    let known: HashSet<PathBuf> = self.tracks.iter().filter_map(|t| t.file_path.clone()).collect();
                    for t in batch {
                        if !t.file_path.as_ref().is_some_and(|p| known.contains(p)) {
                            let id = self.push_track(t);
                            self.scanned_ids.push(id);
                        }
                    }
                    self.touch();
                }
                Msg::Checked(res) => {
                    crate::diag::stage("library: missing-file check results");
                    for (id, missing, stems) in res {
                        if let Some(t) = self.track_mut(id) {
                            t.missing = missing;
                            t.stems_ready = stems;
                        }
                    }
                    self.version += 1;
                }
                Msg::Relocated(fixed) => {
                    crate::diag::stage("library: relinked tracks");
                    self.status_message = format!("Re-linked {} tracks; reading their tags…", fixed.len());
                    for (id, p) in fixed {
                        if let Some(t) = self.track_mut(id) {
                            t.file_path = Some(p);
                            t.missing = false;
                        }
                    }
                    self.touch();
                }
                Msg::Tags(batch) => {
                    crate::diag::stage("library: tag batch");
                    for (id, tags, cues) in batch {
                        if let Some(t) = self.track_mut(id).filter(|t| t.file_path == tags.file_path) {
                            t.adopt_serato_cues(cues);
                            t.fill_empty_from(tags);
                        }
                    }
                    self.touch();
                }
                Msg::Analyzed(id, a, overwrite) => {
                    crate::diag::stage("library: analysis result");
                    if let Some(t) = self.track_mut(id) {
                        t.apply_analysis(&a, overwrite, true);
                    }
                    self.touch();
                }
                Msg::Saved(Err(e)) => {
                    self.saving = false;
                    self.status_message = format!("Could not save collection: {e}");
                }
                Msg::Saved(Ok(_)) => self.saving = false,
                Msg::LibraryRead(path, read) => {
                    crate::diag::stage("library: merging a linked collection");
                    let t0 = std::time::Instant::now();
                    let Some(i) = self.libraries.iter().position(|l| l.path == path) else { continue };
                    let name = self.library_name(i);
                    match read {
                        Ok((col, root, files)) => {
                            let (added, ids) = self.merge_collection(&col, Some(&name), root.as_deref(), &files);
                            let prefix = format!("{name} / ");
                            let lists = self.playlists.iter().filter(|p| p.name.starts_with(&prefix)).count();
                            let lib = &mut self.libraries[i];
                            lib.note = format!("{} tracks, {lists} playlists ({added} new to your collection)", ids.len());
                            lib.track_ids = ids;
                            self.status_message = format!("{name}: {}", lib.note);
                            self.relocate_linked |= added > 0;
                            crate::diag::log(&format!("merged linked library {name}: {} tracks, {added} new, in {} ms", self.libraries[i].track_ids.len(), t0.elapsed().as_millis()));
                        }
                        Err(e) => {
                            self.libraries[i].note = format!("Couldn't read it: {e}");
                            self.status_message = format!("{name}: couldn't read {}: {e}", path.display());
                        }
                    }
                }
                Msg::Done(s) => {
                    crate::diag::stage("library: job done");
                    self.jobs = self.jobs.saturating_sub(1);
                    if !s.is_empty() {
                        self.status_message = s;
                    }
                    if self.jobs == 0 {
                        self.job_status.clear();
                        let scanned = std::mem::take(&mut self.scanned_ids);
                        if self.auto_analyze && !scanned.is_empty() {
                            self.start_analysis(scanned, false);
                        }
                        if self.missing_count() > 0 {
                            if let Some(roots) = self.relocate_after_check.take() {
                                self.start_relocate(roots);
                            }
                        }
                    }
                }
            }
        }
        crate::diag::stage("library: autosave snapshot");
        self.autosave();
    }

    // ---------------------------------------------------------------- selection & loading

    /// Track on deck A, the reference for Suggestions.
    fn reference(&self) -> Option<UiTrackEntry> {
        self.deck_tracks[0].and_then(|id| self.track(id)).cloned()
    }

    fn ensure_view(&mut self) {
        let reference = self.reference();
        let ref_s = reference.as_ref().map(|r| format!("{}@{}@{:.2}@{}", r.id, r.musical_key, r.bpm, r.energy)).unwrap_or_default();
        let stamp = (
            self.version,
            self.selected_nav,
            self.search_query.clone(),
            self.sort_col,
            self.sort_asc,
            ref_s,
            self.hide_missing_files,
            self.quick_filter.clone(),
        );
        if self.view_stamp.as_ref() == Some(&stamp) {
            return;
        }
        if self.id_index.len() != self.tracks.len() {
            self.rebuild_index();
        }
        let ids: Vec<usize> = match self.selected_nav {
            NavItem::Playlist(i) => self.playlists.get(i).map(|p| p.track_ids.clone()).unwrap_or_default(),
            NavItem::Library(i) => self.libraries.get(i).map(|l| l.track_ids.clone()).unwrap_or_default(),
            NavItem::History => self.history.iter().rev().copied().collect(),
            NavItem::Missing => self.tracks.iter().filter(|t| t.missing).map(|t| t.id).collect(),
            NavItem::Stems => self.tracks.iter().filter(|t| t.stems_ready).map(|t| t.id).collect(),
            _ => self.tracks.iter().map(|t| t.id).collect(),
        };
        let hide_missing = self.hide_missing_files && self.selected_nav != NavItem::Missing;
        let terms: Vec<String> = self.search_query.to_lowercase().split_whitespace().map(str::to_string).collect();
        let mut rows: Vec<(usize, f32)> = ids
            .into_iter()
            .filter_map(|id| self.id_index.get(&id).map(|&i| i))
            .filter(|&i| !(hide_missing && self.tracks[i].missing))
            .filter(|&i| self.quick_filter.matches(&self.tracks[i]))
            .filter(|&i| terms.is_empty() || matches_query(&self.tracks[i], &terms))
            .map(|i| (i, reference.as_ref().map_or(0.0, |r| match_score(r, &self.tracks[i]))))
            .collect();
        if self.selected_nav == NavItem::Suggestions {
            rows.retain(|&(i, s)| s >= 0.6 && reference.as_ref().is_some_and(|r| r.id != self.tracks[i].id));
        }
        let default_suggestions = self.selected_nav == NavItem::Suggestions && self.sort_col == SortColumn::Order;
        let col = if default_suggestions { SortColumn::Match } else { self.sort_col };
        let sort_asc = if default_suggestions { false } else { self.sort_asc };
        let t = &self.tracks;
        rows.sort_by(|a, b| {
            let (x, y) = (&t[a.0], &t[b.0]);
            let o = match col {
                SortColumn::Order | SortColumn::Preview => std::cmp::Ordering::Equal,
                SortColumn::TrackNum => x.track_number.cmp(&y.track_number),
                SortColumn::Title => cmp_ignore_case(&x.title, &y.title),
                SortColumn::Artist => cmp_ignore_case(&x.artist, &y.artist),
                SortColumn::Album => cmp_ignore_case(&x.album, &y.album),
                SortColumn::Bpm => x.bpm.total_cmp(&y.bpm),
                SortColumn::Key => KeyWheel::parse_open_key(&x.musical_key).cmp(&KeyWheel::parse_open_key(&y.musical_key)),
                SortColumn::Energy => x.energy.cmp(&y.energy),
                SortColumn::Duration => x.duration_sec.total_cmp(&y.duration_sec),
                SortColumn::Genre => cmp_ignore_case(&x.genre, &y.genre),
                SortColumn::Rating => x.rating.cmp(&y.rating),
                SortColumn::Comment => cmp_ignore_case(&x.comment, &y.comment),
                SortColumn::Remixer => cmp_ignore_case(&x.remixer, &y.remixer),
                SortColumn::Label => cmp_ignore_case(&x.label, &y.label),
                SortColumn::Year => x.year.cmp(&y.year),
                SortColumn::Bitrate => x.bitrate_kbps.cmp(&y.bitrate_kbps),
                SortColumn::Format => x.format.cmp(&y.format),
                SortColumn::Stems => x.stems_ready.cmp(&y.stems_ready),
                SortColumn::Match => a.1.total_cmp(&b.1),
            };
            if sort_asc { o } else { o.reverse() }
        });
        self.view = rows.iter().map(|r| r.0).collect();
        self.view_scores = rows.iter().map(|r| r.1).collect();
        self.view_stamp = Some(stamp);
    }

    fn view_pos(&self, id: usize) -> Option<usize> {
        self.view.iter().position(|&i| self.tracks[i].id == id)
    }

    fn select_offset(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        let pos = self.selected_track_id.and_then(|id| self.view_pos(id)).map_or(0, |p| (p as isize + delta).clamp(0, self.view.len() as isize - 1) as usize);
        let id = self.tracks[self.view[pos]].id;
        self.selected_track_id = Some(id);
        self.selection = BTreeSet::from([id]);
        self.scroll_to_selected = true;
    }

    /// Moves the sidebar selection `step` entries down (up when negative), in the order the sidebar draws them,
    /// stopping at either end: SHIFT + a browse encoder, as in Traktor, so playlists change from the deck.
    pub fn select_nav_offset(&mut self, step: i32) {
        let mut order = vec![NavItem::Collection];
        if self.missing_count() > 0 {
            order.push(NavItem::Missing);
        }
        order.extend([NavItem::Suggestions, NavItem::Stems, NavItem::History, NavItem::Explorer]);
        order.extend((0..self.libraries.len()).map(NavItem::Library));
        order.extend((0..self.playlists.len()).map(NavItem::Playlist));
        let at = order.iter().position(|n| *n == self.selected_nav).unwrap_or(0) as i32;
        let next = order[(at + step).clamp(0, order.len() as i32 - 1) as usize];
        if next != self.selected_nav {
            self.selected_nav = next;
            self.selection.clear();
        }
    }

    pub fn select_next_track(&mut self) {
        self.select_offset(1);
    }

    pub fn select_prev_track(&mut self) {
        self.select_offset(-1);
    }

    pub fn selected_ids(&self) -> Vec<usize> {
        if !self.selection.is_empty() {
            self.selection.iter().copied().collect()
        } else if let Some(id) = self.selected_track_id {
            vec![id]
        } else {
            Vec::new()
        }
    }

    fn selected_or(&self, id: usize) -> Vec<usize> {
        if self.selection.contains(&id) {
            self.selection.iter().copied().collect()
        } else {
            vec![id]
        }
    }

    /// Dev screenshot step: the editor open on the first six tracks in view.
    pub fn shot_bulk_edit(&mut self) {
        let ids: Vec<usize> = self.view.iter().take(6).map(|&i| self.tracks[i].id).collect();
        self.selection = ids.iter().copied().collect();
        self.open_editor(&ids);
        if let Some(f) = self.editing.as_mut().and_then(|ed| ed.fields.iter_mut().find(|f| f.0 == SortColumn::Genre)) {
            f.2 = "UK Garage".into();
        }
    }

    /// Dev screenshot step: the second track's Genre cell open for typing.
    pub fn shot_cell_edit(&mut self) {
        if let Some(t) = self.view.get(1).map(|&i| &self.tracks[i]) {
            self.selection = BTreeSet::from([t.id]);
            self.cell_edit = Some((t.id, SortColumn::Genre, field_text(t, SortColumn::Genre), true));
        }
    }

    /// Opens the editor on these tracks (one, or many at once).
    fn open_editor(&mut self, ids: &[usize]) {
        let tracks: Vec<&UiTrackEntry> = ids.iter().filter_map(|&id| self.track(id)).collect();
        let editor = (!tracks.is_empty()).then(|| Editor::new(&tracks));
        if editor.is_some() {
            self.cell_edit = None;
            self.editing = editor;
        }
    }

    /// Writes `changes` to every track in `ids`, and scales each one's BPM by `bpm_scale` unless BPM is among
    /// them, as one undoable edit. Every value is checked first, so a bad one changes nothing.
    fn edit_tracks(&mut self, ids: &[usize], changes: &[(SortColumn, String)], bpm_scale: f64) -> Result<(), String> {
        let mut probe = UiTrackEntry::default();
        for (c, v) in changes {
            set_field(&mut probe, *c, v)?;
        }
        let scale = bpm_scale != 1.0 && !changes.iter().any(|(c, _)| *c == SortColumn::Bpm);
        if changes.is_empty() && !scale {
            return Ok(());
        }
        if scale {
            let over: Vec<&str> = ids.iter().filter_map(|&id| self.track(id)).filter(|t| t.bpm * bpm_scale > 250.0).map(|t| t.title.as_str()).collect();
            if let Some(first) = over.first() {
                return Err(format!("That would put {} over 250 BPM, e.g. '{first}'. Nothing was changed.", plural(over.len())));
            }
        }
        let what = match (changes, ids) {
            ([(c, _)], [id]) => format!("edited {} of '{}'", field_name(*c), self.track(*id).map_or(String::new(), |t| t.title.clone())),
            _ => format!("edited {}", plural(ids.len())),
        };
        self.remember_for_undo(what);
        for &id in ids {
            if let Some(t) = self.track_mut(id) {
                for (c, v) in changes {
                    let _ = set_field(t, *c, v);
                }
                if scale {
                    t.bpm *= bpm_scale;
                }
            }
        }
        self.touch();
        Ok(())
    }

    fn remember_for_undo(&mut self, what: String) {
        self.undo = Some((what, self.tracks.clone(), self.playlists.clone()));
    }

    pub fn undo(&mut self) {
        if let Some((what, tracks, playlists)) = self.undo.take() {
            self.tracks = tracks;
            self.playlists = playlists;
            let back: Vec<PathBuf> = self.tracks.iter().filter_map(|t| t.file_path.clone()).collect();
            if back.iter().fold(false, |any, p| self.removed.remove(p) || any) {
                self.save_removed();
            }
            self.selection.clear();
            self.rebuild_index();
            self.status_message = format!("Undid: {what}");
            self.touch();
        }
    }

    pub fn finish_deck_load(&mut self, d: usize, id: usize, stems_ready: bool) {
        self.deck_tracks[d] = Some(id);
        self.history.push(id);
        self.last_loaded_deck = Some((d, stems_ready));
        self.version += 1;
    }

    /// Queues `id` for loading onto deck `deck` (1..=4) after safety checks.
    pub fn load_to_deck(&mut self, deck: usize, id: usize, locked: [bool; 4]) {
        let d = deck.clamp(1, 4) - 1;
        let Some(t) = self.track(id).cloned() else { return };
        if locked[d] {
            self.status_message = format!("Deck {} is playing and locked — not loaded", DECK_NAMES[d]);
            self.deck_refused[d] = Some(std::time::Instant::now());
        } else if !t.file_path.as_ref().is_some_and(|p| p.is_file()) {
            self.status_message = format!("File not found for '{}' — use Relocate", t.title);
        } else {
            self.status_message = format!("Loading '{}' on deck {}", t.title, DECK_NAMES[d]);
            self.pending_loads.push((deck, t));
            self.version += 1;
        }
    }

    /// Queues tracks for Demucs, skipping ones already queued or already separated. Returns how many were added.
    pub fn queue_stems(&mut self, ids: &[usize]) -> usize {
        let before = self.stem_queue.len();
        for &id in ids {
            let ready = self.tracks.iter().find(|t| t.id == id).map_or(true, |t| t.stems_ready || t.file_path.is_none());
            if !ready && !self.stem_queue.contains(&id) {
                self.stem_queue.push_back(id);
            }
        }
        self.stem_queue.len() - before
    }

    fn add_to_playlist(&mut self, ids: &[usize], playlist: Option<usize>) {
        let idx = playlist.unwrap_or_else(|| {
            self.playlists.push(UserPlaylist { name: format!("New playlist {}", self.playlists.len() + 1), track_ids: Vec::new(), missing: Vec::new() });
            // Empty box with the default as a hint, so typing replaces it rather than appending.
            self.renaming = Some((self.playlists.len() - 1, String::new()));
            self.playlists.len() - 1
        });
        let Some(pl) = self.playlists.get_mut(idx) else { return };
        let before = pl.track_ids.len();
        for &id in ids {
            if !pl.track_ids.contains(&id) {
                pl.track_ids.push(id);
            }
        }
        self.status_message = if ids.is_empty() {
            "New playlist: type a name and press Enter".into()
        } else {
            format!("Added {} tracks to '{}'", pl.track_ids.len() - before, pl.name)
        };
        self.touch();
    }

    /// ADD TO LIST on a controller: the selected track joins the Preparation list, as in Traktor.
    pub fn add_selected_to_preparation(&mut self) {
        let Some(id) = self.selected_track_id else {
            self.status_message = "Select a track first".into();
            return;
        };
        let idx = self.preparation_index();
        self.add_to_playlist(&[id], Some(idx));
    }

    /// The Set view's Save: playlist `name` (made if missing) becomes `ids`, in that order. Ctrl+Z undoes it.
    pub fn save_playlist(&mut self, name: &str, ids: &[usize]) {
        self.remember_for_undo(format!("saved the set to '{name}'"));
        match self.playlists.iter_mut().find(|p| p.name == name) {
            Some(p) => p.track_ids = ids.to_vec(),
            None => self.playlists.push(UserPlaylist { name: name.to_string(), track_ids: ids.to_vec(), missing: Vec::new() }),
        }
        self.status_message = format!("Saved {} to playlist '{name}' · Ctrl+Z in the library undoes it", plural(ids.len()));
        self.touch();
    }

    fn preparation_index(&mut self) -> usize {
        self.playlists.iter().position(|p| p.name == "Preparation").unwrap_or_else(|| {
            self.playlists.push(UserPlaylist { name: "Preparation".to_string(), track_ids: Vec::new(), missing: Vec::new() });
            self.playlists.len() - 1
        })
    }

    fn apply(&mut self, action: Action, locked: [bool; 4], deck_count: usize) {
        match action {
            Action::Select(id, m) => {
                if m.command {
                    if !self.selection.remove(&id) {
                        self.selection.insert(id);
                    }
                } else if m.shift {
                    if let (Some(a), Some(b)) = (self.selected_track_id.and_then(|s| self.view_pos(s)), self.view_pos(id)) {
                        let (lo, hi) = (a.min(b), a.max(b));
                        self.selection = self.view[lo..=hi].iter().map(|&i| self.tracks[i].id).collect();
                        return;
                    }
                } else {
                    self.selection = BTreeSet::from([id]);
                }
                self.selected_track_id = Some(id);
            }
            Action::Load(deck, id) => self.load_to_deck(deck, id, locked),
            Action::LoadAuto(id) => match (0..deck_count).find(|&d| !locked[d]) {
                Some(d) => self.load_to_deck(d + 1, id, locked),
                None => self.status_message = "All decks are playing and locked".to_string(),
            },
            Action::AddToPlaylist(ids, pl) => self.add_to_playlist(&ids, pl),
            Action::AddToSet(ids) => {
                let n = ids.len();
                self.add_to_set_requested = Some(ids);
                self.status_message = format!("Added {} to set list", plural(n));
            }
            Action::RemoveFromPlaylist(ids) => {
                if let NavItem::Playlist(i) = self.selected_nav {
                    if let Some(name) = self.playlists.get(i).map(|p| p.name.clone()) {
                        self.remember_for_undo(format!("removed {} from '{name}'", plural(ids.len())));
                        self.playlists[i].track_ids.retain(|id| !ids.contains(id));
                        self.status_message = format!("Removed {} from '{name}'", plural(ids.len()));
                        self.touch();
                    }
                }
            }
            Action::Edit(ids) => self.open_editor(&ids),
            Action::FillTags(ids) => self.start_tag_fill(&ids),
            Action::Analyze(ids) => self.start_analysis(ids, true),
            Action::Stems(id) => {
                self.queue_stems(&[id]);
            }
            Action::Reveal(p) => {
                let dir = p.parent().unwrap_or(&p).to_path_buf();
                if let Err(e) = std::process::Command::new("xdg-open").arg(&dir).spawn() {
                    self.status_message = format!("Could not open {}: {e}", dir.display());
                }
            }
            Action::RemoveFromCollection(ids) => {
                self.remember_for_undo(format!("removed {} from the collection", plural(ids.len())));
                let paths: Vec<PathBuf> = ids.iter().filter_map(|&id| self.track(id)?.file_path.clone()).collect();
                self.removed.extend(paths);
                self.save_removed();
                self.tracks.retain(|t| !ids.contains(&t.id));
                for pl in &mut self.playlists {
                    pl.track_ids.retain(|id| !ids.contains(id));
                }
                self.selection.clear();
                self.rebuild_index();
                self.status_message = format!("Removed {} from the collection (files untouched)", plural(ids.len()));
                self.touch();
            }
            Action::Sort(col) => {
                if self.sort_col == col {
                    self.sort_asc = !self.sort_asc;
                } else {
                    self.sort_col = col;
                    self.sort_asc = col != SortColumn::Match;
                }
            }
            Action::QuickFilter(f) => match f {
                QuickFilterAction::Bpm(v) => self.quick_filter.toggle_bpm(v),
                QuickFilterAction::Key(k) => self.quick_filter.toggle_key(&k),
                QuickFilterAction::Energy(e) => self.quick_filter.toggle_energy(e),
                QuickFilterAction::Genre(g) => self.quick_filter.toggle_genre(&g),
            },
        }
    }

    // ---------------------------------------------------------------- UI

    fn nav_button(&mut self, ui: &mut Ui, label: String, item: NavItem, drop_target: Option<Option<usize>>) -> egui::Response {
        let selected = self.selected_nav == item;
        // A left-aligned layout makes the full-width button's text left-aligned too.
        let resp = ui
            .with_layout(Layout::top_down(Align::Min), |ui| {
                ui.add(
                    egui::Button::new(RichText::new(label).size(12.0).color(TraktorTheme::TEXT_PRIMARY()))
                        .fill(if selected { TraktorTheme::CONTROL_HOVER() } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .min_size(egui::vec2(ui.available_width(), 22.0)),
                )
            })
            .inner;
        // Selection is a neutral fill plus a bar, so red stays for deck B and actions.
        if selected {
            let r = resp.rect;
            ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height())), 1.0, TraktorTheme::ACCENT());
        }
        if resp.clicked() {
            self.selected_nav = item;
            self.selection.clear();
        }
        if let Some(target) = drop_target {
            if resp.dnd_hover_payload::<DragTracks>().is_some() {
                ui.painter().rect_stroke(resp.rect, 3.0, Stroke::new(1.5_f32, TraktorTheme::DECK_A()));
            }
            if let Some(p) = resp.dnd_release_payload::<DragTracks>() {
                self.add_to_playlist(&p.0, target);
            }
        }
        resp
    }

    pub fn show_nav_tree(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().id_salt("nav_tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let missing = self.missing_count();
            let stems = self.tracks.iter().filter(|t| t.stems_ready).count();
            self.nav_button(ui, format!("Collection  {}", self.tracks.len()), NavItem::Collection, None);
            if missing > 0 {
                self.nav_button(ui, format!("Missing files  {missing}"), NavItem::Missing, None);
            }
            self.nav_button(ui, "Suggestions".to_string(), NavItem::Suggestions, None);
            self.nav_button(ui, format!("Stems  {stems}"), NavItem::Stems, None);
            self.nav_button(ui, format!("History  {}", self.history.len()), NavItem::History, None);
            self.nav_button(ui, "Files".to_string(), NavItem::Explorer, None);

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("LIBRARIES").size(10.5).color(TraktorTheme::TEXT_MUTED()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.menu_button("Link…", |ui| {
                        if ui.button("Traktor collection (.nml)…").clicked() {
                            self.pick(Pick::Collection);
                            ui.close_menu();
                        }
                        if ui.button("rekordbox XML…").on_hover_text("In rekordbox: File > Export Collection in xml format, then pick that file").clicked() {
                            self.pick(Pick::Rekordbox);
                            ui.close_menu();
                        }
                        let found: Vec<PathBuf> = self.detected.clone().unwrap_or_default().into_iter().filter(|p| !self.libraries.iter().any(|l| &l.path == p)).collect();
                        for p in found {
                            if ui.button(format!("Traktor: {}", p.display())).clicked() {
                                self.link_library(LibKind::Traktor, p);
                                ui.close_menu();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Show another DJ app's library here. ReTraktor reads it and never changes it.");
                });
            });
            if self.libraries.is_empty() {
                ui.label(RichText::new("Link Traktor or rekordbox to use their playlists here.").size(11.0).color(TraktorTheme::TEXT_SECONDARY()));
            }
            let mut unlink = None;
            let mut reread = None;
            for i in 0..self.libraries.len() {
                let label = format!("{}  {}", self.library_name(i), self.libraries[i].track_ids.len());
                let lib = &self.libraries[i];
                let tip = format!("{}\n{}", lib.note, lib.path.display());
                let resp = self.nav_button(ui, label, NavItem::Library(i), None).on_hover_text(tip);
                resp.context_menu(|ui| {
                    if ui.button("Read again").on_hover_text("Pick up changes made in the other app").clicked() {
                        reread = Some(i);
                        ui.close_menu();
                    }
                    if ui.button("Unlink").on_hover_text("Remove its playlist folder; its tracks stay in your collection").clicked() {
                        unlink = Some(i);
                        ui.close_menu();
                    }
                });
            }
            if let Some(i) = reread {
                self.read_library(i);
            }
            if let Some(i) = unlink {
                self.unlink_library(i);
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("PLAYLISTS").size(10.5).color(TraktorTheme::TEXT_MUTED()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("+").on_hover_text("New playlist (or drop tracks here)").clicked() {
                        self.add_to_playlist(&[], None);
                        self.selected_nav = NavItem::Playlist(self.playlists.len() - 1);
                    }
                    if ui.small_button("Import…").on_hover_text("Import track list from CSV, M3U or text").clicked() {
                        self.pick(Pick::TrackList);
                    }
                });
            });

            // Entry point for AI playlist generator
            let ai_sel = self.show_ai_playlist_panel;
            let ai_btn = egui::Button::new(
                RichText::new("AI playlist")
                    .size(11.5)
                    .color(TraktorTheme::ACCENT()),
            )
            .fill(if ai_sel { TraktorTheme::CONTROL_HOVER() } else { Color32::TRANSPARENT })
            .rounding(3.0)
            .min_size(egui::vec2(ui.available_width(), 22.0));
            if ui.add(ai_btn).on_hover_text("Build a playlist from a sentence with AI").clicked() {
                self.show_ai_playlist_panel = !self.show_ai_playlist_panel;
                self.ai_settings_cache = self.show_ai_playlist_panel.then(crate::ai::AiSettings::load);
            }

            let mut delete = None;
            let mut export = None;
            for i in 0..self.playlists.len() {
                if let Some((ri, buf)) = &mut self.renaming {
                    if *ri == i {
                        let r = ui.add(egui::TextEdit::singleline(buf).hint_text(self.playlists[i].name.as_str()).desired_width(f32::INFINITY));
                        // Re-requesting focus every frame swallowed Enter, so the name could never be committed.
                        if !r.has_focus() && !r.lost_focus() {
                            r.request_focus();
                        }
                        if r.lost_focus() {
                            let name = buf.trim().to_string();
                            if !name.is_empty() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.playlists[i].name = name;
                                self.save_missing_sidecar();
                                self.touch();
                            }
                            self.renaming = None;
                        }
                        continue;
                    }
                }
                let label = format!("{}  {}", self.playlists[i].name, self.playlists[i].track_ids.len());
                let resp = self.nav_button(ui, label, NavItem::Playlist(i), Some(Some(i)));
                resp.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        self.renaming = Some((i, self.playlists[i].name.clone()));
                        ui.close_menu();
                    }
                    if ui.button("Import track list…").clicked() {
                        self.pick(Pick::TrackList);
                        ui.close_menu();
                    }
                    if ui.button("Export as M3U8").clicked() {
                        export = Some(i);
                        ui.close_menu();
                    }
                    if ui.button("Prepare stems").on_hover_text("Separate every track in this playlist with Demucs, one after another").clicked() {
                        let ids = self.playlists[i].track_ids.clone();
                        let n = self.queue_stems(&ids);
                        self.status_message = format!("Queued {n} tracks for stems ({} waiting). Leave ReTraktor open; it runs in the background.", self.stem_queue.len());
                        ui.close_menu();
                    }
                    if ui.button("Delete playlist").clicked() {
                        delete = Some(i);
                        ui.close_menu();
                    }
                });
            }
            // Drop zone that creates a playlist from the dragged tracks.
            if egui::DragAndDrop::has_payload_of_type::<DragTracks>(ui.ctx()) {
                self.nav_button(ui, "+ Drop here for new playlist".to_string(), NavItem::Collection, Some(None));
            }
            if let Some(i) = export {
                let pl = &self.playlists[i];
                let safe: String = pl.name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
                let path = env::var("HOME").map(PathBuf::from).unwrap_or_default().join("Music").join(format!("{safe}.m3u8"));
                self.status_message = match self.write_m3u8(&path, &pl.name, pl.track_ids.clone()) {
                    Ok(n) => format!("Exported {n} tracks to {}", path.display()),
                    Err(e) => format!("Export failed: {e}"),
                };
            }
            if let Some(i) = delete {
                self.remember_for_undo(format!("deleted playlist '{}'", self.playlists[i].name));
                self.status_message = format!("Deleted playlist '{}'", self.playlists[i].name);
                self.playlists.remove(i);
                self.selected_nav = NavItem::Collection;
                self.save_missing_sidecar();
                self.touch();
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn show_browser_content(
        &mut self,
        ui: &mut Ui,
        four_deck_mode: bool,
        locked: [bool; 4],
        columns: &mut LibraryColumnsConfig,
        music_folders: &mut Vec<PathBuf>,
    ) {
        self.apply_picks(music_folders);
        let deck_count = if four_deck_mode { 4 } else { 2 };
        // Status bar first (bottom) so the table can take the remaining height.
        egui::TopBottomPanel::bottom("library_status").show_inside(ui, |ui| {
            // Right side first, so Undo and Stop preview keep their place however long the message or narrow the window.
            ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{} shown", self.view.len())).size(11.0).color(TraktorTheme::TEXT_MUTED()));
                // Here rather than in the track toolbar: visible in every browser view, whatever is selected.
                if let Some((id, loading)) = self.previewing {
                    let title = self.track(id).map_or("the track", |t| t.title.as_str());
                    let hint = if loading { format!("Stop loading '{title}' for preview") } else { format!("Stop previewing '{title}'") };
                    if ui.button("Stop preview").on_hover_text(hint).clicked() {
                        self.preview_requested = Some(None);
                    }
                }
                if self.undo.is_some() && ui.small_button("Undo").on_hover_text("Ctrl+Z").clicked() {
                    self.undo();
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if self.is_busy() {
                        ui.spinner();
                        ui.label(RichText::new(&self.job_status).size(11.0).color(TraktorTheme::TEXT_SECONDARY()));
                    }
                    let tip = self.history.is_empty() && !self.tracks.is_empty() && self.status_message.is_empty();
                    let msg = if tip { "Double-click a track, or drag it onto a deck, to load it." } else { self.status_message.as_str() };
                    ui.add(egui::Label::new(RichText::new(msg).size(12.0).color(TraktorTheme::TEXT_PRIMARY())).truncate());
                });
            }));
        });

        if let NavItem::Playlist(i) = self.selected_nav {
            if let Some(pl) = self.playlists.get(i) {
                if !pl.missing.is_empty() {
                    egui::TopBottomPanel::bottom("playlist_missing").show_inside(ui, |ui| {
                        Self::show_missing_shopping_list(ui, &pl.missing);
                    });
                }
            }
        }

        ui.vertical(|ui| {
            if !self.show_ai_playlist_panel || self.keys_blocked {
                self.ai_settings_cache = None;
            }
            if self.show_ai_playlist_panel {
                self.show_ai_playlist_panel_ui(ui);
                ui.add_space(6.0);
            }
            if self.selected_nav == NavItem::Explorer {
                self.show_explorer(ui, locked, deck_count, music_folders);
            } else if self.tracks.is_empty() {
                self.show_empty_state(ui, music_folders);
            } else {
                self.show_toolbar(ui, music_folders, columns, locked, deck_count);
                if self.selected_nav == NavItem::Missing {
                    self.show_missing_banner(ui, music_folders);
                }
                if self.selected_nav == NavItem::Suggestions && self.deck_tracks[0].is_none() {
                    ui.label(RichText::new("Load a track on deck A to see matching tracks.").color(TraktorTheme::TEXT_SECONDARY()));
                }
                self.ensure_view();
                self.show_table(ui, columns, locked, deck_count, four_deck_mode);
            }
        });
        self.show_editor(ui.ctx());

        if let Some(p) = egui::DragAndDrop::payload::<DragTracks>(ui.ctx()) {
            let n = p.0.len();
            egui::show_tooltip_at_pointer(ui.ctx(), ui.layer_id(), egui::Id::new("drag_tracks_tip"), |ui| {
                ui.label(if n == 1 { "1 track".to_string() } else { format!("{n} tracks") });
            });
        }
    }

    fn show_ai_playlist_panel_ui(&mut self, ui: &mut Ui) {
        // Poll background AI playlist job if active
        if let Some(runner) = &self.ai_playlist_runner {
            if let Some(res) = runner.poll() {
                match res {
                    Ok(ids) => {
                        let name = if self.ai_prompt_input.trim().is_empty() {
                            "AI Playlist".to_string()
                        } else {
                            self.ai_prompt_input.trim().to_string()
                        };
                        self.remember_for_undo(format!("created AI playlist '{name}'"));
                        self.playlists.push(UserPlaylist { name: name.clone(), track_ids: ids.clone(), missing: Vec::new() });
                        let new_idx = self.playlists.len() - 1;
                        self.selected_nav = NavItem::Playlist(new_idx);
                        self.status_message = format!("Created playlist '{name}' with {} tracks", ids.len());
                        self.touch();
                        self.ai_playlist_runner = None;
                        self.ai_playlist_status = None;
                        self.show_ai_playlist_panel = false;
                    }
                    Err(e) => {
                        self.ai_playlist_status = Some(format!("Error: {e}"));
                        self.ai_playlist_runner = None;
                    }
                }
            }
        }

        let ai_settings = self.ai_settings_cache.get_or_insert_with(crate::ai::AiSettings::load).clone();

        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL_ALT())
            .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_ACCENT()))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(10.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("AI playlist").color(TraktorTheme::TEXT_PRIMARY()).strong());
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("✕").on_hover_text("Close").clicked() {
                            self.show_ai_playlist_panel = false;
                        }
                    });
                });
                ui.add_space(4.0);

                if !ai_settings.is_configured() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("To build playlists with AI, configure your API key in Settings.")
                                .color(TraktorTheme::TEXT_SECONDARY())
                                .font(egui::FontId::proportional(12.0)),
                        );
                        if ui
                            .add(egui::Button::new(RichText::new("Open AI Settings").color(TraktorTheme::ACCENT()).underline()).frame(false))
                            .clicked()
                        {
                            self.open_prefs_ai = true;
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(RichText::new("Prompt:").color(TraktorTheme::TEXT_SECONDARY()).size(12.0));
                        let w = (ui.available_width() - 200.0).max(140.0);
                        ui.add(
                            egui::TextEdit::singleline(&mut self.ai_prompt_input)
                                .hint_text("e.g. 45 minutes of warm-up house, 118 to 124, ending bright")
                                .desired_width(w),
                        );

                        ui.add(egui::DragValue::new(&mut self.ai_target_duration_mins).range(5..=300).suffix(" min"));

                        let is_busy = self.ai_playlist_runner.is_some();
                        let btn = egui::Button::new(
                            RichText::new(if is_busy { "BUILDING…" } else { "BUILD" })
                                .color(crate::theme::on(TraktorTheme::ROW_SELECTED()))
                                .strong(),
                        )
                        .fill(TraktorTheme::ROW_SELECTED())
                        .rounding(3.0)
                        .min_size(egui::vec2(75.0, 24.0));

                        if ui.add_enabled(!is_busy && !self.ai_prompt_input.trim().is_empty(), btn).clicked() {
                            let max_tracks = ((self.ai_target_duration_mins as f64 * 60.0) / 210.0).round().max(3.0) as usize;
                            let (briefs, total) = crate::ai::prefilter_tracks(&self.tracks, &self.ai_prompt_input, 1500);
                            self.ai_playlist_sent_info = Some((briefs.len(), total));
                            let valid_ids: Vec<usize> = briefs.iter().map(|b| b.id).collect();
                            let prompt = crate::ai::build_playlist_prompt(&self.ai_prompt_input, &briefs, max_tracks);
                            let status_text = if total > 1500 {
                                format!("Sent {} of {} tracks to AI (pre-filtered by request)…", briefs.len(), total)
                            } else {
                                format!("Analyzing {} tracks with {}…", briefs.len(), ai_settings.provider.label())
                            };
                            self.ai_playlist_status = Some(status_text);
                            self.ai_playlist_runner = Some(crate::ai::AiPlaylistRunner::start(ai_settings, prompt, valid_ids));
                        }
                    });

                    if let Some(status) = &self.ai_playlist_status {
                        ui.add_space(3.0);
                        ui.horizontal(|ui| {
                            if self.ai_playlist_runner.is_some() {
                                ui.spinner();
                                ui.label(RichText::new(status).color(TraktorTheme::METER_YELLOW()).size(11.5));
                            } else if status.starts_with("Error") {
                                ui.label(RichText::new(status).color(TraktorTheme::CUE_ACTIVE()).size(11.5));
                            } else {
                                ui.label(RichText::new(status).color(TraktorTheme::PLAY_ACTIVE()).size(11.5));
                            }
                        });
                    }
                }
            });
    }

    fn show_missing_banner(&mut self, ui: &mut Ui, music_folders: &[PathBuf]) {
        let missing = self.missing_count();
        if missing == 0 {
            return;
        }
        let busy = self.is_busy();
        egui::Frame::none().fill(TraktorTheme::BG_PANEL()).stroke(Stroke::new(1.0_f32, TraktorTheme::METER_RED())).rounding(4.0).inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} can't be found where they were saved.", plural(missing))).size(12.5).color(TraktorTheme::TEXT_PRIMARY()));
                if ui.add_enabled(!busy, egui::Button::new("Search another folder…")).on_hover_text("Pick the folder they moved to").clicked() {
                    self.pick(Pick::RelocateFolder);
                }
                if !music_folders.is_empty()
                    && ui.add_enabled(!busy, egui::Button::new("Search my music folders")).on_hover_text(music_folders.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("\n")).clicked()
                {
                    self.start_relocate(music_folders.to_vec());
                }
            });
        });
        ui.add_space(4.0);
    }

    fn show_missing_shopping_list(ui: &mut Ui, missing: &[crate::playlist_import::Wanted]) {
        let border = TraktorTheme::BORDER_COLOR();
        egui::Frame::none()
            .fill(TraktorTheme::BG_PANEL())
            .stroke(Stroke::new(1.0_f32, border))
            .rounding(4.0)
            .inner_margin(egui::Margin::symmetric(10.0, 6.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Not in your library ({})", missing.len()))
                            .size(12.0)
                            .strong()
                            .color(TraktorTheme::TEXT_PRIMARY()),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_sized([90.0, 24.0], egui::Button::new(RichText::new("COPY LIST").size(11.0).strong()))
                            .on_hover_text("Copy missing tracks to clipboard (shopping list)")
                            .clicked()
                        {
                            let list = missing
                                .iter()
                                .map(|w| format!("{} - {}", w.artist, w.title))
                                .collect::<Vec<_>>()
                                .join("\n");
                            ui.output_mut(|o| o.copied_text = list);
                        }
                    });
                });
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .max_height(80.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for item in missing {
                            ui.horizontal(|ui| {
                                ui.set_min_height(24.0);
                                ui.label(RichText::new("•").size(11.0).color(TraktorTheme::TEXT_MUTED()));
                                ui.label(RichText::new(&item.artist).size(12.0).color(TraktorTheme::TEXT_SECONDARY()));
                                ui.label(RichText::new("–").size(12.0).color(TraktorTheme::TEXT_MUTED()));
                                ui.label(RichText::new(&item.title).size(12.0).color(TraktorTheme::TEXT_PRIMARY()));
                            });
                        }
                    });
            });
    }

    fn show_toolbar(&mut self, ui: &mut Ui, music_folders: &[PathBuf], columns: &mut LibraryColumnsConfig, locked: [bool; 4], deck_count: usize) {
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Search title, artist, genre…").desired_width((ui.available_width() * 0.28).clamp(150.0, 320.0)));
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                r.request_focus();
            }
            if !self.search_query.is_empty() && ui.small_button("Clear").clicked() {
                self.search_query.clear();
            }
            ui.menu_button("Filters", |ui| {
                ui.label(RichText::new("Add these to the search; click one to try it").color(TraktorTheme::TEXT_SECONDARY()));
                for (q, what) in [("bpm:120-128", "tempo range"), ("key:~8m", "keys that mix with 8m"), ("energy:>6", "energy above 6")] {
                    if ui.button(format!("{q}   {what}")).clicked() {
                        self.search_query = format!("{} {q}", self.search_query.trim()).trim().to_string();
                        ui.close_menu();
                    }
                }
            });
            ui.separator();

            let busy = self.is_busy();
            if ui.add_enabled(!busy, egui::Button::new("Rescan")).on_hover_text("Add new files from your music folders").clicked() {
                self.start_scan(music_folders.to_vec());
            }
            let pending = self.unanalyzed_ids();
            if !pending.is_empty()
                && ui
                    .add_enabled(!busy, egui::Button::new(format!("Analyze {}", pending.len())))
                    .on_hover_text("Detect BPM, key and energy where they're unknown (existing values are kept)")
                    .clicked()
            {
                self.start_analysis(pending, false);
            }
            let missing = self.missing_count();
            if missing > 0 && self.selected_nav != NavItem::Missing && ui.link(RichText::new(format!("{missing} missing")).color(TraktorTheme::METER_RED())).on_hover_text("Show tracks whose files can't be found").clicked() {
                self.selected_nav = NavItem::Missing;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.menu_button("Export", |ui| if ui.button("rekordbox XML").on_hover_text("Export collection + playlists to ~/Music/ReTraktor-rekordbox.xml").clicked() {
                    ui.close_menu();
                    let path = env::var("HOME").map(PathBuf::from).unwrap_or_default().join("Music/ReTraktor-rekordbox.xml");
                    let xml = retraktor_formats::to_rekordbox_xml(&self.snapshot());
                    self.status_message = match fs::create_dir_all(path.parent().unwrap()).and_then(|_| fs::write(&path, xml)) {
                        Ok(()) => format!("Exported rekordbox XML to {}", path.display()),
                        Err(e) => format!("rekordbox export failed: {e}"),
                    };
                });
                ui.menu_button("Columns", |ui| column_menu(ui, columns));
                if ui.small_button("Folders").clicked() {
                    self.open_prefs_requested = true;
                    self.open_prefs_file_management = true;
                }
            });
        });

        // Active filter pills (Shift+click on BPM, Key, Energy, Genre cells or Hide missing files).
        let hiding_missing = self.hide_missing_files && self.selected_nav != NavItem::Missing;
        let active_filters = self.quick_filter.active_count() + usize::from(hiding_missing);
        if active_filters > 0 {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 5.0;
                let chip = |ui: &mut Ui, label: String, tip: &str| {
                    ui.add(
                        egui::Button::new(RichText::new(label).size(11.0).color(crate::theme::on(TraktorTheme::ROW_SELECTED())))
                            .fill(TraktorTheme::ROW_SELECTED())
                            .rounding(10.0),
                    )
                    .on_hover_text(tip)
                    .clicked()
                };
                if let Some(c) = self.quick_filter.bpm_center {
                    let lo = (c - 10.0).max(0.0).round() as i32;
                    let hi = (c + 10.0).round() as i32;
                    if chip(ui, format!("BPM {lo}–{hi} ×"), "Click to clear BPM filter") {
                        self.quick_filter.bpm_center = None;
                    }
                }
                if let Some(k) = self.quick_filter.key_center.clone() {
                    if chip(ui, format!("Key {k} ±1 ×"), "Click to clear key filter") {
                        self.quick_filter.key_center = None;
                    }
                }
                if let Some(e) = self.quick_filter.energy_center {
                    let lo = e.saturating_sub(1).max(1);
                    let hi = (e + 1).min(10);
                    if chip(ui, format!("Energy {lo}–{hi} ×"), "Click to clear energy filter") {
                        self.quick_filter.energy_center = None;
                    }
                }
                if let Some(g) = self.quick_filter.genre_exact.clone() {
                    if chip(ui, format!("Genre: {g} ×"), "Click to clear genre filter") {
                        self.quick_filter.genre_exact = None;
                    }
                }
                if hiding_missing && chip(ui, "Hiding missing ×".to_string(), "Click to show missing tracks") {
                    self.hide_missing_files = false;
                }
                if active_filters > 1 && ui.small_button("Clear").on_hover_text("Clear all active filters").clicked() {
                    self.quick_filter.clear();
                    if hiding_missing {
                        self.hide_missing_files = false;
                    }
                }
            });
        }

        // Second row: what to do with the selection, always in the same place.
        ui.horizontal_wrapped(|ui| {
            let sel = self.selected_track_id;
            ui.label(RichText::new("Load").color(TraktorTheme::TEXT_SECONDARY()));
            for (d, name) in DECK_NAMES.iter().enumerate().take(deck_count) {
                let enabled = sel.is_some() && !locked[d];
                let why = if sel.is_none() { "Select a track first".to_string() } else { format!("Deck {name} is playing and locked") };
                if ui.add_enabled(enabled, egui::Button::new(*name)).on_hover_text(format!("Load the selected track on deck {name}")).on_disabled_hover_text(why).clicked() {
                    if let Some(id) = sel {
                        self.load_to_deck(d + 1, id, locked);
                    }
                }
            }
            let ids = sel.map(|id| self.selected_or(id)).unwrap_or_default();
            let n = ids.len();
            ui.add_enabled_ui(n > 0, |ui| {
                ui.menu_button("Prepare", |ui| {
                    ui.label(RichText::new(format!("For {}", plural(n))).color(TraktorTheme::TEXT_SECONDARY()));
                    if ui.button("Analyse BPM, key and energy").clicked() {
                        self.start_analysis(ids.clone(), true);
                        ui.close_menu();
                    }
                    if ui.button("Prepare stems").on_hover_text("Separate with Demucs, one after another").clicked() {
                        let added = self.queue_stems(&ids);
                        self.status_message = format!("Queued stems for {}", plural(added));
                        ui.close_menu();
                    }
                    if ui.button("Add to set").clicked() {
                        self.add_to_set_requested = Some(ids.clone());
                        self.status_message = format!("Added {} to set list", plural(n));
                        ui.close_menu();
                    }
                    if ui.button(if n == 1 { "Edit track info…".to_string() } else { format!("Edit {n} tracks…") }).clicked() {
                        self.open_editor(&ids);
                        ui.close_menu();
                    }
                })
                .response
                .on_disabled_hover_text("Select a track first");
            });
            ui.separator();
            // Lit while the selected track is the one previewing; pressing it then stops it.
            let this_one = self.previewing.is_some_and(|(id, _)| Some(id) == sel);
            let hint = if this_one { "Stop previewing the selected track" } else { "Hear the selected track in the headphones only. The decks and the main output are untouched" };
            if ui
                .add_enabled(sel.is_some_and(|id| self.track(id).is_some()), egui::Button::new("Preview").selected(this_one))
                .on_hover_text(hint)
                .on_disabled_hover_text("Select a track first")
                .clicked()
            {
                self.preview_requested = Some(sel);
            }
            ui.separator();
            ui.label(
                RichText::new(if sel.is_some() { "Double-click: first unlocked deck · Shift+double-click: B · right-click for more" } else { "Select a track to load or prepare it · double-click loads the first unlocked deck" })
                    .size(11.0)
                    .color(TraktorTheme::TEXT_SECONDARY()),
            );
        });
    }

    fn show_empty_state(&mut self, ui: &mut Ui, music_folders: &mut Vec<PathBuf>) {
        if self.detected.is_none() && !self.is_busy() {
            self.detected = Some(Vec::new()); // filled in when the search reports back
            self.start_detect(music_folders);
        }
        let home = env::var("HOME").unwrap_or_default();
        let pretty = |p: &Path| p.display().to_string().replacen(&home, "~", 1);
        let muted = TraktorTheme::TEXT_SECONDARY();
        let column_w = 340.0;

        ui.add_space(36.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("Set up your library").size(22.0).color(TraktorTheme::TEXT_PRIMARY()));
            ui.add_space(4.0);
            ui.label(RichText::new("Start from your Traktor collection, a music folder, or both.").size(13.0).color(muted));
        });
        ui.add_space(24.0);

        let total_w = column_w * 2.0 + 40.0;
        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - total_w) / 2.0).max(0.0));
            ui.allocate_ui_with_layout(egui::vec2(column_w, 260.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(column_w);
                ui.label(RichText::new("Import from Traktor").size(15.0).strong().color(TraktorTheme::TEXT_PRIMARY()));
                ui.label(RichText::new("Brings across cue points, beatgrids, ratings and playlists. Your Traktor files aren't changed.").size(12.0).color(muted));
                ui.add_space(12.0);
                let found = self.detected.clone().unwrap_or_default();
                if self.is_busy() && found.is_empty() {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Looking for collections…").size(12.0).color(muted));
                    });
                }
                for p in &found {
                    let r = ui
                        .add(egui::Button::new(RichText::new(format!("Import  {}", pretty(p))).size(12.5)).min_size(egui::vec2(column_w, 30.0)).wrap_mode(egui::TextWrapMode::Truncate))
                        .on_hover_text(p.display().to_string());
                    if r.clicked() {
                        self.picked.push((Pick::Collection, p.clone()));
                    }
                }
                if found.is_empty() && !self.is_busy() {
                    ui.label(RichText::new("No collection found automatically.").size(12.0).color(muted));
                }
                ui.add_space(6.0);
                if ui.add(egui::Button::new(RichText::new("Choose collection.nml…").size(12.5)).min_size(egui::vec2(column_w, 30.0))).clicked() {
                    self.pick(Pick::Collection);
                }
            });
            ui.add_space(40.0);
            ui.allocate_ui_with_layout(egui::vec2(column_w, 260.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(column_w);
                ui.label(RichText::new("Add your music").size(15.0).strong().color(TraktorTheme::TEXT_PRIMARY()));
                ui.label(RichText::new("ReTraktor reads each file's tags and keeps watching these folders for new tracks.").size(12.0).color(muted));
                ui.add_space(12.0);
                for p in music_folders.clone() {
                    let r = ui
                        .add(egui::Button::new(RichText::new(format!("Scan  {}", pretty(&p))).size(12.5)).min_size(egui::vec2(column_w, 30.0)).wrap_mode(egui::TextWrapMode::Truncate))
                        .on_hover_text(p.display().to_string());
                    if r.clicked() {
                        self.start_scan(vec![p]);
                    }
                }
                ui.add_space(6.0);
                if ui.add(egui::Button::new(RichText::new("Choose music folder…").size(12.5)).min_size(egui::vec2(column_w, 30.0))).clicked() {
                    self.pick(Pick::MusicFolder);
                }
            });
        });

        // Fallback for desktops without a file-chooser service.
        ui.add_space(20.0);
        ui.vertical_centered(|ui| match &mut self.typed_path {
            None => {
                if ui.add(egui::Label::new(RichText::new("No file dialog appeared? Type a path instead").size(11.5).color(TraktorTheme::TEXT_MUTED()).underline()).sense(Sense::click())).clicked() {
                    self.typed_path = Some(String::new());
                }
            }
            Some(text) => {
                ui.horizontal(|ui| {
                    ui.add_space(((ui.available_width() - 520.0) / 2.0).max(0.0));
                    let r = ui.add(egui::TextEdit::singleline(text).hint_text("A collection.nml file or a music folder").desired_width(420.0));
                    if ui.button("Use").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                        let p = PathBuf::from(text.trim().replacen('~', &home, 1));
                        let kind = if p.is_dir() { Some(Pick::MusicFolder) } else if p.is_file() { Some(Pick::Collection) } else { None };
                        match kind {
                            Some(k) => self.picked.push((k, p)),
                            None => self.status_message = format!("Nothing found at {}", p.display()),
                        }
                    }
                });
            }
        });
    }

    fn visible_columns(&self, c: &LibraryColumnsConfig) -> Vec<SortColumn> {
        use SortColumn::*;
        // Row number only means something where order matters.
        let ordered = matches!(self.selected_nav, NavItem::Playlist(_) | NavItem::History);
        let mut cols = if ordered { vec![Order, Preview, Title, Artist] } else { vec![Preview, Title, Artist] };
        let opt = [
            (c.track_number, TrackNum),
            (c.album, Album),
            (c.bpm, Bpm),
            (c.key, Key),
            (c.energy, Energy),
            (c.duration, Duration),
            (c.genre, Genre),
            (c.rating, Rating),
            (c.remixer, Remixer),
            (c.label, Label),
            (c.year, Year),
            (c.comment, Comment),
            (c.bitrate, Bitrate),
            (c.format, Format),
            (c.stems, Stems),
        ];
        cols.extend(opt.iter().filter(|(on, _)| *on).map(|(_, col)| *col));
        if self.selected_nav == NavItem::Suggestions {
            let after_title = cols.iter().position(|&c| c == Title).map_or(0, |i| i + 1);
            cols.insert(after_title, Match);
        }
        cols
    }

    fn cell_text(t: &UiTrackEntry, col: SortColumn, row: usize, score: f32) -> RichText {
        let dim = TraktorTheme::TEXT_SECONDARY();
        let txt = |s: String| RichText::new(s).size(12.0);
        match col {
            SortColumn::Order => txt((row + 1).to_string()).color(TraktorTheme::TEXT_MUTED()),
            SortColumn::Title => txt(t.title.clone()).color(if t.missing { TraktorTheme::METER_RED() } else { TraktorTheme::TEXT_PRIMARY() }),
            SortColumn::Artist => txt(t.artist.clone()),
            SortColumn::Album => txt(t.album.clone()).color(dim),
            SortColumn::Bpm if t.bpm > 0.0 => txt(format!("{:.1}", t.bpm)).monospace(),
            SortColumn::Key if !t.musical_key.is_empty() => {
                // Colour keys around the wheel so compatible neighbours look alike.
                let hue = KeyWheel::parse_open_key(&t.musical_key).map_or(0.0, |(n, _)| (n as f32 - 1.0) / 12.0);
                {
                    // Light themes need darker, more saturated key colours to stay readable.
                    let (sat, val) = if crate::theme::palette().is_dark() { (0.55, 0.95) } else { (1.0, 0.4) };
                    txt(t.musical_key.clone()).monospace().color(Color32::from(egui::ecolor::Hsva::new(hue, sat, val, 1.0)))
                }
            }
            SortColumn::TrackNum if t.track_number > 0 => txt(t.track_number.to_string()).monospace().color(dim),
            SortColumn::Energy if t.energy > 0 => txt(t.energy.to_string()).monospace(),
            SortColumn::Duration if t.duration_sec > 0.0 => {
                let s = t.duration_sec.round() as u64;
                txt(format!("{}:{:02}", s / 60, s % 60)).monospace().color(dim)
            }
            SortColumn::Genre => txt(t.genre.clone()).color(dim),
            SortColumn::Rating => txt("★".repeat(t.rating.min(5) as usize)).color(TraktorTheme::ACCENT()),
            SortColumn::Comment => txt(t.comment.clone()).color(dim),
            SortColumn::Remixer => txt(t.remixer.clone()).color(dim),
            SortColumn::Label => txt(t.label.clone()).color(dim),
            SortColumn::Year => txt(t.year.clone()).color(dim),
            SortColumn::Bitrate if t.bitrate_kbps > 0 => txt(t.bitrate_kbps.to_string()).monospace().color(dim),
            SortColumn::Format => txt(t.format.clone()).color(dim),
            SortColumn::Stems if t.stems_ready => txt("S".to_string()).strong().color(TraktorTheme::DECK_A()),
            SortColumn::Match => txt(format!("{:.0}%", score * 100.0)).monospace().color(TraktorTheme::DECK_A()),
            _ => txt("–".to_string()).color(TraktorTheme::TEXT_MUTED()),
        }
    }

    /// Track `t`'s waveform in the Preview column, as far as it is decoded, with its hot cues, and the playhead
    /// while it is the track previewing. Senses clicks only, so drags reach the row. Also returns why the track
    /// can't be previewed, once known.
    fn row_waveform(&self, ui: &mut Ui, t: &UiTrackEntry) -> (egui::Response, Option<String>) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::click());
        let made = self.row_waveforms.get(&t.id).filter(|w| t.file_path.as_ref() == Some(&w.path)).map(|w| &w.peaks);
        let (peaks, length, problem): (&[retraktor_formats::StripeColumn], Option<f64>, Option<String>) = match made {
            _ if t.missing || t.file_path.is_none() => (&[], None, Some("File not found — use Relocate".into())),
            Some(Ok((peaks, length))) => (peaks, *length, None),
            Some(Err(e)) => (&[], None, Some(format!("Can't read this file: {e}"))),
            None => (&[], None, None),
        };
        let pending = made.is_none() && problem.is_none();
        let playhead = self.preview_position.filter(|_| self.previewing.is_some_and(|(id, _)| id == t.id));
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let at_x = |frac: f64| rect.left() + frac.clamp(0.0, 1.0) as f32 * rect.width();
            crate::widgets::paint_overview_waveform(&painter, rect, peaks, playhead.map_or(f32::NEG_INFINITY, at_x));
            if let Some(total) = length.or(Some(t.duration_sec)).filter(|d| d.is_finite() && *d > 0.0) {
                for c in t.cues.iter().filter(|c| (0..8).contains(&c.hotcue)) {
                    let x = at_x(c.start_ms / 1000.0 / total);
                    painter.vline(x, rect.y_range(), Stroke::new(1.0_f32, TraktorTheme::hotcue_color((c.hotcue + 1) as usize)));
                }
            }
            let note = match (&problem, pending) {
                (Some(_), _) => Some(("Not available", TraktorTheme::METER_RED())),
                (None, true) => Some(("Preparing…", TraktorTheme::TEXT_MUTED())),
                (None, false) => None,
            };
            if let Some((text, color)) = note {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(10.0), color);
            }
            if let Some(x) = response.hover_pos().map(|p| p.x) {
                painter.vline(x, rect.y_range(), Stroke::new(1.0_f32, TraktorTheme::WAVEFORM_PLAYHEAD().gamma_multiply(0.6)));
            }
            if let Some(at) = playhead {
                painter.vline(at_x(at), rect.y_range(), Stroke::new(1.5_f32, TraktorTheme::WAVEFORM_PLAYHEAD()));
            }
        }
        let state = match (&problem, pending) {
            (Some(why), _) => format!(" ({why})"),
            (None, true) => " (preparing)".to_string(),
            (None, false) => String::new(),
        };
        let label = format!("Waveform of '{}': click to preview from there{state}", t.title);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let hint = problem.clone().unwrap_or_else(|| "Click to hear it from there in the headphones · Shift/Ctrl+click only selects".into());
        (response.on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand), problem)
    }

    fn show_table(&mut self, ui: &mut Ui, columns: &mut LibraryColumnsConfig, locked: [bool; 4], deck_count: usize, four_deck: bool) {
        let cols = self.visible_columns(columns);
        let in_playlist = matches!(self.selected_nav, NavItem::Playlist(_));
        let playlist_names: Vec<String> = self.playlists.iter().map(|p| p.name.clone()).collect();
        let mut actions: Vec<Action> = Vec::new();

        // Keyboard: arrows browse, Enter loads A / Shift+Enter loads B, Delete removes from playlist.
        let keys = !ui.ctx().wants_keyboard_input() && !self.keys_blocked;
        if keys && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }
        if keys && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::I)) {
            let ids = self.selected_ids();
            self.open_editor(&ids);
        }
        if keys {
            let (down, up, enter, shift, del, all, add_set) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::Enter),
                    i.modifiers.shift,
                    i.key_pressed(egui::Key::Delete),
                    i.modifiers.command && i.key_pressed(egui::Key::A),
                    i.modifiers.shift && i.key_pressed(egui::Key::A),
                )
            });
            if down {
                self.select_next_track();
            }
            if up {
                self.select_prev_track();
            }
            if let (true, Some(id)) = (enter, self.selected_track_id) {
                actions.push(Action::Load(if shift { 2 } else { 1 }, id));
            }
            if del && in_playlist && !self.selection.is_empty() {
                actions.push(Action::RemoveFromPlaylist(self.selection.iter().copied().collect()));
            }
            if all {
                self.selection = self.view.iter().map(|&i| self.tracks[i].id).collect();
            }
            if add_set {
                let ids = self.selected_ids();
                if !ids.is_empty() {
                    actions.push(Action::AddToSet(ids));
                }
            }
        }

        let mut table = TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .sense(Sense::click_and_drag())
            .cell_layout(Layout::left_to_right(Align::Center))
            .min_scrolled_height(0.0)
            .max_scroll_height(f32::INFINITY);
        if std::mem::take(&mut self.scroll_to_selected) {
            if let Some(pos) = self.selected_track_id.and_then(|id| self.view_pos(id)) {
                table = table.scroll_to_row(pos, Some(Align::Center));
            }
        }
        for (i, c) in cols.iter().enumerate() {
            let min = if *c == SortColumn::Preview { 60.0 } else { 28.0 };
            let col = if i + 1 == cols.len() { Column::remainder().at_least(40.0) } else { Column::initial(c.width()).at_least(min).clip(true) };
            table = table.column(col);
        }
        let (sort_col, sort_asc) = (self.sort_col, self.sort_asc);
        let (mut visible, mut preview_click, mut refused_click) = (Vec::new(), None, None);
        // The cell being typed into; how its editing ended (save?, Tab step); an Alt+click that starts one.
        let mut cell_edit = self.cell_edit.take();
        let mut cell_done: Option<(bool, i32)> = None;
        let mut cell_start: Option<(usize, SortColumn)> = None;
        let mut cell_drawn = false;
        table
            .header(22.0, |mut header| {
                for &c in &cols {
                    header.col(|ui| {
                        let sortable = c != SortColumn::Preview;
                        // Plain label + whole-cell interact: a Button named "Preview" would shadow the toolbar toggle, and an exact-size allocation stops the remainder column shrinking.
                        let r = ui.interact(ui.max_rect(), ui.id().with(("col_hdr", c)), Sense::click());
                        if r.hovered() && sortable {
                            ui.painter().rect_filled(ui.max_rect(), 2.0, TraktorTheme::CONTROL_HOVER());
                        }
                        let arrow = if c == sort_col { if sort_asc { " ▲" } else { " ▼" } } else { "" };
                        let color = if c == sort_col { TraktorTheme::TEXT_PRIMARY() } else { TraktorTheme::TEXT_SECONDARY() };
                        ui.add(egui::Label::new(RichText::new(format!("{}{arrow}", c.header())).size(11.0).strong().color(color)).truncate().selectable(false).sense(Sense::hover()));
                        if r.clicked() && sortable {
                            actions.push(Action::Sort(c));
                        }
                        let hint = if sortable { "Click to sort · right-click to choose columns" } else { "Click a track's waveform to hear it from there in the headphones" };
                        let r = if sortable { r.on_hover_cursor(egui::CursorIcon::PointingHand) } else { r };
                        r.on_hover_text(hint).context_menu(|ui| column_menu(ui, columns));
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_H, self.view.len(), |mut row| {
                    let r = row.index();
                    let t = &self.tracks[self.view[r]];
                    visible.push(t.id);
                    let score = self.view_scores.get(r).copied().unwrap_or(0.0);
                    let selected = self.selection.contains(&t.id);
                    row.set_selected(selected);
                    let (mut wave, mut problem) = (None, None);
                    let mut filter_cells: Vec<(egui::Response, QuickFilterAction)> = Vec::new();
                    let mut cells: Vec<(SortColumn, egui::Rect)> = Vec::new();
                    for &c in &cols {
                        row.col(|ui| {
                            cells.push((c, ui.max_rect()));
                            if let Some((_, _, text, fresh)) = cell_edit.as_mut().filter(|e| e.0 == t.id && e.1 == c) {
                                cell_drawn = true;
                                let r = ui.add(egui::TextEdit::singleline(text).id(egui::Id::new(("cell_edit", t.id, c))).font(egui::FontId::proportional(12.0)).margin(egui::vec2(3.0, 1.0)).desired_width(f32::INFINITY));
                                if std::mem::take(fresh) {
                                    // Opens with the text selected, so typing replaces it.
                                    let mut st = egui::text_edit::TextEditState::load(ui.ctx(), r.id).unwrap_or_default();
                                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(text.chars().count()))));
                                    st.store(ui.ctx(), r.id);
                                    r.request_focus();
                                } else if r.lost_focus() {
                                    // Enter or a click elsewhere saves, Esc cancels, Tab / Shift+Tab save and move along the row.
                                    let (esc, tab, shift) = ui.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Tab), i.modifiers.shift));
                                    cell_done = Some((!esc, if !tab { 0 } else if shift { -1 } else { 1 }));
                                }
                                return;
                            }
                            if c == SortColumn::Preview {
                                let (response, why) = self.row_waveform(ui, t);
                                (wave, problem) = (Some(response), why);
                                return;
                            }
                            let mut text = Self::cell_text(t, c, r, score);
                            if selected {
                                // Per-column colours (dim, key hue, missing red) vanish on the selection fill.
                                text = text.color(crate::theme::on(TraktorTheme::ROW_SELECTED()));
                            }
                            let qf = match c {
                                SortColumn::Bpm if t.bpm > 0.0 => Some((QuickFilterAction::Bpm(t.bpm), "Shift+click to filter ±10 BPM")),
                                SortColumn::Key if KeyWheel::parse_open_key(&t.musical_key).is_some() => Some((QuickFilterAction::Key(t.musical_key.clone()), "Shift+click to filter ±1 key")),
                                SortColumn::Energy if t.energy > 0 => Some((QuickFilterAction::Energy(t.energy), "Shift+click to filter ±1 energy")),
                                SortColumn::Genre if !t.genre.is_empty() => Some((QuickFilterAction::Genre(t.genre.clone()), "Shift+click to filter this genre")),
                                _ => None,
                            };
                            if let Some((action, tip)) = qf {
                                let mut cr = ui.add(egui::Label::new(text).truncate().selectable(false).sense(Sense::click())).on_hover_text(tip);
                                if ui.input(|i| i.modifiers.shift) {
                                    cr = cr.on_hover_cursor(egui::CursorIcon::PointingHand);
                                }
                                filter_cells.push((cr, action));
                            } else {
                                ui.add(egui::Label::new(text).truncate().selectable(false));
                            }
                        });
                    }
                    // The waveform takes its own clicks, above the row: they select, and a plain one previews from
                    // there, but never reach the row's double-click load. Drags pass through to the row.
                    let row_resp = row.response();
                    // Alt+click edits the cell under the pointer in place (Shift filters, Ctrl selects).
                    let alt = ui_modifiers(&row_resp).alt;
                    if alt && (row_resp.clicked() || filter_cells.iter().any(|(cr, _)| cr.clicked())) {
                        let pos = row_resp.ctx.input(|i| i.pointer.interact_pos());
                        if let Some(&(c, _)) = pos.and_then(|p| cells.iter().find(|(_, r)| r.contains(p))) {
                            if EDITABLE.contains(&c) {
                                cell_start = Some((t.id, c));
                                if !selected {
                                    actions.push(Action::Select(t.id, egui::Modifiers::NONE));
                                }
                            }
                        }
                    }
                    let mut resp = row_resp.clone();
                    let wave_clicked = wave.as_ref().filter(|w| w.clicked());
                    let mut filter_clicked = false;
                    let mut cell_plain_click = None;
                    let mut cell_double_click = false;
                    for (cr, action) in filter_cells {
                        if cr.clicked() {
                            let mods = ui_modifiers(&cr);
                            if mods.shift {
                                actions.push(Action::QuickFilter(action));
                                filter_clicked = true;
                            } else {
                                cell_plain_click = Some(mods);
                            }
                        }
                        if cr.double_clicked() && !ui_modifiers(&cr).shift {
                            cell_double_click = true;
                        }
                        resp = resp.union(cr);
                    }
                    if !filter_clicked && !alt {
                        if let Some(mods) = cell_plain_click {
                            actions.push(Action::Select(t.id, mods));
                        } else if row_resp.clicked() || wave_clicked.is_some() {
                            actions.push(Action::Select(t.id, ui_modifiers(&row_resp)));
                        }
                    }
                    if let Some(w) = wave_clicked.filter(|w| ui_modifiers(w).is_none()) {
                        // A row known to be unplayable only says why: the preview playing carries on.
                        match (&problem, w.interact_pointer_pos()) {
                            (Some(why), _) => refused_click = Some(format!("Can't preview '{}': {why}", t.title)),
                            (None, Some(pos)) => preview_click = Some((t.id, ((pos.x - w.rect.left()) / w.rect.width().max(1.0)).clamp(0.0, 1.0) as f64)),
                            (None, None) => {}
                        }
                    }
                    if alt {
                        // A quick second Alt+click is still editing, not a load.
                    } else if cell_double_click {
                        actions.push(Action::LoadAuto(t.id));
                    } else if row_resp.double_clicked() {
                        // Shift sends it to deck B; plain double-click takes the first free deck.
                        actions.push(if ui_modifiers(&row_resp).shift { Action::Load(2, t.id) } else { Action::LoadAuto(t.id) });
                    }
                    if row_resp.drag_started() {
                        row_resp.dnd_set_drag_payload(DragTracks(self.selected_or(t.id)));
                    }
                    // Right-clicking the waveform opens the row's menu too.
                    let resp = match wave {
                        Some(w) => resp.union(w),
                        None => resp,
                    };
                    if resp.secondary_clicked() && !self.selection.contains(&t.id) {
                        actions.push(Action::Select(t.id, egui::Modifiers::NONE));
                    }
                    resp.context_menu(|ui| {
                        let ids = self.selected_or(t.id);
                        let decks = if four_deck { 4 } else { 2 };
                        ui.horizontal(|ui| {
                            ui.label("Load to deck");
                            for (d, name) in DECK_NAMES.iter().enumerate().take(decks) {
                                if ui.button(*name).clicked() {
                                    actions.push(Action::Load(d + 1, t.id));
                                    ui.close_menu();
                                }
                            }
                        });
                        ui.separator();
                        ui.menu_button(format!("Add {} to playlist", if ids.len() == 1 { "track".to_string() } else { format!("{} tracks", ids.len()) }), |ui| {
                            if ui.button("New playlist…").clicked() {
                                actions.push(Action::AddToPlaylist(ids.clone(), None));
                                ui.close_menu();
                            }
                            egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                                for (i, name) in playlist_names.iter().enumerate() {
                                    if ui.button(name).clicked() {
                                        actions.push(Action::AddToPlaylist(ids.clone(), Some(i)));
                                        ui.close_menu();
                                    }
                                }
                            });
                        });
                        if ui.button(format!("Add {} to set list (Shift+A)", if ids.len() == 1 { "track" } else { "tracks" })).clicked() {
                            actions.push(Action::AddToSet(ids.clone()));
                            ui.close_menu();
                        }
                        if in_playlist && ui.button("Remove from this playlist").clicked() {
                            actions.push(Action::RemoveFromPlaylist(ids.clone()));
                            ui.close_menu();
                        }
                        ui.separator();
                        let edit = if ids.len() == 1 { "Edit track info…".to_string() } else { format!("Edit {} tracks…", ids.len()) };
                        if ui.button(edit).on_hover_text("Ctrl+I · or Alt+click a cell to edit it in place").clicked() {
                            actions.push(Action::Edit(ids.clone()));
                            ui.close_menu();
                        }
                        if ui.button("Analyze BPM, key, energy").on_hover_text("Replaces existing BPM/key/energy values").clicked() {
                            actions.push(Action::Analyze(ids.clone()));
                            ui.close_menu();
                        }
                        if ui.button("Fill empty fields from file tags").clicked() {
                            actions.push(Action::FillTags(ids.clone()));
                            ui.close_menu();
                        }
                        if ui.button(if t.stems_ready { "Re-separate stems (Demucs)" } else { "Separate stems (Demucs)" }).clicked() {
                            actions.push(Action::Stems(t.id));
                            ui.close_menu();
                        }
                        if let Some(p) = &t.file_path {
                            if ui.button("Show in folder").clicked() {
                                actions.push(Action::Reveal(p.clone()));
                                ui.close_menu();
                            }
                        }
                        ui.separator();
                        if ui.button(format!("Remove {} from collection", ids.len())).on_hover_text("Files on disk are not deleted").clicked() {
                            actions.push(Action::RemoveFromCollection(ids));
                            ui.close_menu();
                        }
                    });
                });
            });
        self.visible_rows = visible;
        // A cell scrolled or filtered out of view while typing saves, as clicking away does.
        if cell_done.is_none() && !cell_drawn && cell_edit.as_ref().is_some_and(|e| !e.3) {
            cell_done = Some((true, 0));
        }
        if let Some(((save, step), (id, col, text, _))) = cell_done.and_then(|d| Some((d, cell_edit.take()?))) {
            let unchanged = self.track(id).is_some_and(|t| field_text(t, col).trim() == text.trim());
            match if save && !unchanged { self.edit_tracks(&[id], &[(col, text.clone())], 1.0) } else { Ok(()) } {
                Err(e) => {
                    self.status_message = e;
                    cell_edit = Some((id, col, text, true));
                }
                Ok(()) if step != 0 => {
                    let editable: Vec<SortColumn> = cols.iter().copied().filter(|c| EDITABLE.contains(c)).collect();
                    let next = editable.iter().position(|&c| c == col).and_then(|i| editable.get(i.checked_add_signed(step as isize)?));
                    cell_start = cell_start.or(next.map(|&c| (id, c)));
                }
                Ok(()) => {}
            }
        }
        if let Some((id, c)) = cell_start {
            cell_edit = self.track(id).map(|t| (id, c, field_text(t, c), true));
        }
        self.cell_edit = cell_edit;
        if preview_click.is_some() {
            self.overview_clicked = preview_click;
        }
        if let Some(why) = refused_click {
            let still = self.previewing.and_then(|(id, _)| self.track(id)).map(|p| format!(" · still previewing '{}'", p.title));
            self.status_message = why + &still.unwrap_or_default();
        }

        // "Add to preparation" lives in the playlist menu; make sure the list exists for drag targets.
        if self.playlists.is_empty() {
            self.preparation_index();
        }
        for a in actions {
            self.apply(a, locked, deck_count);
        }
    }

    fn show_editor(&mut self, ctx: &egui::Context) {
        let Some(mut ed) = self.editing.take() else { return };
        let n = ed.ids.len();
        let path = if n == 1 { self.track(ed.ids[0]).and_then(|t| t.file_path.clone()) } else { None };
        let single_bpm = if n == 1 { self.track(ed.ids[0]).map(|t| t.bpm) } else { None };
        let (mut open, mut save, mut write, mut cancel) = (true, false, false, false);
        let title = if n == 1 { "Track info".to_string() } else { format!("Edit {n} tracks") };
        egui::Window::new(title).id(egui::Id::new("track_editor")).pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.screen_rect().center()).collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            if n > 1 {
                ui.label(RichText::new("What you change here is written to every selected track. Fields marked Mixed differ between them and stay as they are unless you type in them.").size(11.5).color(TraktorTheme::TEXT_SECONDARY()));
                ui.add_space(6.0);
            }
            egui::Grid::new("track_editor").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (c, was, now) in &mut ed.fields {
                    let changed = Editor::is_changed(was, now) || (*c == SortColumn::Bpm && ed.bpm_scale != 1.0);
                    let name = RichText::new(field_name(*c)).color(if changed { TraktorTheme::ACCENT() } else { TraktorTheme::TEXT_SECONDARY() });
                    ui.label(if changed { name.strong() } else { name }).on_hover_text(if changed { "Changed: written on save" } else { "" });
                    ui.horizontal(|ui| {
                        let hint = match (*c, was.is_some()) {
                            (_, false) => "Mixed",
                            (SortColumn::Key, _) => "e.g. 8m, 8A or Am",
                            (SortColumn::Rating, _) => "0–5 stars",
                            (SortColumn::Energy, _) => "1–10",
                            _ => "",
                        };
                        // Italic and muted, so a hint never reads as a value the tracks have.
                        let hint = RichText::new(hint).italics().color(TraktorTheme::TEXT_SECONDARY());
                        let r = ui.add(egui::TextEdit::singleline(now).hint_text(hint).desired_width(if *c == SortColumn::Bpm { 180.0 } else { 320.0 }));
                        save |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if *c == SortColumn::Bpm {
                            for (label, f, tip) in [("×½", 0.5, "Halve (fix double-time detection)"), ("×2", 2.0, "Double (fix half-time detection)")] {
                                if ui.small_button(label).on_hover_text(tip).clicked() {
                                    // Scale the exact stored BPM, not the 2-decimal text, unless a new value was typed.
                                    match now.trim().parse::<f64>() {
                                        Ok(bpm) if Editor::is_changed(was, now) => *now = format!("{:.2}", bpm * f),
                                        _ => ed.bpm_scale *= f,
                                    }
                                }
                            }
                            if ed.bpm_scale != 1.0 && !Editor::is_changed(was, now) {
                                let x = if ed.bpm_scale > 1.0 { format!("×{}", ed.bpm_scale) } else { format!("×1/{}", 1.0 / ed.bpm_scale) };
                                let shown = match single_bpm {
                                    Some(b) if b > 0.0 => format!("→ {:.2}", b * ed.bpm_scale),
                                    _ => format!("each track {x}"),
                                };
                                ui.label(RichText::new(shown).size(11.5).color(TraktorTheme::ACCENT()));
                            }
                        }
                    });
                    ui.end_row();
                }
            });
            if let Some(p) = &path {
                ui.label(RichText::new(p.display().to_string()).size(10.5).color(TraktorTheme::TEXT_MUTED()));
            }
            if let Some(e) = &ed.error {
                ui.label(RichText::new(e).size(11.5).color(TraktorTheme::METER_RED()));
            }
            ui.add_space(4.0);
            let any = n == 1 || !ed.changes().is_empty() || ed.bpm_scale != 1.0;
            ui.horizontal(|ui| {
                let label = if n == 1 { "Save".to_string() } else { format!("Apply to {n} tracks") };
                save |= ui.add_enabled(any, egui::Button::new(label)).on_disabled_hover_text("Change a field first").clicked();
                let tags = if n == 1 { "Save and write tags to file" } else { "Apply and write tags to files" };
                if ui.add_enabled(any, egui::Button::new(tags)).on_hover_text("Also updates the tags inside the audio files").on_disabled_hover_text("Change a field first").clicked() {
                    (save, write) = (true, true);
                }
                cancel = ui.button("Cancel").clicked();
            });
        });
        if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            open = false;
        }
        if save && open {
            let changes = ed.changes();
            match self.edit_tracks(&ed.ids, &changes, ed.bpm_scale) {
                Ok(()) => {
                    let mut fields = ed.changed_fields();
                    let names: Vec<&str> = fields.iter().map(|&c| field_name(c)).collect();
                    if !names.is_empty() {
                        self.status_message = format!("Changed {} on {} · Ctrl+Z undoes it", names.join(", "), plural(n));
                    }
                    if write {
                        // Nothing changed on one track: write what it has, never blanks over the file's own tags.
                        if fields.is_empty() {
                            let t = self.track(ed.ids[0]);
                            fields = EDITABLE.iter().copied().filter(|&c| t.is_some_and(|t| !field_text(t, c).is_empty())).collect();
                        }
                        let copies: Vec<UiTrackEntry> = ed.ids.iter().filter_map(|&id| self.track(id).cloned()).collect();
                        self.spawn_job(move |_| {
                            let failed: Vec<String> = copies.iter().filter_map(|t| write_track_tags(t, &fields).err().map(|e| format!("{}: {e}", t.title))).collect();
                            match failed.first() {
                                None => format!("Wrote tags to {}", plural(copies.len())),
                                Some(first) => format!("Could not write tags to {} of {} ({first})", failed.len(), copies.len()),
                            }
                        });
                    }
                    return;
                }
                Err(e) => ed.error = Some(e),
            }
        }
        if open {
            self.editing = Some(ed);
        }
    }

    fn refresh_explorer(&mut self) {
        let mut entries: Vec<(PathBuf, bool)> = fs::read_dir(&self.explorer_dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
                    .filter_map(|e| {
                        let p = e.path();
                        let dir = p.is_dir();
                        let nml = p.extension().is_some_and(|x| x == "nml");
                        (dir || nml || is_audio_file(&p)).then_some((p, dir))
                    })
                    .collect()
            })
            .unwrap_or_default();
        entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.file_name().cmp(&b.0.file_name())));
        self.explorer_entries = entries;
        self.explorer_path_input = self.explorer_dir.display().to_string();
    }

    fn add_file(&mut self, p: &Path) -> usize {
        if let Some(t) = self.tracks.iter().find(|t| t.file_path.as_deref() == Some(p)) {
            return t.id;
        }
        let id = self.push_track(read_track_file(p));
        self.touch();
        id
    }

    fn show_explorer(&mut self, ui: &mut Ui, locked: [bool; 4], deck_count: usize, music_folders: &mut Vec<PathBuf>) {
        if self.explorer_entries.is_empty() {
            self.refresh_explorer();
        }
        let mut go: Option<PathBuf> = None;
        ui.horizontal(|ui| {
            if ui.button("Up").clicked() {
                go = self.explorer_dir.parent().map(Path::to_path_buf);
            }
            for f in music_folders.iter() {
                if ui.button(f.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| f.display().to_string())).clicked() {
                    go = Some(f.clone());
                }
            }
            let r = ui.add(egui::TextEdit::singleline(&mut self.explorer_path_input).desired_width(300.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = Some(PathBuf::from(self.explorer_path_input.trim()));
            }
            if ui.button("Add this folder to collection").clicked() {
                let dir = self.explorer_dir.clone();
                if !music_folders.iter().any(|f| dir.starts_with(f)) {
                    music_folders.push(dir.clone());
                }
                self.start_scan(vec![dir]);
            }
        });
        ui.separator();
        let mut load: Option<(Option<usize>, PathBuf)> = None;
        let mut import: Option<PathBuf> = None;
        let mut add: Option<PathBuf> = None;
        let entries = &self.explorer_entries;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show_rows(ui, ROW_H, entries.len(), |ui, range| {
            for (p, is_dir) in &entries[range] {
                let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                let label = if *is_dir { format!("[{name}]") } else { name };
                let r = ui.add(egui::Label::new(RichText::new(label).size(12.0)).sense(Sense::click()).truncate());
                if *is_dir && r.clicked() {
                    go = Some(p.clone());
                } else if !*is_dir {
                    let nml = p.extension().is_some_and(|x| x == "nml");
                    if r.double_clicked() {
                        if nml {
                            import = Some(p.clone());
                        } else {
                            load = Some((None, p.clone()));
                        }
                    }
                    r.context_menu(|ui| {
                        if nml {
                            if ui.button("Import collection").clicked() {
                                import = Some(p.clone());
                                ui.close_menu();
                            }
                            return;
                        }
                        for (d, n) in DECK_NAMES.iter().enumerate().take(deck_count) {
                            if ui.button(format!("Load to deck {n}")).clicked() {
                                load = Some((Some(d + 1), p.clone()));
                                ui.close_menu();
                            }
                        }
                        if ui.button("Add to collection").clicked() {
                            add = Some(p.clone());
                            ui.close_menu();
                        }
                    });
                }
            }
        });
        if let Some(dir) = go.filter(|d| d.is_dir()) {
            self.explorer_dir = dir;
            self.refresh_explorer();
        }
        if let Some(p) = add {
            self.add_file(&p);
        }
        if let Some((deck, p)) = load {
            let id = self.add_file(&p);
            let action = deck.map_or(Action::LoadAuto(id), |d| Action::Load(d, id));
            self.apply(action, locked, deck_count);
        }
        if let Some(p) = import {
            if let Err(e) = self.import_nml_file(&p) {
                self.status_message = format!("Import failed: {e}");
            } else {
                self.relocate_after_check = Some(music_folders.clone());
            }
        }
    }
}

fn ui_modifiers(resp: &egui::Response) -> egui::Modifiers {
    resp.ctx.input(|i| i.modifiers)
}

/// Highlights `rect` while tracks are dragged over it and returns the dropped track id.
pub fn deck_drop_target(ui: &Ui, rect: egui::Rect) -> Option<usize> {
    if !egui::DragAndDrop::has_payload_of_type::<DragTracks>(ui.ctx()) || !ui.rect_contains_pointer(rect) {
        return None;
    }
    // The drop outline breathes while a track hovers over the deck.
    let pulse = if crate::theme::animations() { 0.55 + 0.45 * (ui.input(|i| i.time) * 6.0).sin().abs() as f32 } else { 1.0 };
    ui.painter().rect_stroke(rect.shrink(1.0), 4.0, Stroke::new(2.0_f32, TraktorTheme::ACCENT().gamma_multiply(pulse)));
    if ui.input(|i| i.pointer.any_released()) {
        return egui::DragAndDrop::take_payload::<DragTracks>(ui.ctx()).and_then(|p| p.0.first().copied());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_atomic_writes_never_mix_and_leave_no_temp_files() {
        let dir = std::env::temp_dir().join(format!("rt-atomic-{}", std::process::id()));
        let path = dir.join("collection.nml");
        let (a, b) = ("a".repeat(1 << 20), "b".repeat(1 << 20));
        std::thread::scope(|s| {
            s.spawn(|| write_atomic(&path, &a).unwrap());
            s.spawn(|| write_atomic(&path, &b).unwrap());
        });
        let got = fs::read_to_string(&path).unwrap();
        assert!(got == a || got == b, "a save mixed two writers' bytes");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1, "a temp file was left behind");
        let _ = fs::remove_dir_all(&dir);
    }

    fn wait_for_library(lib: &mut LibraryManager) {
        for _ in 0..500 {
            lib.poll();
            if lib.libraries.iter().all(|l| l.note != "Reading…") && lib.jobs == 0 {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("library read did not finish");
    }

    #[test]
    fn linked_rekordbox_library_merges_without_overwriting_and_mirrors_playlists() {
        let (mut lib, dir) = temp_lib("rbx-link");
        // My copy of a track rekordbox knows from a Mac, with my own BPM and no hotcues.
        let mine = dir.join("Deep Low.mp3");
        fs::write(&mine, b"x").unwrap();
        let my_id = lib.push_track(UiTrackEntry { title: "Deep Low (mine)".into(), bpm: 125.0, file_path: Some(mine.clone()), ..Default::default() });
        lib.playlists.push(UserPlaylist { name: "My set".into(), track_ids: vec![my_id], missing: Vec::new() });

        let xml = |with_friday: bool| format!(r#"<DJ_PLAYLISTS Version="1.0.0"><COLLECTION Entries="2">
<TRACK TrackID="1" Name="Deep Low" Artist="A" AverageBpm="126.00" Genre="Techno" Rating="204" Location="file://localhost/Users/dj/Music/Deep%20Low.mp3">
  <TEMPO Inizio="0.1" Bpm="126.00" Metro="4/4" Battito="1"/><POSITION_MARK Name="Drop" Type="0" Start="64.0" Num="0"/></TRACK>
<TRACK TrackID="2" Name="Only In Rekordbox" Artist="B" Location="file://localhost/Users/dj/Music/new.mp3"/>
</COLLECTION><PLAYLISTS><NODE Type="0" Name="ROOT" Count="2">
  <NODE Name="Warmup" Type="1" KeyType="0" Entries="1"><TRACK Key="2"/></NODE>
  {}
</NODE></PLAYLISTS></DJ_PLAYLISTS>"#, if with_friday { r#"<NODE Name="Friday" Type="1" KeyType="0" Entries="2"><TRACK Key="1"/><TRACK Key="2"/></NODE>"# } else { "" });
        let xml_path = dir.join("rekordbox.xml");
        fs::write(&xml_path, xml(true)).unwrap();
        lib.link_library(LibKind::Rekordbox, xml_path.clone());
        wait_for_library(&mut lib);

        assert_eq!(lib.tracks.len(), 2, "the moved file matched my copy; one new track");
        let t = lib.tracks.iter().find(|t| t.id == my_id).unwrap();
        assert_eq!(t.title, "Deep Low (mine)");
        assert_eq!(t.bpm, 125.0, "my BPM is kept");
        assert_eq!(t.genre, "Techno", "empty tags are filled");
        assert_eq!(t.rating, 4);
        assert!(t.cues.iter().any(|c| c.hotcue == 0 && c.name == "Drop"), "missing hotcues are adopted");
        assert_eq!(t.file_path.as_deref(), Some(mine.as_path()), "my file path is kept");
        let friday = lib.playlists.iter().find(|p| p.name == "rekordbox / Friday").expect("library playlist");
        assert_eq!(friday.track_ids[0], my_id);
        assert_eq!(lib.libraries[0].track_ids.len(), 2);

        // Deleting a playlist in rekordbox removes it here on the next read; mine stay.
        fs::write(&xml_path, xml(false)).unwrap();
        lib.read_library(0);
        wait_for_library(&mut lib);
        let names: Vec<&str> = lib.playlists.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["My set", "rekordbox / Warmup"]);
        assert_eq!(lib.tracks.len(), 2, "re-reading adds nothing twice");

        // A track I add to a library's playlist here survives the re-read every start does, and a restart.
        let warmup = lib.playlists.iter().position(|p| p.name == "rekordbox / Warmup").unwrap();
        lib.add_to_playlist(&[my_id], Some(warmup));
        lib.read_library(0);
        wait_for_library(&mut lib);
        assert!(lib.playlists[warmup].track_ids.contains(&my_id), "re-read dropped the track I added");
        lib.save_collection_nml(&dir.join("collection.nml")).unwrap();
        let mut reopened = LibraryManager::new(dir.join("collection.nml"));
        wait_for_library(&mut reopened);
        let back = reopened.playlists.iter().find(|p| p.name == "rekordbox / Warmup").unwrap();
        let mine_id = reopened.tracks.iter().find(|t| t.title == "Deep Low (mine)").unwrap().id;
        assert!(back.track_ids.contains(&mine_id), "restart dropped the track I added");


        // The link survives a restart.
        assert!(fs::read_to_string(lib.libraries_file()).unwrap().contains("rekordbox\t"));

        lib.unlink_library(0);
        assert!(lib.libraries.is_empty());
        assert_eq!(lib.playlists.len(), 1);
        assert_eq!(lib.tracks.len(), 2, "unlinking keeps the tracks");
        lib.link_library(LibKind::Rekordbox, xml_path.clone());
        wait_for_library(&mut lib);
        // Removed tracks stay removed after a restart, though the library still lists them.
        let all: Vec<usize> = lib.tracks.iter().map(|t| t.id).collect();
        lib.apply(Action::RemoveFromCollection(all), [false; 4], 2);
        lib.save_collection_nml(&dir.join("collection.nml")).unwrap();
        let mut reopened = LibraryManager::new(dir.join("collection.nml"));
        wait_for_library(&mut reopened);
        assert!(reopened.tracks.is_empty(), "removed tracks came back: {:?}", reopened.tracks.iter().map(|t| &t.title).collect::<Vec<_>>());
        // Adding the file myself brings it back for good.
        reopened.add_file(&mine);
        reopened.read_library(0);
        wait_for_library(&mut reopened);
        assert_eq!(reopened.tracks.len(), 1, "only the re-added track, merged once");
        assert!(!fs::read_to_string(reopened.removed_file()).unwrap().contains("Deep Low"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn linked_traktor_paths_are_found_on_the_volume_holding_the_nml() {
        let (mut lib, dir) = temp_lib("volume-root");
        // The share mounted on this laptop; the Mac saved its paths from the volume root ("/Volumes/Music").
        let volume = dir.join("smb-share:server=nas,share=music/Music");
        let song = volume.join("Audio Tracks/Hip Hop/song.flac");
        fs::create_dir_all(song.parent().unwrap()).unwrap();
        fs::write(&song, b"x").unwrap();
        fs::create_dir_all(volume.join("Traktor")).unwrap();
        let nml = volume.join("Traktor/MEEKMAC.nml");
        fs::write(&nml, r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?><NML VERSION="20"><COLLECTION ENTRIES="1">
<ENTRY TITLE="Song" ARTIST="A"><LOCATION DIR="/:Audio Tracks/:Hip Hop/:" FILE="song.flac" VOLUME="Music" VOLUMEID="Music"/></ENTRY>
</COLLECTION><PLAYLISTS><NODE TYPE="FOLDER" NAME="$ROOT"><SUBNODES COUNT="1"><NODE TYPE="PLAYLIST" NAME="Set"><PLAYLIST ENTRIES="1" TYPE="LIST">
<ENTRY><PRIMARYKEY TYPE="TRACK" KEY="Music/:Audio Tracks/:Hip Hop/:song.flac"/></ENTRY></PLAYLIST></NODE></SUBNODES></NODE></PLAYLISTS></NML>"#).unwrap();
        // Read once before this fix: the track sits at the bare saved path and must move, not duplicate.
        let old = lib.push_track(track("Song", Path::new("/Audio Tracks/Hip Hop/song.flac")));
        lib.link_library(LibKind::Traktor, nml.clone());
        wait_for_library(&mut lib);
        assert_eq!(lib.tracks.len(), 1, "{:?}", lib.tracks.iter().map(|t| &t.file_path).collect::<Vec<_>>());
        assert_eq!(lib.tracks[0].id, old);
        assert_eq!(lib.tracks[0].file_path.as_deref(), Some(song.as_path()));
        let set = lib.playlists.iter().find(|p| p.name.ends_with("/ Set")).expect("library playlist");
        assert_eq!(set.track_ids, vec![old], "playlists still find the track by its saved key");
        let _ = fs::remove_dir_all(dir);
    }

    /// A linked merge asks the folder listing, never the disk per track: over SMB that froze the UI for minutes.
    #[test]
    fn linked_merge_reads_existence_from_the_folder_listing() {
        let (mut lib, dir) = temp_lib("listing");
        let mine = lib.push_track(track("Song", Path::new("/elsewhere/song.flac")));
        let col = NmlCollection::parse_xml(r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?><NML VERSION="20"><COLLECTION ENTRIES="1">
<ENTRY TITLE="Song" ARTIST="A"><LOCATION DIR="/:Audio Tracks/:" FILE="song.flac" VOLUME="Music" VOLUMEID="Music"/></ENTRY></COLLECTION></NML>"#).unwrap();
        assert_eq!(lib.merge_collection(&col, Some("T"), None, &HashSet::new()).1, vec![mine], "not listed: moved, so it's the collection's song.flac");
        let (added, _) = lib.merge_collection(&col, Some("T"), None, &HashSet::from([PathBuf::from("/Audio Tracks/song.flac")]));
        assert_eq!(added, 1, "listed: another file that shares the name");

        let folder = dir.join("music");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.mp3"), b"x").unwrap();
        fs::write(folder.join("b.mp3"), b"x").unwrap();
        assert_eq!(listed_files([folder.join("a.mp3").as_path()].into_iter()), HashSet::from([folder.join("a.mp3"), folder.join("b.mp3")]));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn typed_fields_are_checked_and_normalised() {
        let mut t = UiTrackEntry::default();
        set_field(&mut t, SortColumn::Key, "Am").unwrap();
        assert_eq!(t.musical_key, "1m", "keys are stored as Open Key");
        assert!(set_field(&mut t, SortColumn::Key, "H#").is_err());
        assert_eq!(t.musical_key, "1m", "a bad key leaves the old one");
        set_field(&mut t, SortColumn::Bpm, " 126.5 ").unwrap();
        assert_eq!(t.bpm, 126.5);
        assert!(set_field(&mut t, SortColumn::Bpm, "fast").is_err() && set_field(&mut t, SortColumn::Rating, "6").is_err());
        set_field(&mut t, SortColumn::Rating, "").unwrap();
        assert_eq!(t.rating, 0, "empty clears a number");
        assert!(set_field(&mut t, SortColumn::Duration, "3:00").is_err(), "only editable fields");
        assert_eq!(field_text(&t, SortColumn::Bpm), "126.50");
    }

    #[test]
    fn bulk_edit_writes_only_changed_fields_to_every_track_and_undoes() {
        let (mut lib, dir) = temp_lib("bulk-edit");
        let ids: Vec<usize> = [("A", 120.0, "House"), ("B", 87.0, "House"), ("C", 0.0, "House")]
            .iter()
            .map(|&(artist, bpm, genre)| lib.push_track(UiTrackEntry { title: format!("T{artist}"), artist: artist.into(), bpm, genre: genre.into(), ..Default::default() }))
            .collect();
        let tracks: Vec<&UiTrackEntry> = ids.iter().filter_map(|&id| lib.track(id)).collect();
        let mut ed = Editor::new(&tracks);
        let field = |ed: &Editor, c: SortColumn| ed.fields.iter().find(|f| f.0 == c).cloned().unwrap();
        assert_eq!(field(&ed, SortColumn::Genre).1.as_deref(), Some("House"), "a shared value shows");
        assert_eq!(field(&ed, SortColumn::Artist).1, None, "differing values are mixed");
        assert!(ed.changes().is_empty(), "opening changes nothing");
        for f in &mut ed.fields {
            match f.0 {
                SortColumn::Genre => f.2 = "UK Garage".into(),
                SortColumn::Label => f.2 = " ".into(),
                _ => {}
            }
        }
        ed.bpm_scale = 2.0;
        assert_eq!(ed.changes(), vec![(SortColumn::Genre, "UK Garage".to_string())], "blank mixed or untouched fields aren't written");
        lib.edit_tracks(&ed.ids, &ed.changes(), ed.bpm_scale).unwrap();
        let after: Vec<(String, String, f64)> = ids.iter().map(|&id| lib.track(id).map(|t| (t.artist.clone(), t.genre.clone(), t.bpm)).unwrap()).collect();
        assert_eq!(after, vec![("A".into(), "UK Garage".into(), 240.0), ("B".into(), "UK Garage".into(), 174.0), ("C".into(), "UK Garage".into(), 0.0)]);

        assert!(lib.edit_tracks(&ids, &[(SortColumn::Genre, "Techno".into()), (SortColumn::Key, "nope".into())], 1.0).is_err());
        let over = lib.edit_tracks(&ids, &[], 2.0).unwrap_err();
        assert!(over.contains("over 250") && over.contains("TA"), "{over}");
        assert_eq!(lib.track(ids[0]).unwrap().bpm, 240.0, "a scale past 250 BPM changes no track");
        assert!(ids.iter().all(|&id| lib.track(id).unwrap().genre == "UK Garage"), "one bad value changes nothing");

        lib.undo();
        assert!(ids.iter().all(|&id| lib.track(id).unwrap().genre == "House"), "Ctrl+Z puts every track back");
        assert_eq!(lib.track(ids[0]).unwrap().bpm, 120.0);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cell_editor_saves_on_tab_and_moves_along_cancels_on_esc_and_keeps_bad_values_open() {
        use egui::{Event, Key, Modifiers};
        let (mut lib, dir) = temp_lib("cell-edit");
        let id = lib.push_track(track("Old title", &dir.join("a.flac")));
        let ctx = egui::Context::default();
        let mut cols = crate::preferences::LibraryColumnsConfig::default();
        let mut frame = |lib: &mut LibraryManager, events: Vec<Event>| {
            let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    lib.ensure_view();
                    lib.show_table(ui, &mut cols, [false; 4], 2, false);
                });
            });
        };
        let key = |key, modifiers| vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }, Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers }];
        let open = |lib: &mut LibraryManager, c: SortColumn| lib.cell_edit = Some((id, c, field_text(lib.track(id).unwrap(), c), true));

        // Typing replaces the selected text; Tab saves and opens the next field along the row.
        open(&mut lib, SortColumn::Title);
        frame(&mut lib, vec![]);
        frame(&mut lib, vec![Event::Text("New title".into())]);
        frame(&mut lib, key(Key::Tab, Modifiers::NONE));
        frame(&mut lib, vec![]);
        assert_eq!(lib.track(id).unwrap().title, "New title");
        assert_eq!(lib.cell_edit.as_ref().map(|e| e.1), Some(SortColumn::Artist), "Tab moves to the next field");

        // Esc leaves the value as it was.
        frame(&mut lib, vec![Event::Text("Someone else".into())]);
        frame(&mut lib, key(Key::Escape, Modifiers::NONE));
        frame(&mut lib, vec![]);
        assert_eq!(lib.track(id).unwrap().artist, "Artist");
        assert!(lib.cell_edit.is_none());

        // A value that isn't allowed stays open with the reason, and changes nothing.
        open(&mut lib, SortColumn::Bpm);
        frame(&mut lib, vec![]);
        frame(&mut lib, vec![Event::Text("fast".into())]);
        frame(&mut lib, key(Key::Enter, Modifiers::NONE));
        frame(&mut lib, vec![]);
        assert_eq!(lib.track(id).unwrap().bpm, 128.0);
        assert_eq!(lib.cell_edit.as_ref().map(|e| e.1), Some(SortColumn::Bpm));
        assert!(lib.status_message.contains("BPM"), "{}", lib.status_message);

        // Filtered out of view while typing: saved, as clicking away does.
        frame(&mut lib, vec![Event::Text("124".into())]);
        lib.search_query = "nothing matches this".into();
        frame(&mut lib, vec![]);
        assert_eq!(lib.track(id).unwrap().bpm, 124.0);
        assert!(lib.cell_edit.is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn writing_tags_touches_only_the_fields_asked_for() {
        use lofty::file::TaggedFileExt;
        use lofty::tag::Accessor;
        let (_, dir) = temp_lib("tag-write");
        // A tiny valid WAV: 44-byte header, 4 silent 16-bit mono frames.
        let p = dir.join("t.wav");
        let mut wav = b"RIFF".to_vec();
        wav.extend(44u32.to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend([1, 0, 1, 0]);
        wav.extend(44100u32.to_le_bytes());
        wav.extend(88200u32.to_le_bytes());
        wav.extend([2, 0, 16, 0]);
        wav.extend(b"data");
        wav.extend(8u32.to_le_bytes());
        wav.extend([0u8; 8]);
        fs::write(&p, wav).unwrap();
        let mut t = UiTrackEntry { artist: "Kept Artist".into(), genre: "House".into(), file_path: Some(p.clone()), ..Default::default() };
        write_track_tags(&t, &[SortColumn::Artist, SortColumn::Genre]).unwrap();
        // ReTraktor doesn't know the artist; a genre-only write must leave the file's artist alone.
        (t.artist, t.genre) = (String::new(), "Garage".into());
        write_track_tags(&t, &[SortColumn::Genre]).unwrap();
        let tagged = lofty::read_from_path(&p).unwrap();
        let tag = tagged.primary_tag().unwrap();
        assert_eq!(tag.artist().as_deref(), Some("Kept Artist"));
        assert_eq!(tag.genre().as_deref(), Some("Garage"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn add_to_list_puts_the_selected_track_in_preparation_and_saves() {
        let (mut lib, dir) = temp_lib("add-to-list");
        let id = lib.push_track(track("Chosen", &dir.join("a.flac")));
        lib.add_selected_to_preparation();
        assert!(lib.playlists.iter().all(|p| p.track_ids.is_empty()), "nothing selected, nothing added");
        lib.selected_track_id = Some(id);
        lib.add_selected_to_preparation();
        let prep = lib.playlists.iter().find(|p| p.name == "Preparation").unwrap();
        assert_eq!(prep.track_ids, [id]);
        assert!(lib.dirty_since.is_some(), "the change is queued for autosave");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn browse_encoder_with_shift_steps_through_the_sidebar() {
        let (mut lib, dir) = temp_lib("nav-step");
        lib.playlists.push(UserPlaylist { name: "Warmup".into(), track_ids: Vec::new(), missing: Vec::new() });
        lib.playlists.push(UserPlaylist { name: "Peak".into(), track_ids: Vec::new(), missing: Vec::new() });
        lib.select_nav_offset(-1);
        assert_eq!(lib.selected_nav, NavItem::Collection, "stops at the top");
        lib.select_nav_offset(5);
        assert_eq!(lib.selected_nav, NavItem::Playlist(0), "Collection, Suggestions, Stems, History, Files, then playlists");
        lib.select_nav_offset(9);
        assert_eq!(lib.selected_nav, NavItem::Playlist(1), "stops at the last playlist");
        let _ = fs::remove_dir_all(dir);
    }

    fn temp_lib(name: &str) -> (LibraryManager, PathBuf) {
        let dir = env::temp_dir().join(format!("retraktor-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (LibraryManager::new(dir.join("collection.nml")), dir)
    }

    fn track(title: &str, path: &Path) -> UiTrackEntry {
        UiTrackEntry { title: title.into(), artist: "Artist".into(), bpm: 128.0, musical_key: "8m".into(), rating: 3, file_path: Some(path.to_path_buf()), ..Default::default() }
    }

    /// The Preview column sits directly before Title in every track view: after # where order matters, before the
    /// Match column Suggestions puts after Title.
    #[test]
    fn preview_column_comes_just_before_title_in_every_view() {
        use SortColumn::*;
        let (mut lib, dir) = temp_lib("preview-column");
        let columns = LibraryColumnsConfig::default();
        for (nav, start) in [
            (NavItem::Collection, &[Preview, Title, Artist][..]),
            (NavItem::History, &[Order, Preview, Title, Artist][..]),
            (NavItem::Playlist(0), &[Order, Preview, Title, Artist][..]),
            (NavItem::Suggestions, &[Preview, Title, Match, Artist][..]),
        ] {
            lib.selected_nav = nav;
            let cols = lib.visible_columns(&columns);
            assert_eq!(&cols[..start.len()], start, "{nav:?}");
            assert_eq!(cols.iter().filter(|&&c| c == Preview).count(), 1, "{nav:?}");
        }
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stem_queue_skips_ready_queued_and_fileless_tracks() {
        let (mut lib, dir) = temp_lib("stemq");
        let mut ready = track("Ready", &dir.join("r.wav"));
        ready.id = 1;
        ready.stems_ready = true;
        let mut a = track("A", &dir.join("a.wav"));
        a.id = 2;
        let mut nofile = track("No file", &dir.join("n.wav"));
        nofile.id = 3;
        nofile.file_path = None;
        lib.tracks = vec![ready, a, nofile];
        assert_eq!(lib.queue_stems(&[1, 2, 3, 2, 99]), 1);
        assert_eq!(lib.queue_stems(&[2]), 0);
        assert_eq!(lib.stem_queue, [2]);
    }

    #[test]
    fn new_playlist_name_commits_on_enter() {
        let (mut lib, _) = temp_lib("rename");
        lib.add_to_playlist(&[], None);
        let ctx = egui::Context::default();
        let frame = |lib: &mut LibraryManager, events: Vec<egui::Event>| {
            let _ = ctx.run(egui::RawInput { events, ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| lib.show_nav_tree(ui));
            });
        };
        frame(&mut lib, vec![]);
        frame(&mut lib, vec![]);
        let key = |key, pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut lib, vec![key(egui::Key::A, true), egui::Event::Text("Friday".into())]);
        frame(&mut lib, vec![key(egui::Key::Enter, true), key(egui::Key::Enter, false)]);
        frame(&mut lib, vec![]);
        assert!(lib.renaming.is_none(), "still editing after Enter");
        assert_eq!(lib.playlists[0].name, "Friday");
        // Enter on an empty box keeps the default name.
        lib.add_to_playlist(&[], None);
        frame(&mut lib, vec![]);
        frame(&mut lib, vec![]);
        frame(&mut lib, vec![key(egui::Key::Enter, true), key(egui::Key::Enter, false)]);
        frame(&mut lib, vec![]);
        assert_eq!(lib.playlists[1].name, "New playlist 2");
    }

    #[test]
    fn untagged_file_gets_no_invented_metadata() {
        let (_, dir) = temp_lib("untagged");
        let p = dir.join("Some Artist - Some Title.mp3");
        fs::write(&p, b"not really audio").unwrap();
        let t = read_track_file(&p);
        assert_eq!((t.artist.as_str(), t.title.as_str()), ("Some Artist", "Some Title"));
        assert_eq!(t.bpm, 0.0);
        assert!(t.musical_key.is_empty() && t.genre.is_empty());
        assert_eq!(t.rating, 0);
        assert!(!t.stems_ready);
    }

    #[test]
    fn playlists_cues_and_ratings_survive_save_and_reload() {
        let (mut lib, dir) = temp_lib("roundtrip");
        let mut a = track("Alpha", &dir.join("a.flac"));
        a.cues.push(NmlCue { name: "Drop".into(), displ_order: 0, cue_type: retraktor_core::CueType::Cue, start_ms: 61_000.0, len_ms: 0.0, repeats: -1, hotcue: 2, grid_bpm: None });
        let a = lib.push_track(a);
        let b = lib.push_track(track("Beta", &dir.join("b.flac")));
        lib.add_to_playlist(&[b, a], None);
        let path = lib.collection_path.clone();
        lib.save_collection_nml(&path).unwrap();

        let back = LibraryManager::new(path);
        assert_eq!(back.tracks.len(), 2);
        assert_eq!(back.playlists.len(), 1);
        let titles: Vec<_> = back.playlists[0].track_ids.iter().map(|id| back.track(*id).unwrap().title.clone()).collect();
        assert_eq!(titles, ["Beta", "Alpha"]);
        let alpha = back.tracks.iter().find(|t| t.title == "Alpha").unwrap();
        assert_eq!(alpha.rating, 3);
        assert_eq!(alpha.musical_key, "8m");
        assert_eq!(alpha.cues.len(), 1);
        assert_eq!(alpha.cues[0].hotcue, 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn hotcue_pad_edits_keep_other_cues_through_nml_reload() {
        use retraktor_core::CueType;
        let (mut lib, dir) = temp_lib("hotcue-pads");
        let cue = |name: &str, displ_order, cue_type, start_ms, len_ms, repeats, hotcue, grid_bpm| NmlCue {
            name: name.into(), displ_order, cue_type, start_ms, len_ms, repeats, hotcue, grid_bpm,
        };
        let grid = cue("Beat", 0, CueType::Grid, 250.0, 0.0, -1, 0, Some(126.5));
        let intro = cue("Intro", 1, CueType::Cue, 1500.0, 0.0, -1, -1, None);
        let build = cue("Build", 2, CueType::Loop, 30000.0, 7619.0, 4, 2, None);
        let drop = cue("Drop", 3, CueType::Cue, 61000.0, 0.0, -1, 4, None);
        let mut t = track("Pads", &dir.join("pads.flac"));
        t.cues = vec![grid.clone(), intro.clone(), build.clone(), drop.clone()];
        let id = lib.push_track(t);
        let cues = |lib: &LibraryManager| lib.track(id).unwrap().cues.clone();

        // A filled pad, an invalid pad or an empty clear never touches the cue list.
        let before = cues(&lib);
        for slot in [0, 9] {
            lib.add_hotcue(id, slot, 1.0);
            lib.clear_hotcue(id, slot);
        }
        lib.add_hotcue(id, 5, 99.0);
        lib.clear_hotcue(id, 8);
        assert_eq!(cues(&lib), before);

        // Setting and clearing pad 2 leaves every other cue as it was, in order.
        lib.add_hotcue(id, 2, 10.0);
        assert_eq!(cues(&lib)[..4], before[..]);
        assert_eq!((cues(&lib)[4].hotcue, cues(&lib)[4].start_ms), (1, 10000.0));
        lib.clear_hotcue(id, 2);
        assert_eq!(cues(&lib), before);

        // Clearing the grid's pad keeps the grid; clearing the loop's pad deletes the loop.
        lib.clear_hotcue(id, 1);
        lib.clear_hotcue(id, 3);
        lib.add_hotcue(id, 7, 90.0);
        let unslotted_grid = NmlCue { hotcue: -1, ..grid };
        let pad7 = cue("Hotcue 7", 0, CueType::Cue, 90000.0, 0.0, -1, 6, None);
        let expected = vec![unslotted_grid, intro, drop, pad7];
        assert_eq!(cues(&lib), expected);
        assert_eq!(lib.track(id).unwrap().hotcue_slots(), [(5, 61.0), (7, 90.0)]);

        let path = lib.collection_path.clone();
        lib.save_collection_nml(&path).unwrap();
        let back = LibraryManager::new(path);
        assert_eq!(back.tracks.iter().find(|t| t.title == "Pads").unwrap().cues, expected);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn track_ids_are_never_reused_while_a_load_is_pending() {
        let (mut lib, dir) = temp_lib("deck-ids");
        lib.push_track(track("Older", &dir.join("a.flac")));
        let file = dir.join("b.flac");
        fs::write(&file, b"queued, never decoded").unwrap();
        let pending = lib.push_track(track("Pending", &file));
        lib.load_to_deck(1, pending, [false; 4]);
        assert_eq!(lib.pending_loads.len(), 1, "load is queued but unfinished");

        // Removing the highest id while its load is pending must not free that id for the next import.
        lib.apply(Action::RemoveFromCollection(vec![pending]), [false; 4], 2);
        let new = lib.push_track(track("New", &dir.join("c.flac")));
        assert!(new > pending);
        assert!(lib.track(pending).is_none());

        // Entries pushed with their own id (demo data) raise the mark as well.
        lib.tracks.push(UiTrackEntry { id: 50, title: "Manual".into(), ..Default::default() });
        lib.rebuild_index();
        assert_eq!(lib.push_track(track("After manual", &dir.join("d.flac"))), 51);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_sort_and_suggestions() {
        let (mut lib, dir) = temp_lib("view");
        let mut t1 = track("Zed", &dir.join("1.flac"));
        t1.bpm = 124.0;
        t1.musical_key = "9m".into();
        let mut t2 = track("Ace", &dir.join("2.flac"));
        t2.bpm = 140.0;
        t2.musical_key = "3d".into();
        lib.push_track(t1);
        lib.push_track(t2);
        lib.sort_col = SortColumn::Title;
        lib.ensure_view();
        assert_eq!(lib.tracks[lib.view[0]].title, "Ace");
        lib.search_query = "zed".into();
        lib.ensure_view();
        assert_eq!(lib.view.len(), 1);
        lib.search_query.clear();
        lib.selected_nav = NavItem::Suggestions;
        let mut r = track("Ref", &dir.join("r.flac"));
        r.bpm = 125.0;
        lib.deck_tracks[0] = Some(lib.push_track(r));
        lib.ensure_view();
        assert_eq!(lib.view.len(), 1, "only the harmonic, near-tempo track matches");
        assert_eq!(lib.tracks[lib.view[0]].title, "Zed");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn analysis_fills_unknowns_and_keeps_user_cues() {
        let a = TrackAnalysis { bpm: 126.0, beatgrid_offset_sec: 0.5, key_value: 21, key_open: "1m".into(), energy: 7, loudness_db: -9.0, peak_db: -0.3, cue_points_sec: vec![0.5, 31.0], phrases: Vec::new() };
        let mut fresh = UiTrackEntry::default();
        fresh.apply_analysis(&a, false, true);
        assert_eq!((fresh.bpm, fresh.musical_key.as_str(), fresh.energy), (126.0, "1m", 7));
        assert_eq!(fresh.cues.len(), 3, "grid + 2 auto cues");

        let mut tagged = UiTrackEntry { bpm: 128.0, musical_key: "8m".into(), ..Default::default() };
        tagged.cues.push(NmlCue { name: "Drop".into(), displ_order: 0, cue_type: retraktor_core::CueType::Cue, start_ms: 1.0, len_ms: 0.0, repeats: -1, hotcue: 0, grid_bpm: None });
        tagged.apply_analysis(&a, false, true);
        assert_eq!((tagged.bpm, tagged.musical_key.as_str(), tagged.energy), (128.0, "8m", 7));
        assert_eq!(tagged.cues.iter().filter(|c| c.name.starts_with("Auto ")).count(), 0);
        tagged.apply_analysis(&a, true, true);
        assert_eq!(tagged.bpm, 126.0);
    }

    #[test]
    fn finds_traktor_collections_a_few_levels_down() {
        let (_, dir) = temp_lib("detect");
        let deep = dir.join("nas/Music/Traktor/Library");
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("collection.nml"), "<NML/>").unwrap();
        assert_eq!(find_collections(&[dir.clone()], 4, 100), vec![deep.join("collection.nml")]);
        assert!(find_collections(&[dir.clone()], 2, 100).is_empty(), "depth limit respected");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn remove_and_undo_restores_tracks_and_playlists() {
        let (mut lib, dir) = temp_lib("undo");
        let a = lib.push_track(track("A", &dir.join("a.flac")));
        lib.push_track(track("B", &dir.join("b.flac")));
        lib.add_to_playlist(&[a], None);
        lib.apply(Action::RemoveFromCollection(vec![a]), [false; 4], 2);
        assert_eq!((lib.tracks.len(), lib.playlists[0].track_ids.len()), (1, 0));
        lib.undo();
        assert_eq!((lib.tracks.len(), lib.playlists[0].track_ids.clone()), (2, vec![a]));
        assert!(lib.track(a).is_some());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn search_filters() {
        let t = UiTrackEntry { title: "Deep Thing".into(), genre: "UK Garage".into(), bpm: 130.0, musical_key: "8m".into(), energy: 7, rating: 4, year: "2001".into(), ..Default::default() };
        let q = |s: &str| matches_query(&t, &s.to_lowercase().split_whitespace().map(str::to_string).collect::<Vec<_>>());
        assert!(q("bpm:128-132 key:8m"));
        assert!(q("bpm:65")); // half-time counts
        assert!(q("key:~9m energy:>6 rating:>=4 year:<2005 genre:garage deep"));
        assert!(q("key:~Bbm")); // musical notation: Bbm is 8m
        assert!(!q("key:~Am")); // Am is 1m, not next to 8m
        assert!(!q("key:3d"));
        assert!(!q("energy:<5"));
        assert!(!q("bpm:140-150"));
    }

    #[test]
    fn tag_fill_never_overwrites() {
        let mut t = UiTrackEntry { title: "Kept".into(), bpm: 128.0, ..Default::default() };
        t.fill_empty_from(UiTrackEntry { title: "Other".into(), genre: "Garage".into(), bpm: 64.0, musical_key: "8m".into(), ..Default::default() });
        assert_eq!((t.title.as_str(), t.genre.as_str(), t.bpm, t.musical_key.as_str()), ("Kept", "Garage", 128.0, "8m"));
    }

    #[test]
    fn serato_tags_fill_only_tracks_without_cues() {
        use lofty::config::WriteOptions;
        use lofty::id3::v2::{BinaryFrame, Frame, FrameId, Id3v2Tag};
        use lofty::tag::TagExt;
        use retraktor_core::CueType;

        let (mut lib, dir) = temp_lib("serato");
        let path = dir.join("Tagged.mp3");
        let mut id3 = Id3v2Tag::new();
        id3.insert(Frame::Binary(BinaryFrame::new(
            FrameId::Valid("GEOB".into()),
            b"\0application/octet-stream\0\0Serato BeatGrid\0\x01\x00\x00\x00\x00\x01\x3e\x80\x00\x00\x42\xf0\x00\x00\x37".to_vec(),
        )));
        let mut file = Vec::new();
        id3.dump_to(&mut file, WriteOptions::default()).unwrap();
        // Two MPEG-1 Layer III frames, 128 kbps at 44.1 kHz.
        let mut frame = [0; 417];
        frame[..4].copy_from_slice(&[0xff, 0xfb, 0x90, 0x64]);
        file.extend_from_slice(&frame);
        file.extend_from_slice(&frame);
        fs::write(&path, &file).unwrap();

        let id = lib.add_file(&path);
        let added = lib.track(id).unwrap();
        assert_eq!(added.bpm, 120.0);
        assert_eq!(added.cues.len(), 1);
        assert_eq!((added.cues[0].cue_type, added.cues[0].start_ms), (CueType::Grid, 250.0));

        let existing = NmlCue {
            name: "Existing".into(), displ_order: 0, cue_type: CueType::Cue,
            start_ms: 2000.0, len_ms: 0.0, repeats: -1, hotcue: 0, grid_bpm: None,
        };
        let track = lib.track_mut(id).unwrap();
        track.cues = vec![existing.clone()];
        track.bpm = 0.0;
        read_tags_into(&lib.tx, &[(id, path.clone())]);
        lib.poll();
        let track = lib.track(id).unwrap();
        assert_eq!(track.cues, [existing]);
        assert_eq!(track.bpm, 0.0);
        assert_eq!(fs::read(&path).unwrap(), file);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn match_score_handles_half_tempo_and_keys() {
        let r = UiTrackEntry { bpm: 128.0, musical_key: "8m".into(), ..Default::default() };
        let t = UiTrackEntry { bpm: 64.0, musical_key: "8m".into(), ..Default::default() };
        assert!(match_score(&r, &t) > 0.99);
        let far = UiTrackEntry { bpm: 100.0, musical_key: "2d".into(), ..Default::default() };
        assert_eq!(match_score(&r, &far), 0.0);
        let r_e = UiTrackEntry { energy: 5, ..r.clone() };
        let calm = UiTrackEntry { energy: 1, ..t.clone() };
        let close = UiTrackEntry { energy: 6, ..t.clone() };
        assert!(match_score(&r_e, &close) > match_score(&r_e, &calm));

        assert_eq!(genre_score("Deep House", "deep house"), Some(1.0));
        assert_eq!(genre_score("House", "Deep House"), Some(0.7));
        assert_eq!(genre_score("Deep House", "Tech House"), Some(0.5));
        assert_eq!(genre_score("Deep House", "Deep Techno"), Some(0.0));
        assert_eq!(genre_score("", "House"), None);

        let r_g = UiTrackEntry { genre: "Deep House".into(), ..r };
        let same_g = UiTrackEntry { genre: "Tech House".into(), ..t.clone() };
        let diff_g = UiTrackEntry { genre: "Techno".into(), ..t };
        assert!(match_score(&r_g, &same_g) > match_score(&r_g, &diff_g));
    }

    #[test]
    fn relocate_finds_moved_files_by_name() {
        let (mut lib, dir) = temp_lib("relocate");
        let moved = dir.join("new/Album");
        fs::create_dir_all(&moved).unwrap();
        fs::write(moved.join("song.flac"), b"x").unwrap();
        let mut t = track("Song", Path::new("/old/Album/song.flac"));
        t.missing = true;
        lib.push_track(t);
        lib.start_relocate(vec![dir.clone()]);
        let deadline = Instant::now() + Duration::from_secs(5);
        while lib.is_busy() && Instant::now() < deadline {
            lib.poll();
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(lib.tracks[0].file_path.as_deref(), Some(moved.join("song.flac").as_path()));
        assert!(!lib.tracks[0].missing);
        let _ = fs::remove_dir_all(dir);
    }

    /// Relocates one missing track saved at `/old/Album/song.flac` among `files` (relative to the
    /// temp dir), searching `roots` (relative too); returns its path, missing flag and the final status.
    fn relocate_one(name: &str, files: &[&str], roots: &[&str]) -> (PathBuf, bool, String, PathBuf) {
        let (mut lib, dir) = temp_lib(name);
        for f in files {
            let p = dir.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, b"x").unwrap();
        }
        let mut t = track("Song", Path::new("/old/Album/song.flac"));
        t.missing = true;
        lib.push_track(t);
        lib.start_relocate(roots.iter().map(|r| dir.join(r)).collect());
        let deadline = Instant::now() + Duration::from_secs(5);
        while lib.is_busy() && Instant::now() < deadline {
            lib.poll();
            thread::sleep(Duration::from_millis(10));
        }
        let t = &lib.tracks[0];
        let out = (t.file_path.clone().unwrap(), t.missing, lib.status_message.clone(), dir.clone());
        let _ = fs::remove_dir_all(dir);
        out
    }

    #[test]
    fn relocate_counts_a_file_found_through_overlapping_roots_once() {
        let (path, missing, status, dir) = relocate_one("reloc-overlap", &["new/Other/song.flac"], &["", "new"]);
        assert_eq!(path, dir.join("new/Other/song.flac"));
        assert!(!missing);
        assert!(!status.contains("Multiple matches"), "{status}");
    }

    #[test]
    fn relocate_picks_the_only_file_in_a_folder_named_like_the_original() {
        let (path, missing, _, dir) = relocate_one("reloc-parent", &["a/Other/song.flac", "b/ALBUM/song.flac"], &[""]);
        assert_eq!(path, dir.join("b/ALBUM/song.flac"));
        assert!(!missing);
    }

    #[test]
    fn relocate_leaves_tracks_with_tied_candidates_alone() {
        for (name, files) in [("reloc-noparent", ["x/song.flac", "y/song.flac"]), ("reloc-twoparents", ["a/Album/song.flac", "b/album/song.flac"])] {
            let (path, missing, status, _) = relocate_one(name, &files, &[""]);
            assert_eq!(path, Path::new("/old/Album/song.flac"), "{name}");
            assert!(missing, "{name}");
            assert_eq!(status, "Relocated 0 of 1 missing tracks. Multiple matches for 1. Use Search another folder…", "{name}");
        }
    }

    #[test]
    fn hide_missing_files_hides_in_collection_but_keeps_missing_view() {
        let (mut lib, dir) = temp_lib("hide-missing");
        let present = track("Present", &dir.join("ok.flac"));
        let mut gone = track("Gone", &dir.join("gone.flac"));
        gone.missing = true;
        lib.push_track(present);
        lib.push_track(gone);

        lib.ensure_view();
        assert_eq!(lib.view.len(), 2);

        lib.hide_missing_files = true;
        lib.ensure_view();
        assert_eq!(lib.view.len(), 1);
        assert_eq!(lib.tracks[lib.view[0]].title, "Present");

        lib.selected_nav = NavItem::Missing;
        lib.ensure_view();
        assert_eq!(lib.view.len(), 1);
        assert_eq!(lib.tracks[lib.view[0]].title, "Gone");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn quick_filters_bpm_key_energy_genre() {
        let (mut lib, dir) = temp_lib("quick-filters");
        let make = |title: &str, bpm: f64, key: &str, energy: u8, genre: &str| UiTrackEntry {
            title: title.into(),
            bpm,
            musical_key: key.into(),
            energy,
            genre: genre.into(),
            file_path: Some(dir.join(format!("{title}.flac"))),
            ..Default::default()
        };
        lib.push_track(make("A", 128.0, "12d", 7, "House"));
        lib.push_track(make("B", 138.0, "1d", 8, "house"));
        lib.push_track(make("C", 118.0, "12m", 6, "Techno"));
        lib.push_track(make("D", 145.0, "8d", 9, "House"));
        lib.push_track(make("E", 128.0, "8m", 5, "House"));
        lib.push_track(make("F", 0.0, "", 0, ""));

        let titles = |lib: &mut LibraryManager| -> Vec<String> {
            lib.ensure_view();
            lib.view.iter().map(|&i| lib.tracks[i].title.clone()).collect()
        };

        // BPM ±10 (inclusive boundaries 118..=138, excludes 0 and 145)
        lib.quick_filter.toggle_bpm(128.0);
        assert_eq!(titles(&mut lib), vec!["A", "B", "C", "E"]);
        lib.quick_filter.toggle_bpm(128.0);
        assert!(lib.quick_filter.bpm_center.is_none());

        // Key ±1 with 12d <-> 1d wrap and 12d <-> 12m relative
        lib.quick_filter.toggle_key("12d");
        assert_eq!(titles(&mut lib), vec!["A", "B", "C"]);
        lib.quick_filter.toggle_key("8d");
        assert_eq!(titles(&mut lib), vec!["D", "E"]);
        lib.quick_filter.clear();

        // Energy ±1 (center 7 matches 6, 7, 8)
        lib.quick_filter.toggle_energy(7);
        assert_eq!(titles(&mut lib), vec!["A", "B", "C"]);
        lib.quick_filter.clear();

        // Genre exact case-insensitive
        lib.quick_filter.toggle_genre("HOUSE");
        assert_eq!(titles(&mut lib), vec!["A", "B", "D", "E"]);
        lib.quick_filter.toggle_genre("house");
        assert!(lib.quick_filter.genre_exact.is_none());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn match_sort_toggles_ascending_and_descending() {
        let (mut lib, dir) = temp_lib("match-sort");
        let mut exact = track("Exact", &dir.join("exact.flac"));
        exact.bpm = 128.0;
        exact.musical_key = "8m".into();
        let mut close = track("Close", &dir.join("close.flac"));
        close.bpm = 126.0;
        close.musical_key = "9m".into();
        let mut ref_t = track("Ref", &dir.join("ref.flac"));
        ref_t.bpm = 128.0;
        ref_t.musical_key = "8m".into();
        lib.push_track(close);
        lib.push_track(exact);
        lib.deck_tracks[0] = Some(lib.push_track(ref_t));
        lib.selected_nav = NavItem::Suggestions;

        // Default Suggestions view sorts best match first.
        lib.ensure_view();
        assert_eq!(lib.tracks[lib.view[0]].title, "Exact");
        assert_eq!(lib.tracks[lib.view[1]].title, "Close");

        // Clicking Match starts descending (best first), clicking again flips to ascending.
        lib.apply(Action::Sort(SortColumn::Match), [false; 4], 2);
        lib.ensure_view();
        assert!(!lib.sort_asc);
        assert_eq!(lib.tracks[lib.view[0]].title, "Exact");

        lib.apply(Action::Sort(SortColumn::Match), [false; 4], 2);
        lib.ensure_view();
        assert!(lib.sort_asc);
        assert_eq!(lib.tracks[lib.view[0]].title, "Close");
        assert_eq!(lib.tracks[lib.view[1]].title, "Exact");

        let _ = fs::remove_dir_all(dir);
    }
}
