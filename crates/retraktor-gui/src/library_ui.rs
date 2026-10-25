use crate::theme::TraktorTheme;
use eframe::egui::{self, Color32, Stroke, Ui, Vec2};
use retraktor_formats::NmlCollection;
use retraktor_library::KeyWheel;
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

pub struct LibraryManager {
    pub tracks: Vec<UiTrackEntry>,
    pub search_query: String,
    pub selected_nav: NavItem,
    pub selected_track_id: Option<usize>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NavItem {
    AllTracks,
    Playlists,
    Smartlists,
    History,
    Explorer,
    Controllers,
}

impl Default for LibraryManager {
    fn default() -> Self {
        let mut mgr = Self {
            tracks: Vec::new(),
            search_query: String::new(),
            selected_nav: NavItem::AllTracks,
            selected_track_id: None,
        };
        mgr.populate_default_collection();
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
                musical_key: "4m".to_string(), // Am
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
                musical_key: "5m".to_string(), // Em (Harmonic +1 step!)
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
                musical_key: "4d".to_string(), // C Major (Relative Major of 4m!)
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
                musical_key: "11m".to_string(), // F#m
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
                musical_key: "3m".to_string(), // Dm (Harmonic -1 step!)
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
                musical_key: "4m".to_string(), // Exact Match!
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
                musical_key: "6m".to_string(), // Bm
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
    }

    /// Imports a Traktor `collection.nml` XML into the library
    pub fn import_nml_file(&mut self, path: &Path) -> Result<usize, String> {
        let content = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let col = NmlCollection::parse_xml(&content).map_err(|e| e.to_string())?;
        let mut count = 0;
        let start_id = self.tracks.len() + 1;
        for (i, entry) in col.entries.iter().enumerate() {
            let key = entry.key_text.clone().unwrap_or_else(|| "1d".to_string());
            let bpm = entry.tempo.as_ref().map(|t| t.bpm).unwrap_or(120.0);
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
        Ok(count)
    }

    /// Renders the Left Navigation Tree
    pub fn show_nav_tree(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(0.0, 4.0);
            ui.add_space(4.0);

            let mut nav_button = |label: &str, item: NavItem| {
                let selected = self.selected_nav == item;
                let text_color = if selected {
                    TraktorTheme::DECK_A
                } else {
                    TraktorTheme::TEXT_PRIMARY
                };
                let bg_color = if selected {
                    Color32::from_rgb(34, 38, 48)
                } else {
                    Color32::TRANSPARENT
                };

                let resp = ui.add(
                    egui::Button::new(
                        egui::RichText::new(label)
                            .color(text_color)
                            .font(egui::FontId::proportional(12.0)),
                    )
                    .fill(bg_color)
                    .stroke(Stroke::NONE)
                    .min_size(Vec2::new(ui.available_width(), 24.0)),
                );
                if resp.clicked() {
                    self.selected_nav = item;
                }
            };

            nav_button("📁 Track Collection", NavItem::AllTracks);
            nav_button("📂 Playlists", NavItem::Playlists);
            nav_button("⚡ Smartlists (Harmonic)", NavItem::Smartlists);
            nav_button("🕒 History", NavItem::History);
            nav_button("💾 File Explorer", NavItem::Explorer);
            nav_button("🎛️ NI Controllers", NavItem::Controllers);
        });
    }

    /// Renders the Track Table with Harmonic Matching and [Load A] / [Load B] buttons
    pub fn show_track_table(
        &mut self,
        ui: &mut Ui,
        current_deck_a_key: &str,
        mut on_load_deck: impl FnMut(usize, &UiTrackEntry),
    ) {
        ui.vertical(|ui| {
            // Search Bar & Filter Header
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("🔍 Search:")
                        .color(TraktorTheme::TEXT_SECONDARY)
                        .font(egui::FontId::proportional(12.0)),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .hint_text("Filter by title, artist, BPM, or key...")
                        .desired_width(280.0),
                );
                if !self.search_query.is_empty() && ui.button("✖ Clear").clicked() {
                    self.search_query.clear();
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(format!("{} tracks", self.tracks.len()))
                            .color(TraktorTheme::TEXT_MUTED)
                            .font(egui::FontId::monospace(11.0)),
                    );
                });
            });

            ui.add_space(4.0);

            // Table Header Bar
            egui::Frame::none()
                .fill(Color32::from_rgb(20, 22, 26))
                .stroke(Stroke::new(1.0_f32, TraktorTheme::BORDER_COLOR))
                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.set_min_height(20.0);
                        let col_style = |t: &str| {
                            egui::RichText::new(t)
                                .color(TraktorTheme::TEXT_SECONDARY)
                                .font(egui::FontId::proportional(11.0))
                        };
                        ui.allocate_ui_with_layout(
                            Vec2::new(110.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("Load")); },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(220.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("Title")); },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(180.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("Artist")); },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(70.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("BPM")); },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(80.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("Key")); },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(60.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("Time")); },
                        );
                        ui.allocate_ui_with_layout(
                            Vec2::new(120.0, 18.0),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| { ui.label(col_style("Genre")); },
                        );
                    });
                });

            // Table Rows
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    let query = self.search_query.to_lowercase();
                    for track in &self.tracks {
                        // Filter logic
                        if !query.is_empty() {
                            let match_title = track.title.to_lowercase().contains(&query);
                            let match_artist = track.artist.to_lowercase().contains(&query);
                            let match_key = track.musical_key.to_lowercase().contains(&query);
                            let match_bpm = format!("{:.1}", track.bpm).contains(&query);
                            if !match_title && !match_artist && !match_key && !match_bpm {
                                continue;
                            }
                        }

                        // Check Harmonic Match with Deck A
                        let is_harmonic = KeyWheel::are_harmonically_compatible(
                            current_deck_a_key,
                            &track.musical_key,
                        );

                        let row_bg = if is_harmonic {
                            Color32::from_rgba_premultiplied(0, 230, 118, 18)
                        } else {
                            Color32::TRANSPARENT
                        };

                        egui::Frame::none()
                            .fill(row_bg)
                            .stroke(Stroke::new(0.5_f32, TraktorTheme::BORDER_COLOR))
                            .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Load Buttons
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(110.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            if ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new("LOAD A")
                                                        .color(Color32::BLACK)
                                                        .font(egui::FontId::monospace(9.0))
                                                        .strong(),
                                                )
                                                .fill(TraktorTheme::DECK_A)
                                                .min_size(Vec2::new(48.0, 18.0)),
                                            ).clicked() {
                                                on_load_deck(1, track);
                                            }

                                            if ui.add(
                                                egui::Button::new(
                                                    egui::RichText::new("LOAD B")
                                                        .color(Color32::BLACK)
                                                        .font(egui::FontId::monospace(9.0))
                                                        .strong(),
                                                )
                                                .fill(TraktorTheme::DECK_B)
                                                .min_size(Vec2::new(48.0, 18.0)),
                                            ).clicked() {
                                                on_load_deck(2, track);
                                            }
                                        },
                                    );

                                    // Title
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(220.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(&track.title)
                                                    .color(TraktorTheme::TEXT_PRIMARY)
                                                    .font(egui::FontId::proportional(12.0))
                                                    .strong(),
                                            );
                                        },
                                    );

                                    // Artist
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(180.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(&track.artist)
                                                    .color(TraktorTheme::TEXT_SECONDARY)
                                                    .font(egui::FontId::proportional(12.0)),
                                            );
                                        },
                                    );

                                    // BPM
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(70.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(format!("{:.2}", track.bpm))
                                                    .color(TraktorTheme::TEXT_HIGHLIGHT)
                                                    .font(egui::FontId::monospace(12.0)),
                                            );
                                        },
                                    );

                                    // Musical Key with Harmonic Glow Badge
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(80.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let (badge_bg, badge_txt) = if is_harmonic {
                                                (TraktorTheme::PLAY_ACTIVE, Color32::BLACK)
                                            } else {
                                                (Color32::from_rgb(38, 42, 50), TraktorTheme::TEXT_PRIMARY)
                                            };
                                            let text = if is_harmonic {
                                                format!("★ {}", track.musical_key)
                                            } else {
                                                track.musical_key.clone()
                                            };
                                            egui::Frame::none()
                                                .fill(badge_bg)
                                                .rounding(2.0)
                                                .inner_margin(egui::Margin::symmetric(4.0, 1.0))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        egui::RichText::new(text)
                                                            .color(badge_txt)
                                                            .font(egui::FontId::monospace(10.0))
                                                            .strong(),
                                                    );
                                                });
                                        },
                                    );

                                    // Time (mm:ss)
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(60.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            let m = (track.duration_sec / 60.0).floor() as u32;
                                            let s = (track.duration_sec % 60.0).floor() as u32;
                                            ui.label(
                                                egui::RichText::new(format!("{m:02}:{s:02}"))
                                                    .color(TraktorTheme::TEXT_MUTED)
                                                    .font(egui::FontId::monospace(11.0)),
                                            );
                                        },
                                    );

                                    // Genre
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(120.0, 22.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                egui::RichText::new(&track.genre)
                                                    .color(TraktorTheme::TEXT_MUTED)
                                                    .font(egui::FontId::proportional(11.0)),
                                            );
                                        },
                                    );
                                });
                            });
                    }
                });
        });
    }
}
