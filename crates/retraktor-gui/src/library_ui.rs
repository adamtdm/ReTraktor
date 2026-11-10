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

    /// Applies detected values. Without `overwrite` only unknown values are filled.
    /// Auto cues and a beatgrid are only added to tracks that have no cues of that kind.
    pub fn apply_analysis(&mut self, a: &TrackAnalysis, overwrite: bool) {
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
        if !self.cues.iter().any(|c| c.cue_type == CueType::Cue && c.hotcue >= 0) {
            for (i, t) in a.cue_points_sec.iter().take(8).enumerate() {
                self.cues.push(cue(format!("Auto {}", i + 1), *t, CueType::Cue, i as i64, None));
            }
        }
    }

    /// Copies values from `tags` into fields that are still empty; never overwrites.
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
    use lofty::file::{AudioFile, TaggedFileExt};
    use lofty::tag::{Accessor, ItemKey};

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
    let Ok(tagged) = lofty::read_from_path(path) else {
        return t;
    };
    let props = tagged.properties();
    t.duration_sec = props.duration().as_secs_f64();
    t.bitrate_kbps = props.audio_bitrate().or(props.overall_bitrate()).unwrap_or(0);
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return t;
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
    t
}

/// Writes the editable fields back into the audio file's own tags.
fn write_track_tags(t: &UiTrackEntry) -> Result<(), String> {
    use lofty::config::WriteOptions;
    use lofty::file::TaggedFileExt;
    use lofty::tag::{Accessor, ItemKey, Tag, TagExt};

    let path = t.file_path.as_ref().ok_or("track has no file")?;
    let mut tagged = lofty::read_from_path(path).map_err(|e| e.to_string())?;
    if tagged.primary_tag().is_none() {
        let tag_type = tagged.primary_tag_type();
        tagged.insert_tag(Tag::new(tag_type));
    }
    let tag = tagged.primary_tag_mut().ok_or("cannot create tag")?;
    tag.set_title(t.title.clone());
    tag.set_artist(t.artist.clone());
    tag.set_album(t.album.clone());
    tag.set_genre(t.genre.clone());
    tag.set_comment(t.comment.clone());
    for (key, val) in [
        (ItemKey::Remixer, t.remixer.clone()),
        (ItemKey::Label, t.label.clone()),
        (ItemKey::InitialKey, t.musical_key.clone()),
        (ItemKey::Bpm, if t.bpm > 0.0 { format!("{:.2}", t.bpm) } else { String::new() }),
    ] {
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

pub fn stereo_to_mono(pcm: &[f32]) -> Vec<f32> {
    pcm.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])).collect()
}

/// Reads tags for `items` and sends them in batches.
fn read_tags_into(tx: &Sender<Msg>, items: &[(usize, PathBuf)]) {
    for (n, chunk) in items.chunks(100).enumerate() {
        let batch = chunk.iter().map(|(id, p)| (*id, read_track_file(p))).collect();
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

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("nml.tmp");
    fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

/// Harmonic + tempo (+ energy when known) compatibility of `t` with `r`, `0.0..=1.0`.
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
    if r.energy > 0 && t.energy > 0 {
        let energy_s = (1.0 - (t.energy as f32 - r.energy as f32).abs() / 4.0).max(0.0);
        0.5 * key_s + 0.4 * bpm_s + 0.1 * energy_s
    } else {
        0.55 * key_s + 0.45 * bpm_s
    }
}

#[derive(Clone, Debug)]
pub struct UserPlaylist {
    pub name: String,
    pub track_ids: Vec<usize>,
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
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SortColumn {
    Order,
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
    MusicFolder,
    RelocateFolder,
}

pub enum Msg {
    Picked(Pick, PathBuf),
    Detected(Vec<PathBuf>),
    Status(String),
    Scanned(Vec<UiTrackEntry>),
    Checked(Vec<(usize, bool, bool)>),
    Relocated(Vec<(usize, PathBuf)>),
    Tags(Vec<(usize, UiTrackEntry)>),
    /// (track id, analysis, overwrite existing values)
    Analyzed(usize, TrackAnalysis, bool),
    Saved(Result<usize, String>),
    Done(String),
}

enum Action {
    Select(usize, egui::Modifiers),
    Load(usize, usize),
    LoadAuto(usize),
    AddToPlaylist(Vec<usize>, Option<usize>),
    RemoveFromPlaylist(Vec<usize>),
    Edit(usize),
    FillTags(Vec<usize>),
    Analyze(Vec<usize>),
    Stems(usize),
    Reveal(PathBuf),
    RemoveFromCollection(Vec<usize>),
    Sort(SortColumn),
}

pub struct LibraryManager {
    pub tracks: Vec<UiTrackEntry>,
    pub playlists: Vec<UserPlaylist>,
    pub history: Vec<usize>,
    pub search_query: String,
    pub selected_nav: NavItem,
    pub selected_track_id: Option<usize>,
    selection: BTreeSet<usize>,
    pub sort_col: SortColumn,
    pub sort_asc: bool,

    version: u64,
    view: Vec<usize>,
    view_scores: Vec<f32>,
    view_stamp: Option<(u64, NavItem, String, SortColumn, bool, String)>,
    id_index: HashMap<usize, usize>,
    scroll_to_selected: bool,

    collection_path: PathBuf,
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
    /// Last destructive change: (description, tracks, playlists) before it happened.
    undo: Option<(String, Vec<UiTrackEntry>, Vec<UserPlaylist>)>,

    renaming: Option<(usize, String)>,
    editing: Option<UiTrackEntry>,
    /// Traktor collections found on this computer; `None` while still searching.
    detected: Option<Vec<PathBuf>>,
    picked: Vec<(Pick, PathBuf)>,
    typed_path: Option<String>,
    explorer_dir: PathBuf,
    explorer_path_input: String,
    explorer_entries: Vec<(PathBuf, bool)>,

    // App integration
    pub pending_loads: Vec<(usize, UiTrackEntry)>,
    pub status_message: String,
    pub stem_job_progress: Option<(usize, f32)>,
    pub request_stem_separation: Option<usize>,
    pub last_stemmed_track_id: Option<usize>,
    pub last_loaded_deck: Option<(usize, bool)>,
    pub open_prefs_requested: bool,
    pub open_prefs_file_management: bool,
    pub open_prefs_browser_columns: bool,
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
            history: Vec::new(),
            search_query: String::new(),
            selected_nav: NavItem::Collection,
            selected_track_id: None,
            selection: BTreeSet::new(),
            sort_col: SortColumn::Order,
            sort_asc: true,
            version: 0,
            view: Vec::new(),
            view_scores: Vec::new(),
            view_stamp: None,
            id_index: HashMap::new(),
            scroll_to_selected: false,
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
            undo: None,
            renaming: None,
            editing: None,
            detected: None,
            picked: Vec::new(),
            typed_path: None,
            explorer_path_input: home.display().to_string(),
            explorer_dir: home,
            explorer_entries: Vec::new(),
            pending_loads: Vec::new(),
            status_message: String::new(),
            stem_job_progress: None,
            request_stem_separation: None,
            last_stemmed_track_id: None,
            last_loaded_deck: None,
            open_prefs_requested: false,
            open_prefs_file_management: false,
            open_prefs_browser_columns: false,
        };
        let path = mgr.collection_path.clone();
        if path.is_file() {
            match mgr.import_nml_file(&path) {
                Ok(_) => mgr.status_message.clear(),
                Err(e) => mgr.status_message = format!("Could not read {}: {e}", path.display()),
            }
            mgr.dirty_since = None;
        }
        mgr
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
                Pick::MusicFolder => dialog.set_title("Choose a music folder").pick_folder(),
                Pick::RelocateFolder => dialog.set_title("Choose the folder your missing tracks moved to").pick_folder(),
            };
            if let Some(p) = picked {
                let _ = tx.send(Msg::Picked(kind, p));
            }
        });
    }

    /// Processes background results and chosen files; call every frame, even while preferences are open.
    pub fn apply_picks(&mut self, music_folders: &mut Vec<PathBuf>) {
        self.poll();
        for (kind, path) in std::mem::take(&mut self.picked) {
            self.handle_pick(kind, path, music_folders);
        }
    }

    /// Acts on a chosen file or folder.
    fn handle_pick(&mut self, kind: Pick, path: PathBuf, music_folders: &mut Vec<PathBuf>) {
        match kind {
            Pick::Collection => match self.import_nml_file(&path) {
                Ok(_) => self.relocate_after_check = Some(music_folders.clone()),
                Err(e) => self.status_message = format!("Couldn't read {}: {e}", path.display()),
            },
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

    fn touch(&mut self) {
        self.version += 1;
        self.dirty_since = Some(Instant::now());
    }

    fn rebuild_index(&mut self) {
        self.id_index = self.tracks.iter().enumerate().map(|(i, t)| (t.id, i)).collect();
    }

    pub fn track(&self, id: usize) -> Option<&UiTrackEntry> {
        self.id_index.get(&id).and_then(|&i| self.tracks.get(i))
    }

    fn track_mut(&mut self, id: usize) -> Option<&mut UiTrackEntry> {
        self.id_index.get(&id).and_then(|&i| self.tracks.get_mut(i))
    }

    fn next_id(&self) -> usize {
        self.tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1
    }

    fn push_track(&mut self, mut t: UiTrackEntry) -> usize {
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
        write_atomic(path, &col.to_xml())?;
        if path == self.collection_path {
            self.dirty_since = None;
        }
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
            let _ = tx.send(Msg::Saved(write_atomic(&path, &col.to_xml()).map(|_| n)));
        });
    }

    /// Merges a Traktor `.nml` (tracks, cues, grids, playlists) into the collection.
    pub fn import_nml_file(&mut self, path: &Path) -> Result<usize, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let col = NmlCollection::parse_xml(&content)?;
        let mut by_path: HashMap<PathBuf, usize> = self
            .tracks
            .iter()
            .filter_map(|t| Some((t.file_path.clone()?, t.id)))
            .collect();
        let mut by_key: HashMap<String, usize> = HashMap::new();
        let mut added = 0;
        for e in &col.entries {
            let t = UiTrackEntry::from_nml(e);
            let p = t.file_path.clone().unwrap_or_default();
            let id = match by_path.get(&p) {
                Some(&id) => id,
                None => {
                    added += 1;
                    let id = self.push_track(t);
                    by_path.insert(p, id);
                    id
                }
            };
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
                }),
                NmlPlaylistNode::Smartlist { .. } => {}
            }
        }
        let mut imported = Vec::new();
        col.playlists.iter().for_each(|n| walk(n, "", &by_key, &mut imported));
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
        self.status_message = format!("Imported {added} new tracks and {} playlists from {}", self.playlists.len(), path.display());
        Ok(col.entries.len())
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
            let fixed: Vec<(usize, PathBuf)> = missing
                .iter()
                .filter_map(|(id, name, parent)| {
                    let cands = found.get(name)?;
                    // Prefer a candidate in a folder with the same name as the original.
                    let best = cands
                        .iter()
                        .find(|c| c.parent().and_then(|d| d.file_name()).is_some_and(|d| d.to_string_lossy().to_lowercase() == *parent))
                        .unwrap_or(&cands[0]);
                    Some((*id, best.clone()))
                })
                .collect();
            let n = fixed.len();
            let _ = tx.send(Msg::Relocated(fixed.clone()));
            read_tags_into(tx, &fixed);
            format!("Relocated {n} of {} missing tracks", missing.len())
        });
    }

    /// Sender for results produced outside the library (e.g. analysis done while loading a deck).
    pub fn sender(&self) -> Sender<Msg> {
        self.tx.clone()
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
                    for (id, missing, stems) in res {
                        if let Some(t) = self.track_mut(id) {
                            t.missing = missing;
                            t.stems_ready = stems;
                        }
                    }
                    self.version += 1;
                }
                Msg::Relocated(fixed) => {
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
                    for (id, tags) in batch {
                        if let Some(t) = self.track_mut(id) {
                            t.fill_empty_from(tags);
                        }
                    }
                    self.touch();
                }
                Msg::Analyzed(id, a, overwrite) => {
                    if let Some(t) = self.track_mut(id) {
                        t.apply_analysis(&a, overwrite);
                    }
                    self.touch();
                }
                Msg::Saved(Err(e)) => {
                    self.saving = false;
                    self.status_message = format!("Could not save collection: {e}");
                }
                Msg::Saved(Ok(_)) => self.saving = false,
                Msg::Done(s) => {
                    self.jobs = self.jobs.saturating_sub(1);
                    self.status_message = s;
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
        let stamp = (self.version, self.selected_nav, self.search_query.clone(), self.sort_col, self.sort_asc, ref_s);
        if self.view_stamp.as_ref() == Some(&stamp) {
            return;
        }
        if self.id_index.len() != self.tracks.len() {
            self.rebuild_index();
        }
        let ids: Vec<usize> = match self.selected_nav {
            NavItem::Playlist(i) => self.playlists.get(i).map(|p| p.track_ids.clone()).unwrap_or_default(),
            NavItem::History => self.history.iter().rev().copied().collect(),
            NavItem::Missing => self.tracks.iter().filter(|t| t.missing).map(|t| t.id).collect(),
            NavItem::Stems => self.tracks.iter().filter(|t| t.stems_ready).map(|t| t.id).collect(),
            _ => self.tracks.iter().map(|t| t.id).collect(),
        };
        let terms: Vec<String> = self.search_query.to_lowercase().split_whitespace().map(str::to_string).collect();
        let mut rows: Vec<(usize, f32)> = ids
            .into_iter()
            .filter_map(|id| self.id_index.get(&id).map(|&i| i))
            .filter(|&i| terms.is_empty() || {
                let blob = self.tracks[i].search_blob();
                terms.iter().all(|w| blob.contains(w.as_str()))
            })
            .map(|i| (i, reference.as_ref().map_or(0.0, |r| match_score(r, &self.tracks[i]))))
            .collect();
        if self.selected_nav == NavItem::Suggestions {
            rows.retain(|&(i, s)| s >= 0.6 && reference.as_ref().is_some_and(|r| r.id != self.tracks[i].id));
        }
        let col = if self.selected_nav == NavItem::Suggestions && self.sort_col == SortColumn::Order { SortColumn::Match } else { self.sort_col };
        let t = &self.tracks;
        let lc = |s: &str| s.to_lowercase();
        rows.sort_by(|a, b| {
            let (x, y) = (&t[a.0], &t[b.0]);
            let o = match col {
                SortColumn::Order => std::cmp::Ordering::Equal,
                SortColumn::TrackNum => x.track_number.cmp(&y.track_number),
                SortColumn::Title => lc(&x.title).cmp(&lc(&y.title)),
                SortColumn::Artist => lc(&x.artist).cmp(&lc(&y.artist)),
                SortColumn::Album => lc(&x.album).cmp(&lc(&y.album)),
                SortColumn::Bpm => x.bpm.total_cmp(&y.bpm),
                SortColumn::Key => KeyWheel::parse_open_key(&x.musical_key).cmp(&KeyWheel::parse_open_key(&y.musical_key)),
                SortColumn::Energy => x.energy.cmp(&y.energy),
                SortColumn::Duration => x.duration_sec.total_cmp(&y.duration_sec),
                SortColumn::Genre => lc(&x.genre).cmp(&lc(&y.genre)),
                SortColumn::Rating => x.rating.cmp(&y.rating),
                SortColumn::Comment => lc(&x.comment).cmp(&lc(&y.comment)),
                SortColumn::Remixer => lc(&x.remixer).cmp(&lc(&y.remixer)),
                SortColumn::Label => lc(&x.label).cmp(&lc(&y.label)),
                SortColumn::Year => x.year.cmp(&y.year),
                SortColumn::Bitrate => x.bitrate_kbps.cmp(&y.bitrate_kbps),
                SortColumn::Format => x.format.cmp(&y.format),
                SortColumn::Stems => x.stems_ready.cmp(&y.stems_ready),
                SortColumn::Match => b.1.total_cmp(&a.1),
            };
            if self.sort_asc || col == SortColumn::Match { o } else { o.reverse() }
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

    pub fn select_next_track(&mut self) {
        self.select_offset(1);
    }

    pub fn select_prev_track(&mut self) {
        self.select_offset(-1);
    }

    fn selected_or(&self, id: usize) -> Vec<usize> {
        if self.selection.contains(&id) {
            self.selection.iter().copied().collect()
        } else {
            vec![id]
        }
    }

    fn remember_for_undo(&mut self, what: String) {
        self.undo = Some((what, self.tracks.clone(), self.playlists.clone()));
    }

    pub fn undo(&mut self) {
        if let Some((what, tracks, playlists)) = self.undo.take() {
            self.tracks = tracks;
            self.playlists = playlists;
            self.selection.clear();
            self.rebuild_index();
            self.status_message = format!("Undid: {what}");
            self.touch();
        }
    }

    /// Queues `id` for loading onto deck `deck` (1..=4) after safety checks.
    pub fn load_to_deck(&mut self, deck: usize, id: usize, locked: [bool; 4]) {
        let d = deck.clamp(1, 4) - 1;
        let Some(t) = self.track(id).cloned() else { return };
        if locked[d] {
            self.status_message = format!("Deck {} is playing and locked — not loaded", DECK_NAMES[d]);
        } else if !t.file_path.as_ref().is_some_and(|p| p.is_file()) {
            self.status_message = format!("File not found for '{}' — use Relocate", t.title);
        } else {
            self.history.push(id);
            self.deck_tracks[d] = Some(id);
            self.last_loaded_deck = Some((d, t.stems_ready));
            self.status_message = format!("Loading '{}' on deck {}", t.title, DECK_NAMES[d]);
            self.pending_loads.push((deck, t));
            self.version += 1;
        }
    }

    fn add_to_playlist(&mut self, ids: &[usize], playlist: Option<usize>) {
        let idx = playlist.unwrap_or_else(|| {
            self.playlists.push(UserPlaylist { name: format!("New playlist {}", self.playlists.len() + 1), track_ids: Vec::new() });
            self.renaming = Some((self.playlists.len() - 1, self.playlists.last().unwrap().name.clone()));
            self.playlists.len() - 1
        });
        let Some(pl) = self.playlists.get_mut(idx) else { return };
        let before = pl.track_ids.len();
        for &id in ids {
            if !pl.track_ids.contains(&id) {
                pl.track_ids.push(id);
            }
        }
        self.status_message = format!("Added {} tracks to '{}'", pl.track_ids.len() - before, pl.name);
        self.touch();
    }

    fn preparation_index(&mut self) -> usize {
        self.playlists.iter().position(|p| p.name == "Preparation").unwrap_or_else(|| {
            self.playlists.push(UserPlaylist { name: "Preparation".to_string(), track_ids: Vec::new() });
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
            Action::Edit(id) => self.editing = self.track(id).cloned(),
            Action::FillTags(ids) => self.start_tag_fill(&ids),
            Action::Analyze(ids) => self.start_analysis(ids, true),
            Action::Stems(id) => self.request_stem_separation = Some(id),
            Action::Reveal(p) => {
                let dir = p.parent().unwrap_or(&p).to_path_buf();
                if let Err(e) = std::process::Command::new("xdg-open").arg(&dir).spawn() {
                    self.status_message = format!("Could not open {}: {e}", dir.display());
                }
            }
            Action::RemoveFromCollection(ids) => {
                self.remember_for_undo(format!("removed {} from the collection", plural(ids.len())));
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
                    self.sort_asc = true;
                }
            }
        }
    }

    // ---------------------------------------------------------------- UI

    fn nav_button(&mut self, ui: &mut Ui, label: String, item: NavItem, drop_target: Option<Option<usize>>) -> egui::Response {
        let selected = self.selected_nav == item;
        // A left-aligned layout makes the full-width button's text left-aligned too.
        let resp = ui
            .with_layout(Layout::top_down(Align::Min), |ui| {
                ui.add(
                    egui::Button::new(RichText::new(label).size(12.0).color(if selected { Color32::WHITE } else { TraktorTheme::TEXT_PRIMARY() }))
                        .fill(if selected { TraktorTheme::ROW_SELECTED() } else { Color32::TRANSPARENT })
                        .stroke(Stroke::NONE)
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .min_size(egui::vec2(ui.available_width(), 22.0)),
                )
            })
            .inner;
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
                ui.label(RichText::new("PLAYLISTS").size(10.5).color(TraktorTheme::TEXT_MUTED()));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("+").on_hover_text("New playlist (or drop tracks here)").clicked() {
                        self.add_to_playlist(&[], None);
                        self.selected_nav = NavItem::Playlist(self.playlists.len() - 1);
                    }
                });
            });

            let mut delete = None;
            let mut export = None;
            for i in 0..self.playlists.len() {
                if let Some((ri, buf)) = &mut self.renaming {
                    if *ri == i {
                        let r = ui.add(egui::TextEdit::singleline(buf).desired_width(f32::INFINITY));
                        r.request_focus();
                        if r.lost_focus() {
                            let name = buf.trim().to_string();
                            if !name.is_empty() && !ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.playlists[i].name = name;
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
                    if ui.button("Export as M3U8").clicked() {
                        export = Some(i);
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
            ui.horizontal(|ui| {
                if self.is_busy() {
                    ui.spinner();
                    ui.label(RichText::new(&self.job_status).size(11.0).color(TraktorTheme::TEXT_SECONDARY()));
                }
                let tip = self.history.is_empty() && !self.tracks.is_empty() && self.status_message.is_empty();
                let msg = if tip { "Double-click a track, or drag it onto a deck, to load it." } else { self.status_message.as_str() };
                ui.label(RichText::new(msg).size(12.0).color(TraktorTheme::TEXT_PRIMARY()));
                if self.undo.is_some() && ui.small_button("Undo").on_hover_text("Ctrl+Z").clicked() {
                    self.undo();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format!("{} shown", self.view.len())).size(11.0).color(TraktorTheme::TEXT_MUTED()));
                });
            });
        });

        ui.vertical(|ui| {
            if self.selected_nav == NavItem::Explorer {
                self.show_explorer(ui, locked, deck_count, music_folders);
            } else if self.tracks.is_empty() {
                self.show_empty_state(ui, music_folders);
            } else {
                self.show_toolbar(ui, music_folders, columns);
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

    fn show_missing_banner(&mut self, ui: &mut Ui, music_folders: &[PathBuf]) {
        let missing = self.missing_count();
        if missing == 0 {
            return;
        }
        let busy = self.is_busy();
        egui::Frame::none().fill(Color32::from_rgb(52, 36, 34)).rounding(4.0).inner_margin(egui::Margin::symmetric(10.0, 8.0)).show(ui, |ui| {
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

    fn show_toolbar(&mut self, ui: &mut Ui, music_folders: &[PathBuf], columns: &mut LibraryColumnsConfig) {
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.search_query).hint_text("Search title, artist, genre, key, BPM…").desired_width(260.0));
            if ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)) {
                r.request_focus();
            }
            if !self.search_query.is_empty() && ui.small_button("Clear").clicked() {
                self.search_query.clear();
            }
            ui.separator();
            let busy = self.is_busy();
            if ui.add_enabled(!busy, egui::Button::new("Rescan folders")).on_hover_text("Add new files from your music folders").clicked() {
                self.start_scan(music_folders.to_vec());
            }
            let pending = self.unanalyzed_ids();
            if !pending.is_empty()
                && ui
                    .add_enabled(!busy, egui::Button::new(format!("Analyze {} tracks", pending.len())))
                    .on_hover_text("Detect BPM, key and energy where they're unknown (existing values are kept)")
                    .clicked()
            {
                self.start_analysis(pending, false);
            }
            let missing = self.missing_count();
            if missing > 0 && self.selected_nav != NavItem::Missing && ui.link(RichText::new(format!("{missing} missing")).color(Color32::from_rgb(214, 120, 100))).on_hover_text("Show tracks whose files can't be found").clicked() {
                self.selected_nav = NavItem::Missing;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("rekordbox XML").on_hover_text("Export collection + playlists to ~/Music/ReTraktor-rekordbox.xml").clicked() {
                    let path = env::var("HOME").map(PathBuf::from).unwrap_or_default().join("Music/ReTraktor-rekordbox.xml");
                    let xml = retraktor_formats::to_rekordbox_xml(&self.snapshot());
                    self.status_message = match fs::create_dir_all(path.parent().unwrap()).and_then(|_| fs::write(&path, xml)) {
                        Ok(()) => format!("Exported rekordbox XML to {}", path.display()),
                        Err(e) => format!("rekordbox export failed: {e}"),
                    };
                }
                ui.menu_button("Columns", |ui| column_menu(ui, columns));
                if ui.small_button("Folders").clicked() {
                    self.open_prefs_requested = true;
                    self.open_prefs_file_management = true;
                }
            });
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
        let mut cols = if ordered { vec![Order, Title, Artist] } else { vec![Title, Artist] };
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
            cols.insert(1, Match);
        }
        cols
    }

    fn cell_text(t: &UiTrackEntry, col: SortColumn, row: usize, score: f32) -> RichText {
        let dim = TraktorTheme::TEXT_SECONDARY();
        let txt = |s: String| RichText::new(s).size(12.0);
        match col {
            SortColumn::Order => txt((row + 1).to_string()).color(TraktorTheme::TEXT_MUTED()),
            SortColumn::Title => txt(t.title.clone()).color(if t.missing { Color32::from_rgb(200, 90, 80) } else { TraktorTheme::TEXT_PRIMARY() }),
            SortColumn::Artist => txt(t.artist.clone()),
            SortColumn::Album => txt(t.album.clone()).color(dim),
            SortColumn::Bpm if t.bpm > 0.0 => txt(format!("{:.1}", t.bpm)).monospace(),
            SortColumn::Key if !t.musical_key.is_empty() => {
                // Colour keys around the wheel so compatible neighbours look alike.
                let hue = KeyWheel::parse_open_key(&t.musical_key).map_or(0.0, |(n, _)| (n as f32 - 1.0) / 12.0);
                {
                    // Light themes need darker, more saturated key colours to stay readable.
                    let (sat, val) = if crate::theme::palette().is_dark() { (0.55, 0.95) } else { (0.9, 0.5) };
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
            SortColumn::Rating => txt("★".repeat(t.rating.min(5) as usize)).color(Color32::from_rgb(98, 144, 196)),
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

    fn show_table(&mut self, ui: &mut Ui, columns: &mut LibraryColumnsConfig, locked: [bool; 4], deck_count: usize, four_deck: bool) {
        let cols = self.visible_columns(columns);
        let in_playlist = matches!(self.selected_nav, NavItem::Playlist(_));
        let playlist_names: Vec<String> = self.playlists.iter().map(|p| p.name.clone()).collect();
        let mut actions: Vec<Action> = Vec::new();

        // Keyboard: arrows browse, Enter loads A / Shift+Enter loads B, Delete removes from playlist.
        if !ui.ctx().wants_keyboard_input() && ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Z)) {
            self.undo();
        }
        if !ui.ctx().wants_keyboard_input() {
            let (down, up, enter, shift, del, all) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::ArrowDown),
                    i.key_pressed(egui::Key::ArrowUp),
                    i.key_pressed(egui::Key::Enter),
                    i.modifiers.shift,
                    i.key_pressed(egui::Key::Delete),
                    i.modifiers.command && i.key_pressed(egui::Key::A),
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
            let col = if i + 1 == cols.len() { Column::remainder().at_least(40.0) } else { Column::initial(c.width()).at_least(28.0).clip(true) };
            table = table.column(col);
        }
        let (sort_col, sort_asc) = (self.sort_col, self.sort_asc);
        table
            .header(22.0, |mut header| {
                for &c in &cols {
                    header.col(|ui| {
                        let arrow = if c == sort_col { if sort_asc { " ▲" } else { " ▼" } } else { "" };
                        let r = ui.add(egui::Label::new(RichText::new(format!("{}{arrow}", c.header())).size(11.0).strong().color(TraktorTheme::TEXT_SECONDARY())).sense(Sense::click()));
                        if r.clicked() {
                            actions.push(Action::Sort(c));
                        }
                        r.on_hover_text("Click to sort · right-click to choose columns").context_menu(|ui| column_menu(ui, columns));
                    });
                }
            })
            .body(|body| {
                body.rows(ROW_H, self.view.len(), |mut row| {
                    let r = row.index();
                    let t = &self.tracks[self.view[r]];
                    let score = self.view_scores.get(r).copied().unwrap_or(0.0);
                    row.set_selected(self.selection.contains(&t.id));
                    for &c in &cols {
                        row.col(|ui| {
                            ui.add(egui::Label::new(Self::cell_text(t, c, r, score)).truncate().selectable(false));
                        });
                    }
                    let resp = row.response();
                    if resp.clicked() {
                        actions.push(Action::Select(t.id, ui_modifiers(&resp)));
                    }
                    if resp.double_clicked() {
                        actions.push(Action::LoadAuto(t.id));
                    }
                    if resp.drag_started() {
                        resp.dnd_set_drag_payload(DragTracks(self.selected_or(t.id)));
                    }
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
                        if in_playlist && ui.button("Remove from this playlist").clicked() {
                            actions.push(Action::RemoveFromPlaylist(ids.clone()));
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("Edit track info…").clicked() {
                            actions.push(Action::Edit(t.id));
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

        // "Add to preparation" lives in the playlist menu; make sure the list exists for drag targets.
        if self.playlists.is_empty() {
            self.preparation_index();
        }
        for a in actions {
            self.apply(a, locked, deck_count);
        }
    }

    fn show_editor(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.editing else { return };
        let mut open = true;
        let mut result: Option<bool> = None; // Some(write_tags)
        egui::Window::new("Track info").collapsible(false).resizable(false).open(&mut open).show(ctx, |ui| {
            egui::Grid::new("track_editor").num_columns(2).spacing([10.0, 6.0]).show(ui, |ui| {
                for (label, field) in [
                    ("Title", &mut draft.title),
                    ("Artist", &mut draft.artist),
                    ("Album", &mut draft.album),
                    ("Genre", &mut draft.genre),
                    ("Remixer", &mut draft.remixer),
                    ("Label", &mut draft.label),
                    ("Year", &mut draft.year),
                    ("Key", &mut draft.musical_key),
                    ("Comment", &mut draft.comment),
                ] {
                    ui.label(label);
                    ui.add(egui::TextEdit::singleline(field).desired_width(320.0));
                    ui.end_row();
                }
                ui.label("BPM");
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut draft.bpm).range(0.0..=250.0).speed(0.05).fixed_decimals(2));
                    if ui.small_button("×½").on_hover_text("Halve (fix double-time detection)").clicked() {
                        draft.bpm /= 2.0;
                    }
                    if ui.small_button("×2").on_hover_text("Double (fix half-time detection)").clicked() {
                        draft.bpm *= 2.0;
                    }
                });
                ui.end_row();
                ui.label("Rating");
                ui.add(egui::Slider::new(&mut draft.rating, 0..=5));
                ui.end_row();
                ui.label("Energy");
                ui.add(egui::Slider::new(&mut draft.energy, 0..=10));
                ui.end_row();
            });
            if let Some(p) = &draft.file_path {
                ui.label(RichText::new(p.display().to_string()).size(10.5).color(TraktorTheme::TEXT_MUTED()));
            }
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    result = Some(false);
                }
                if ui.button("Save and write tags to file").on_hover_text("Also updates the tags inside the audio file").clicked() {
                    result = Some(true);
                }
            });
        });
        if let Some(write) = result {
            let mut t = self.editing.take().unwrap();
            if !t.musical_key.is_empty() {
                match KeyWheel::normalize(&t.musical_key).and_then(KeyWheel::to_open_key) {
                    Some(k) => t.musical_key = k.to_string(),
                    None => {
                        self.status_message = format!("Unrecognised key '{}' — use e.g. 8m, 8A or Am", t.musical_key);
                        self.editing = Some(t);
                        return;
                    }
                }
            }
            if write {
                let copy = t.clone();
                self.spawn_job(move |_| match write_track_tags(&copy) {
                    Ok(()) => format!("Wrote tags to {}", copy.title),
                    Err(e) => format!("Could not write tags: {e}"),
                });
            }
            if let Some(slot) = self.track_mut(t.id) {
                *slot = t;
            }
            self.touch();
        } else if !open {
            self.editing = None;
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
    ui.painter().rect_stroke(rect.shrink(1.0), 4.0, Stroke::new(2.0_f32, TraktorTheme::DECK_A()));
    if ui.input(|i| i.pointer.any_released()) {
        return egui::DragAndDrop::take_payload::<DragTracks>(ui.ctx()).and_then(|p| p.0.first().copied());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_lib(name: &str) -> (LibraryManager, PathBuf) {
        let dir = env::temp_dir().join(format!("retraktor-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        (LibraryManager::new(dir.join("collection.nml")), dir)
    }

    fn track(title: &str, path: &Path) -> UiTrackEntry {
        UiTrackEntry { title: title.into(), artist: "Artist".into(), bpm: 128.0, musical_key: "8m".into(), rating: 3, file_path: Some(path.to_path_buf()), ..Default::default() }
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
        let a = TrackAnalysis { bpm: 126.0, beatgrid_offset_sec: 0.5, key_value: 21, key_open: "1m".into(), energy: 7, loudness_db: -9.0, peak_db: -0.3, cue_points_sec: vec![0.5, 31.0] };
        let mut fresh = UiTrackEntry::default();
        fresh.apply_analysis(&a, false);
        assert_eq!((fresh.bpm, fresh.musical_key.as_str(), fresh.energy), (126.0, "1m", 7));
        assert_eq!(fresh.cues.len(), 3, "grid + 2 auto cues");

        let mut tagged = UiTrackEntry { bpm: 128.0, musical_key: "8m".into(), ..Default::default() };
        tagged.cues.push(NmlCue { name: "Drop".into(), displ_order: 0, cue_type: retraktor_core::CueType::Cue, start_ms: 1.0, len_ms: 0.0, repeats: -1, hotcue: 0, grid_bpm: None });
        tagged.apply_analysis(&a, false);
        assert_eq!((tagged.bpm, tagged.musical_key.as_str(), tagged.energy), (128.0, "8m", 7));
        assert_eq!(tagged.cues.iter().filter(|c| c.name.starts_with("Auto ")).count(), 0);
        tagged.apply_analysis(&a, true);
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
    fn tag_fill_never_overwrites() {
        let mut t = UiTrackEntry { title: "Kept".into(), bpm: 128.0, ..Default::default() };
        t.fill_empty_from(UiTrackEntry { title: "Other".into(), genre: "Garage".into(), bpm: 64.0, musical_key: "8m".into(), ..Default::default() });
        assert_eq!((t.title.as_str(), t.genre.as_str(), t.bpm, t.musical_key.as_str()), ("Kept", "Garage", 128.0, "8m"));
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
        let close = UiTrackEntry { energy: 6, ..t };
        assert!(match_score(&r_e, &close) > match_score(&r_e, &calm));
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
}
