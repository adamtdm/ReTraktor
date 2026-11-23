//! The controller rules file in Preferences > Controllers: loaded at startup and on Reload, with its status, the
//! last control touched and the active layers, and the mapping import. The app writes the file only when asked:
//! "Create example file" when none exists, and Import's "Add to rules file", which refuses when the file changed
//! since the preview and keeps the previous file as a backup. Edits made in another editor are never overwritten.

use crate::theme::TraktorTheme;
use eframe::egui::{self, RichText, Ui};
use retraktor_hid::controller_rules::{parse_rules, read_rules_file, RuleEngine, RuleSet, EXAMPLE_RULES};
use retraktor_hid::mapping_import::{self, Conversion, Source};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

pub struct RulesFile {
    pub path: PathBuf,
    pub engine: RuleEngine,
    /// Outcome of the last load or reload: a summary, or every problem found.
    pub status: Result<String, Vec<String>>,
    pub import: ImportPanel,
}

/// Importing a Traktor, Mixxx or rekordbox mapping: pick the file, the device and the MIDI port, preview the
/// coverage report, then save the draft as a new file or add it to the rules file.
#[derive(Default)]
pub struct ImportPanel {
    picking: Option<Receiver<Option<PathBuf>>>,
    saving: Option<Receiver<Option<PathBuf>>>,
    source: Option<(PathBuf, Result<Source, String>)>,
    /// 1-based device in the source.
    device: usize,
    port: String,
    preview: Option<Result<Conversion, String>>,
    /// The preview's coverage report, made once rather than every frame.
    report: String,
    /// The rules file when the preview was made (`None`: no file), so Add refuses if it has changed since.
    snapshot: Option<Option<String>>,
    message: Option<Result<String, String>>,
    /// "Add to rules file" was pressed; it is carried out after the panel, which needs the whole rules file.
    add_requested: bool,
}

impl ImportPanel {
    pub fn open(&mut self, path: PathBuf) {
        let source = mapping_import::read_source(&path);
        *self = Self { port: std::mem::take(&mut self.port), device: 1, source: Some((path, source)), ..Default::default() };
    }

    /// Converts the chosen device and remembers the rules file the draft would be added to.
    pub fn preview(&mut self, rules_path: &Path) {
        let Some((_, Ok(source))) = &self.source else { return };
        self.message = None;
        self.preview = Some(mapping_import::convert(source, self.device, self.port.trim()));
        self.report = self.preview.as_ref().and_then(|p| p.as_ref().ok()).map(Conversion::report).unwrap_or_default();
        self.snapshot = match read_rules_file(rules_path) {
            Ok(text) => Some(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(None),
            Err(e) => {
                self.message = Some(Err(format!("Can't read {}: {e}", rules_path.display())));
                None
            }
        };
    }

    fn draft(&self) -> Option<&Conversion> {
        self.preview.as_ref().and_then(|p| p.as_ref().ok()).filter(|c| c.draft.is_some())
    }
}

impl RulesFile {
    pub fn load(path: PathBuf) -> Self {
        let mut rules = Self::unloaded(path);
        rules.reload();
        rules
    }

    /// Knows the path but has read nothing yet (unit tests never read the user's own rules).
    pub fn unloaded(path: PathBuf) -> Self {
        Self { path, engine: RuleEngine::default(), status: Ok("Not loaded yet: press Reload.".into()), import: ImportPanel::default() }
    }

    /// Reads the file again. A file with any problem changes nothing, so the last good rules stay active;
    /// a missing file means no rules.
    pub fn reload(&mut self) {
        let text = match read_rules_file(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                self.engine.replace_rules(RuleSet::default());
                self.status = Ok("No rules file: every control uses the mappings below.".into());
                return;
            }
            Err(e) => {
                self.status = Err(vec![format!("Can't read {}: {e}", self.path.display())]);
                return;
            }
        };
        match parse_rules(&text) {
            Ok(set) => {
                self.status = Ok(format!("Loaded {} rules and {} pad lights.", set.rule_count(), set.led_count()));
                self.engine.replace_rules(set);
            }
            Err(problems) => self.status = Err(problems.iter().map(ToString::to_string).collect()),
        }
    }

    /// Adds the previewed draft to the rules file: refused on any collision with existing rules or when the file
    /// changed since the preview; the previous file is kept as a backup. Then the rules are reloaded.
    pub fn add_imported(&mut self) {
        let (Some(conversion), Some(snapshot)) = (self.import.draft(), &self.import.snapshot) else { return };
        let count = conversion.rule_count;
        let result = mapping_import::merge_draft(snapshot.as_deref(), conversion.draft.as_deref().unwrap_or_default())
            .map_err(|problems| format!("Not added, nothing changed:\n{}", problems.join("\n")))
            .and_then(|merged| mapping_import::replace_rules_file(&self.path, snapshot.as_deref(), &merged));
        match result {
            Ok(backup) => {
                self.reload();
                let kept = backup.map(|b| format!(" The previous file is kept as {}.", b.display())).unwrap_or_default();
                self.import.message = Some(Ok(format!("Added {count} imported rules to {}.{kept}", self.path.display())));
                self.import.preview = None;
                self.import.snapshot = None;
            }
            Err(e) => self.import.message = Some(Err(e)),
        }
    }

    /// Writes the example only when no file exists yet, then loads it.
    fn create_example(&mut self) {
        let created = self
            .path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::OpenOptions::new().write(true).create_new(true).open(&self.path))
            .and_then(|mut file| file.write_all(EXAMPLE_RULES.as_bytes()));
        match created {
            Ok(()) => self.reload(),
            Err(e) => self.status = Err(vec![format!("Can't create {}: {e}", self.path.display())]),
        }
    }
}

/// The rules file section shown on both controller pages. `ports` are the connected MIDI ports' names.
pub fn show(ui: &mut Ui, rules: &mut RulesFile, last_control: Option<&str>, ports: &[String]) {
    egui::CollapsingHeader::new(RichText::new("Rules file: Shift, layers and macros").size(14.0))
        .id_salt("controller_rules_file")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(
                RichText::new("Rules run before the mappings below; a control with no matching rule keeps its mapping. Edit the file in any editor, then press Reload. Grammar, actions and control IDs: retraktor-cli mapping-reference.")
                    .size(11.5)
                    .color(TraktorTheme::TEXT_SECONDARY()),
            );
            ui.horizontal_wrapped(|ui| {
                let path = rules.path.display().to_string();
                ui.label(RichText::new(&path).monospace().size(11.5).color(TraktorTheme::TEXT_PRIMARY()));
                if ui.button("Copy path").clicked() {
                    ui.ctx().copy_text(path);
                }
                if ui.button("Reload").on_hover_text("Read the file again. If it has a problem, the rules loaded before stay active.").clicked() {
                    rules.reload();
                }
                if !rules.path.exists() && ui.button("Create example file").on_hover_text("Writes the MX2 example, only while there is no file").clicked() {
                    rules.create_example();
                }
            });
            match &rules.status {
                Ok(summary) => {
                    ui.label(RichText::new(summary).size(12.0).color(TraktorTheme::PLAY_ACTIVE()));
                }
                Err(problems) => {
                    let kept = rules.engine.rules().rule_count();
                    ui.label(RichText::new(format!("Not loaded, {} problem(s). The {kept} rules loaded before stay active.", problems.len())).size(12.0).color(TraktorTheme::METER_RED()));
                    for problem in problems.iter().take(12) {
                        ui.label(RichText::new(problem).monospace().size(11.0).color(TraktorTheme::METER_RED()));
                    }
                    if problems.len() > 12 {
                        ui.label(RichText::new(format!("… and {} more: retraktor-cli validate-mapping lists them all.", problems.len() - 12)).size(11.0));
                    }
                }
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("Last control:").size(12.0));
                match last_control {
                    Some(id) => {
                        ui.label(RichText::new(id).monospace().size(12.0).color(TraktorTheme::TEXT_PRIMARY()));
                        if ui.button("Copy").on_hover_text("Copy this ID for a rule's \"input\"").clicked() {
                            ui.ctx().copy_text(id.to_string());
                        }
                    }
                    None => {
                        ui.label(RichText::new("touch a control on any connected controller").size(12.0).color(TraktorTheme::TEXT_MUTED()));
                    }
                }
            });
            let active = rules.engine.active_modifiers();
            let layers = if active.is_empty() { "none".to_string() } else { active.iter().map(|(device, name, value)| format!("{device} {name}={value}")).collect::<Vec<_>>().join(" · ") };
            ui.label(RichText::new(format!("Active layers: {layers}")).size(12.0).color(TraktorTheme::TEXT_SECONDARY()));
        });
    show_import(ui, rules, ports);
    ui.add_space(8.0);
}

fn show_import(ui: &mut Ui, rules: &mut RulesFile, ports: &[String]) {
    let small = |text: &str| RichText::new(text).size(11.5).color(TraktorTheme::TEXT_SECONDARY());
    let import = &mut rules.import;
    // File dialogs run on their own thread so the app keeps drawing while they are open.
    if let Some(rx) = &import.picking {
        match rx.try_recv() {
            Ok(choice) => {
                import.picking = None;
                if let Some(path) = choice {
                    import.open(path);
                }
            }
            Err(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }
    if let Some(rx) = &import.saving {
        match rx.try_recv() {
            Ok(choice) => {
                import.saving = None;
                if let (Some(path), Some(draft)) = (choice, import.draft().and_then(|c| c.draft.clone())) {
                    import.message = Some(match mapping_import::write_new_file(&path, &draft) {
                        Ok(()) => Ok(format!("Draft saved to {}. It is not active until you add it.", path.display())),
                        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(format!("{} already exists and was not overwritten; choose a new name.", path.display())),
                        Err(e) => Err(format!("Can't write {}: {e}", path.display())),
                    });
                }
            }
            Err(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
        }
    }
    egui::CollapsingHeader::new(RichText::new("Import a mapping: Traktor .tsi, Mixxx .midi.xml, rekordbox .midi.csv").size(14.0))
        .id_salt("controller_rules_import")
        .default_open(false)
        .show(ui, |ui| {
            ui.label(small("Converts one device's MIDI mapping into rules and lists every row as translated, approximated, merged or unsupported. Nothing is run from the file, and nothing changes until you add the draft. Traktor's native controllers (such as the X1 MK3) run Traktor's own scripts and have nothing to convert."));
            ui.horizontal_wrapped(|ui| {
                if ui.add_enabled(import.picking.is_none(), egui::Button::new("Choose mapping file…")).clicked() {
                    let (tx, rx) = channel();
                    std::thread::spawn(move || {
                        let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
                        let _ = tx.send(rfd::FileDialog::new().set_directory(home).set_title("Choose a controller mapping").add_filter("Controller mapping (*.tsi, *.midi.xml, *.midi.csv)", &["tsi", "xml", "csv"]).pick_file());
                    });
                    import.picking = Some(rx);
                }
                if let Some((path, _)) = &import.source {
                    ui.label(RichText::new(path.display().to_string()).monospace().size(11.5));
                }
            });
            let Some((_, source)) = &import.source else { return };
            let source = match source {
                Ok(s) => s,
                Err(e) => {
                    ui.label(RichText::new(e).size(12.0).color(TraktorTheme::METER_RED()));
                    return;
                }
            };
            let before = (import.device, import.port.clone());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Device:").size(12.0));
                let name = |n: usize| source.devices.get(n - 1).map_or(String::new(), |d| format!("{n}: {} ({})", d.name, d.detail));
                egui::ComboBox::from_id_salt("import_device").selected_text(name(import.device)).width(420.0).show_ui(ui, |ui| {
                    for n in 1..=source.devices.len() {
                        ui.selectable_value(&mut import.device, n, name(n));
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("MIDI port:").size(12.0)).on_hover_text("The <port> in midi.<port>.ch1.note36: touch a control and read Last control above");
                ui.add(egui::TextEdit::singleline(&mut import.port).desired_width(140.0).hint_text("e.g. xonek2"));
                for port in ports {
                    if ui.selectable_label(import.port == *port, port).on_hover_text("A connected MIDI port").clicked() {
                        import.port = port.clone();
                    }
                }
                if ui.add_enabled(!import.port.trim().is_empty(), egui::Button::new("Preview")).clicked() {
                    import.preview(&rules.path);
                }
            });
            if before != (import.device, import.port.clone()) {
                import.preview = None;
                import.snapshot = None;
            }
            match &import.preview {
                None => {}
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(12.0).color(TraktorTheme::METER_RED()));
                }
                Some(Ok(c)) => {
                    ui.label(RichText::new(c.summary()).size(12.0).color(TraktorTheme::TEXT_PRIMARY()));
                    match (&c.draft, &c.empty_reason) {
                        (Some(_), _) => ui.label(RichText::new(format!("Draft: {} rules, checked by the same validator as the rules file.", c.rule_count)).size(12.0).color(TraktorTheme::PLAY_ACTIVE())),
                        (None, reason) => ui.label(RichText::new(format!("No draft. {}", reason.as_deref().unwrap_or(""))).size(12.0).color(TraktorTheme::METER_RED())),
                    };
                    egui::ScrollArea::vertical().id_salt("import_report").max_height(220.0).show(ui, |ui| {
                        ui.label(RichText::new(&import.report).monospace().size(11.0));
                    });
                    if c.draft.is_some() {
                        ui.horizontal_wrapped(|ui| {
                            if ui.add_enabled(import.saving.is_none(), egui::Button::new("Save draft…")).on_hover_text("Write the draft to a new file; an existing file is never overwritten").clicked() {
                                let (tx, rx) = channel();
                                let name = format!("{}-device{}-rules-draft.json", c.title.split('.').next().unwrap_or("mapping").replace(' ', "-"), c.device);
                                std::thread::spawn(move || {
                                    let _ = tx.send(rfd::FileDialog::new().set_file_name(name).add_filter("Controller rules", &["json"]).save_file());
                                });
                                import.saving = Some(rx);
                            }
                            if ui
                                .button("Add to rules file")
                                .on_hover_text("Add these rules after your own. Refused if a control or modifier is already used, or if the file changed since the preview; the previous file is kept as a backup.")
                                .clicked()
                            {
                                import.add_requested = true;
                            }
                        });
                    }
                }
            }
            match &import.message {
                Some(Ok(m)) => {
                    ui.label(RichText::new(m).size(12.0).color(TraktorTheme::PLAY_ACTIVE()));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(12.0).color(TraktorTheme::METER_RED()));
                }
                None => {}
            }
        });
    if std::mem::take(&mut rules.import.add_requested) {
        rules.add_imported();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An outside edit, then Reload: a good file loads; a broken one reports where and keeps the last good rules.
    #[test]
    fn reload_keeps_the_last_good_rules_when_the_file_is_broken() {
        let dir = std::env::temp_dir().join(format!("retraktor-rules-reload-{}", std::process::id()));
        let path = dir.join("controller-rules.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut rules = RulesFile::load(path.clone());
        assert_eq!(rules.engine.rules().rule_count(), 0);
        assert!(rules.status.as_ref().is_ok_and(|s| s.starts_with("No rules file")));

        rules.create_example();
        assert_eq!(rules.engine.rules().rule_count(), 11, "{:?}", rules.status);
        std::fs::write(&path, "{\"format\": \"retraktor-controller-rules\"}").unwrap();
        rules.create_example(); // never overwrites an existing file
        assert!(std::fs::read_to_string(&path).unwrap().len() < 50, "the example replaced an existing file");

        std::fs::write(&path, EXAMPLE_RULES.replace("\"slot\": 2", "\"slot\": 9").replace("\"filter_on\"", "\"filter_onn\"")).unwrap();
        rules.reload();
        let problems = rules.status.clone().unwrap_err();
        assert!(problems.iter().any(|p| p.contains("rules[2] \"SHIFT + left pad 2 mutes bass\".do[0].slot: 9 is out of range")), "{problems:#?}");
        assert!(problems.iter().any(|p| p.contains("unknown action \"filter_onn\" (did you mean \"filter_on\"?)")), "{problems:#?}");
        assert_eq!(rules.engine.rules().rule_count(), 11, "a broken file replaced the good rules");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Import in the panel: a rekordbox file previewed for a port and added to a missing rules file; Add refused
    /// when the file changed after the preview; added after existing rules with a backup; a repeat refused.
    #[test]
    fn import_previews_adds_and_protects_the_rules_file() {
        let dir = std::env::temp_dir().join(format!("retraktor-rules-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("controller-rules.json");
        let mapping = dir.join("Test.midi.csv");
        std::fs::write(&mapping, "@file,1,Test\r\n#name,function,type,input,deck1,deck2,deck3,deck4,output,deck1,deck2,deck3,deck4,option,comment\r\nPlayPause,PlayPause,Button,900B,0,1,,,,,,,,,Play\r\nCue,Cue,Button,900C,0,1,,,,,,,,,Cue\r\n").unwrap();
        let mut rules = RulesFile::load(path.clone());
        rules.import.open(mapping);
        rules.import.port = "ddj".into();
        rules.import.preview(&rules.path);
        let text = panel_text(&mut rules, None);
        for needle in ["Import a mapping", "rekordbox"] {
            assert!(text.contains(needle), "{needle:?} missing:\n{text}");
        }
        assert!(rules.import.draft().is_some_and(|c| c.rule_count == 2 && c.count(mapping_import::Coverage::Unsupported) == 1));

        rules.add_imported();
        assert!(matches!(&rules.import.message, Some(Ok(m)) if m.starts_with("Added 2 imported rules")), "{:?}", rules.import.message);
        assert_eq!(rules.engine.rules().rule_count(), 2);

        // An outside edit after the preview: Add refuses and leaves the edit alone.
        std::fs::write(&path, EXAMPLE_RULES).unwrap();
        rules.import.preview(&rules.path);
        let edited = format!("{EXAMPLE_RULES}\n");
        std::fs::write(&path, &edited).unwrap();
        rules.add_imported();
        assert!(matches!(&rules.import.message, Some(Err(e)) if e.contains("changed on disk")), "{:?}", rules.import.message);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), edited);

        // Preview again: the draft goes after the example's own rules, with a backup; a second add of the same
        // controls is a collision and writes nothing.
        rules.import.preview(&rules.path);
        rules.add_imported();
        assert!(matches!(&rules.import.message, Some(Ok(m)) if m.contains("The previous file is kept as")), "{:?}", rules.import.message);
        assert_eq!(rules.engine.rules().rule_count(), 13);
        rules.import.preview(&rules.path);
        let before = std::fs::read_to_string(&path).unwrap();
        rules.add_imported();
        assert!(matches!(&rules.import.message, Some(Err(e)) if e.contains("midi.ddj.ch1.note11 already has rules")), "{:?}", rules.import.message);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Every text the panel drew in one offscreen frame.
    fn panel_text(rules: &mut RulesFile, last_control: Option<&str>) -> String {
        fn collect(shape: &egui::Shape, out: &mut String) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push_str(text.galley.text());
                    out.push('\n');
                }
                egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, out)),
                _ => {}
            }
        }
        let ctx = egui::Context::default();
        let mut out = String::new();
        for _ in 0..2 {
            let frame = ctx.run(egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() }, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| show(ui, rules, last_control, &[]));
            });
            out.clear();
            frame.shapes.iter().for_each(|clipped| collect(&clipped.shape, &mut out));
        }
        out
    }

    /// The Preferences panel offscreen, with a temporary rules path: missing file, broken file, loaded file.
    #[test]
    fn preferences_panel_shows_path_status_last_control_and_layers() {
        let dir = std::env::temp_dir().join(format!("retraktor-rules-panel-{}", std::process::id()));
        let path = dir.join("controller-rules.json");
        let _ = std::fs::remove_dir_all(&dir);
        let mut rules = RulesFile::load(path.clone());
        let text = panel_text(&mut rules, Some("midi.xonek2.ch15.note36"));
        for needle in [path.to_str().unwrap(), "Reload", "Create example file", "No rules file", "Last control:", "midi.xonek2.ch15.note36", "Active layers: none"] {
            assert!(text.contains(needle), "{needle:?} missing from the panel:\n{text}");
        }

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, EXAMPLE_RULES.replace("\"deck\": 2", "\"deck\": 9")).unwrap();
        rules.reload();
        let text = panel_text(&mut rules, None);
        assert!(text.contains("Not loaded, 4 problem(s). The 0 rules loaded before stay active."), "{text}");
        assert!(text.contains("do[0].deck: 9 is out of range"), "{text}");
        assert!(!text.contains("Create example file"), "offered to create a file that exists");

        std::fs::write(&path, EXAMPLE_RULES).unwrap();
        rules.reload();
        rules.engine.handle(&retraktor_core::SurfaceEvent::Button { control: "surface.left.shift".into(), pressed: true });
        let text = panel_text(&mut rules, None);
        assert!(text.contains("Loaded 11 rules and 7 pad lights."), "{text}");
        assert!(text.contains("Active layers: mx2 shift=1"), "{text}");
        assert!(text.contains("touch a control on any connected controller"), "{text}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
