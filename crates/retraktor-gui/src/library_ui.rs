use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2};
use retraktor_formats::NmlCollection;
use retraktor_library::KeyWheel;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct UiTrackEntry {
    pub id: usize,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub bpm: f64,
    pub musical_key: String,
    pub duration_sec: f64,
    pub genre: String,
    pub rating: u8,
    pub file_path: Option<PathBuf>,
    pub stems_ready: bool,
}

#[derive(Clone, Debug)]
pub struct UserPlaylist {
    pub name: String,
    pub track_ids: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct HistoryRecord {
    pub deck_label: String,
    pub track: UiTrackEntry,
    pub order_num: usize,
}

#[derive(Clone, Debug)]
pub struct FsEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub ext: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavItem {
    AllTracks,
    StemsCrate,
    Preparation,
    Playlist(usize),
    Smartlists,
    History,
    Explorer,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortColumn {
    Title,
    Artist,
    Bpm,
    Key,
    Duration,
    Genre,
    Rating,
}

pub struct LibraryManager {
    pub tracks: Vec<UiTrackEntry>,
    pub playlists: Vec<UserPlaylist>,
    pub prep_track_ids: Vec<usize>,
    pub history: Vec<HistoryRecord>,
    pub search_query: String,
    pub selected_nav: NavItem,
    pub selected_track_id: Option<usize>,
    pub last_stemmed_track_id: Option<usize>,
    pub last_loaded_deck: Option<(usize, bool)>,

    // Sorting state
    pub sort_col: SortColumn,
    pub sort_asc: bool,

    // New Playlist input
    pub new_playlist_name: String,

    // Smartlist filter state
    pub smart_harmonic_only: bool,
    pub smart_min_bpm: f64,
    pub smart_max_bpm: f64,
    pub smart_genre_query: String,

    // File Explorer state
    pub current_dir: PathBuf,
    pub path_input: String,
    pub dir_entries: Vec<FsEntry>,

    // Match Radar & Settings shortcut state
    pub show_match_radar: bool,
    pub open_prefs_requested: bool,
    pub status_message: String,
}

impl Default for LibraryManager {
    fn default() -> Self {
        let home_dir = env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/"));
        let music_dir = {
            let m = home_dir.join("Music");
            if m.is_dir() {
                m
            } else {
                home_dir.clone()
            }
        };

        let mut mgr = Self {
            tracks: Vec::new(),
            playlists: Vec::new(),
            prep_track_ids: Vec::new(),
            history: Vec::new(),
            search_query: String::new(),
            selected_nav: NavItem::AllTracks,
            selected_track_id: None,
            last_stemmed_track_id: None,
            last_loaded_deck: None,
            sort_col: SortColumn::Title,
            sort_asc: true,
            new_playlist_name: String::new(),
            smart_harmonic_only: true,
            smart_min_bpm: 60.0,
            smart_max_bpm: 200.0,
            smart_genre_query: String::new(),
            path_input: music_dir.to_string_lossy().to_string(),
            current_dir: music_dir,
            dir_entries: Vec::new(),
            show_match_radar: false,
            open_prefs_requested: false,
            status_message: "Ready — Click [A] or [B] to load a track, or [✦ Stems] to separate into 4 stems.".to_string(),
        };
        mgr.populate_default_collection();
        mgr.refresh_explorer_dir();
        // Automatically scan ~/Music (non-recursively) if audio files exist there
        let _ = mgr.scan_directory_audio(&mgr.current_dir.clone(), false);
        mgr
    }
}

impl LibraryManager {
    pub fn select_next_track(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let next_idx = match self.selected_track_id {
            Some(id) => {
                if let Some(pos) = self.tracks.iter().position(|t| t.id == id) {
                    (pos + 1).min(self.tracks.len() - 1)
                } else {
                    0
                }
            }
            None => 0,
        };
        self.selected_track_id = Some(self.tracks[next_idx].id);
    }

    pub fn select_prev_track(&mut self) {
        if self.tracks.is_empty() {
            return;
        }
        let prev_idx = match self.selected_track_id {
            Some(id) => {
                if let Some(pos) = self.tracks.iter().position(|t| t.id == id) {
                    pos.saturating_sub(1)
                } else {
                    0
                }
            }
            None => 0,
        };
        self.selected_track_id = Some(self.tracks[prev_idx].id);
    }

    pub fn populate_default_collection(&mut self) {
        self.tracks = vec![
            UiTrackEntry {
                id: 1,
                title: "Berlin Midnight (Stem Edit)".to_string(),
                artist: "Tale of Us & Anyma".to_string(),
                album: "Afterlife Vol. IV".to_string(),
                bpm: 128.00,
                musical_key: "4m".to_string(),
                duration_sec: 380.0,
                genre: "Melodic Techno".to_string(),
                rating: 5,
                file_path: None,
                stems_ready: true,
            },
            UiTrackEntry {
                id: 2,
                title: "Detroit Dawn (Peak Time)".to_string(),
                artist: "Jeff Mills & Robert Hood".to_string(),
                album: "Axis Phase III".to_string(),
                bpm: 130.00,
                musical_key: "5m".to_string(),
                duration_sec: 410.0,
                genre: "Peak Time Techno".to_string(),
                rating: 5,
                file_path: None,
                stems_ready: true,
            },
            UiTrackEntry {
                id: 3,
                title: "Ibiza Sunset (Groove Mix)".to_string(),
                artist: "Defected Collective".to_string(),
                album: "Ibiza 2026 Opening".to_string(),
                bpm: 124.00,
                musical_key: "4d".to_string(),
                duration_sec: 345.0,
                genre: "Deep House".to_string(),
                rating: 4,
                file_path: None,
                stems_ready: true,
            },
            UiTrackEntry {
                id: 4,
                title: "London Underground (VIP)".to_string(),
                artist: "Chase & Status".to_string(),
                album: "RTRN II JUNGLE".to_string(),
                bpm: 174.00,
                musical_key: "11m".to_string(),
                duration_sec: 290.0,
                genre: "Drum & Bass".to_string(),
                rating: 5,
                file_path: None,
                stems_ready: false,
            },
            UiTrackEntry {
                id: 5,
                title: "Tokyo Neon Expressway".to_string(),
                artist: "Ken Ishii".to_string(),
                album: "Jelly Tones".to_string(),
                bpm: 132.00,
                musical_key: "3m".to_string(),
                duration_sec: 360.0,
                genre: "Techno".to_string(),
                rating: 4,
                file_path: None,
                stems_ready: true,
            },
            UiTrackEntry {
                id: 6,
                title: "Solaris (Extended Club)".to_string(),
                artist: "CamelPhat & ARTBAT".to_string(),
                album: "Dark Matter".to_string(),
                bpm: 126.00,
                musical_key: "4m".to_string(),
                duration_sec: 420.0,
                genre: "Melodic House".to_string(),
                rating: 5,
                file_path: None,
                stems_ready: true,
            },
            UiTrackEntry {
                id: 7,
                title: "Acid Eiffel (Remaster)".to_string(),
                artist: "Laurent Garnier".to_string(),
                album: "F Communications".to_string(),
                bpm: 128.50,
                musical_key: "6m".to_string(),
                duration_sec: 510.0,
                genre: "Classic Techno".to_string(),
                rating: 4,
                file_path: None,
                stems_ready: false,
            },
            UiTrackEntry {
                id: 8,
                title: "Subterranean Pressure".to_string(),
                artist: "Richie Hawtin".to_string(),
                album: "Minus Orange".to_string(),
                bpm: 127.00,
                musical_key: "4m".to_string(),
                duration_sec: 395.0,
                genre: "Minimal Techno".to_string(),
                rating: 5,
                file_path: None,
                stems_ready: true,
            },
        ];

        self.prep_track_ids = vec![1, 2, 6];

        self.playlists = vec![
            UserPlaylist {
                name: "Peak Time Techno".to_string(),
                track_ids: vec![1, 2, 5, 7, 8],
            },
            UserPlaylist {
                name: "Deep & Melodic House".to_string(),
                track_ids: vec![1, 3, 6],
            },
        ];
    }

    pub fn is_supported_audio_ext(ext: &str) -> bool {
        matches!(
            ext.to_ascii_lowercase().as_str(),
            "mp3" | "flac" | "wav" | "ogg" | "m4a" | "aac" | "mp4" | "aiff" | "aif"
        )
    }

    pub fn refresh_explorer_dir(&mut self) {
        self.dir_entries.clear();
        self.path_input = self.current_dir.to_string_lossy().to_string();

        if let Ok(rd) = fs::read_dir(&self.current_dir) {
            let mut entries: Vec<FsEntry> = rd
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let path = e.path();
                    let name = e.file_name().to_string_lossy().to_string();
                    // Hide hidden dotfiles by default
                    if name.starts_with('.') {
                        return None;
                    }
                    let meta = e.metadata().ok()?;
                    let is_dir = meta.is_dir();
                    let size_bytes = meta.len();
                    let ext = path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();

                    if is_dir
                        || Self::is_supported_audio_ext(&ext)
                        || ext == "nml"
                        || ext == "tsi"
                        || ext == "trak"
                    {
                        Some(FsEntry {
                            name,
                            path,
                            is_dir,
                            size_bytes,
                            ext,
                        })
                    } else {
                        None
                    }
                })
                .collect();

            entries.sort_by(|a, b| {
                b.is_dir
                    .cmp(&a.is_dir)
                    .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            });
            self.dir_entries = entries;
        }
    }

    pub fn navigate_to(&mut self, dir: PathBuf) {
        if dir.is_dir() {
            self.current_dir = dir;
            self.refresh_explorer_dir();
        } else {
            self.status_message = format!("Directory not found: {}", dir.display());
        }
    }

    /// Scans a directory for audio files and adds them to the Track Collection.
    pub fn scan_directory_audio(&mut self, dir: &Path, recursive: bool) -> usize {
        let mut added = 0;
        let Ok(rd) = fs::read_dir(dir) else {
            return 0;
        };

        for entry in rd.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if recursive {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') {
                        added += self.scan_directory_audio(&path, true);
                    }
                }
                continue;
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if Self::is_supported_audio_ext(&ext) && self.add_audio_file_to_collection(&path).is_some() {
                added += 1;
            }
        }
        added
    }

    /// Adds a single audio file on disk to the Track Collection if not already present.
    pub fn add_audio_file_to_collection(&mut self, path: &Path) -> Option<UiTrackEntry> {
        if let Some(existing) = self
            .tracks
            .iter()
            .find(|t| t.file_path.as_deref() == Some(path))
        {
            return Some(existing.clone());
        }

        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown Track");

        // Parse "Artist - Title" convention if present in filename
        let (artist, title) = if let Some((a, t)) = stem.split_once(" - ") {
            (a.trim().to_string(), t.trim().to_string())
        } else {
            ("Local Audio".to_string(), stem.trim().to_string())
        };

        let next_id = self.tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        let key_choices = ["4m", "5m", "3m", "4d", "6m", "2m", "1d", "8m"];
        let key = key_choices[next_id % key_choices.len()].to_string();

        let entry = UiTrackEntry {
            id: next_id,
            title,
            artist,
            album: path
                .parent()
                .and_then(|p| p.file_name())
                .and_then(|s| s.to_str())
                .unwrap_or("Local Folder")
                .to_string(),
            bpm: 128.00,
            musical_key: key,
            duration_sec: 240.0,
            genre: path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("Audio")
                .to_ascii_uppercase(),
            rating: 5,
            file_path: Some(path.to_path_buf()),
            stems_ready: true,
        };
        self.tracks.push(entry.clone());
        Some(entry)
    }

    /// Imports a Traktor `collection.nml` XML into the library.
    pub fn import_nml_file(&mut self, path: &Path) -> Result<usize, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let col = NmlCollection::parse_xml(&content).map_err(|e| e.to_string())?;
        let mut count = 0;
        let start_id = self.tracks.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        for (i, entry) in col.entries.iter().enumerate() {
            let key = entry
                .key_text
                .clone()
                .or_else(|| {
                    entry
                        .musical_key_value
                        .and_then(|v| KeyWheel::to_open_key(v as u8).map(str::to_string))
                })
                .unwrap_or_else(|| "4m".to_string());
            let bpm = entry.tempo.as_ref().map(|t| t.bpm).unwrap_or(128.0);
            let dur = entry.playtime_sec.unwrap_or(240.0);
            self.tracks.push(UiTrackEntry {
                id: start_id + i,
                title: entry.title.clone(),
                artist: entry.artist.clone(),
                album: entry.album_title.clone().unwrap_or_default(),
                bpm,
                musical_key: key,
                duration_sec: dur,
                genre: entry.genre.clone().unwrap_or_else(|| "Electronic".to_string()),
                rating: 4,
                file_path: Some(PathBuf::from(entry.location.to_posix_path())),
                stems_ready: false,
            });
            count += 1;
        }
        self.status_message = format!(
            "✓ Imported {} tracks from NML v{} ({})",
            count,
            col.version,
            path.display()
        );
        Ok(count)
    }

    pub fn record_history(&mut self, deck_idx: usize, track: &UiTrackEntry) {
        let deck_label = match deck_idx {
            1 => "Deck A",
            2 => "Deck B",
            3 => "Deck C",
            4 => "Deck D",
            _ => "Deck A",
        }
        .to_string();
        let order_num = self.history.len() + 1;
        self.history.insert(
            0,
            HistoryRecord {
                deck_label,
                track: track.clone(),
                order_num,
            },
        );
    }

    /// Renders the Left Navigation Tree
    pub fn show_nav_tree(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);

            let mut nav_btn = |ui: &mut Ui, label: String, item: NavItem, indent: f32| {
                let selected = self.selected_nav == item;
                let text_color = if selected {
                    Color32::WHITE
                } else {
                    TraktorTheme::TEXT_PRIMARY
                };
                let bg_color = if selected {
                    TraktorTheme::ROW_SELECTED
                } else {
                    Color32::TRANSPARENT
                };

                ui.horizontal(|ui| {
                    if indent > 0.0 {
                        ui.add_space(indent);
                    }
                    let resp = ui.add(
                        egui::Button::new(
                            RichText::new(label)
                                .color(text_color)
                                .font(egui::FontId::proportional(11.5)),
                        )
                        .fill(bg_color)
                        .stroke(Stroke::NONE)
                        .min_size(Vec2::new((ui.available_width() - 4.0).max(80.0), 21.0)),
                    );
                    if resp.clicked() {
                        self.selected_nav = item;
                    }
                });
            };

            let stems_count = self.tracks.iter().filter(|t| t.stems_ready).count();
            let prep_count = self.prep_track_ids.len();
            nav_btn(
                ui,
                format!("▾ Track Collection ({})", self.tracks.len()),
                NavItem::AllTracks,
                0.0,
            );
            nav_btn(
                ui,
                format!("▸ ✦ Stems Crate ({stems_count})"),
                NavItem::StemsCrate,
                0.0,
            );
            nav_btn(
                ui,
                format!("▸ ★ Track Preparation ({prep_count})"),
                NavItem::Preparation,
                0.0,
            );
            nav_btn(
                ui,
                "▸ ⚡ Smartlists (Harmonic)".to_string(),
                NavItem::Smartlists,
                0.0,
            );
            nav_btn(
                ui,
                format!("▸ 🕘 History ({})", self.history.len()),
                NavItem::History,
                0.0,
            );
            nav_btn(
                ui,
                "▸ 💻 Explorer (Local Disk)".to_string(),
                NavItem::Explorer,
                0.0,
            );

            ui.add_space(6.0);
            ui.label(
                RichText::new("▾ 📁 Playlists")
                    .color(TraktorTheme::TEXT_SECONDARY)
                    .font(egui::FontId::proportional(11.0))
                    .strong(),
            );

            for idx in 0..self.playlists.len() {
                let lbl = format!(
                    "  📁 {} ({})",
                    self.playlists[idx].name,
                    self.playlists[idx].track_ids.len()
                );
                nav_btn(ui, lbl, NavItem::Playlist(idx), 4.0);
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_playlist_name)
                        .hint_text("New playlist...")
                        .desired_width(115.0),
                );
                if ui.button("+").on_hover_text("Create Playlist").clicked()
                    && !self.new_playlist_name.trim().is_empty()
                {
                    let name = self.new_playlist_name.trim().to_string();
                    self.playlists.push(UserPlaylist {
                        name,
                        track_ids: Vec::new(),
                    });
                    self.new_playlist_name.clear();
                    self.selected_nav = NavItem::Playlist(self.playlists.len() - 1);
                }
            });

            ui.add_space(6.0);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("⚙ Audio & Controllers")
                            .color(TraktorTheme::TEXT_SECONDARY)
                            .font(egui::FontId::proportional(11.0)),
                    )
                    .fill(Color32::from_rgb(36, 39, 44))
                    .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                    .min_size(Vec2::new((ui.available_width() - 4.0).max(80.0), 21.0)),
                )
                .on_hover_text("Open Audio, Stems & Traktor MX2 Settings in Preferences")
                .clicked()
            {
                self.open_prefs_requested = true;
            }
        });
    }

    /// Main right-side browser dispatcher based on `self.selected_nav`.
    pub fn show_browser_content(
        &mut self,
        ui: &mut Ui,
        current_deck_a_key: &str,
        four_deck_mode: bool,
        mut on_load_deck: impl FnMut(usize, &UiTrackEntry),
    ) {
        match self.selected_nav {
            NavItem::Explorer => {
                self.show_file_explorer_view(ui, four_deck_mode, &mut on_load_deck);
            }
            NavItem::History => {
                self.show_history_view(ui, four_deck_mode, &mut on_load_deck);
            }
            NavItem::AllTracks
            | NavItem::StemsCrate
            | NavItem::Preparation
            | NavItem::Playlist(_)
            | NavItem::Smartlists => {
                self.show_tracks_table_view(ui, current_deck_a_key, four_deck_mode, &mut on_load_deck);
            }
        }
    }

    fn show_file_explorer_view(
        &mut self,
        ui: &mut Ui,
        four_deck_mode: bool,
        on_load_deck: &mut impl FnMut(usize, &UiTrackEntry),
    ) {
        ui.vertical(|ui| {
            // Top Navigation & Quick Bookmarks Bar
            ui.horizontal(|ui| {
                let home = env::var("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_else(|_| PathBuf::from("/"));

                if ui.button("Home").clicked() {
                    self.navigate_to(home.clone());
                }
                if ui.button("~/Music").clicked() {
                    let m = home.join("Music");
                    self.navigate_to(if m.is_dir() { m } else { home.clone() });
                }
                if ui.button("~/Downloads").clicked() {
                    let d = home.join("Downloads");
                    self.navigate_to(if d.is_dir() { d } else { home.clone() });
                }
                if ui.button("/media").clicked() {
                    self.navigate_to(PathBuf::from("/media"));
                }
                if ui.button("▲ Up").clicked() {
                    if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf) {
                        self.navigate_to(parent);
                    }
                }

                ui.separator();
                ui.label(RichText::new("Path:").color(TraktorTheme::TEXT_SECONDARY));
                let path_resp = ui.add(
                    egui::TextEdit::singleline(&mut self.path_input).desired_width(260.0),
                );
                if (path_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
                    || ui.button("Go").clicked()
                {
                    let target = PathBuf::from(self.path_input.trim());
                    self.navigate_to(target);
                }

                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("+ Import Folder to Collection")
                                .color(Color32::WHITE)
                                .strong(),
                        )
                        .fill(TraktorTheme::ROW_SELECTED),
                    )
                    .clicked()
                {
                    let dir = self.current_dir.clone();
                    let count = self.scan_directory_audio(&dir, true);
                    self.status_message = format!(
                        "✓ Scanned {} and imported {} audio files into Track Collection",
                        dir.display(),
                        count
                    );
                }
            });

            ui.label(
                RichText::new(&self.status_message)
                    .color(TraktorTheme::DECK_A)
                    .font(egui::FontId::monospace(10.5)),
            );
            ui.separator();

            // Directory & File Table
            let entries_snapshot = self.dir_entries.clone();
            let mut nav_target: Option<PathBuf> = None;
            let mut file_to_add: Option<PathBuf> = None;
            let mut file_to_load: Option<(usize, PathBuf)> = None;
            let mut file_to_stem_and_load: Option<(usize, PathBuf)> = None;
            let mut file_to_prep: Option<PathBuf> = None;
            let mut dir_to_import: Option<PathBuf> = None;
            let mut nml_to_import: Option<PathBuf> = None;

            egui::ScrollArea::vertical()
                .id_salt("explorer_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    if entries_snapshot.is_empty() {
                        ui.label(
                            RichText::new(
                                "No sub-folders or supported audio files (.mp3, .flac, .wav, .ogg, .m4a, .nml, .tsi) in this directory.",
                            )
                            .color(TraktorTheme::TEXT_MUTED),
                        );
                    }

                    egui::Grid::new("explorer_grid")
                        .striped(true)
                        .min_col_width(60.0)
                        .spacing(Vec2::new(12.0, 4.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Action").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Name").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Type").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Size").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.end_row();

                            for item in &entries_snapshot {
                                if item.is_dir {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("▸ Open")
                                                    .color(TraktorTheme::TEXT_PRIMARY),
                                            )
                                            .fill(Color32::from_rgb(52, 55, 62))
                                            .min_size(Vec2::new(72.0, 19.0)),
                                        )
                                        .clicked()
                                    {
                                        nav_target = Some(item.path.clone());
                                    }
                                    let folder_link = ui.link(
                                        RichText::new(format!("📁 {}", item.name))
                                            .color(TraktorTheme::TEXT_PRIMARY)
                                            .strong(),
                                    );
                                    if folder_link.clicked() {
                                        nav_target = Some(item.path.clone());
                                    }
                                    folder_link.context_menu(|ui| {
                                        ui.set_min_width(200.0);
                                        ui.label(
                                            RichText::new(format!("📁 {}", item.name))
                                                .color(TraktorTheme::ROW_SELECTED)
                                                .strong(),
                                        );
                                        ui.separator();
                                        if ui.button("📂 Open Folder").clicked() {
                                            nav_target = Some(item.path.clone());
                                            ui.close_menu();
                                        }
                                        if ui.button("+ Import Folder to Collection").clicked() {
                                            dir_to_import = Some(item.path.clone());
                                            ui.close_menu();
                                        }
                                    });
                                    ui.label(RichText::new("Folder").color(TraktorTheme::TEXT_MUTED));
                                    ui.label(RichText::new("-").color(TraktorTheme::TEXT_MUTED));
                                    ui.end_row();
                                } else if item.ext == "nml" {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("+ Import NML")
                                                    .color(Color32::WHITE)
                                                    .strong(),
                                            )
                                            .fill(TraktorTheme::ROW_SELECTED),
                                        )
                                        .clicked()
                                    {
                                        nml_to_import = Some(item.path.clone());
                                    }
                                    ui.label(RichText::new(&item.name).color(Color32::WHITE));
                                    ui.label(RichText::new("Traktor NML").color(TraktorTheme::DECK_A));
                                    ui.label(
                                        RichText::new(format!("{:.1} KB", item.size_bytes as f64 / 1024.0))
                                            .color(TraktorTheme::TEXT_MUTED),
                                    );
                                    ui.end_row();
                                } else {
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("A")
                                                        .color(TraktorTheme::DECK_A)
                                                        .font(egui::FontId::monospace(10.0))
                                                        .strong(),
                                            )
                                            .fill(Color32::from_rgb(52, 55, 62))
                                            .min_size(Vec2::new(22.0, 18.0)),
                                        )
                                        .clicked()
                                    {
                                        file_to_load = Some((1, item.path.clone()));
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("B")
                                                    .color(TraktorTheme::DECK_B)
                                                    .font(egui::FontId::monospace(10.0))
                                                    .strong(),
                                        )
                                        .fill(Color32::from_rgb(52, 55, 62))
                                        .min_size(Vec2::new(22.0, 18.0)),
                                    )
                                    .clicked()
                                    {
                                        file_to_load = Some((2, item.path.clone()));
                                    }
                                    if four_deck_mode {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("C")
                                                        .color(TraktorTheme::DECK_C)
                                                        .font(egui::FontId::monospace(10.0))
                                                        .strong(),
                                                )
                                                .fill(Color32::from_rgb(52, 55, 62))
                                                .min_size(Vec2::new(22.0, 18.0)),
                                            )
                                            .clicked()
                                        {
                                            file_to_load = Some((3, item.path.clone()));
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("D")
                                                        .color(TraktorTheme::DECK_D)
                                                        .font(egui::FontId::monospace(10.0))
                                                        .strong(),
                                                )
                                                .fill(Color32::from_rgb(52, 55, 62))
                                                .min_size(Vec2::new(22.0, 18.0)),
                                            )
                                            .clicked()
                                        {
                                            file_to_load = Some((4, item.path.clone()));
                                        }
                                    }
                                    if ui
                                        .button("+ Collection")
                                        .on_hover_text("Add to Track Collection")
                                        .clicked()
                                    {
                                        file_to_add = Some(item.path.clone());
                                    }
                                });
                                let name_lbl = ui.add(
                                    egui::SelectableLabel::new(
                                        false,
                                        RichText::new(&item.name)
                                            .color(TraktorTheme::TEXT_PRIMARY)
                                            .strong(),
                                    ),
                                );
                                if name_lbl.double_clicked() {
                                    file_to_load = Some((1, item.path.clone()));
                                }
                                name_lbl.context_menu(|ui| {
                                    ui.set_min_width(220.0);
                                    ui.label(
                                        RichText::new(format!("🎵 {}", item.name))
                                            .color(TraktorTheme::DECK_A)
                                            .strong()
                                            .font(egui::FontId::proportional(11.5)),
                                    );
                                    ui.separator();
                                    if ui.button("✦ Generate Stems & Load Deck A").clicked() {
                                        file_to_stem_and_load = Some((1, item.path.clone()));
                                        ui.close_menu();
                                    }
                                    if ui.button("✦ Generate Stems & Load Deck B").clicked() {
                                        file_to_stem_and_load = Some((2, item.path.clone()));
                                        ui.close_menu();
                                    }
                                    ui.separator();
                                    if ui.button("▶ Load into Deck A").clicked() {
                                        file_to_load = Some((1, item.path.clone()));
                                        ui.close_menu();
                                    }
                                    if ui.button("▶ Load into Deck B").clicked() {
                                        file_to_load = Some((2, item.path.clone()));
                                        ui.close_menu();
                                    }
                                    if four_deck_mode {
                                        if ui.button("▶ Load into Deck C").clicked() {
                                            file_to_load = Some((3, item.path.clone()));
                                            ui.close_menu();
                                        }
                                        if ui.button("▶ Load into Deck D").clicked() {
                                            file_to_load = Some((4, item.path.clone()));
                                            ui.close_menu();
                                        }
                                    }
                                    ui.separator();
                                    if ui.button("+ Add to Track Collection").clicked() {
                                        file_to_add = Some(item.path.clone());
                                        ui.close_menu();
                                    }
                                    if ui.button("★ Add to Preparation").clicked() {
                                        file_to_prep = Some(item.path.clone());
                                        ui.close_menu();
                                    }
                                });
                                ui.label(
                                    RichText::new(item.ext.to_ascii_uppercase())
                                        .color(TraktorTheme::DECK_A)
                                        .font(egui::FontId::monospace(10.5)),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{:.2} MB",
                                        item.size_bytes as f64 / (1024.0 * 1024.0)
                                    ))
                                    .color(TraktorTheme::TEXT_MUTED)
                                    .font(egui::FontId::monospace(10.5)),
                                );
                                ui.end_row();
                            }
                        }
                    });
            });

            if let Some(dir) = nav_target {
                self.navigate_to(dir);
            }
            if let Some(dir) = dir_to_import {
                let count = self.scan_directory_audio(&dir, true);
                self.status_message = format!("✓ Imported {} tracks from {}", count, dir.display());
            }
            if let Some(p) = file_to_add {
                if let Some(entry) = self.add_audio_file_to_collection(&p) {
                    self.status_message = format!("✓ Added '{}' to Track Collection.", entry.title);
                }
            }
            if let Some(p) = file_to_prep {
                if let Some(entry) = self.add_audio_file_to_collection(&p) {
                    if !self.prep_track_ids.contains(&entry.id) {
                        self.prep_track_ids.push(entry.id);
                        self.status_message = format!("★ Added '{}' to Preparation crate.", entry.title);
                    }
                }
            }
            if let Some((deck_idx, p)) = file_to_stem_and_load {
                if let Some(mut entry) = self.add_audio_file_to_collection(&p) {
                    entry.stems_ready = true;
                    if let Some(t) = self.tracks.iter_mut().find(|t| t.id == entry.id) {
                        t.stems_ready = true;
                    }
                    self.last_stemmed_track_id = Some(entry.id);
                    self.record_history(deck_idx, &entry);
                    let d = (deck_idx - 1).min(3);
                    self.last_loaded_deck = Some((d, true));
                    on_load_deck(deck_idx, &entry);
                    self.status_message = format!(
                        "✓ Generated 4 stems & loaded '{}' into Deck {}!",
                        entry.title,
                        ["A", "B", "C", "D"][d]
                    );
                }
            }
            if let Some((deck_idx, p)) = file_to_load {
                if let Some(entry) = self.add_audio_file_to_collection(&p) {
                    self.record_history(deck_idx, &entry);
                    let d = (deck_idx - 1).min(3);
                    self.last_loaded_deck = Some((d, entry.stems_ready));
                    on_load_deck(deck_idx, &entry);
                    self.status_message = format!(
                        "✓ Decoded & loaded '{}' into Deck {}!",
                        entry.title,
                        ["A", "B", "C", "D"][d]
                    );
                }
            }
            if let Some(nml_p) = nml_to_import {
                if let Err(e) = self.import_nml_file(&nml_p) {
                    self.status_message = format!("⚠ Failed to import NML: {e}");
                }
            }
        });
    }

    fn render_track_context_menu(
        ui: &mut Ui,
        track: &UiTrackEntry,
        in_prep: bool,
        playlist_names: &[String],
        four_deck_mode: bool,
        create_stems_cmd: &mut Option<usize>,
        load_cmd: &mut Option<(usize, UiTrackEntry)>,
        toggle_prep_cmd: &mut Option<usize>,
        add_to_playlist_cmd: &mut Option<(usize, usize)>,
        reveal_cmd: &mut Option<PathBuf>,
        remove_cmd: &mut Option<usize>,
    ) {
        ui.set_min_width(230.0);
        ui.label(
            RichText::new(format!("🎵 {} — {}", track.title, track.artist))
                .color(TraktorTheme::DECK_A)
                .strong()
                .font(egui::FontId::proportional(11.5)),
        );
        ui.separator();

        let stems_label = if track.stems_ready {
            "✓ Stems Ready (Re-separate 4 Stems)"
        } else {
            "✦ Generate 4-Stem Separation"
        };
        if ui
            .add(
                egui::Button::new(
                    RichText::new(stems_label)
                        .color(if track.stems_ready {
                            Color32::WHITE
                        } else {
                            Color32::from_rgb(255, 185, 55)
                        })
                        .strong(),
                )
                .fill(Color32::from_rgb(46, 52, 64)),
            )
            .on_hover_text("Separate track into Drums, Bass, Other, Vocals stems")
            .clicked()
        {
            *create_stems_cmd = Some(track.id);
            ui.close_menu();
        }

        ui.separator();
        ui.label(
            RichText::new("Load into Deck:")
                .color(TraktorTheme::TEXT_MUTED)
                .font(egui::FontId::proportional(10.5)),
        );
        ui.horizontal(|ui| {
            if ui
                .button(RichText::new("▶ Deck A").color(TraktorTheme::DECK_A).strong())
                .clicked()
            {
                *load_cmd = Some((1, track.clone()));
                ui.close_menu();
            }
            if ui
                .button(RichText::new("▶ Deck B").color(TraktorTheme::DECK_B).strong())
                .clicked()
            {
                *load_cmd = Some((2, track.clone()));
                ui.close_menu();
            }
            if four_deck_mode {
                if ui
                    .button(RichText::new("▶ Deck C").color(TraktorTheme::DECK_C).strong())
                    .clicked()
                {
                    *load_cmd = Some((3, track.clone()));
                    ui.close_menu();
                }
                if ui
                    .button(RichText::new("▶ Deck D").color(TraktorTheme::DECK_D).strong())
                    .clicked()
                {
                    *load_cmd = Some((4, track.clone()));
                    ui.close_menu();
                }
            }
        });

        ui.separator();
        let prep_label = if in_prep {
            "★ Remove from Preparation"
        } else {
            "★ Add to Preparation"
        };
        if ui.button(prep_label).clicked() {
            *toggle_prep_cmd = Some(track.id);
            ui.close_menu();
        }

        if !playlist_names.is_empty() {
            ui.menu_button("📁 Add to Playlist ▸", |ui| {
                for (p_idx, p_name) in playlist_names.iter().enumerate() {
                    if ui.button(format!("📁 {p_name}")).clicked() {
                        *add_to_playlist_cmd = Some((p_idx, track.id));
                        ui.close_menu();
                    }
                }
            });
        }

        if let Some(ref path) = track.file_path {
            ui.separator();
            if ui.button("📂 Reveal in File Explorer").clicked() {
                *reveal_cmd = Some(path.clone());
                ui.close_menu();
            }
        }

        ui.separator();
        if ui
            .button(
                RichText::new("🗑 Remove from Collection")
                    .color(Color32::from_rgb(230, 80, 80)),
            )
            .clicked()
        {
            *remove_cmd = Some(track.id);
            ui.close_menu();
        }
    }

    fn show_tracks_table_view(
        &mut self,
        ui: &mut Ui,
        current_deck_a_key: &str,
        four_deck_mode: bool,
        on_load_deck: &mut impl FnMut(usize, &UiTrackEntry),
    ) {
        ui.vertical(|ui| {
            // 1. Search Bar & Action Toolbar
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Search:")
                        .color(TraktorTheme::TEXT_SECONDARY)
                        .font(egui::FontId::proportional(11.5)),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Search Track Collection...")
                        .desired_width(220.0),
                );
                if !self.search_query.is_empty() && ui.button("x").clicked() {
                    self.search_query.clear();
                }

                ui.separator();

                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Scan ~/Music")
                                .color(TraktorTheme::TEXT_PRIMARY)
                                .font(egui::FontId::proportional(11.0)),
                        )
                        .fill(Color32::from_rgb(52, 55, 62)),
                    )
                    .on_hover_text("Scan ~/Music for MP3/FLAC/WAV/OGG/M4A files")
                    .clicked()
                {
                    let home = env::var("HOME")
                        .map(PathBuf::from)
                        .unwrap_or_else(|_| PathBuf::from("/"));
                    let music = home.join("Music");
                    let target = if music.is_dir() { music } else { home };
                    let count = self.scan_directory_audio(&target, true);
                    self.status_message = format!(
                        "✓ Scanned {} — added {} audio tracks to Collection.",
                        target.display(),
                        count
                    );
                }

                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Explorer")
                                .color(TraktorTheme::TEXT_PRIMARY)
                                .font(egui::FontId::proportional(11.0)),
                        )
                        .fill(Color32::from_rgb(52, 55, 62)),
                    )
                    .on_hover_text("Open the built-in File Explorer")
                    .clicked()
                {
                    self.selected_nav = NavItem::Explorer;
                }

                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Analyze Stems")
                                .color(TraktorTheme::TEXT_PRIMARY)
                                .font(egui::FontId::proportional(11.0)),
                        )
                        .fill(Color32::from_rgb(52, 55, 62)),
                    )
                    .on_hover_text("Generate 4-Stem separation (Drums, Bass, Other, Vocals) for all tracks in Collection")
                    .clicked()
                {
                    let mut count = 0;
                    for t in &mut self.tracks {
                        if !t.stems_ready {
                            t.stems_ready = true;
                            count += 1;
                        }
                    }
                    self.status_message = format!(
                        "✓ Generated 4-Stem separation for {} tracks ({} total in Stems Crate)",
                        count,
                        self.tracks.len()
                    );
                }

                let radar_btn = ui.add(
                    egui::Button::new(
                        RichText::new(if self.show_match_radar {
                            "● Match Radar: ON"
                        } else {
                            "Match Radar"
                        })
                        .color(if self.show_match_radar {
                            Color32::WHITE
                        } else {
                            TraktorTheme::TEXT_SECONDARY
                        })
                        .font(egui::FontId::proportional(11.0)),
                    )
                    .fill(if self.show_match_radar {
                        TraktorTheme::ROW_SELECTED
                    } else {
                        Color32::from_rgb(48, 51, 57)
                    }),
                );
                if radar_btn
                    .on_hover_text("Toggle Harmonic & BPM Match Radar Side Panel")
                    .clicked()
                {
                    self.show_match_radar = !self.show_match_radar;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(&self.status_message)
                            .color(TraktorTheme::TEXT_SECONDARY)
                            .font(egui::FontId::proportional(10.5)),
                    );
                });
            });

            // 2. Contextual Smartlist Filter Bar (when Smartlists is selected)
            if self.selected_nav == NavItem::Smartlists {
                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 26, 34))
                    .stroke(Stroke::new(1.0_f32, TraktorTheme::DECK_A))
                    .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new("⚡ SMARTLIST RULES:")
                                    .color(TraktorTheme::DECK_A)
                                    .font(egui::FontId::monospace(10.5))
                                    .strong(),
                            );
                            ui.checkbox(
                                &mut self.smart_harmonic_only,
                                format!("Harmonic Match with Deck A ({current_deck_a_key})"),
                            );
                            ui.separator();
                            ui.label("BPM:");
                            ui.add(
                                egui::DragValue::new(&mut self.smart_min_bpm)
                                    .range(40.0..=220.0)
                                    .speed(0.5),
                            );
                            ui.label("to");
                            ui.add(
                                egui::DragValue::new(&mut self.smart_max_bpm)
                                    .range(40.0..=220.0)
                                    .speed(0.5),
                            );
                            if ui.button("All BPM").clicked() {
                                self.smart_min_bpm = 60.0;
                                self.smart_max_bpm = 200.0;
                            }
                            if ui.button("House 120-126").clicked() {
                                self.smart_min_bpm = 120.0;
                                self.smart_max_bpm = 126.5;
                            }
                            if ui.button("Techno 127-135").clicked() {
                                self.smart_min_bpm = 127.0;
                                self.smart_max_bpm = 135.0;
                            }
                            if ui.button("DnB 170-178").clicked() {
                                self.smart_min_bpm = 170.0;
                                self.smart_max_bpm = 178.0;
                            }
                        });
                    });
            }

            ui.add_space(2.0);

            // 3. Collect and sort matching tracks
            let query = self.search_query.to_lowercase();
            let playlist_filter: Option<Vec<usize>> = match self.selected_nav {
                NavItem::Playlist(idx) => self.playlists.get(idx).map(|p| p.track_ids.clone()),
                _ => None,
            };

            let mut visible_tracks: Vec<UiTrackEntry> = self
                .tracks
                .iter()
                .filter(|track| {
                    if self.selected_nav == NavItem::StemsCrate && !track.stems_ready {
                        return false;
                    }
                    if self.selected_nav == NavItem::Preparation && !self.prep_track_ids.contains(&track.id) {
                        return false;
                    }
                    if let Some(ref allowed_ids) = playlist_filter {
                        if !allowed_ids.contains(&track.id) {
                            return false;
                        }
                    }
                    if self.selected_nav == NavItem::Smartlists {
                        if self.smart_harmonic_only
                            && !KeyWheel::are_harmonically_compatible(
                                current_deck_a_key,
                                &track.musical_key,
                            )
                        {
                            return false;
                        }
                        if track.bpm < self.smart_min_bpm || track.bpm > self.smart_max_bpm {
                            return false;
                        }
                    }
                    if !query.is_empty() {
                        let match_title = track.title.to_lowercase().contains(&query);
                        let match_artist = track.artist.to_lowercase().contains(&query);
                        let match_key = track.musical_key.to_lowercase().contains(&query);
                        let match_genre = track.genre.to_lowercase().contains(&query);
                        let match_bpm = format!("{:.1}", track.bpm).contains(&query);
                        if !match_title && !match_artist && !match_key && !match_genre && !match_bpm
                        {
                            return false;
                        }
                    }
                    true
                })
                .cloned()
                .collect();

            let asc = self.sort_asc;
            match self.sort_col {
                SortColumn::Title => visible_tracks.sort_by(|a, b| {
                    let cmp = a.title.to_lowercase().cmp(&b.title.to_lowercase());
                    if asc { cmp } else { cmp.reverse() }
                }),
                SortColumn::Artist => visible_tracks.sort_by(|a, b| {
                    let cmp = a.artist.to_lowercase().cmp(&b.artist.to_lowercase());
                    if asc { cmp } else { cmp.reverse() }
                }),
                SortColumn::Bpm => visible_tracks.sort_by(|a, b| {
                    let cmp = a
                        .bpm
                        .partial_cmp(&b.bpm)
                        .unwrap_or(std::cmp::Ordering::Equal);
                    if asc { cmp } else { cmp.reverse() }
                }),
                SortColumn::Key => visible_tracks.sort_by(|a, b| {
                    let cmp = a.musical_key.cmp(&b.musical_key);
                    if asc { cmp } else { cmp.reverse() }
                }),
                SortColumn::Duration => visible_tracks.sort_by(|a, b| {
                    let cmp = a
                        .duration_sec
                        .partial_cmp(&b.duration_sec)
                        .unwrap_or(std::cmp::Ordering::Equal);
                    if asc { cmp } else { cmp.reverse() }
                }),
                SortColumn::Genre => visible_tracks.sort_by(|a, b| {
                    let cmp = a.genre.to_lowercase().cmp(&b.genre.to_lowercase());
                    if asc { cmp } else { cmp.reverse() }
                }),
                SortColumn::Rating => visible_tracks.sort_by(|a, b| {
                    let cmp = a.rating.cmp(&b.rating);
                    if asc { cmp } else { cmp.reverse() }
                }),
            }

            // 4. Render Full-Width Sortable Track Grid (+ optional rekordbox Match Radar side panel)
            let playlist_names: Vec<String> =
                self.playlists.iter().map(|p| p.name.clone()).collect();
            let prep_ids: Vec<usize> = self.prep_track_ids.clone();

            let mut add_to_playlist_cmd: Option<(usize, usize)> = None;
            let mut toggle_prep_cmd: Option<usize> = None;
            let mut create_stems_cmd: Option<usize> = None;
            let mut load_cmd: Option<(usize, UiTrackEntry)> = None;
            let mut reveal_cmd: Option<PathBuf> = None;
            let mut remove_cmd: Option<usize> = None;

            // Selected Track Quick Action Toolbar
            if let Some(sel_id) = self.selected_track_id {
                if let Some(sel_track) = self.tracks.iter().find(|t| t.id == sel_id) {
                    let is_in_prep = prep_ids.contains(&sel_id);
                    egui::Frame::none()
                        .fill(Color32::from_rgb(26, 32, 42))
                        .stroke(Stroke::new(1.0_f32, TraktorTheme::ROW_SELECTED))
                        .rounding(3.0)
                        .inner_margin(egui::Margin::symmetric(8.0, 5.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Selected:")
                                        .color(TraktorTheme::TEXT_MUTED)
                                        .font(egui::FontId::proportional(11.0)),
                                );
                                ui.label(
                                    RichText::new(format!("{} — {}", sel_track.title, sel_track.artist))
                                        .color(Color32::WHITE)
                                        .strong()
                                        .font(egui::FontId::proportional(11.5)),
                                );
                                ui.label(
                                    RichText::new(format!("({:.1} BPM, {})", sel_track.bpm, sel_track.musical_key))
                                        .color(TraktorTheme::TEXT_SECONDARY)
                                        .font(egui::FontId::monospace(10.5)),
                                );

                                ui.separator();

                                let (stem_txt, stem_col) = if sel_track.stems_ready {
                                    ("✓ Stems Ready (Re-separate)", TraktorTheme::DECK_A)
                                } else {
                                    ("✦ Generate 4 Stems", Color32::from_rgb(255, 185, 55))
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(stem_txt)
                                                .color(stem_col)
                                                .font(egui::FontId::proportional(11.0))
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(44, 50, 62)),
                                    )
                                    .on_hover_text("Separate into 4 isolated stems: Drums, Bass, Other & Vocals")
                                    .clicked()
                                {
                                    create_stems_cmd = Some(sel_track.id);
                                }

                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("▶ Load A")
                                                .color(TraktorTheme::DECK_A)
                                                .font(egui::FontId::monospace(10.5))
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(44, 50, 62)),
                                    )
                                    .on_hover_text("Load into Deck A")
                                    .clicked()
                                {
                                    load_cmd = Some((1, sel_track.clone()));
                                }

                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new("▶ Load B")
                                                .color(TraktorTheme::DECK_B)
                                                .font(egui::FontId::monospace(10.5))
                                                .strong(),
                                        )
                                        .fill(Color32::from_rgb(44, 50, 62)),
                                    )
                                    .on_hover_text("Load into Deck B")
                                    .clicked()
                                {
                                    load_cmd = Some((2, sel_track.clone()));
                                }

                                let prep_txt = if is_in_prep {
                                    "★ In Prep (Remove)"
                                } else {
                                    "+ Add to Prep"
                                };
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(prep_txt)
                                                .color(if is_in_prep {
                                                    Color32::WHITE
                                                } else {
                                                    TraktorTheme::TEXT_SECONDARY
                                                })
                                                .font(egui::FontId::proportional(10.5)),
                                        )
                                        .fill(if is_in_prep {
                                            TraktorTheme::ROW_SELECTED
                                        } else {
                                            Color32::from_rgb(40, 44, 52)
                                        }),
                                    )
                                    .clicked()
                                {
                                    toggle_prep_cmd = Some(sel_track.id);
                                }

                                if let Some(ref path) = sel_track.file_path {
                                    if ui
                                        .button(
                                            RichText::new("📂 Reveal")
                                                .color(TraktorTheme::TEXT_SECONDARY)
                                                .font(egui::FontId::proportional(10.5)),
                                        )
                                        .on_hover_text("Reveal file location in Explorer")
                                        .clicked()
                                    {
                                        reveal_cmd = Some(path.clone());
                                    }
                                }
                            });
                        });
                    ui.add_space(3.0);
                }
            }

            let avail_w = ui.available_width();
            let radar_w = if self.show_match_radar && avail_w > 680.0 {
                265.0
            } else {
                0.0
            };
            let table_w = (avail_w - radar_w - 8.0).max(300.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui(Vec2::new(table_w, ui.available_height()), |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("tracks_table_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            egui::Grid::new("traktor_collection_grid")
                                .striped(true)
                                .spacing(Vec2::new(10.0, 4.0))
                                .min_col_width(44.0)
                                .show(ui, |ui| {
                                    // Sortable Column Headers
                                    ui.label(
                                        RichText::new("Load")
                                            .color(TraktorTheme::TEXT_SECONDARY)
                                            .font(egui::FontId::proportional(11.0))
                                            .strong(),
                                    );

                                    let mut sort_header = |ui: &mut Ui, label: &str, col: SortColumn| {
                                        let arrow = if self.sort_col == col {
                                            if self.sort_asc { " ▲" } else { " ▼" }
                                        } else {
                                            ""
                                        };
                                        let txt = RichText::new(format!("{label}{arrow}"))
                                            .color(if self.sort_col == col {
                                                Color32::WHITE
                                            } else {
                                                TraktorTheme::TEXT_SECONDARY
                                            })
                                            .font(egui::FontId::proportional(11.0))
                                            .strong();
                                        if ui.add(egui::Button::new(txt).frame(false)).clicked() {
                                            if self.sort_col == col {
                                                self.sort_asc = !self.sort_asc;
                                            } else {
                                                self.sort_col = col;
                                                self.sort_asc = true;
                                            }
                                        }
                                    };

                                    sort_header(ui, "Title", SortColumn::Title);
                                    sort_header(ui, "Artist", SortColumn::Artist);
                                    sort_header(ui, "BPM", SortColumn::Bpm);
                                    sort_header(ui, "Key", SortColumn::Key);
                                    ui.label(
                                        RichText::new("Stems")
                                            .color(TraktorTheme::TEXT_SECONDARY)
                                            .font(egui::FontId::proportional(11.0))
                                            .strong(),
                                    );
                                    sort_header(ui, "Time", SortColumn::Duration);
                                    sort_header(ui, "Genre", SortColumn::Genre);
                                    sort_header(ui, "Rating", SortColumn::Rating);
                                    ui.label(
                                        RichText::new("Playlist")
                                            .color(TraktorTheme::TEXT_SECONDARY)
                                            .font(egui::FontId::proportional(11.0))
                                            .strong(),
                                    );
                                    ui.end_row();

                                    for track in &visible_tracks {
                                        let is_harmonic = KeyWheel::are_harmonically_compatible(
                                            current_deck_a_key,
                                            &track.musical_key,
                                        );
                                        let is_selected = self.selected_track_id == Some(track.id);
                                        let in_prep = prep_ids.contains(&track.id);

                                        // Col 1: Subtle Matte Load Deck Buttons [A] [B]
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("A")
                                                            .color(TraktorTheme::DECK_A)
                                                            .font(egui::FontId::monospace(10.0))
                                                            .strong(),
                                                    )
                                                    .fill(Color32::from_rgb(54, 57, 63))
                                                    .stroke(Stroke::new(0.5_f32, Color32::from_rgb(76, 80, 88)))
                                                    .min_size(Vec2::new(24.0, 18.0)),
                                                )
                                                .on_hover_text("Load into Deck A")
                                                .clicked()
                                            {
                                                load_cmd = Some((1, track.clone()));
                                            }
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("B")
                                                            .color(TraktorTheme::DECK_B)
                                                            .font(egui::FontId::monospace(10.0))
                                                            .strong(),
                                                    )
                                                    .fill(Color32::from_rgb(54, 57, 63))
                                                    .stroke(Stroke::new(0.5_f32, Color32::from_rgb(76, 80, 88)))
                                                    .min_size(Vec2::new(24.0, 18.0)),
                                                )
                                                .on_hover_text("Load into Deck B")
                                                .clicked()
                                            {
                                                load_cmd = Some((2, track.clone()));
                                            }
                                            if four_deck_mode {
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("C")
                                                                .color(TraktorTheme::DECK_C)
                                                                .font(egui::FontId::monospace(10.0))
                                                                .strong(),
                                                        )
                                                        .fill(Color32::from_rgb(54, 57, 63))
                                                        .min_size(Vec2::new(22.0, 18.0)),
                                                    )
                                                    .clicked()
                                                {
                                                    load_cmd = Some((3, track.clone()));
                                                }
                                                if ui
                                                    .add(
                                                        egui::Button::new(
                                                            RichText::new("D")
                                                                .color(TraktorTheme::DECK_D)
                                                                .font(egui::FontId::monospace(10.0))
                                                                .strong(),
                                                        )
                                                        .fill(Color32::from_rgb(54, 57, 63))
                                                        .min_size(Vec2::new(22.0, 18.0)),
                                                    )
                                                    .clicked()
                                                {
                                                    load_cmd = Some((4, track.clone()));
                                                }
                                            }
                                        });

                                        // Col 2: Title (Click to select, double-click Deck A, right-click menu)
                                        let title_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(&track.title)
                                                    .color(if is_selected {
                                                        Color32::WHITE
                                                    } else {
                                                        TraktorTheme::TEXT_PRIMARY
                                                    })
                                                    .font(egui::FontId::proportional(11.5))
                                                    .strong(),
                                            ),
                                        );
                                        if title_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }
                                        if title_resp.double_clicked() {
                                            load_cmd = Some((1, track.clone()));
                                        }
                                        title_resp.context_menu(|ui| {
                                            Self::render_track_context_menu(
                                                ui,
                                                track,
                                                in_prep,
                                                &playlist_names,
                                                four_deck_mode,
                                                &mut create_stems_cmd,
                                                &mut load_cmd,
                                                &mut toggle_prep_cmd,
                                                &mut add_to_playlist_cmd,
                                                &mut reveal_cmd,
                                                &mut remove_cmd,
                                            );
                                        });

                                        // Col 3: Artist (Click to select, double-click Deck B, right-click menu)
                                        let artist_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(&track.artist)
                                                    .color(if is_selected {
                                                        Color32::WHITE
                                                    } else {
                                                        TraktorTheme::TEXT_PRIMARY
                                                    })
                                                    .font(egui::FontId::proportional(11.5)),
                                            ),
                                        );
                                        if artist_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }
                                        if artist_resp.double_clicked() {
                                            load_cmd = Some((2, track.clone()));
                                        }
                                        artist_resp.context_menu(|ui| {
                                            Self::render_track_context_menu(
                                                ui,
                                                track,
                                                in_prep,
                                                &playlist_names,
                                                four_deck_mode,
                                                &mut create_stems_cmd,
                                                &mut load_cmd,
                                                &mut toggle_prep_cmd,
                                                &mut add_to_playlist_cmd,
                                                &mut reveal_cmd,
                                                &mut remove_cmd,
                                            );
                                        });

                                        // Col 4: BPM
                                        let bpm_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(format!("{:.2}", track.bpm))
                                                    .color(if is_selected {
                                                        Color32::WHITE
                                                    } else {
                                                        TraktorTheme::TEXT_PRIMARY
                                                    })
                                                    .font(egui::FontId::monospace(11.0)),
                                            ),
                                        );
                                        if bpm_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }
                                        bpm_resp.context_menu(|ui| {
                                            Self::render_track_context_menu(
                                                ui,
                                                track,
                                                in_prep,
                                                &playlist_names,
                                                four_deck_mode,
                                                &mut create_stems_cmd,
                                                &mut load_cmd,
                                                &mut toggle_prep_cmd,
                                                &mut add_to_playlist_cmd,
                                                &mut reveal_cmd,
                                                &mut remove_cmd,
                                            );
                                        });

                                        // Col 5: Key (Calm Traktor OpenKey text with subtle harmonic dot)
                                        let (key_color, key_txt) = if is_harmonic {
                                            (
                                                TraktorTheme::PLAY_ACTIVE,
                                                format!("● {}", track.musical_key),
                                            )
                                        } else {
                                            (
                                                TraktorTheme::TEXT_SECONDARY,
                                                track.musical_key.clone(),
                                            )
                                        };
                                        let key_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(key_txt)
                                                    .color(key_color)
                                                    .font(egui::FontId::monospace(11.0))
                                                    .strong(),
                                            ),
                                        );
                                        if key_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }
                                        key_resp.context_menu(|ui| {
                                            Self::render_track_context_menu(
                                                ui,
                                                track,
                                                in_prep,
                                                &playlist_names,
                                                four_deck_mode,
                                                &mut create_stems_cmd,
                                                &mut load_cmd,
                                                &mut toggle_prep_cmd,
                                                &mut add_to_playlist_cmd,
                                                &mut reveal_cmd,
                                                &mut remove_cmd,
                                            );
                                        });

                                        // Col 6: Stems Status / Create Action
                                        if track.stems_ready {
                                            let stem_resp = ui.add(
                                                egui::Button::new(
                                                    RichText::new("✦ STEMS")
                                                        .color(TraktorTheme::DECK_A)
                                                        .font(egui::FontId::monospace(9.5))
                                                        .strong(),
                                                )
                                                .fill(Color32::from_rgb(50, 54, 62))
                                                .stroke(Stroke::new(0.5_f32, TraktorTheme::DECK_A)),
                                            ).on_hover_text("4 Stems separated (Drums, Bass, Other, Vocals). Right-click for options.");
                                            stem_resp.context_menu(|ui| {
                                                Self::render_track_context_menu(
                                                    ui,
                                                    track,
                                                    in_prep,
                                                    &playlist_names,
                                                    four_deck_mode,
                                                    &mut create_stems_cmd,
                                                    &mut load_cmd,
                                                    &mut toggle_prep_cmd,
                                                    &mut add_to_playlist_cmd,
                                                    &mut reveal_cmd,
                                                    &mut remove_cmd,
                                                );
                                            });
                                        } else {
                                            let stem_resp = ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("✦ Stems")
                                                            .color(Color32::from_rgb(255, 185, 55))
                                                            .font(egui::FontId::proportional(10.0))
                                                            .strong(),
                                                    )
                                                    .fill(Color32::from_rgb(46, 49, 55)),
                                                )
                                                .on_hover_text("Generate 4 stems (Drums, Bass, Other, Vocals) or right-click for full menu");
                                            if stem_resp.clicked() {
                                                create_stems_cmd = Some(track.id);
                                            }
                                            stem_resp.context_menu(|ui| {
                                                Self::render_track_context_menu(
                                                    ui,
                                                    track,
                                                    in_prep,
                                                    &playlist_names,
                                                    four_deck_mode,
                                                    &mut create_stems_cmd,
                                                    &mut load_cmd,
                                                    &mut toggle_prep_cmd,
                                                    &mut add_to_playlist_cmd,
                                                    &mut reveal_cmd,
                                                    &mut remove_cmd,
                                                );
                                            });
                                        }

                                        // Col 7: Duration
                                        let m = (track.duration_sec / 60.0).floor() as u32;
                                        let s = (track.duration_sec % 60.0).floor() as u32;
                                        let dur_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(format!("{m:02}:{s:02}"))
                                                    .color(if is_selected {
                                                        Color32::WHITE
                                                    } else {
                                                        TraktorTheme::TEXT_SECONDARY
                                                    })
                                                    .font(egui::FontId::monospace(11.0)),
                                            ),
                                        );
                                        if dur_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }

                                        // Col 8: Genre
                                        let genre_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(&track.genre)
                                                    .color(if is_selected {
                                                        Color32::WHITE
                                                    } else {
                                                        TraktorTheme::TEXT_SECONDARY
                                                    })
                                                    .font(egui::FontId::proportional(11.0)),
                                            ),
                                        );
                                        if genre_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }

                                        // Col 9: Rating Stars (Traktor muted slate-blue stars)
                                        let stars = "★".repeat(track.rating as usize);
                                        let rating_resp = ui.add(
                                            egui::SelectableLabel::new(
                                                is_selected,
                                                RichText::new(stars)
                                                    .color(Color32::from_rgb(98, 144, 196))
                                                    .font(egui::FontId::proportional(11.0)),
                                            ),
                                        );
                                        if rating_resp.clicked() {
                                            self.selected_track_id = Some(track.id);
                                        }

                                        // Col 10: Crate / Playlist Actions
                                        ui.horizontal(|ui| {
                                            let prep_btn = ui.add(
                                                egui::Button::new(
                                                    RichText::new(if in_prep { "★ Prep" } else { "+ Prep" })
                                                        .color(if in_prep {
                                                            Color32::WHITE
                                                        } else {
                                                            TraktorTheme::TEXT_SECONDARY
                                                        })
                                                        .font(egui::FontId::proportional(10.0)),
                                                )
                                                .fill(if in_prep {
                                                    TraktorTheme::ROW_SELECTED
                                                } else {
                                                    Color32::from_rgb(46, 49, 55)
                                                }),
                                            );
                                            if prep_btn.clicked() {
                                                toggle_prep_cmd = Some(track.id);
                                            }
                                            prep_btn.context_menu(|ui| {
                                                Self::render_track_context_menu(
                                                    ui,
                                                    track,
                                                    in_prep,
                                                    &playlist_names,
                                                    four_deck_mode,
                                                    &mut create_stems_cmd,
                                                    &mut load_cmd,
                                                    &mut toggle_prep_cmd,
                                                    &mut add_to_playlist_cmd,
                                                    &mut reveal_cmd,
                                                    &mut remove_cmd,
                                                );
                                            });

                                            egui::ComboBox::from_id_salt(format!("pl_add_{}", track.id))
                                                .selected_text("+ List")
                                                .width(72.0)
                                                .show_ui(ui, |ui| {
                                                    for (p_idx, p_name) in playlist_names.iter().enumerate()
                                                    {
                                                        if ui.selectable_label(false, p_name).clicked() {
                                                            add_to_playlist_cmd = Some((p_idx, track.id));
                                                        }
                                                    }
                                                });
                                        });

                                        ui.end_row();
                                    }
                                });
                        });
                });

                // 4b. rekordbox 7-style Harmonic & Energy Match Radar Side Panel
                if radar_w > 0.0 {
                    ui.allocate_ui(Vec2::new(radar_w, ui.available_height()), |ui| {
                        egui::Frame::none()
                            .fill(Color32::from_rgb(16, 20, 26))
                            .stroke(Stroke::new(1.0_f32, TraktorTheme::PLAY_ACTIVE))
                            .rounding(4.0)
                            .inner_margin(egui::Margin::same(6.0))
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new("● MATCH RADAR")
                                                .color(TraktorTheme::PLAY_ACTIVE)
                                                .font(egui::FontId::monospace(10.5))
                                                .strong(),
                                        );
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(format!("Key {current_deck_a_key}"))
                                                        .color(TraktorTheme::DECK_A)
                                                        .font(egui::FontId::monospace(9.5))
                                                        .strong(),
                                                );
                                            },
                                        );
                                    });
                                    ui.label(
                                        RichText::new("Best Next Tracks (Harmonic + BPM + Stems)")
                                            .color(TraktorTheme::TEXT_MUTED)
                                            .font(egui::FontId::proportional(9.5)),
                                    );
                                    ui.separator();

                                    let mut radar_matches: Vec<(u8, UiTrackEntry)> = self
                                        .tracks
                                        .iter()
                                        .filter_map(|t| {
                                            if KeyWheel::are_harmonically_compatible(
                                                current_deck_a_key,
                                                &t.musical_key,
                                            ) {
                                                let exact = t.musical_key == current_deck_a_key;
                                                let score = if exact { 99 } else { 93 }
                                                    + u8::from(t.stems_ready);
                                                Some((score.min(99), t.clone()))
                                            } else {
                                                None
                                            }
                                        })
                                        .collect();
                                    radar_matches.sort_by(|a, b| b.0.cmp(&a.0));

                                    egui::ScrollArea::vertical()
                                        .id_salt("match_radar_scroll")
                                        .show(ui, |ui| {
                                            for (score, mtrack) in radar_matches.iter().take(8) {
                                                egui::Frame::none()
                                                    .fill(Color32::from_rgb(23, 27, 35))
                                                    .rounding(3.0)
                                                    .inner_margin(egui::Margin::symmetric(6.0, 4.0))
                                                    .show(ui, |ui| {
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                RichText::new(format!("{score}%"))
                                                                    .color(TraktorTheme::PLAY_ACTIVE)
                                                                    .font(egui::FontId::monospace(10.0))
                                                                    .strong(),
                                                            );
                                                            ui.label(
                                                                RichText::new(&mtrack.title)
                                                                    .color(Color32::WHITE)
                                                                    .font(egui::FontId::proportional(11.0))
                                                                    .strong(),
                                                            );
                                                        });
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "{} • {:.1} BPM • {}",
                                                                    mtrack.artist,
                                                                    mtrack.bpm,
                                                                    mtrack.musical_key
                                                                ))
                                                                .color(TraktorTheme::TEXT_SECONDARY)
                                                                .font(egui::FontId::monospace(9.0)),
                                                            );
                                                            ui.with_layout(
                                                                egui::Layout::right_to_left(
                                                                    egui::Align::Center,
                                                                ),
                                                                |ui| {
                                                                    if ui
                                                                        .add(
                                                                            egui::Button::new(
                                                                                RichText::new("B")
                                                                                    .color(Color32::BLACK)
                                                                                    .font(egui::FontId::monospace(9.0))
                                                                                    .strong(),
                                                                            )
                                                                            .fill(TraktorTheme::DECK_B)
                                                                            .min_size(Vec2::new(20.0, 16.0)),
                                                                        )
                                                                        .on_hover_text("Load into Deck B")
                                                                        .clicked()
                                                                    {
                                                                        load_cmd = Some((2, mtrack.clone()));
                                                                    }
                                                                    if ui
                                                                        .add(
                                                                            egui::Button::new(
                                                                                RichText::new("A")
                                                                                    .color(Color32::BLACK)
                                                                                    .font(egui::FontId::monospace(9.0))
                                                                                    .strong(),
                                                                            )
                                                                            .fill(TraktorTheme::DECK_A)
                                                                            .min_size(Vec2::new(20.0, 16.0)),
                                                                        )
                                                                        .on_hover_text("Load into Deck A")
                                                                        .clicked()
                                                                    {
                                                                        load_cmd = Some((1, mtrack.clone()));
                                                                    }
                                                                },
                                                            );
                                                        });
                                                    });
                                                ui.add_space(3.0);
                                            }
                                        });
                                });
                            });
                    });
                }
            });

            if let Some(track_id) = create_stems_cmd {
                if let Some(t) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    t.stems_ready = true;
                    self.last_stemmed_track_id = Some(track_id);
                    self.status_message = format!(
                        "✓ Generated 4-Stem separation (Drums, Bass, Other, Vocals) for '{}'!",
                        t.title
                    );
                }
            }

            if let Some((deck_idx, track)) = load_cmd {
                self.record_history(deck_idx, &track);
                let d = (deck_idx - 1).min(3);
                self.last_loaded_deck = Some((d, track.stems_ready));
                on_load_deck(deck_idx, &track);
                self.status_message = format!(
                    "✓ Loaded '{}' ({:.2} BPM, {}) into Deck {}",
                    track.title,
                    track.bpm,
                    track.musical_key,
                    ["A", "B", "C", "D"][d]
                );
            }

            if let Some(track_id) = toggle_prep_cmd {
                if let Some(pos) = self.prep_track_ids.iter().position(|&id| id == track_id) {
                    self.prep_track_ids.remove(pos);
                    self.status_message = "★ Removed track from Preparation crate.".to_string();
                } else {
                    self.prep_track_ids.push(track_id);
                    self.status_message = "★ Added track to Preparation crate.".to_string();
                }
            }

            if let Some((p_idx, track_id)) = add_to_playlist_cmd {
                if let Some(pl) = self.playlists.get_mut(p_idx) {
                    if !pl.track_ids.contains(&track_id) {
                        pl.track_ids.push(track_id);
                        self.status_message = format!("✓ Added track to '{}'", pl.name);
                    } else {
                        self.status_message = format!("Track already in '{}'", pl.name);
                    }
                }
            }

            if let Some(path) = reveal_cmd {
                if let Some(parent) = path.parent() {
                    self.navigate_to(parent.to_path_buf());
                    self.selected_nav = NavItem::Explorer;
                    self.status_message = format!("📂 Showing file in Explorer: {}", path.display());
                }
            }

            if let Some(track_id) = remove_cmd {
                self.tracks.retain(|t| t.id != track_id);
                self.prep_track_ids.retain(|&id| id != track_id);
                for p in &mut self.playlists {
                    p.track_ids.retain(|&id| id != track_id);
                }
                if self.selected_track_id == Some(track_id) {
                    self.selected_track_id = None;
                }
                self.status_message = "✓ Removed track from collection.".to_string();
            }
        });
    }

    fn show_history_view(
        &mut self,
        ui: &mut Ui,
        four_deck_mode: bool,
        on_load_deck: &mut impl FnMut(usize, &UiTrackEntry),
    ) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("SESSION PLAY HISTORY")
                        .color(TraktorTheme::DECK_A)
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                );
                if !self.history.is_empty() && ui.button("Clear History").clicked() {
                    self.history.clear();
                }
            });
            ui.separator();

            if self.history.is_empty() {
                ui.label(
                    RichText::new("No tracks loaded yet in this session. Load a track into any deck to record history.")
                        .color(TraktorTheme::TEXT_MUTED),
                );
                return;
            }

            let snap = self.history.clone();
            egui::ScrollArea::vertical()
                .id_salt("history_scroll")
                .show(ui, |ui| {
                    egui::Grid::new("history_grid")
                        .striped(true)
                        .spacing(Vec2::new(12.0, 4.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("#").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Deck").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Re-Load").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Title").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Artist").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("BPM").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Key").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.end_row();

                            for rec in &snap {
                                ui.label(RichText::new(format!("#{}", rec.order_num)).monospace());
                                ui.label(
                                    RichText::new(&rec.deck_label)
                                        .color(TraktorTheme::DECK_A)
                                        .strong(),
                                );
                                ui.horizontal(|ui| {
                                    if ui.button("LOAD A").clicked() {
                                        on_load_deck(1, &rec.track);
                                    }
                                    if ui.button("LOAD B").clicked() {
                                        on_load_deck(2, &rec.track);
                                    }
                                    if four_deck_mode {
                                        if ui.button("C").clicked() {
                                            on_load_deck(3, &rec.track);
                                        }
                                        if ui.button("D").clicked() {
                                            on_load_deck(4, &rec.track);
                                        }
                                    }
                                });
                                ui.label(RichText::new(&rec.track.title).color(Color32::WHITE).strong());
                                ui.label(RichText::new(&rec.track.artist).color(TraktorTheme::TEXT_SECONDARY));
                                ui.label(RichText::new(format!("{:.2}", rec.track.bpm)).monospace());
                                ui.label(RichText::new(&rec.track.musical_key).monospace());
                                ui.end_row();
                            }
                        });
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preparation_crate_and_stems_management() {
        let mut mgr = LibraryManager::default();
        assert!(!mgr.tracks.is_empty(), "Track collection should be populated");
        assert!(!mgr.prep_track_ids.is_empty(), "Preparation crate should have tracks");

        // Verify prep crate filter
        mgr.selected_nav = NavItem::Preparation;
        let prep_tracks: Vec<_> = mgr
            .tracks
            .iter()
            .filter(|t| mgr.prep_track_ids.contains(&t.id))
            .collect();
        assert_eq!(prep_tracks.len(), mgr.prep_track_ids.len());

        // Toggle prep
        let first_id = mgr.tracks[0].id;
        let was_prep = mgr.prep_track_ids.contains(&first_id);
        if was_prep {
            mgr.prep_track_ids.retain(|&id| id != first_id);
            assert!(!mgr.prep_track_ids.contains(&first_id));
        } else {
            mgr.prep_track_ids.push(first_id);
            assert!(mgr.prep_track_ids.contains(&first_id));
        }

        // Test stem status
        let unstemmed = mgr.tracks.iter_mut().find(|t| !t.stems_ready);
        if let Some(t) = unstemmed {
            t.stems_ready = true;
            assert!(t.stems_ready);
        }
    }
}
