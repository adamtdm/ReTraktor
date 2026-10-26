use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, RichText, Stroke, Ui, Vec2};
use retraktor_formats::{NmlCollection, TsiFile};
use retraktor_hid::NI_DEVICES;
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
    Playlist(usize),
    Smartlists,
    History,
    Explorer,
    Controllers,
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
    pub history: Vec<HistoryRecord>,
    pub search_query: String,
    pub selected_nav: NavItem,
    pub selected_track_id: Option<usize>,

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

    // Controller / TSI state
    pub loaded_tsi_summary: Option<String>,
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
            history: Vec::new(),
            search_query: String::new(),
            selected_nav: NavItem::AllTracks,
            selected_track_id: None,
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
            loaded_tsi_summary: None,
            status_message: "Ready — Click any track's [LOAD A] or [LOAD B] button, or browse local audio files in File Explorer.".to_string(),
        };
        mgr.populate_default_collection();
        mgr.refresh_explorer_dir();
        // Automatically scan ~/Music (non-recursively) if audio files exist there
        let _ = mgr.scan_directory_audio(&mgr.current_dir.clone(), false);
        mgr
    }
}

impl LibraryManager {
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
            },
        ];

        self.playlists = vec![
            UserPlaylist {
                name: "⭐ Preparation List".to_string(),
                track_ids: vec![1, 2, 6],
            },
            UserPlaylist {
                name: "🔥 Peak Time Techno".to_string(),
                track_ids: vec![1, 2, 5, 7, 8],
            },
            UserPlaylist {
                name: "🌴 Deep & Melodic House".to_string(),
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
            ui.spacing_mut().item_spacing = Vec2::new(2.0, 3.0);

            let mut nav_btn = |ui: &mut Ui, label: String, item: NavItem, indent: f32| {
                let selected = self.selected_nav == item;
                let text_color = if selected {
                    Color32::BLACK
                } else {
                    TraktorTheme::TEXT_PRIMARY
                };
                let bg_color = if selected {
                    TraktorTheme::DECK_A
                } else {
                    Color32::from_rgb(28, 30, 36)
                };

                ui.horizontal(|ui| {
                    if indent > 0.0 {
                        ui.add_space(indent);
                    }
                    let resp = ui.add(
                        egui::Button::new(
                            RichText::new(label)
                                .color(text_color)
                                .font(egui::FontId::proportional(11.5))
                                .strong(),
                        )
                        .fill(bg_color)
                        .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                        .min_size(Vec2::new((ui.available_width() - 4.0).max(80.0), 22.0)),
                    );
                    if resp.clicked() {
                        self.selected_nav = item;
                    }
                });
            };

            nav_btn(
                ui,
                format!("📁 Track Collection ({})", self.tracks.len()),
                NavItem::AllTracks,
                0.0,
            );
            nav_btn(ui, "💾 File Explorer (Local Disk)".to_string(), NavItem::Explorer, 0.0);
            nav_btn(ui, "⚡ Smartlists (Harmonic)".to_string(), NavItem::Smartlists, 0.0);
            nav_btn(
                ui,
                format!("🕒 Session History ({})", self.history.len()),
                NavItem::History,
                0.0,
            );
            nav_btn(ui, "🎛️ NI Controllers & TSI".to_string(), NavItem::Controllers, 0.0);

            ui.add_space(4.0);
            ui.label(
                RichText::new("📂 PLAYLISTS & CRATES")
                    .color(TraktorTheme::TEXT_MUTED)
                    .font(egui::FontId::monospace(9.5))
                    .strong(),
            );

            for idx in 0..self.playlists.len() {
                let lbl = format!(
                    "{} ({})",
                    self.playlists[idx].name,
                    self.playlists[idx].track_ids.len()
                );
                nav_btn(ui, lbl, NavItem::Playlist(idx), 6.0);
            }

            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_playlist_name)
                        .hint_text("New playlist...")
                        .desired_width(105.0),
                );
                if ui.button("+").on_hover_text("Create Playlist").clicked()
                    && !self.new_playlist_name.trim().is_empty()
                {
                    let name = format!("🎵 {}", self.new_playlist_name.trim());
                    self.playlists.push(UserPlaylist {
                        name,
                        track_ids: Vec::new(),
                    });
                    self.new_playlist_name.clear();
                    self.selected_nav = NavItem::Playlist(self.playlists.len() - 1);
                }
            });
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
            NavItem::Controllers => {
                self.show_controllers_view(ui);
            }
            NavItem::History => {
                self.show_history_view(ui, four_deck_mode, &mut on_load_deck);
            }
            NavItem::AllTracks | NavItem::Playlist(_) | NavItem::Smartlists => {
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

                if ui.button("🏠 Home").clicked() {
                    self.navigate_to(home.clone());
                }
                if ui.button("🎵 ~/Music").clicked() {
                    let m = home.join("Music");
                    self.navigate_to(if m.is_dir() { m } else { home.clone() });
                }
                if ui.button("⬇️ ~/Downloads").clicked() {
                    let d = home.join("Downloads");
                    self.navigate_to(if d.is_dir() { d } else { home.clone() });
                }
                if ui.button("💿 /media").clicked() {
                    self.navigate_to(PathBuf::from("/media"));
                }
                if ui.button("⬆️ Up Dir").clicked() {
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
                            RichText::new("📥 Import Folder to Collection")
                                .color(Color32::BLACK)
                                .strong(),
                        )
                        .fill(TraktorTheme::PLAY_ACTIVE),
                    )
                    .clicked()
                {
                    let dir = self.current_dir.clone();
                    let count = self.scan_directory_audio(&dir, true);
                    self.status_message = format!(
                        "✓ Scanned {} and imported {} audio files into Track Collection!",
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
                                                RichText::new("📂 Open Folder")
                                                    .color(TraktorTheme::DECK_A),
                                            )
                                            .min_size(Vec2::new(110.0, 20.0)),
                                        )
                                        .clicked()
                                    {
                                        nav_target = Some(item.path.clone());
                                    }
                                    if ui
                                        .link(
                                            RichText::new(format!("📁 {}", item.name))
                                                .color(TraktorTheme::TEXT_PRIMARY)
                                                .strong(),
                                        )
                                        .clicked()
                                    {
                                        nav_target = Some(item.path.clone());
                                    }
                                    ui.label(RichText::new("Directory").color(TraktorTheme::TEXT_MUTED));
                                    ui.label(RichText::new("-").color(TraktorTheme::TEXT_MUTED));
                                    ui.end_row();
                                } else if item.ext == "nml" {
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("📥 Import NML")
                                                    .color(Color32::BLACK)
                                                    .strong(),
                                            )
                                            .fill(TraktorTheme::PLAY_ACTIVE),
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
                                                    RichText::new("LOAD A")
                                                        .color(Color32::BLACK)
                                                        .font(egui::FontId::monospace(9.5))
                                                        .strong(),
                                                )
                                                .fill(TraktorTheme::DECK_A),
                                            )
                                            .clicked()
                                        {
                                            file_to_load = Some((1, item.path.clone()));
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("LOAD B")
                                                        .color(Color32::BLACK)
                                                        .font(egui::FontId::monospace(9.5))
                                                        .strong(),
                                                )
                                                .fill(TraktorTheme::DECK_B),
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
                                                            .color(Color32::BLACK)
                                                            .font(egui::FontId::monospace(9.5))
                                                            .strong(),
                                                    )
                                                    .fill(TraktorTheme::DECK_C),
                                                )
                                                .clicked()
                                            {
                                                file_to_load = Some((3, item.path.clone()));
                                            }
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("D")
                                                            .color(Color32::BLACK)
                                                            .font(egui::FontId::monospace(9.5))
                                                            .strong(),
                                                    )
                                                    .fill(TraktorTheme::DECK_D),
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
                                    ui.label(
                                        RichText::new(format!("🎵 {}", item.name))
                                            .color(TraktorTheme::TEXT_PRIMARY)
                                            .strong(),
                                    );
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
            if let Some(p) = file_to_add {
                if let Some(entry) = self.add_audio_file_to_collection(&p) {
                    self.status_message = format!("✓ Added '{}' to Track Collection.", entry.title);
                }
            }
            if let Some((deck_idx, p)) = file_to_load {
                if let Some(entry) = self.add_audio_file_to_collection(&p) {
                    self.record_history(deck_idx, &entry);
                    on_load_deck(deck_idx, &entry);
                    self.status_message = format!(
                        "✓ Decoded & loaded '{}' into Deck {}!",
                        entry.title,
                        ["A", "B", "C", "D"][(deck_idx - 1).min(3)]
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
                    RichText::new("🔍 Search:")
                        .color(TraktorTheme::TEXT_SECONDARY)
                        .font(egui::FontId::proportional(12.0)),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Filter by title, artist, BPM, key, or genre...")
                        .desired_width(240.0),
                );
                if !self.search_query.is_empty() && ui.button("✖").clicked() {
                    self.search_query.clear();
                }

                ui.separator();

                if ui
                    .button("🎵 Scan ~/Music")
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
                        "✓ Scanned {} — added {} new audio tracks to Collection.",
                        target.display(),
                        count
                    );
                }

                if ui
                    .button("💾 Browse Local Files")
                    .on_hover_text("Open the built-in File Explorer")
                    .clicked()
                {
                    self.selected_nav = NavItem::Explorer;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(&self.status_message)
                            .color(TraktorTheme::TEXT_SECONDARY)
                            .font(egui::FontId::monospace(10.0)),
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

            // 4. Render Full-Width Sortable Track Grid
            let playlist_names: Vec<String> =
                self.playlists.iter().map(|p| p.name.clone()).collect();
            let prep_ids: Vec<usize> = self
                .playlists
                .first()
                .map(|p| p.track_ids.clone())
                .unwrap_or_default();

            let mut add_to_playlist_cmd: Option<(usize, usize)> = None;
            let mut toggle_prep_cmd: Option<usize> = None;
            let mut load_cmd: Option<(usize, UiTrackEntry)> = None;

            egui::ScrollArea::vertical()
                .id_salt("tracks_table_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    egui::Grid::new("traktor_collection_grid")
                        .striped(true)
                        .spacing(Vec2::new(10.0, 4.0))
                        .min_col_width(48.0)
                        .show(ui, |ui| {
                            // Sortable Column Headers
                            ui.label(
                                RichText::new("Load Deck")
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
                                        TraktorTheme::DECK_A
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
                            sort_header(ui, "Key (Open)", SortColumn::Key);
                            sort_header(ui, "Time", SortColumn::Duration);
                            sort_header(ui, "Genre", SortColumn::Genre);
                            sort_header(ui, "Rating", SortColumn::Rating);
                            ui.label(
                                RichText::new("Crate / Playlist")
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

                                // Col 1: Load Deck Buttons
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("LOAD A")
                                                    .color(Color32::BLACK)
                                                    .font(egui::FontId::monospace(9.5))
                                                    .strong(),
                                            )
                                            .fill(TraktorTheme::DECK_A)
                                            .min_size(Vec2::new(48.0, 19.0)),
                                        )
                                        .clicked()
                                    {
                                        load_cmd = Some((1, track.clone()));
                                    }
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new("LOAD B")
                                                    .color(Color32::BLACK)
                                                    .font(egui::FontId::monospace(9.5))
                                                    .strong(),
                                            )
                                            .fill(TraktorTheme::DECK_B)
                                            .min_size(Vec2::new(48.0, 19.0)),
                                        )
                                        .clicked()
                                    {
                                        load_cmd = Some((2, track.clone()));
                                    }
                                    if four_deck_mode {
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("C")
                                                        .color(Color32::BLACK)
                                                        .font(egui::FontId::monospace(9.5))
                                                        .strong(),
                                                )
                                                .fill(TraktorTheme::DECK_C)
                                                .min_size(Vec2::new(22.0, 19.0)),
                                            )
                                            .clicked()
                                        {
                                            load_cmd = Some((3, track.clone()));
                                        }
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("D")
                                                        .color(Color32::BLACK)
                                                        .font(egui::FontId::monospace(9.5))
                                                        .strong(),
                                                )
                                                .fill(TraktorTheme::DECK_D)
                                                .min_size(Vec2::new(22.0, 19.0)),
                                            )
                                            .clicked()
                                        {
                                            load_cmd = Some((4, track.clone()));
                                        }
                                    }
                                });

                                // Col 2: Title
                                ui.label(
                                    RichText::new(&track.title)
                                        .color(Color32::WHITE)
                                        .font(egui::FontId::proportional(12.0))
                                        .strong(),
                                );

                                // Col 3: Artist
                                ui.label(
                                    RichText::new(&track.artist)
                                        .color(Color32::from_rgb(195, 202, 215))
                                        .font(egui::FontId::proportional(12.0)),
                                );

                                // Col 4: BPM
                                ui.label(
                                    RichText::new(format!("{:.2}", track.bpm))
                                        .color(TraktorTheme::DECK_A)
                                        .font(egui::FontId::monospace(11.5))
                                        .strong(),
                                );

                                // Col 5: Key Badge (Harmonic Match Glow)
                                let (badge_bg, badge_fg, badge_txt) = if is_harmonic {
                                    (
                                        TraktorTheme::PLAY_ACTIVE,
                                        Color32::BLACK,
                                        format!("★ {} MATCH", track.musical_key),
                                    )
                                } else {
                                    (
                                        Color32::from_rgb(38, 42, 52),
                                        TraktorTheme::TEXT_PRIMARY,
                                        track.musical_key.clone(),
                                    )
                                };
                                egui::Frame::none()
                                    .fill(badge_bg)
                                    .rounding(3.0)
                                    .inner_margin(egui::Margin::symmetric(6.0, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(badge_txt)
                                                .color(badge_fg)
                                                .font(egui::FontId::monospace(10.0))
                                                .strong(),
                                        );
                                    });

                                // Col 6: Duration
                                let m = (track.duration_sec / 60.0).floor() as u32;
                                let s = (track.duration_sec % 60.0).floor() as u32;
                                ui.label(
                                    RichText::new(format!("{m:02}:{s:02}"))
                                        .color(TraktorTheme::TEXT_SECONDARY)
                                        .font(egui::FontId::monospace(11.0)),
                                );

                                // Col 7: Genre
                                ui.label(
                                    RichText::new(&track.genre)
                                        .color(TraktorTheme::TEXT_SECONDARY)
                                        .font(egui::FontId::proportional(11.0)),
                                );

                                // Col 8: Rating Stars
                                let stars = "★".repeat(track.rating as usize);
                                ui.label(
                                    RichText::new(stars)
                                        .color(Color32::from_rgb(255, 195, 0))
                                        .font(egui::FontId::proportional(11.0)),
                                );

                                // Col 9: Crate / Playlist Actions
                                ui.horizontal(|ui| {
                                    let in_prep = prep_ids.contains(&track.id);
                                    let prep_btn = ui.add(
                                        egui::Button::new(
                                            RichText::new(if in_prep { "★ Prepped" } else { "+ Prep" })
                                                .color(if in_prep {
                                                    Color32::BLACK
                                                } else {
                                                    TraktorTheme::TEXT_SECONDARY
                                                })
                                                .font(egui::FontId::proportional(10.0)),
                                        )
                                        .fill(if in_prep {
                                            Color32::from_rgb(255, 200, 0)
                                        } else {
                                            Color32::from_rgb(32, 35, 42)
                                        }),
                                    );
                                    if prep_btn.clicked() {
                                        toggle_prep_cmd = Some(track.id);
                                    }

                                    egui::ComboBox::from_id_salt(format!("pl_add_{}", track.id))
                                        .selected_text("+ Playlist")
                                        .width(85.0)
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

            if let Some((deck_idx, track)) = load_cmd {
                self.record_history(deck_idx, &track);
                on_load_deck(deck_idx, &track);
                self.status_message = format!(
                    "✓ Loaded '{}' ({:.2} BPM, {}) into Deck {}",
                    track.title,
                    track.bpm,
                    track.musical_key,
                    ["A", "B", "C", "D"][(deck_idx - 1).min(3)]
                );
            }

            if let Some(track_id) = toggle_prep_cmd {
                if let Some(prep) = self.playlists.first_mut() {
                    if let Some(pos) = prep.track_ids.iter().position(|&id| id == track_id) {
                        prep.track_ids.remove(pos);
                    } else {
                        prep.track_ids.push(track_id);
                    }
                }
            }

            if let Some((p_idx, track_id)) = add_to_playlist_cmd {
                if let Some(pl) = self.playlists.get_mut(p_idx) {
                    if !pl.track_ids.contains(&track_id) {
                        pl.track_ids.push(track_id);
                        self.status_message = format!("✓ Added track to '{}'", pl.name);
                    }
                }
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
                    RichText::new("🕒 SESSION PLAY HISTORY")
                        .color(TraktorTheme::DECK_A)
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                );
                if !self.history.is_empty() && ui.button("🗑 Clear History").clicked() {
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

    fn show_controllers_view(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("🎛️ NATIVE INSTRUMENTS KONTROL HARDWARE & TSI MAPPINGS")
                        .color(TraktorTheme::DECK_A)
                        .font(egui::FontId::monospace(12.0))
                        .strong(),
                );
                if let Some(ref tsi_info) = self.loaded_tsi_summary {
                    ui.label(RichText::new(tsi_info).color(TraktorTheme::PLAY_ACTIVE));
                }
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("controllers_scroll")
                .show(ui, |ui| {
                    egui::Grid::new("ni_controllers_grid")
                        .striped(true)
                        .spacing(Vec2::new(16.0, 4.0))
                        .show(ui, |ui| {
                            ui.label(RichText::new("Controller Model").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("USB VID:PID").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("CSI Module").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("RGB565 Display").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.label(RichText::new("Haptic Drive Platters").color(TraktorTheme::TEXT_SECONDARY).strong());
                            ui.end_row();

                            for dev in &NI_DEVICES {
                                ui.label(RichText::new(dev.model).color(Color32::WHITE).strong());
                                ui.label(
                                    RichText::new(format!("{:04X}:{:04X}", dev.vid, dev.pid))
                                        .color(TraktorTheme::DECK_A)
                                        .monospace(),
                                );
                                ui.label(RichText::new(format!("CSI/{}", dev.csi_folder)).monospace());
                                ui.label(
                                    RichText::new(if dev.has_color_screen {
                                        "✓ OffscreenView (0x8400)"
                                    } else {
                                        "LED Matrix"
                                    })
                                    .color(if dev.has_color_screen {
                                        TraktorTheme::PLAY_ACTIVE
                                    } else {
                                        TraktorTheme::TEXT_MUTED
                                    }),
                                );
                                ui.label(
                                    RichText::new(if dev.has_motorized_jog {
                                        "✓ Report 0x30 Motorized"
                                    } else {
                                        "Capacitive Jog / Strip"
                                    })
                                    .color(if dev.has_motorized_jog {
                                        TraktorTheme::DECK_B
                                    } else {
                                        TraktorTheme::TEXT_MUTED
                                    }),
                                );
                                ui.end_row();
                            }
                        });

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label("Load .tsi Mapping File Path:");
                        ui.add(egui::TextEdit::singleline(&mut self.path_input).desired_width(300.0));
                        if ui.button("Load .tsi").clicked() {
                            let p = PathBuf::from(self.path_input.trim());
                            match fs::read_to_string(&p)
                                .map_err(|e| e.to_string())
                                .and_then(|xml| TsiFile::parse_xml(&xml).map_err(|e| e.to_string()))
                            {
                                Ok(tsi) => {
                                    self.loaded_tsi_summary = Some(format!(
                                        "✓ Loaded {} ({} devices mapped)",
                                        p.display(),
                                        tsi.devices.len()
                                    ));
                                }
                                Err(e) => {
                                    self.loaded_tsi_summary = Some(format!("⚠ TSI error: {e}"));
                                }
                            }
                        }
                    });
                });
        });
    }
}
