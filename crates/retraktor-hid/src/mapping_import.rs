//! Imports controller mappings from Traktor (`.tsi`, Generic MIDI devices), Mixxx (`.midi.xml`) and rekordbox
//! (`.midi.csv`) into a draft controller rules file, with a coverage report that lists every source row as
//! translated, approximated (with what differs), merged into another row's rule, or unsupported (with why).
//!
//! Nothing from the source is executed. Mixxx scripts and Traktor's native controller scripts (CSI QML) never run;
//! a Mixxx script row is only translated when its companion files are known, unmodified versions whose behaviour
//! for that row was read by hand. The draft is checked with the same `parse_rules` the app loads rules with.
//! Preferences > Controllers and `retraktor-cli import-mapping` both use this module.

use crate::controller_rules::{classify_input, device_of, is_midi_cc, parse_rules, read_rules_file, CcMode, Diagnostic, LAYOUTS, RULES_FORMAT, RULES_VERSION};
use retraktor_formats::tsi::base64_decode;
use retraktor_formats::xml::{XmlDocument, XmlNode};
use retraktor_formats::TsiChunk;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::io::{Read as _, Write as _};
use std::path::{Path, PathBuf};

/// Mapping files larger than this are refused unread (a real Traktor settings file is about 1.3 MiB).
const MAX_SOURCE_BYTES: u64 = 32 << 20;
/// A control whose rows need more modifier combinations than this is reported instead of expanded.
const MAX_COMBINATIONS: usize = 64;
/// The step per encoder tick for imported value encoders (the rules file's default).
const ENCODER_STEP: f64 = 0.02;

/// What happened to one source row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Coverage {
    /// Does the same thing in ReTraktor.
    Translated,
    /// Imported, but something differs; the note says what.
    Approximated,
    /// Needs no rule of its own: another row's rule covers it (a note-off, a 14-bit fine half, a hardware Shift).
    Merged,
    /// Not imported; the note says why.
    Unsupported,
}

impl Coverage {
    fn title(self) -> &'static str {
        match self {
            Coverage::Translated => "TRANSLATED",
            Coverage::Approximated => "APPROXIMATED",
            Coverage::Merged => "MERGED (covered by another row)",
            Coverage::Unsupported => "UNSUPPORTED",
        }
    }
}

/// One source row in the coverage report.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub coverage: Coverage,
    /// Where it is in the source, e.g. `row 3`, `control 12`, `line 40 deck 2`.
    pub location: String,
    /// What the row is in the source's own terms.
    pub source: String,
    pub note: String,
}

/// One device (or the single mapping) a source file holds.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceDevice {
    pub name: String,
    pub detail: String,
}

/// A parsed mapping file, ready to convert one of its devices.
#[derive(Debug)]
pub struct Source {
    /// `Traktor .tsi`, `Mixxx MIDI mapping` or `rekordbox MIDI mapping`.
    pub format: &'static str,
    pub title: String,
    pub devices: Vec<SourceDevice>,
    kind: Kind,
}

#[derive(Debug)]
enum Kind {
    Traktor { devices: Vec<TsiDev>, layouts: Vec<(u32, String)> },
    Mixxx(MixxxMap),
    Rekordbox(RekordboxMap),
}

/// The result of converting one device.
#[derive(Debug, Clone)]
pub struct Conversion {
    pub format: &'static str,
    pub title: String,
    /// 1-based device number in the source.
    pub device: usize,
    pub device_name: String,
    /// Destination MIDI port, the `<port>` in `midi.<port>.ch<N>…`.
    pub port: String,
    /// Assignments in the source for this device. A rekordbox row with two deck columns is one assignment but two
    /// report entries, and device-level notes (a native controller, a damaged section) are entries without one.
    pub assignments: usize,
    pub items: Vec<Item>,
    /// Things the imported rules rely on that live outside them.
    pub depends: Vec<String>,
    /// The draft rules file, already validated; `None` when nothing could be converted.
    pub draft: Option<String>,
    pub rule_count: usize,
    /// Why there is no draft.
    pub empty_reason: Option<String>,
}

impl Conversion {
    pub fn count(&self, coverage: Coverage) -> usize {
        self.items.iter().filter(|i| i.coverage == coverage).count()
    }

    /// One-line summary, e.g. `17 source assignments, 17 report entries: 4 translated, 13 approximated, 0 merged, 0 unsupported`.
    pub fn summary(&self) -> String {
        let plural = |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        format!(
            "{}, {}: {} translated, {} approximated, {} merged, {} unsupported",
            plural(self.assignments, "source assignment", "source assignments"),
            plural(self.items.len(), "report entry", "report entries"),
            self.count(Coverage::Translated),
            self.count(Coverage::Approximated),
            self.count(Coverage::Merged),
            self.count(Coverage::Unsupported)
        )
    }

    /// The full coverage report, the same text for the CLI and the app.
    pub fn report(&self) -> String {
        let mut s = String::new();
        let _ = writeln!(s, "ReTraktor mapping import");
        let _ = writeln!(s, "Source:      {} \"{}\"", self.format, self.title);
        let _ = writeln!(s, "Device:      {}: {}", self.device, self.device_name);
        let _ = writeln!(s, "Destination: MIDI port midi.{} (rules use midi.{}.ch<N>.<control>)", self.port, self.port);
        let _ = writeln!(s, "Coverage:    {}", self.summary());
        match (&self.draft, &self.empty_reason) {
            (Some(_), _) => {
                let _ = writeln!(s, "Draft:       {} rules, valid (checked by the validator the app loads rules with)", self.rule_count);
            }
            (None, reason) => {
                let _ = writeln!(s, "Draft:       none. {}", reason.as_deref().unwrap_or("Nothing could be converted."));
            }
        }
        for coverage in [Coverage::Translated, Coverage::Approximated, Coverage::Merged, Coverage::Unsupported] {
            let items: Vec<&Item> = self.items.iter().filter(|i| i.coverage == coverage).collect();
            if items.is_empty() {
                continue;
            }
            let _ = writeln!(s, "\n{} ({})", coverage.title(), items.len());
            for item in items {
                let _ = writeln!(s, "  {}: {}\n      {}", item.location, item.source, item.note);
            }
        }
        if !self.depends.is_empty() {
            let _ = writeln!(s, "\nDEPENDS ON");
            for d in &self.depends {
                let _ = writeln!(s, "  - {d}");
            }
        }
        s
    }
}

/// Checks a destination port name: the `<port>` of `midi.<port>.ch<N>…`, as Last control shows it.
pub fn check_port(port: &str) -> Result<(), String> {
    classify_input(&format!("midi.{port}.ch1.note0")).map(|_| ()).map_err(|e| format!("port: {e}"))
}

/// Reads and parses a mapping file. Companion files (Mixxx scripts) are looked up next to it.
pub fn read_source(path: &Path) -> Result<Source, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(MAX_SOURCE_BYTES + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("can't read {}: {e}", path.display()))?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(format!("{} is larger than 32 MiB; that is not a controller mapping", path.display()));
    }
    let text = String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8 text; expected a Traktor .tsi, a Mixxx .midi.xml or a rekordbox .midi.csv", path.display()))?;
    let title = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    parse_source(&text, &title, path.parent())
}

/// Parses mapping text; `dir` is where Mixxx companion scripts are looked up.
pub fn parse_source(text: &str, title: &str, dir: Option<&Path>) -> Result<Source, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let head = text.trim_start();
    if head.starts_with("@file") {
        let map = parse_rekordbox(text)?;
        let rows = map.rows.len();
        return Ok(Source {
            format: "rekordbox MIDI mapping",
            title: title.into(),
            devices: vec![SourceDevice { name: map.device.clone(), detail: format!("{rows} mapping rows") }],
            kind: Kind::Rekordbox(map),
        });
    }
    // Older Mixxx mappings use the legacy root element, which Mixxx still loads.
    if text.contains("<MixxxControllerPreset") || text.contains("<MixxxMIDIPreset") {
        let map = parse_mixxx(text, dir)?;
        let detail = format!("{} inputs, {} outputs, scripts: {}", map.controls.len(), map.outputs.len(), if map.scripts.is_empty() { "none".into() } else { map.scripts.join(", ") });
        return Ok(Source { format: "Mixxx MIDI mapping", title: title.into(), devices: vec![SourceDevice { name: map.name.clone(), detail }], kind: Kind::Mixxx(map) });
    }
    if text.contains("<TraktorSettings") {
        let (devices, layouts) = parse_traktor(text)?;
        let list = devices
            .iter()
            .map(|d| {
                let name = if d.comment.is_empty() { d.name.clone() } else { format!("{}: {}", d.name, d.comment) };
                let detail = format!("port {}, {} assignments, {} named inputs, {} outputs", if d.in_port.is_empty() { "none" } else { &d.in_port }, d.rows.len(), d.inputs, d.outputs);
                SourceDevice { name, detail }
            })
            .collect();
        return Ok(Source { format: "Traktor .tsi", title: title.into(), devices: list, kind: Kind::Traktor { devices, layouts } });
    }
    Err("not a controller mapping this build reads: expected a Traktor .tsi (TraktorSettings), a Mixxx .midi.xml (MixxxControllerPreset) or a rekordbox .midi.csv (@file)".into())
}

/// Converts device `device` (1-based) of `source` for MIDI port `port`.
pub fn convert(source: &Source, device: usize, port: &str) -> Result<Conversion, String> {
    check_port(port)?;
    let Some(dev) = device.checked_sub(1).and_then(|i| source.devices.get(i)) else {
        return Err(format!("device {device} doesn't exist; this file has {} (list them with --list)", source.devices.len()));
    };
    let mut b = Builder { port: port.into(), ..Default::default() };
    match &source.kind {
        Kind::Traktor { devices, layouts } => convert_traktor(&mut b, &devices[device - 1], layouts),
        Kind::Mixxx(map) => convert_mixxx(&mut b, map),
        Kind::Rekordbox(map) => convert_rekordbox(&mut b, map),
    }
    b.finish(source, device, &dev.name)
}

// ---------------------------------------------------------------------------------------------------------------
// Shared: planned rules, condition grouping, draft text.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InKind {
    Button,
    Fader,
    Encoder(CcMode),
}

#[derive(Debug, Clone)]
struct Planned {
    input: String,
    kind: InKind,
    when: Vec<(String, u8)>,
    actions: Vec<Value>,
    label: String,
    item: usize,
}

/// What one translated row becomes before grouping.
struct Plan {
    kind: InKind,
    actions: Vec<Value>,
    coverage: Coverage,
    note: String,
}

impl Plan {
    fn new(kind: InKind, actions: Vec<Value>, note: impl Into<String>) -> Self {
        Plan { kind, actions, coverage: Coverage::Translated, note: note.into() }
    }

    fn approx(mut self, why: impl Into<String>) -> Self {
        self.coverage = Coverage::Approximated;
        let why = why.into();
        self.note = if self.note.is_empty() { why } else { format!("{}; {why}", self.note) };
        self
    }
}

#[derive(Default)]
struct Builder {
    port: String,
    assignments: usize,
    items: Vec<Item>,
    planned: Vec<Planned>,
    depends: BTreeSet<String>,
    empty_reason: Option<String>,
}

impl Builder {
    fn item(&mut self, coverage: Coverage, location: String, source: String, note: impl Into<String>) -> usize {
        self.items.push(Item { coverage, location, source, note: note.into() });
        self.items.len() - 1
    }

    fn skip(&mut self, location: String, source: String, why: impl Into<String>) {
        self.item(Coverage::Unsupported, location, source, why);
    }

    fn plan(&mut self, location: String, source: String, input: String, when: Vec<(String, u8)>, label: String, plan: Plan) {
        let describe: Vec<String> = plan.actions.iter().map(describe_action).collect();
        let note = if plan.note.is_empty() { format!("-> {input}: {}", describe.join(", ")) } else { format!("-> {input}: {}. {}", describe.join(", "), plan.note) };
        let item = self.item(plan.coverage, location, source, note);
        self.planned.push(Planned { input, kind: plan.kind, when, actions: plan.actions, label, item });
    }

    fn unplan(&mut self, index: usize, why: String) {
        let item = &mut self.items[self.planned[index].item];
        item.coverage = Coverage::Unsupported;
        item.note = why;
    }

    fn input_id(&self, channel: u8, address: Address) -> String {
        let port = &self.port;
        match address {
            Address::Note(n) => format!("midi.{port}.ch{channel}.note{n}"),
            Address::Cc(n) => format!("midi.{port}.ch{channel}.cc{n}"),
            Address::Bend => format!("midi.{port}.ch{channel}.bend"),
        }
    }

    fn finish(mut self, source: &Source, device: usize, device_name: &str) -> Result<Conversion, String> {
        // Group the rows by control, in source order.
        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in self.planned.iter().enumerate() {
            groups.entry(p.input.clone()).or_insert_with(|| {
                order.push(p.input.clone());
                Vec::new()
            });
            groups.get_mut(&p.input).unwrap().push(i);
        }
        let mut dropped: HashSet<usize> = HashSet::new();
        for input in &order {
            let rows = &groups[input];
            if rows.iter().any(|&i| self.planned[i].kind != self.planned[rows[0]].kind) {
                for &i in rows {
                    dropped.insert(i);
                    self.unplan(i, format!("rows on {input} disagree about whether it is a button, a knob or an encoder; none of them is imported"));
                }
            }
        }
        // Drop rows until everything left can be written: conditions on a modifier value no kept row sets (its
        // setter may itself have been dropped), controls needing too many combinations, and whatever the rules
        // validator rejects, one control at a time. Each pass drops at least one row, so this ends.
        let draft = loop {
            let reachable = self.reachable_values(&dropped);
            let mut changed = false;
            for i in 0..self.planned.len() {
                if dropped.contains(&i) {
                    continue;
                }
                let problem = self.planned[i].when.iter().find_map(|(m, v)| match reachable.get(m) {
                    None => Some(format!("depends on modifier {m}, which no imported row on this device sets; an imported mapping must set the modifiers it tests itself")),
                    Some(values) if !values.contains(v) => Some(format!("never runs: no imported row on this device sets {m} to {v}")),
                    _ => None,
                });
                if let Some(why) = problem {
                    dropped.insert(i);
                    self.unplan(i, why);
                    changed = true;
                }
            }
            if changed {
                continue;
            }
            let (mut rules, mut owners) = (Vec::new(), Vec::new());
            let mut encoders: BTreeMap<String, &'static str> = BTreeMap::new();
            let mut cc_buttons: Vec<String> = Vec::new();
            for input in &order {
                let rows: Vec<usize> = groups[input].iter().copied().filter(|i| !dropped.contains(i)).collect();
                if rows.is_empty() {
                    continue;
                }
                match self.group_rules(input, &rows, &reachable) {
                    Ok(group) => {
                        if is_midi_cc(input) {
                            match self.planned[rows[0]].kind {
                                InKind::Button => cc_buttons.push(input.clone()),
                                InKind::Encoder(mode) => {
                                    encoders.insert(input.clone(), encoding_name(mode));
                                }
                                InKind::Fader => {}
                            }
                        }
                        owners.extend(std::iter::repeat_n(input.clone(), group.len()));
                        rules.extend(group);
                    }
                    Err(why) => {
                        for i in rows {
                            dropped.insert(i);
                            self.unplan(i, why.clone());
                        }
                        changed = true;
                    }
                }
            }
            if changed {
                continue;
            }
            if rules.is_empty() {
                break None;
            }
            cc_buttons.sort();
            let text = draft_text(source, device, device_name, &encoders, &cc_buttons, &rules);
            match parse_rules(&text) {
                Ok(set) => break Some((text, set.rule_count())),
                Err(problems) => {
                    let mut progress = false;
                    for (input, why) in rejected_inputs(&problems, &owners, &cc_buttons) {
                        for &i in groups.get(&input).into_iter().flatten() {
                            if dropped.insert(i) {
                                self.unplan(i, format!("the rules validator rejected this control: {why}"));
                                progress = true;
                            }
                        }
                    }
                    if !progress {
                        let list: Vec<String> = problems.iter().take(5).map(ToString::to_string).collect();
                        return Err(format!("internal error: the converted draft failed validation ({}); please report this", list.join("; ")));
                    }
                }
            }
        };

        let mut conversion = Conversion {
            format: source.format,
            title: source.title.clone(),
            device,
            device_name: device_name.to_string(),
            port: self.port.clone(),
            assignments: self.assignments,
            items: self.items,
            depends: self.depends.into_iter().collect(),
            draft: None,
            rule_count: 0,
            empty_reason: self.empty_reason,
        };
        match draft {
            Some((text, count)) => {
                conversion.draft = Some(text);
                conversion.rule_count = count;
            }
            None if conversion.empty_reason.is_none() => {
                conversion.empty_reason = Some(if conversion.items.is_empty() { "The source has no assignments for this device.".into() } else { "None of its rows has a ReTraktor equivalent (see UNSUPPORTED).".into() });
            }
            None => {}
        }
        Ok(conversion)
    }

    /// The rules for one control. All of its rows whose conditions hold before the event run together, as in
    /// Traktor, so each combination of the modifiers its rows test becomes one rule with every matching row's
    /// actions. More specific combinations come first, and a combination no row matches gets no rule.
    fn group_rules(&self, input: &str, rows: &[usize], reachable: &BTreeMap<String, BTreeSet<u8>>) -> Result<Vec<String>, String> {
        let mut tested: BTreeMap<&str, BTreeSet<u8>> = BTreeMap::new();
        for &i in rows {
            for (m, v) in &self.planned[i].when {
                tested.entry(m.as_str()).or_default().insert(*v);
            }
        }
        // Each tested modifier takes one of its tested values, or "anything else" (None) when other values exist.
        let mut axes: Vec<(&str, Vec<Option<u8>>)> = Vec::new();
        let mut combinations: usize = 1;
        for (m, values) in &tested {
            let mut choices: Vec<Option<u8>> = values.iter().map(|v| Some(*v)).collect();
            if reachable.get(*m).is_none_or(|all| all.iter().any(|v| !values.contains(v))) {
                choices.push(None);
            }
            combinations = combinations.saturating_mul(choices.len());
            axes.push((m, choices));
        }
        if combinations > MAX_COMBINATIONS {
            return Err(format!("the rows on {input} test {} modifiers in {combinations} combinations (limit {MAX_COMBINATIONS}); not imported", axes.len()));
        }
        let mut combos: Vec<Vec<(&str, Option<u8>)>> = vec![Vec::new()];
        for (m, choices) in &axes {
            combos = combos.into_iter().flat_map(|c| choices.iter().map(move |v| [c.clone(), vec![(*m, *v)]].concat())).collect();
        }
        combos.sort_by_key(|c| std::cmp::Reverse(c.iter().filter(|(_, v)| v.is_some()).count()));
        let mut out = Vec::new();
        for combo in combos {
            let matching: Vec<&Planned> = rows
                .iter()
                .map(|&i| &self.planned[i])
                .filter(|p| p.when.iter().all(|(m, v)| combo.iter().any(|(cm, cv)| cm == m && *cv == Some(*v))))
                .collect();
            if matching.is_empty() {
                continue;
            }
            let when: Map<String, Value> = combo.iter().filter_map(|(m, v)| v.map(|v| (m.to_string(), Value::from(v)))).collect();
            let actions: Vec<Value> = matching.iter().flat_map(|p| p.actions.iter().cloned()).collect();
            let mut labels: Vec<&str> = Vec::new();
            for p in &matching {
                if !labels.contains(&p.label.as_str()) {
                    labels.push(&p.label);
                }
            }
            let joined = labels.join(" + ");
            let name: String = if joined.chars().count() > 120 { joined.chars().take(119).chain(['…']).collect() } else { joined };
            let locations: Vec<&str> = matching.iter().map(|p| self.items[p.item].location.as_str()).collect();
            let mut rule = format!("{{\"name\": {}, \"input\": {}", Value::String(name), Value::String(input.into()));
            if !when.is_empty() {
                let _ = write!(rule, ", \"when\": {}", Value::Object(when));
            }
            let _ = write!(rule, ", \"do\": {}, \"comment\": {}}}", Value::Array(actions), Value::String(format!("from {}", locations.join(", "))));
            out.push(rule);
        }
        Ok(out)
    }

    fn reachable_values(&self, dropped: &HashSet<usize>) -> BTreeMap<String, BTreeSet<u8>> {
        let mut reachable: BTreeMap<String, BTreeSet<u8>> = BTreeMap::new();
        for p in self.planned.iter().enumerate().filter(|(i, _)| !dropped.contains(i)).map(|(_, p)| p) {
            for a in p.actions.iter().filter(|a| a["action"] == "modifier") {
                let set = reachable.entry(a["name"].as_str().unwrap_or_default().to_string()).or_insert_with(|| BTreeSet::from([0]));
                set.insert(a.get("value").and_then(Value::as_u64).unwrap_or(1) as u8);
            }
        }
        reachable
    }
}

/// The draft rules file: one rule per line, so it reads well and diffs cleanly.
fn draft_text(source: &Source, device: usize, device_name: &str, encoders: &BTreeMap<String, &str>, cc_buttons: &[String], rules: &[String]) -> String {
    let mut text = String::from("{\n");
    let _ = writeln!(text, "  \"format\": \"{RULES_FORMAT}\",\n  \"version\": {RULES_VERSION},");
    let comment = format!("Imported from {} \"{}\", device {device} ({device_name}). Review the coverage report before use.", source.format, source.title);
    let _ = writeln!(text, "  \"comment\": {},", Value::String(comment));
    if !encoders.is_empty() {
        let map: Map<String, Value> = encoders.iter().map(|(k, v)| (k.clone(), Value::String((*v).into()))).collect();
        let _ = writeln!(text, "  \"midi_encoders\": {},", Value::Object(map));
    }
    if !cc_buttons.is_empty() {
        let _ = writeln!(text, "  \"midi_cc_buttons\": {},", json!(cc_buttons));
    }
    let _ = write!(text, "  \"rules\": [\n    {}\n  ]\n}}\n", rules.join(",\n    "));
    text
}

/// The control each validator problem belongs to, with the first problem's text: a rule (`rules[7] …`, through
/// `owners`) or a CC declaration. Problems with no control (file level) aren't listed.
fn rejected_inputs(problems: &[Diagnostic], owners: &[String], cc_buttons: &[String]) -> BTreeMap<String, String> {
    let index = |rest: &str| rest.split(']').next().and_then(|n| n.parse::<usize>().ok());
    let mut out = BTreeMap::new();
    for d in problems {
        let input = if let Some(rest) = d.location.strip_prefix("rules[") {
            index(rest).and_then(|n| owners.get(n)).cloned()
        } else if let Some(rest) = d.location.strip_prefix("midi_encoders.\"") {
            rest.split('"').next().map(str::to_string)
        } else if let Some(rest) = d.location.strip_prefix("midi_cc_buttons[") {
            index(rest).and_then(|n| cc_buttons.get(n)).cloned()
        } else {
            None
        };
        if let Some(input) = input {
            out.entry(input).or_insert_with(|| d.message.clone());
        }
    }
    out
}

fn encoding_name(mode: CcMode) -> &'static str {
    match mode {
        CcMode::Offset64 => "offset_64",
        CcMode::SignBit => "sign_bit",
        _ => "twos_complement",
    }
}

fn describe_action(a: &Value) -> String {
    let mut s = a["action"].as_str().unwrap_or("?").to_string();
    for key in ["deck", "unit", "slot", "mode", "name", "value", "range", "step"] {
        if let Some(v) = a.get(key) {
            let _ = write!(s, " {key}={v}");
        }
    }
    s
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Address {
    Note(u8),
    Cc(u8),
    Bend,
}

fn deck_action(action: &str, deck: u64) -> Value {
    json!({"action": action, "deck": deck})
}

/// A value action for a knob/fader (`range`, inverted with `[1, 0]`), an encoder (`step`) or a button (`value`).
fn value_action(action: &str, deck: Option<u64>, kind: InKind, invert: bool, button_value: Option<f64>) -> Value {
    let mut a = json!({"action": action});
    if let Some(deck) = deck {
        a["deck"] = deck.into();
    }
    match kind {
        InKind::Fader if invert => a["range"] = json!([1, 0]),
        InKind::Encoder(_) => a["step"] = json!(if invert { -ENCODER_STEP } else { ENCODER_STEP }),
        InKind::Button => a["value"] = json!(button_value.unwrap_or(0.0)),
        InKind::Fader => {}
    }
    a
}

const FILTER_DEPENDS: &str = "filter: the channel filter is heard only while it is on (filter_on); ReTraktor's channel knob is always the filter, not another mixer or quick effect";
const HOTCUE_NOTE: &str = "ReTraktor jumps to the hot cue, or stores it there when it is empty; the source's play-while-held behaviour on a stopped deck isn't reproduced";

// ---------------------------------------------------------------------------------------------------------------
// Traktor .tsi

#[derive(Debug, Default)]
struct TsiDev {
    name: String,
    comment: String,
    in_port: String,
    inputs: usize,
    outputs: usize,
    /// `encoderMode` of each input definition (DCDT), by MIDI address name.
    encoder_modes: HashMap<String, u32>,
    bindings: HashMap<u32, String>,
    rows: Vec<TsiRow>,
    problems: Vec<String>,
}

#[derive(Debug)]
struct TsiRow {
    binding: u32,
    io: u32,
    command: u32,
    cmad: Vec<u8>,
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// A Traktor wide string (u32 character count, then UTF-16BE) at `at`, and the offset after it.
fn wide(b: &[u8], at: usize) -> Option<(String, usize)> {
    let n = be_u32(b, at)? as usize;
    let end = at.checked_add(4)?.checked_add(n.checked_mul(2)?)?;
    let units: Vec<u16> = b.get(at + 4..end)?.chunks_exact(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
    Some((String::from_utf16_lossy(&units), end))
}

fn parse_traktor(text: &str) -> Result<(Vec<TsiDev>, Vec<(u32, String)>), String> {
    let doc = XmlDocument::parse(text).map_err(|e| format!("not a readable Traktor settings file: {e}"))?;
    let (mut controller, mut layout_list) = (None, None);
    let mut pending: Vec<&XmlNode> = vec![&doc.root];
    while let Some(node) = pending.pop() {
        if node.name == "Entry" {
            match node.attr("Name") {
                Some("DeviceIO.Config.Controller") => controller = node.attr("Value"),
                Some("Gui.Layouts2") => layout_list = node.attr("Value"),
                _ => {}
            }
        }
        pending.extend(node.children.iter());
    }
    let blob = controller.ok_or("this Traktor settings file has no controller section (DeviceIO.Config.Controller)")?;
    let chunks = base64_decode(blob).and_then(|b| TsiChunk::parse_stream(&b)).map_err(|e| format!("the controller section can't be decoded: {e}"))?;
    let mut devices = Vec::new();
    collect_devices(&chunks, &mut devices);
    // "Regular2 0 …;Regular4 1 …": each layout's name and index, as this file names them.
    let layouts = layout_list
        .map(|v| {
            v.split(';')
                .filter_map(|entry| {
                    let mut words = entry.split_whitespace();
                    let name = words.next()?.to_string();
                    Some((words.next()?.parse().ok()?, name))
                })
                .collect()
        })
        .unwrap_or_default();
    Ok((devices, layouts))
}

fn collect_devices(chunks: &[TsiChunk], out: &mut Vec<TsiDev>) {
    for c in chunks {
        if &c.fourcc == b"DEVI" {
            let mut dev = TsiDev { name: wide(&c.payload, 0).map(|w| w.0).unwrap_or_default(), ..Default::default() };
            walk_device(&c.children, &mut dev);
            out.push(dev);
        } else {
            collect_devices(&c.children, out);
        }
    }
}

fn walk_device(chunks: &[TsiChunk], dev: &mut TsiDev) {
    for c in chunks {
        let declared = || be_u32(&c.payload, 0).unwrap_or(0) as usize;
        match &c.fourcc {
            b"DDIC" => dev.comment = wide(&c.payload, 0).map(|w| w.0).unwrap_or_default(),
            b"DDPT" => dev.in_port = wide(&c.payload, 0).map(|w| w.0).unwrap_or_default(),
            b"DDCI" => {
                dev.inputs = c.children.len();
                for def in &c.children {
                    // DCDT: name, control type, min, max (floats), encoder mode, control id.
                    if let Some((name, end)) = wide(&def.payload, 0) {
                        if let Some(mode) = be_u32(&def.payload, end + 12) {
                            dev.encoder_modes.insert(name, mode);
                        }
                    }
                }
            }
            b"DDCO" => dev.outputs = c.children.len(),
            b"CMAS" => {
                if c.children.len() != declared() {
                    dev.problems.push(format!("the assignment list declares {} entries but {} could be read", declared(), c.children.len()));
                }
                for cmai in c.children.iter().filter(|x| &x.fourcc == b"CMAI") {
                    let (Some(binding), Some(io), Some(command)) = (be_u32(&cmai.payload, 0), be_u32(&cmai.payload, 4), be_u32(&cmai.payload, 8)) else {
                        dev.problems.push("an assignment is shorter than its 12-byte header".into());
                        continue;
                    };
                    let cmad = cmai.children.iter().find(|x| &x.fourcc == b"CMAD").map(|x| x.payload.clone()).unwrap_or_default();
                    dev.rows.push(TsiRow { binding, io, command, cmad });
                }
                continue;
            }
            b"DCBM" => {
                if c.children.len() != declared() {
                    dev.problems.push(format!("the MIDI binding list declares {} entries but {} could be read", declared(), c.children.len()));
                }
                for b in &c.children {
                    if let (Some(id), Some((name, _))) = (be_u32(&b.payload, 0), wide(&b.payload, 4)) {
                        dev.bindings.insert(id, name);
                    }
                }
                continue;
            }
            _ => {}
        }
        walk_device(&c.children, dev);
    }
}

/// The settings of one Traktor assignment (CMAD).
struct Cmad {
    control: u32,
    mode: u32,
    target: i32,
    auto_repeat: bool,
    invert: bool,
    soft_takeover: bool,
    sensitivity: f32,
    set_raw: u32,
    comment: String,
    /// Up to two conditions: (Traktor control id, value).
    conditions: Vec<(u32, u32)>,
}

fn read_cmad(b: &[u8]) -> Option<Cmad> {
    let (comment, end) = wide(b, 48)?;
    let mut conditions = Vec::new();
    for at in [end, end + 12] {
        let id = be_u32(b, at)?;
        if id != 0 {
            conditions.push((id, be_u32(b, at + 8)?));
        }
    }
    Some(Cmad {
        control: be_u32(b, 4)?,
        mode: be_u32(b, 8)?,
        target: be_u32(b, 12)? as i32,
        auto_repeat: be_u32(b, 16)? != 0,
        invert: be_u32(b, 20)? != 0,
        soft_takeover: be_u32(b, 24)? != 0,
        sensitivity: f32::from_bits(be_u32(b, 28)?),
        set_raw: be_u32(b, 44)?,
        comment,
        conditions,
    })
}

const TRAKTOR_MODES: [&str; 9] = ["Trigger", "Toggle", "Hold", "Direct", "Relative", "Inc", "Dec", "Reset", "Output"];

fn traktor_control_name(control: u32) -> &'static str {
    match control {
        0 => "Button",
        1 => "Fader/Knob",
        2 => "Encoder",
        65535 => "LED",
        _ => "unknown control type",
    }
}

/// Traktor command names for the commands this importer knows (ids as Traktor stores them).
fn traktor_command_name(id: u32) -> Option<&'static str> {
    Some(match id {
        5 => "Crossfader Position",
        6 => "Master Volume Adjust",
        8 => "Monitor Volume Adjust",
        17 => "Monitor Mix Adjust",
        100 => "Play/Pause",
        102 => "Volume Adjust",
        117 => "Gain Adjust",
        119 => "Monitor Cue On",
        125 => "Sync On",
        206 => "Cue",
        301 => "Low Adjust",
        302 => "Mid Adjust",
        303 => "High Adjust",
        316 => "Mid Low Adjust",
        319 => "Mixer FX On",
        320 => "Mixer FX Adjust",
        400 => "Keylock On",
        2293 => "Set As Tempo Master",
        2548..=2555 => ["Modifier #1", "Modifier #2", "Modifier #3", "Modifier #4", "Modifier #5", "Modifier #6", "Modifier #7", "Modifier #8"][(id - 2548) as usize],
        3076 => "Load Selected",
        4162 => "Waveform Zoom Adjust",
        4208 => "Layout Selector",
        4209 => "Only Browser On",
        _ => return None,
    })
}

/// `Ch14.Note.C#0` (C-1 is note 0, so C#0 is 13) or `Ch01.CC.008`.
fn traktor_address(name: &str) -> Option<(u8, Address)> {
    let (channel, rest) = name.strip_prefix("Ch")?.split_once('.')?;
    let channel: u8 = channel.parse().ok().filter(|c| (1..=16).contains(c))?;
    if let Some(cc) = rest.strip_prefix("CC.") {
        return Some((channel, Address::Cc(cc.parse().ok().filter(|c: &u8| *c <= 127)?)));
    }
    let note = rest.strip_prefix("Note.")?;
    let split = if note.as_bytes().get(1) == Some(&b'#') { 2 } else { 1 };
    let (pitch, octave) = (note.get(..split)?, note.get(split..)?);
    let index = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"].iter().position(|p| *p == pitch)? as i32;
    let number = (octave.parse::<i32>().ok()? + 1) * 12 + index;
    Some((channel, Address::Note(u8::try_from(number).ok().filter(|n| *n <= 127)?)))
}

fn convert_traktor(b: &mut Builder, dev: &TsiDev, layouts: &[(u32, String)]) {
    b.assignments = dev.rows.len();
    // Modifiers this mapping has an input assignment for, supported or not. Each mapping owns its modifiers, so
    // one nothing assigns stays 0 for good.
    let assigned: HashSet<u32> = dev.rows.iter().filter(|r| r.io == 0 && (2548..=2555).contains(&r.command)).map(|r| r.command).collect();
    for problem in &dev.problems {
        b.skip("device".into(), dev.name.clone(), format!("damaged section: {problem}"));
    }
    if dev.name != "Generic MIDI" {
        let note = "a native Native Instruments controller: Traktor runs it with its built-in controller script (CSI QML under Traktor's Resources64/qml/CSI), which this .tsi doesn't contain and ReTraktor can't run, and the .tsi names its controls without their HID report positions. Custom QML behaviour needs a separate port. Only its buttons can be mapped by hand today (Preferences > Other controllers shows a pressed button's ID under Last control): generic HID can report unrelated bit changes for knobs, and ReTraktor has no verified decoder for its encoders";
        if dev.rows.is_empty() {
            b.skip("device".into(), format!("{}: {} named inputs, {} outputs, 0 assignments", dev.name, dev.inputs, dev.outputs), note);
            b.empty_reason = Some(format!("{} is a native controller driven by Traktor's built-in QML script; the .tsi holds no assignments to convert.", dev.name));
        }
        for (i, row) in dev.rows.iter().enumerate() {
            let name = dev.bindings.get(&row.binding).cloned().unwrap_or_else(|| format!("binding {}", row.binding));
            b.skip(format!("row {i}"), format!("{name}, command {}", row.command), format!("assignment on {note}"));
        }
        return;
    }
    let four_band = dev.rows.iter().any(|r| r.command == 316);
    for (i, row) in dev.rows.iter().enumerate() {
        let location = format!("row {i}");
        let address_name = dev.bindings.get(&row.binding).cloned().unwrap_or_else(|| format!("binding {}", row.binding));
        let command = traktor_command_name(row.command).map_or_else(|| format!("command {}", row.command), |n| format!("{n} ({})", row.command));
        let Some(c) = read_cmad(&row.cmad) else {
            b.skip(location, format!("{address_name} {command}"), "its settings (CMAD) are truncated or damaged");
            continue;
        };
        let mode = TRAKTOR_MODES.get(c.mode as usize).copied().unwrap_or("unknown mode");
        let target = match c.target {
            -1 => "device target".to_string(),
            t @ 0..=3 => format!("deck {}", ["A", "B", "C", "D"][t as usize]),
            t => format!("target {t}"),
        };
        let conditions: Vec<String> = c.conditions.iter().map(|(id, v)| if (2548..=2555).contains(id) { format!("M{}={v}", id - 2547) } else { format!("control {id}={v}") }).collect();
        let mut source = format!("{address_name} {command}, {} {mode}, {target}", traktor_control_name(c.control));
        if !conditions.is_empty() {
            let _ = write!(source, ", if {}", conditions.join(" and "));
        }
        if !c.comment.is_empty() {
            let _ = write!(source, " \"{}\"", c.comment);
        }
        if row.io == 1 || c.control == 65535 || c.mode == 8 {
            b.skip(location, source, "LED or other output to the controller; ReTraktor sends nothing to MIDI controllers");
            continue;
        }
        let Some((channel, address)) = traktor_address(&address_name) else {
            b.skip(location, source, format!("MIDI address \"{address_name}\" isn't a note or CC this importer reads"));
            continue;
        };
        let mut when = Vec::new();
        let mut bad_condition = None;
        for (id, value) in &c.conditions {
            match (id, value) {
                // Nothing in the mapping sets this modifier: "= 0" always holds, any other value never does.
                (2548..=2555, 0) if !assigned.contains(id) => {}
                (2548..=2555, 1..=7) if !assigned.contains(id) => bad_condition = Some(format!("never runs: it tests M{} = {value}, but nothing in this mapping sets M{}, so it stays 0", id - 2547, id - 2547)),
                (2548..=2555, 0..=7) => when.push((format!("m{}", id - 2547), *value as u8)),
                (2548..=2555, v) => bad_condition = Some(format!("tests M{} = {v}; modifiers run 0 to 7", id - 2547)),
                (other, _) => bad_condition = Some(format!("tests Traktor control {other}, which isn't a modifier")),
            }
        }
        if let Some(why) = bad_condition {
            b.skip(location, source, why);
            continue;
        }
        if c.control == 0 && (c.auto_repeat || c.invert) {
            let what = if c.auto_repeat { "repeats while held (Auto Repeat)" } else { "is inverted (acts as if pressed while released)" };
            b.skip(location, source, format!("the button {what}; ReTraktor buttons act once per press, so it isn't imported"));
            continue;
        }
        let encoder = || match dev.encoder_modes.get(&address_name) {
            Some(1) => Ok(CcMode::TwosComplement),
            Some(m) => Err(format!("its encoder mode ({m}) isn't verified; only Traktor's 7Fh/01h (mode 1) is")),
            None => Err("the file has no definition for this control, so its encoder mode is unknown".into()),
        };
        match traktor_plan(row.command, &c, address, four_band, layouts, encoder) {
            Ok(mut plan) => {
                if matches!(plan.kind, InKind::Fader | InKind::Encoder(_)) && !matches!(address, Address::Cc(_)) {
                    b.skip(location, source, "a note can't drive a knob, fader or encoder value");
                    continue;
                }
                if c.soft_takeover {
                    plan = plan.approx("Traktor's soft takeover isn't reproduced: the value jumps to the control's position");
                }
                if row.command == 320 {
                    b.depends.insert(FILTER_DEPENDS.into());
                }
                let label = if c.comment.is_empty() { command.clone() } else { c.comment.clone() };
                let input = b.input_id(channel, address);
                b.plan(location, source, input, when, label, plan);
            }
            Err(why) => b.skip(location, source, why),
        }
    }
}

fn traktor_plan(cmd: u32, c: &Cmad, address: Address, four_band: bool, layouts: &[(u32, String)], encoder: impl Fn() -> Result<CcMode, String>) -> Result<Plan, String> {
    let deck = || match c.target {
        t @ 0..=3 => Ok(t as u64 + 1),
        -1 => Err("is assigned to the focused deck (Device Target); ReTraktor rules name a deck".to_string()),
        t => Err(format!("targets {t} (a remix slot or another unit), which ReTraktor has no equivalent for")),
    };
    let need = |control: u32, mode: u32| {
        if c.control == control && c.mode == mode {
            Ok(())
        } else {
            Err(format!(
                "{} as {} {} isn't supported; ReTraktor imports it as {} {}",
                traktor_command_name(cmd).unwrap_or("this command"),
                traktor_control_name(c.control),
                TRAKTOR_MODES.get(c.mode as usize).unwrap_or(&"?"),
                traktor_control_name(control),
                TRAKTOR_MODES[mode as usize]
            ))
        }
    };
    match cmd {
        100 | 125 | 400 | 119 | 319 => {
            need(0, 1)?;
            let action = match cmd {
                100 => "play",
                125 => "sync",
                400 => "keylock",
                119 => "headphone",
                _ => "filter_on",
            };
            Ok(Plan::new(InKind::Button, vec![deck_action(action, deck()?)], ""))
        }
        2293 | 3076 => {
            need(0, 0)?;
            Ok(Plan::new(InKind::Button, vec![deck_action(if cmd == 2293 { "tempo_master" } else { "load" }, deck()?)], ""))
        }
        2548..=2555 => {
            let name = format!("m{}", cmd - 2547);
            let value = c.set_raw;
            if c.control != 0 || value > 7 {
                return Err(format!("a modifier needs a button and a value from 0 to 7 (this row: {}, value {value})", traktor_control_name(c.control)));
            }
            let mode = match (c.mode, value) {
                (3, _) => "set",
                (2, 1..) => "hold",
                (1, 1..) => "toggle",
                _ => return Err(format!("modifier as {} with value {value} isn't supported", TRAKTOR_MODES.get(c.mode as usize).unwrap_or(&"?"))),
            };
            Ok(Plan::new(InKind::Button, vec![json!({"action": "modifier", "name": name, "mode": mode, "value": value})], ""))
        }
        4208 => {
            need(0, 3)?;
            let Some((_, name)) = layouts.iter().find(|(index, _)| *index == c.set_raw) else {
                return Err(format!("selects layout {}, which this file's layout list (Gui.Layouts2) doesn't name", c.set_raw));
            };
            let mode = name.to_lowercase();
            if !LAYOUTS.contains(&mode.as_str()) {
                return Err(format!("selects layout \"{name}\"; only layouts named Regular2, Regular4, Stacked2 and Stacked4 have a ReTraktor equivalent"));
            }
            let shown = match mode.as_str() {
                "regular2" => "2 Decks with Waveforms In Deck: decks A and B either side of the mixer, each with its own waveform",
                "regular4" => "4 Decks with Waveforms In Deck: decks A and B over C and D either side of a four-channel mixer, each with its own waveform",
                "stacked2" => "2 Decks with Waveforms Stacked: two waveforms stacked full width above decks A and B and the mixer",
                _ => "4 Decks with Waveforms Stacked: four waveforms stacked full width above decks A and B over C and D and a four-channel mixer",
            };
            Ok(Plan::new(InKind::Button, vec![json!({"action": "layout", "mode": mode})], format!("layout \"{name}\" (index {}) from this file's layout list", c.set_raw))
                .approx(format!("ReTraktor shows {shown}; the panels and sizes differ from Traktor's")))
        }
        4209 => {
            need(0, 1)?;
            Ok(Plan::new(InKind::Button, vec![json!({"action": "browser_only"})], "")
                .approx("ReTraktor's Browser layout keeps one-line strips of decks A and B above the library; the second press returns to the layout before"))
        }
        4162 => {
            need(2, 4)?;
            if c.invert {
                return Err("an inverted zoom encoder isn't supported: ReTraktor's waveform_zoom always zooms in when turned right".into());
            }
            let mode = encoder()?;
            let deck = deck()?;
            Ok(Plan::new(InKind::Encoder(mode), vec![deck_action("waveform_zoom", deck)], "encoder read as two's complement (Traktor 7Fh/01h); turning right zooms in")
                .approx(format!("ReTraktor zooms in fixed steps, 4 ticks doubling or halving the seconds shown (1/4 to 4 times); Traktor's zoom levels, sensitivity ({}) and acceleration aren't reproduced", c.sensitivity)))
        }
        301 | 302 | 303 | 320 | 102 | 117 | 5 | 6 | 8 | 17 => {
            let (target, per_deck) = match cmd {
                301 => ("eq_low", true),
                302 => ("eq_mid", true),
                303 => ("eq_hi", true),
                320 => ("filter", true),
                102 => ("volume", true),
                117 => ("gain", true),
                5 => ("xfader", false),
                6 => ("master_volume", false),
                8 => ("cue_vol", false),
                _ => ("cue_mix", false),
            };
            let deck = if per_deck { Some(deck()?) } else { None };
            let (kind, button_value, mut note) = match (c.control, c.mode, address) {
                (1, 3, _) => (InKind::Fader, None, String::new()),
                (2, 4, _) => (InKind::Encoder(encoder()?), None, format!("encoder read as two's complement (Traktor 7Fh/01h)")),
                (0, 3, _) => {
                    let v = f32::from_bits(c.set_raw) as f64;
                    if !(0.0..=1.0).contains(&v) {
                        return Err(format!("sets the value to {v}, outside 0 to 1"));
                    }
                    (InKind::Button, Some((v * 1000.0).round() / 1000.0), String::new())
                }
                _ => return Err(format!("{} as {} {} isn't supported", traktor_command_name(cmd).unwrap_or("this command"), traktor_control_name(c.control), TRAKTOR_MODES.get(c.mode as usize).unwrap_or(&"?"))),
            };
            if cmd == 320 {
                note = [note, "Traktor's Mixer FX knob becomes the channel filter (the same knob when Traktor's mixer FX is the filter)".into()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("; ");
            }
            let mut plan = Plan::new(kind, vec![value_action(target, deck, kind, c.invert, button_value)], note);
            if matches!(kind, InKind::Encoder(_)) {
                plan = plan.approx(format!("{ENCODER_STEP} per tick; Traktor's sensitivity ({}) and acceleration aren't reproduced", c.sensitivity));
            }
            Ok(if cmd == 302 && four_band { plan.approx("this device also maps Traktor's Mid Low band, so it uses the four-band Xone EQ, where Mid is the upper mid; ReTraktor's EQ has one mid band") } else { plan })
        }
        316 => Err("ReTraktor's EQ has three bands; Traktor's Mid Low band (four-band Xone EQ) has no equivalent".into()),
        206 => Err("Traktor's CUE (set or return to the cue point, hold to preview) has no ReTraktor equivalent; ReTraktor's cue action jumps to hotcue 1 instead".into()),
        _ => Err(format!("{} has no ReTraktor action", traktor_command_name(cmd).map_or_else(|| format!("Traktor command {cmd}"), str::to_string))),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Mixxx .midi.xml

#[derive(Debug)]
struct MixxxMap {
    name: String,
    controls: Vec<MixxxControl>,
    outputs: Vec<MixxxControl>,
    scripts: Vec<String>,
    dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct MixxxControl {
    group: String,
    key: String,
    status: String,
    midino: String,
    options: Vec<String>,
}

fn parse_mixxx(text: &str, dir: Option<&Path>) -> Result<MixxxMap, String> {
    let doc = XmlDocument::parse(text).map_err(|e| format!("not readable XML: {e}"))?;
    let root = &doc.root;
    if root.name != "MixxxControllerPreset" && root.name != "MixxxMIDIPreset" {
        return Err(format!("the root element is <{}>, not <MixxxControllerPreset>", root.name));
    }
    let text_of = |node: &XmlNode, name: &str| node.child(name).map(|c| c.text.trim().to_string()).unwrap_or_default();
    let name = root.child("info").map(|i| text_of(i, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| "Mixxx mapping".into());
    let controller = root.child("controller").ok_or("the mapping has no <controller> element")?;
    let read = |node: &XmlNode| MixxxControl {
        group: text_of(node, "group"),
        key: text_of(node, "key"),
        status: text_of(node, "status"),
        midino: text_of(node, "midino"),
        options: node.child("options").map(|o| o.children.iter().map(|c| c.name.to_lowercase()).collect()).unwrap_or_default(),
    };
    let controls = controller.child("controls").map(|c| c.children_named("control").map(read).collect()).unwrap_or_default();
    let outputs = controller.child("outputs").map(|c| c.children_named("output").map(read).collect()).unwrap_or_default();
    let scripts = controller.child("scriptfiles").map(|s| s.children_named("file").filter_map(|f| f.attr("filename").map(str::to_string)).collect()).unwrap_or_default();
    Ok(MixxxMap { name, controls, outputs, scripts, dir: dir.map(Path::to_path_buf) })
}

fn hex_or_decimal(s: &str) -> Option<u8> {
    let s = s.trim();
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(hex) => u8::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn mixxx_deck(group: &str) -> Option<u64> {
    let inner = group.strip_prefix("[EqualizerRack1_").and_then(|g| g.strip_suffix("_Effect1]")).or_else(|| group.strip_prefix("[QuickEffectRack1_").and_then(|g| g.strip_suffix(']'))).unwrap_or(group);
    let n: u64 = inner.strip_prefix("[Channel")?.strip_suffix(']')?.parse().ok()?;
    (1..=4).contains(&n).then_some(n)
}

#[derive(Clone, Copy, PartialEq)]
enum Role {
    Button,
    Value,
    Browse,
}

/// The ReTraktor action for a Mixxx group and key: role, action, deck, extra note or approximation.
fn mixxx_target(group: &str, key: &str) -> Result<(Role, &'static str, Option<u64>, Option<u64>, Option<&'static str>), String> {
    let deck = mixxx_deck(group);
    let in_channel = group.starts_with("[Channel") && deck.is_some();
    let approx = None;
    let hotcue = |verb: &str| key.strip_prefix("hotcue_").and_then(|k| k.strip_suffix(verb)).and_then(|n| n.parse::<u64>().ok());
    let found = match (group, key) {
        (_, "play") if in_channel => (Role::Button, "play", deck, None, approx),
        (_, "sync_enabled") if in_channel => (Role::Button, "sync", deck, None, approx),
        (_, "keylock") if in_channel => (Role::Button, "keylock", deck, None, approx),
        (_, "pfl") if in_channel => (Role::Button, "headphone", deck, None, approx),
        (_, "LoadSelectedTrack") if in_channel => (Role::Button, "load", deck, None, approx),
        (_, "loop_halve") if in_channel => (Role::Button, "loop_half", deck, None, approx),
        (_, "loop_double") if in_channel => (Role::Button, "loop_double", deck, None, approx),
        (_, "volume") if in_channel => (Role::Value, "volume", deck, None, approx),
        (_, "pregain") if in_channel => (Role::Value, "gain", deck, None, approx),
        (_, "filterLow") if in_channel => (Role::Value, "eq_low", deck, None, approx),
        (_, "filterMid") if in_channel => (Role::Value, "eq_mid", deck, None, approx),
        (_, "filterHigh") if in_channel => (Role::Value, "eq_hi", deck, None, approx),
        (g, "parameter1") if g.starts_with("[EqualizerRack1_") && deck.is_some() => (Role::Value, "eq_low", deck, None, approx),
        (g, "parameter2") if g.starts_with("[EqualizerRack1_") && deck.is_some() => (Role::Value, "eq_mid", deck, None, approx),
        (g, "parameter3") if g.starts_with("[EqualizerRack1_") && deck.is_some() => (Role::Value, "eq_hi", deck, None, approx),
        (g, "super1") if g.starts_with("[QuickEffectRack1_") && deck.is_some() => (Role::Value, "filter", deck, None, approx),
        (g, "enabled") if g.starts_with("[QuickEffectRack1_") && deck.is_some() => (Role::Button, "filter_on", deck, None, approx),
        ("[Master]", "crossfader") => (Role::Value, "xfader", None, None, approx),
        ("[Master]", "headMix") => (Role::Value, "cue_mix", None, None, approx),
        ("[Library]", "MoveVertical") | ("[Playlist]", "SelectTrackKnob") => (Role::Browse, "browse", None, None, approx),
        ("[Library]", "MoveDown") | ("[Playlist]", "SelectNextTrack") => (Role::Button, "browse_down", None, None, approx),
        ("[Library]", "MoveUp") | ("[Playlist]", "SelectPrevTrack") => (Role::Button, "browse_up", None, None, approx),
        (_, _) if in_channel && hotcue("_clear").is_some() => return Err("deletes a hot cue; deleting is never imported as a trigger".into()),
        (_, _) if in_channel && hotcue("_activate").is_some_and(|n| (1..=8).contains(&n)) => (Role::Button, "hotcue", deck, hotcue("_activate"), Some(HOTCUE_NOTE)),
        (_, "cue_default") if in_channel => return Err("Mixxx's CDJ-style cue (set or return to the cue point, hold to preview) has no ReTraktor equivalent".into()),
        _ => return Err(format!("Mixxx control {group} {key} has no ReTraktor action")),
    };
    Ok(found)
}

/// A known, unmodified Mixxx companion script, identified by length and FNV-1a 64 of its bytes.
/// shortcut: FNV-1a identifies an unmodified public file, not a tamper-proof signature; use a cryptographic hash if
/// a signature ever gates anything security-relevant.
struct KnownScript {
    file: &'static str,
    len: u64,
    fnv: u64,
}

/// The Allen & Heath Xone K1/K2/K3 mapping shipped with Mixxx (its script and the components library it builds on).
const XONE_K2_COMPANIONS: [KnownScript; 2] = [
    KnownScript { file: "Allen-and-Heath-Xone-K2-scripts.js", len: 39411, fnv: 0xf4d2_960c_84af_1bc4 },
    KnownScript { file: "midi-components-0.0.js", len: 54636, fnv: 0x6114_88e2_c46d_2309 },
];

fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// Checks that every known companion is present next to the mapping and unmodified.
fn check_companions(dir: Option<&Path>, known: &[KnownScript]) -> Result<(), String> {
    let dir = dir.ok_or("the mapping's folder is unknown, so its scripts can't be checked")?;
    for k in known {
        let path = dir.join(k.file);
        let mut bytes = Vec::new();
        std::fs::File::open(&path).and_then(|f| f.take(k.len + 1).read_to_end(&mut bytes)).map_err(|_| format!("its script {} isn't next to the mapping", k.file))?;
        if bytes.len() as u64 != k.len || fnv1a(&bytes) != k.fnv {
            return Err(format!("its script {} differs from the known Mixxx version, so its behaviour isn't assumed", k.file));
        }
    }
    Ok(())
}

/// Deck columns of the known Xone K2 script, from its `init` function: for each MIDI channel (0-based, as the
/// script writes it) the deck number of columns 1..=4, or 0 for an effect unit column.
fn xone_k2_column_deck(channel0: u8, column: u8) -> Option<u64> {
    let decks: [u64; 4] = match channel0 {
        0xE => [0, 1, 2, 0],
        0xD => [1, 0, 0, 2],
        0xC => [1, 2, 0, 0],
        0xB => [0, 0, 1, 2],
        0xA => [3, 1, 2, 4],
        0x9 => [1, 2, 3, 4],
        _ => [0; 4],
    };
    decks.get(usize::from(column).checked_sub(1)?).copied().filter(|d| *d > 0)
}

/// A known Xone K2 script row: `XoneK2.controllers[0xE].columns[2].fader.input` etc. Only the controls whose job
/// never changes with the script's Shift or layers are translated: the fader (volume) and the top two knobs of a
/// deck column (high and mid EQ).
fn xone_k2_row(key: &str, status: u8) -> Result<(u64, &'static str), String> {
    let unsupported = || format!("script control {key} changes its job with the script's Shift, layers or state; not imported");
    let rest = key.strip_prefix("XoneK2.controllers[0x").ok_or_else(unsupported)?;
    let (channel, rest) = rest.split_once("].columns[").ok_or_else(unsupported)?;
    let (column, part) = rest.split_once("].").ok_or_else(unsupported)?;
    let channel0 = u8::from_str_radix(channel, 16).map_err(|_| unsupported())?;
    let column: u8 = column.parse().map_err(|_| unsupported())?;
    if status & 0x0F != channel0 || status & 0xF0 != 0xB0 {
        return Err(format!("{key} is bound to status 0x{status:02X}, not a CC on the script's MIDI channel"));
    }
    let action = match part {
        "fader.input" => "volume",
        "knobs[1].input" => "eq_hi",
        "knobs[2].input" => "eq_mid",
        _ => return Err(unsupported()),
    };
    let deck = xone_k2_column_deck(channel0, column).ok_or_else(|| format!("column {column} on MIDI channel {} is an effect unit in this script; effect units aren't imported", channel0 + 1))?;
    Ok((deck, action))
}

fn convert_mixxx(b: &mut Builder, map: &MixxxMap) {
    b.assignments = map.controls.len() + map.outputs.len();
    let k2 = map.scripts.iter().any(|s| s == XONE_K2_COMPANIONS[0].file).then(|| check_companions(map.dir.as_deref(), &XONE_K2_COMPANIONS));
    if let Some(Ok(())) = &k2 {
        b.depends.insert("Mixxx Xone K2 script: only rows read by hand from its unmodified version are imported; the controller's MIDI channel picks the layout (channel 15: decks in the middle, 10: four decks 1 2 3 4, …)".into());
    }
    // Note-on rows by (status low nibble, note, group, key), to pair their note-off rows.
    let mut outcome: HashMap<(u8, u8, String, String), bool> = HashMap::new();
    let mut deferred = Vec::new();
    let lsb_of: HashSet<(u8, u8, String, String)> = map
        .controls
        .iter()
        .filter(|c| c.options.iter().any(|o| o == "fourteen-bit-lsb"))
        .filter_map(|c| Some((hex_or_decimal(&c.status)?, hex_or_decimal(&c.midino)?, c.group.clone(), c.key.clone())))
        .collect();
    let mut planned_index: Vec<(usize, Result<(String, Plan, String), String>)> = Vec::new();
    for (i, ctl) in map.controls.iter().enumerate() {
        let (Some(status), Some(midino)) = (hex_or_decimal(&ctl.status), hex_or_decimal(&ctl.midino)) else {
            planned_index.push((i, Err(format!("status \"{}\" or midino \"{}\" isn't a number from 0 to 255", ctl.status, ctl.midino))));
            continue;
        };
        if status < 0x80 || midino > 0x7F {
            planned_index.push((i, Err(format!("status {} / midino {} isn't a MIDI message: the status must be 0x80 or above and the note or CC number 0 to 127", ctl.status, ctl.midino))));
            continue;
        }
        if status & 0xF0 == 0x80 {
            deferred.push(i);
            continue;
        }
        let result = mixxx_row(map, ctl, status, midino, &k2, &lsb_of, b);
        if status & 0xF0 == 0x90 {
            outcome.insert((status & 0x0F, midino, ctl.group.clone(), ctl.key.clone()), result.is_ok());
        }
        planned_index.push((i, result));
    }
    for i in deferred {
        let ctl = &map.controls[i];
        let (status, midino) = (hex_or_decimal(&ctl.status).unwrap_or(0), hex_or_decimal(&ctl.midino).unwrap_or(0));
        let note_on = outcome.get(&(status & 0x0F, midino, ctl.group.clone(), ctl.key.clone())).copied();
        planned_index.push((
            i,
            Err(match note_on {
                Some(true) => "MERGED:note-off of a button whose note-on row is imported; ReTraktor reads the release itself".into(),
                Some(false) => "note-off of a button whose note-on row isn't imported".into(),
                None => "a note-off row without a matching note-on row; release-only actions aren't supported".into(),
            }),
        ));
    }
    planned_index.sort_by_key(|(i, _)| *i);
    for (i, result) in planned_index {
        let ctl = &map.controls[i];
        let location = format!("control {}", i + 1);
        let mut source = format!("status {} midino {} {} {}", ctl.status, ctl.midino, ctl.group, ctl.key).replace("  ", " ");
        if !ctl.options.is_empty() {
            let _ = write!(source, " [{}]", ctl.options.join(" "));
        }
        match result {
            Ok((input, plan, label)) => b.plan(location, source, input, Vec::new(), label, plan),
            Err(why) if why.starts_with("MERGED:") => {
                b.item(Coverage::Merged, location, source, why.trim_start_matches("MERGED:"));
            }
            Err(why) => b.skip(location, source, why),
        }
    }
    for (i, out) in map.outputs.iter().enumerate() {
        b.skip(format!("output {}", i + 1), format!("status {} midino {} {} {}", out.status, out.midino, out.group, out.key), "LED output to the controller; ReTraktor sends nothing to MIDI controllers");
    }
}

fn mixxx_row(map: &MixxxMap, ctl: &MixxxControl, status: u8, midino: u8, k2: &Option<Result<(), String>>, lsb_of: &HashSet<(u8, u8, String, String)>, b: &mut Builder) -> Result<(String, Plan, String), String> {
    let channel = (status & 0x0F) + 1;
    let address = match status & 0xF0 {
        0x90 => Address::Note(midino),
        0xB0 => Address::Cc(midino),
        0xE0 => Address::Bend,
        other => return Err(format!("MIDI message type 0x{other:02X} isn't a note, CC or pitch bend")),
    };
    let has = |o: &str| ctl.options.iter().any(|x| x == o);
    let script = has("script-binding") || ctl.group.is_empty();
    if script {
        let Some(check) = k2 else {
            return Err(format!("runs the script function {} (JavaScript); ReTraktor runs no scripts", ctl.key));
        };
        check.clone().map_err(|e| format!("runs the script function {}; {e}", ctl.key))?;
        let (deck, action) = xone_k2_row(&ctl.key, status)?;
        let plan = Plan::new(InKind::Fader, vec![value_action(action, Some(deck), InKind::Fader, false, None)], format!("recognised from the unmodified Mixxx script: column on MIDI channel {channel} is deck {deck}"))
            .approx("Mixxx's soft takeover isn't reproduced: the value jumps to the control's position");
        return Ok((b.input_id(channel, address), plan, format!("Deck {deck} {action}")));
    }
    if has("fourteen-bit-lsb") {
        let paired = midino >= 32 && map.controls.iter().any(|c| {
            c.options.iter().any(|o| o == "fourteen-bit-msb") && hex_or_decimal(&c.status) == Some(status) && hex_or_decimal(&c.midino) == Some(midino - 32) && c.group == ctl.group && c.key == ctl.key
        });
        return Err(if paired { "MERGED:fine half of a 14-bit control; ReTraktor combines it with its coarse CC".into() } else { "fine half of a 14-bit control without a coarse CC 32 below it; not imported".into() });
    }
    let unsupported: Vec<&String> = ctl.options.iter().filter(|o| !["normal", "invert", "button", "soft-takeover", "selectknob", "spread64", "fourteen-bit-msb"].contains(&o.as_str())).collect();
    if let Some(o) = unsupported.first() {
        return Err(format!("option <{o}/> isn't supported"));
    }
    let (role, action, deck, slot, approx) = mixxx_target(&ctl.group, &ctl.key)?;
    let invert = has("invert");
    let mut note = String::new();
    let mut approx_extra = None;
    let (kind, a) = match (role, address) {
        (Role::Button, Address::Note(_) | Address::Cc(_)) => {
            if has("selectknob") || has("spread64") || invert {
                return Err("a button binding with encoder or invert options isn't supported".into());
            }
            let mut a = json!({"action": action});
            if let Some(d) = deck {
                a["deck"] = d.into();
            }
            if let Some(s) = slot {
                a["slot"] = s.into();
            }
            (InKind::Button, a)
        }
        (Role::Value, Address::Cc(_) | Address::Bend) => {
            if has("selectknob") || has("spread64") || has("button") {
                return Err("a knob value with encoder or button options isn't supported".into());
            }
            if has("fourteen-bit-msb") {
                let paired = midino < 32 && lsb_of.contains(&(status, midino + 32, ctl.group.clone(), ctl.key.clone()));
                if !paired {
                    return Err("coarse half of a 14-bit control without its fine CC 32 above it; not imported".into());
                }
                note = format!("14-bit with CC {}", midino + 32);
            }
            (InKind::Fader, value_action(action, deck, InKind::Fader, invert, None))
        }
        (Role::Browse, Address::Cc(_)) => {
            // As Mixxx reads them (midicontroller.cpp): selectknob is a two's complement step, spread64 the
            // distance from 64; invert (127 - value) is applied first, which isn't a clean reversal.
            if invert {
                return Err("inverted library scrolling isn't supported".into());
            }
            let mode = if has("selectknob") {
                CcMode::TwosComplement
            } else if has("spread64") {
                CcMode::Offset64
            } else {
                return Err("library scrolling needs the <selectknob/> or <spread64/> option (other options don't send steps)".into());
            };
            note = format!("relative encoder read as {}", encoding_name(mode));
            approx_extra = Some("ReTraktor moves one track per encoder message; Mixxx moves by the message's count, so fast turns move less");
            (InKind::Encoder(mode), json!({"action": action}))
        }
        _ => return Err("this kind of MIDI message can't drive that control".into()),
    };
    let mut plan = Plan::new(kind, vec![a], note);
    for why in [approx, approx_extra].into_iter().flatten() {
        plan = plan.approx(why);
    }
    if has("soft-takeover") {
        plan = plan.approx("Mixxx's soft takeover isn't reproduced: the value jumps to the control's position");
    }
    if action == "filter" {
        b.depends.insert(FILTER_DEPENDS.into());
    }
    let label = format!("{} {}", ctl.group, ctl.key);
    Ok((b.input_id(channel, address), plan, label))
}

// ---------------------------------------------------------------------------------------------------------------
// rekordbox .midi.csv

#[derive(Debug)]
struct RekordboxMap {
    device: String,
    /// Column index of each header name.
    columns: Columns,
    rows: Vec<(usize, Vec<String>)>,
}

#[derive(Debug, Default)]
struct Columns {
    name: usize,
    function: Option<usize>,
    kind: usize,
    input: usize,
    decks: [Option<usize>; 4],
    output: Option<usize>,
    option: Option<usize>,
    comment: Option<usize>,
}

/// Splits one CSV line: commas, double-quoted fields, `""` inside quotes.
/// shortcut: a quoted field can't span lines; rekordbox writes none, upgrade if an export ever does.
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().unwrap().push('"');
            }
            ('"', _) => quoted = !quoted,
            (',', false) => fields.push(String::new()),
            (c, _) => fields.last_mut().unwrap().push(c),
        }
    }
    fields.iter().map(|f| f.trim().to_string()).collect()
}

fn parse_rekordbox(text: &str) -> Result<RekordboxMap, String> {
    let mut lines = text.split('\n').map(|l| l.trim_end_matches('\r')).enumerate().filter(|(_, l)| !l.trim().is_empty());
    let (_, first) = lines.next().ok_or("the file is empty")?;
    let head = csv_fields(first);
    if head.first().map(String::as_str) != Some("@file") {
        return Err("a rekordbox mapping starts with an @file line".into());
    }
    match head.get(1).map(String::as_str) {
        Some("1") => {}
        Some(v) => return Err(format!("rekordbox mapping version {v} isn't supported; this build reads version 1")),
        None => return Err("the @file line has no version".into()),
    }
    let device = head.get(2).filter(|d| !d.is_empty()).cloned().unwrap_or_else(|| "rekordbox mapping".into());
    let mut columns: Option<Columns> = None;
    let mut rows = Vec::new();
    for (index, line) in lines {
        let fields = csv_fields(line);
        let first = fields.first().map(String::as_str).unwrap_or("");
        if first == "#name" {
            let find = |name: &str, from: usize| fields.iter().enumerate().skip(from).find(|(_, f)| f.eq_ignore_ascii_case(name)).map(|(i, _)| i);
            let (Some(name), Some(kind), Some(input)) = (find("#name", 0), find("type", 0), find("input", 0)) else {
                return Err(format!("line {}: the header needs name, type and input columns", index + 1));
            };
            let output = find("output", input);
            // The deck columns after "input" (the ones after "output" are its LED addresses).
            let decks = std::array::from_fn(|k| find(&format!("deck{}", k + 1), input).filter(|i| output.is_none_or(|o| *i < o)));
            columns = Some(Columns { name, function: find("function", 0), kind, input, decks, output, option: find("option", input), comment: find("comment", input) });
            continue;
        }
        if first.starts_with('#') || fields.iter().all(String::is_empty) {
            continue;
        }
        if columns.is_none() {
            return Err(format!("line {}: a mapping row comes before the #name header line", index + 1));
        }
        rows.push((index + 1, fields));
    }
    Ok(RekordboxMap { device, columns: columns.unwrap_or_default(), rows })
}

/// A rekordbox address, `B013`: a MIDI status byte (0x80 or above) and a data byte (0 to 127).
fn hex_pair(s: &str) -> Option<(u8, u8)> {
    if s.len() != 4 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let (status, data) = (u8::from_str_radix(&s[..2], 16).ok()?, u8::from_str_radix(&s[2..], 16).ok()?);
    (status >= 0x80 && data <= 0x7F).then_some((status, data))
}

/// The (deck, status, data) addresses of one row: a base address plus a channel offset per deck column, an
/// address per deck column when the base is empty, or one deck-less address.
fn rekordbox_addresses(input: &str, decks: &[String; 4]) -> Result<Vec<(Option<u64>, u8, u8)>, String> {
    if input.is_empty() {
        let found: Vec<(Option<u64>, u8, u8)> = decks
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.is_empty())
            .map(|(k, d)| hex_pair(d).map(|(s, n)| (Some(k as u64 + 1), s, n)).ok_or_else(|| format!("deck {} address \"{d}\" isn't a MIDI address (4 hex digits: a status from 80 and a data byte up to 7F)", k + 1)))
            .collect::<Result<_, _>>()?;
        return if found.is_empty() { Err("no input address".into()) } else { Ok(found) };
    }
    let (status, data) = hex_pair(input).ok_or_else(|| format!("input \"{input}\" isn't a MIDI address (4 hex digits: a status from 80 and a data byte up to 7F)"))?;
    if decks.iter().all(String::is_empty) {
        return Ok(vec![(None, status, data)]);
    }
    decks
        .iter()
        .enumerate()
        .filter(|(_, d)| !d.is_empty())
        .map(|(k, d)| {
            let offset: u8 = d.parse().ok().filter(|o| *o <= 15).ok_or_else(|| format!("deck {} channel offset \"{d}\" isn't 0 to 15", k + 1))?;
            if (status & 0x0F) + offset > 15 {
                return Err(format!("deck {} channel offset {offset} runs past MIDI channel 16", k + 1));
            }
            Ok((Some(k as u64 + 1), status + offset, data))
        })
        .collect()
}

fn convert_rekordbox(b: &mut Builder, map: &RekordboxMap) {
    b.assignments = map.rows.len();
    if map.rows.is_empty() {
        b.empty_reason = Some(format!("\"{}\" is a header-only export: rekordbox saved no MIDI assignments in it.", map.device));
        return;
    }
    let col = &map.columns;
    for (line, fields) in &map.rows {
        let get = |i: Option<usize>| i.and_then(|i| fields.get(i)).map(String::as_str).unwrap_or("");
        let name = get(Some(col.name));
        let function = get(col.function);
        let kind = get(Some(col.kind));
        let comment = get(col.comment);
        let mut source = format!("{name} ({function}, {kind}");
        if !get(Some(col.input)).is_empty() {
            let _ = write!(source, ", {}", get(Some(col.input)));
        }
        source.push(')');
        if !comment.is_empty() {
            let _ = write!(source, " \"{comment}\"");
        }
        let location = format!("line {line}");
        if name.is_empty() {
            b.skip(location, source, "the row has no function name");
            continue;
        }
        if ["Indicator", "IndicatorHiRes", "JogIndicator", "JogIndicatorHiRes", "Value", "Parameter", "RawValue"].contains(&kind) {
            b.skip(location, source, "an indicator, setting or LED output, not a control; ReTraktor sends nothing to MIDI controllers");
            continue;
        }
        let options: Vec<&str> = get(col.option).split(';').map(str::trim).filter(|o| !o.is_empty()).collect();
        // Fast and Priority tune how rekordbox schedules a row, Blink and RO concern its light or direction.
        if let Some(o) = options.iter().find(|o| !["Fast", "RO"].contains(o) && !o.starts_with("Priority=") && !o.starts_with("Blink=")) {
            b.skip(location, source, format!("option \"{o}\" changes how rekordbox reads this control; not imported"));
            continue;
        }
        let decks: [String; 4] = std::array::from_fn(|k| get(col.decks[k]).to_string());
        let leds = col.output.is_some_and(|o| !get(Some(o)).is_empty());
        let target = match rekordbox_target(name, comment) {
            Ok(t) => t,
            Err(why) if why.starts_with("MERGED:") => {
                b.item(Coverage::Merged, location, source, why.trim_start_matches("MERGED:"));
                continue;
            }
            Err(why) => {
                b.skip(location, source, why);
                continue;
            }
        };
        let addresses = match rekordbox_addresses(get(Some(col.input)), &decks) {
            Ok(a) => a,
            Err(e) => {
                b.skip(location, source, e);
                continue;
            }
        };
        let many = addresses.len() > 1;
        for (deck, status, data) in addresses {
            let location = if many { format!("{location} deck {}", deck.unwrap_or(0)) } else { location.clone() };
            let channel = (status & 0x0F) + 1;
            let address = match status & 0xF0 {
                0x90 => Address::Note(data),
                0xB0 => Address::Cc(data),
                other => {
                    b.skip(location, source.clone(), format!("MIDI message type 0x{other:02X} isn't a note or CC"));
                    continue;
                }
            };
            match rekordbox_plan(&target, deck, kind, address) {
                Ok(mut plan) => {
                    if leds {
                        plan.note = [plan.note, "its LED output isn't sent".into()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join("; ");
                    }
                    if target.action == "filter" {
                        b.depends.insert(FILTER_DEPENDS.into());
                    }
                    let label = match deck {
                        Some(d) if many => format!("{name} deck {d}"),
                        _ => name.to_string(),
                    };
                    let input = b.input_id(channel, address);
                    b.plan(location, source.clone(), input, Vec::new(), label, plan);
                }
                Err(why) => b.skip(location, source.clone(), why),
            }
        }
    }
}

struct RekordboxTarget {
    action: &'static str,
    role: Role,
    /// Deck from the function name (`CFXParameterCH2`), for rows with one deck-less address.
    deck: Option<u64>,
    slot: Option<u64>,
    every_deck: bool,
    approx: Option<&'static str>,
}

fn rekordbox_target(name: &str, comment: &str) -> Result<RekordboxTarget, String> {
    let t = |action, role| RekordboxTarget { action, role, deck: None, slot: None, every_deck: false, approx: None };
    if name.to_lowercase().contains("delete") || comment.to_lowercase().contains("delete") || (name.starts_with("PAD") && name.contains("_HotCue+Shift")) {
        return Err("deletes cues; deleting is never imported as a trigger".into());
    }
    if comment.contains("No function assigned") {
        return Err("rekordbox marks this row \"No function assigned here\": the controller handles it itself".into());
    }
    Ok(match name {
        "PlayPause" => t("play", Role::Button),
        "Sync" => t("sync", Role::Button),
        "Master" => RekordboxTarget { approx: Some("ReTraktor's tempo_master makes this deck master; pressing it again doesn't turn master off as rekordbox does"), ..t("tempo_master", Role::Button) },
        "Load" => t("load", Role::Button),
        "HeadphoneCue" => t("headphone", Role::Button),
        "LoopHalf" => t("loop_half", Role::Button),
        "LoopDouble" => t("loop_double", Role::Button),
        "Browse" => t("browse", Role::Browse),
        "Zoom" => RekordboxTarget { every_deck: true, approx: Some("zooms the scrolling waveform of each of the four decks; rekordbox's zoom levels differ"), ..t("waveform_zoom", Role::Browse) },
        "CrossFader" => t("xfader", Role::Value),
        "ChannelFader" => t("volume", Role::Value),
        "Gain" => t("gain", Role::Value),
        "EQHigh" => t("eq_hi", Role::Value),
        "EQMid" => t("eq_mid", Role::Value),
        "EQLow" => t("eq_low", Role::Value),
        "HeadphonesMix" => t("cue_mix", Role::Value),
        "HeadphonesVolume" => t("cue_vol", Role::Value),
        "MasterLevel" => t("master_volume", Role::Value),
        "Shift" => return Err("MERGED:hardware Shift: the controller sends shifted controls as their own MIDI messages, which are imported as their own rows".into()),
        "Cue" => return Err("rekordbox's CDJ-style cue (set or return to the cue point, hold to preview) has no ReTraktor equivalent".into()),
        "TempoSlider" => return Err("the tempo slider's range and direction depend on rekordbox settings this file doesn't hold".into()),
        n => {
            if let Some(ch) = n.strip_prefix("CFXParameterCH").and_then(|d| d.parse::<u64>().ok()).filter(|d| (1..=4).contains(d)) {
                return Ok(RekordboxTarget { deck: Some(ch), ..t("filter", Role::Value) });
            }
            if let Some(slot) = n.strip_prefix("PAD").and_then(|p| p.strip_suffix("_HotCue")).and_then(|p| p.parse::<u64>().ok()).filter(|s| (1..=8).contains(s)) {
                return Ok(RekordboxTarget { slot: Some(slot), approx: Some(HOTCUE_NOTE), ..t("hotcue", Role::Button) });
            }
            return Err(format!("rekordbox function {n} has no ReTraktor action"));
        }
    })
}

fn rekordbox_plan(t: &RekordboxTarget, deck: Option<u64>, kind: &str, address: Address) -> Result<Plan, String> {
    let per_deck = !matches!(t.action, "browse" | "xfader" | "cue_mix" | "cue_vol" | "master_volume" | "waveform_zoom");
    let deck = deck.or(t.deck);
    if per_deck && deck.is_none() {
        return Err("the row has no deck column, so the deck is unknown".into());
    }
    let deck = if per_deck { deck } else { None };
    let (in_kind, actions, note) = match (t.role, kind, address) {
        (Role::Button, "Button" | "Pad", Address::Note(_) | Address::Cc(_)) => {
            let mut a = json!({"action": t.action});
            if let Some(d) = deck {
                a["deck"] = d.into();
            }
            if let Some(s) = t.slot {
                a["slot"] = s.into();
            }
            (InKind::Button, vec![a], String::new())
        }
        (Role::Value, "KnobSlider" | "KnobSliderHiRes", Address::Cc(cc)) => {
            let note = if kind == "KnobSliderHiRes" && cc < 32 { format!("14-bit with CC {}", cc + 32) } else { String::new() };
            (InKind::Fader, vec![value_action(t.action, deck, InKind::Fader, false, None)], note)
        }
        (Role::Browse, "Rotary", Address::Cc(_)) => {
            let actions = if t.every_deck { (1..=4).map(|d| deck_action(t.action, d)).collect() } else { vec![json!({"action": t.action})] };
            (InKind::Encoder(CcMode::TwosComplement), actions, "relative encoder read as two's complement (1 right, 127 left), as Pioneer controllers send".to_string())
        }
        _ => return Err(format!("a {kind} row on this kind of MIDI message can't drive {}", t.action)),
    };
    let plan = Plan::new(in_kind, actions, note);
    Ok(match t.approx {
        Some(why) => plan.approx(why),
        None => plan,
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Activation: merging into the rules file and writing files without clobbering.

/// Adds a draft's rules and pad lights to an existing rules file (`None`: there is none yet). Refuses, listing
/// every collision, when a draft control already has rules, a draft modifier name is already used on the same
/// device, a draft pad already has a colour, or a CC is already declared another way. Existing rules and settings
/// are kept as they are, before the imported ones.
pub fn merge_draft(existing: Option<&str>, draft: &str) -> Result<String, Vec<String>> {
    let problems = |list: Vec<Diagnostic>| list.iter().map(ToString::to_string).collect::<Vec<_>>();
    let draft_set = parse_rules(draft).map_err(|d| [vec!["the draft has problems:".to_string()], problems(d)].concat())?;
    let Some(existing) = existing else {
        return Ok(draft.to_string());
    };
    let existing_set = parse_rules(existing).map_err(|d| [vec!["the rules file has problems; fix them (retraktor-cli validate-mapping) before adding to it:".to_string()], problems(d)].concat())?;
    let mut current: Value = serde_json::from_str(existing).map_err(|e| vec![e.to_string()])?;
    let new: Value = serde_json::from_str(draft).map_err(|e| vec![e.to_string()])?;
    let rules_of = |v: &Value| v["rules"].as_array().cloned().unwrap_or_default();
    let inputs = |v: &Value| rules_of(v).iter().filter_map(|r| r["input"].as_str().map(str::to_string)).collect::<BTreeSet<_>>();
    let modifiers = |v: &Value| {
        let mut set = BTreeSet::new();
        for rule in rules_of(v) {
            let device = rule["input"].as_str().and_then(device_of).unwrap_or_default();
            for a in rule["do"].as_array().into_iter().flatten().filter(|a| a["action"] == "modifier") {
                set.insert((device.clone(), a["name"].as_str().unwrap_or_default().to_string()));
            }
        }
        set
    };
    let mut errors = Vec::new();
    for input in inputs(&new).intersection(&inputs(&current)) {
        errors.push(format!("{input} already has rules in the rules file"));
    }
    for (device, name) in modifiers(&new).intersection(&modifiers(&current)) {
        errors.push(format!("modifier \"{name}\" on {device} is already used by the rules file"));
    }
    // A pad's colour is decided by its first matching entry, so pads already coloured by the file are refused.
    let pads = |v: &Value| v["leds"].as_array().into_iter().flatten().filter_map(|l| l["control"].as_str().map(str::to_string)).collect::<BTreeSet<_>>();
    for pad in pads(&new).intersection(&pads(&current)) {
        errors.push(format!("pad light {pad} is already coloured by the rules file"));
    }
    // How each CC is read, including CCs that are absolute only because a rule uses them as a knob.
    let reading = |mode: CcMode| match mode {
        CcMode::Absolute => "an absolute knob or fader",
        CcMode::Button => "a button",
        other => encoding_name(other),
    };
    let have: HashMap<&str, CcMode> = existing_set.midi_cc_modes().iter().map(|(id, mode)| (id.as_str(), *mode)).collect();
    for (id, mode) in draft_set.midi_cc_modes() {
        if let Some(old) = have.get(id.as_str()).filter(|old| *old != mode) {
            errors.push(format!("{id} is read as {} in the rules file, but the draft reads it as {}", reading(*old), reading(*mode)));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let top = current.as_object_mut().expect("validated rules file is an object");
    let rules = top.entry("rules").or_insert_with(|| json!([])).as_array_mut().expect("validated rules list");
    rules.extend(rules_of(&new));
    if let Some(leds) = new["leds"].as_array() {
        top.entry("leds").or_insert_with(|| json!([])).as_array_mut().expect("validated LED list").extend(leds.iter().cloned());
    }
    if let Some(enc) = new["midi_encoders"].as_object() {
        let target = top.entry("midi_encoders").or_insert_with(|| json!({})).as_object_mut().expect("validated encoders map");
        for (k, v) in enc {
            target.insert(k.clone(), v.clone());
        }
    }
    if let Some(buttons) = new["midi_cc_buttons"].as_array() {
        let list = top.entry("midi_cc_buttons").or_insert_with(|| json!([])).as_array_mut().expect("validated CC button list");
        for b in buttons {
            if !list.contains(b) {
                list.push(b.clone());
            }
        }
    }
    let text = serde_json::to_string_pretty(&current).map_err(|e| vec![e.to_string()])? + "\n";
    parse_rules(&text).map_err(|d| [vec!["the merged file has problems:".to_string()], problems(d)].concat())?;
    Ok(text)
}

/// Writes a new file, refusing if anything already exists at `path`.
pub fn write_new_file(path: &Path, text: &str) -> std::io::Result<()> {
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// Replaces the rules file with `text`, but only while it still holds `expected` (`None`: no file), so an edit
/// made since the preview is never lost. The new file is swapped in atomically and whatever it displaced is kept,
/// byte for byte, as `<name>.before-import-<unix time>`; if that turns out not to be `expected` (another program
/// saved in the meantime), the swap is undone so that version is active again. Returns the backup's path.
pub fn replace_rules_file(path: &Path, expected: Option<&str>, text: &str) -> Result<Option<PathBuf>, String> {
    let changed = || format!("{} changed on disk since it was read; nothing was written. Check it, then preview or run the command again.", path.display());
    let now = match read_rules_file(path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("can't read {}: {e}", path.display())),
    };
    if now.as_deref() != expected {
        return Err(changed());
    }
    // A symlinked rules file stays a link: the swap happens at its target.
    let target = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => std::fs::canonicalize(path).map_err(|e| format!("{} is a symbolic link whose target can't be reached ({e}); nothing was written", path.display()))?,
        _ => path.to_path_buf(),
    };
    let path = target.as_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("can't create {}: {e}", dir.display()))?;
    }
    // The replacement keeps the old file's permissions (a private 0600 file stays private), set before writing.
    let permissions = expected.and(std::fs::metadata(path).ok()).map(|m| m.permissions());
    let temp = new_sibling(path, "import", ".tmp", text, permissions)?;
    let Some(old) = expected else {
        // A hard link appears in one step and fails if a file has appeared meanwhile.
        let linked = std::fs::hard_link(&temp, path);
        let _ = std::fs::remove_file(&temp);
        return match linked {
            Ok(()) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(changed()),
            Err(e) => Err(format!("can't write {}: {e}", path.display())),
        };
    };
    install(path, old, &temp, text)
}

/// Swaps `temp` (holding `text`) into `path` and keeps what it displaced (see `replace_rules_file`).
fn install(path: &Path, expected: &str, temp: &Path, text: &str) -> Result<Option<PathBuf>, String> {
    if let Err(e) = exchange(temp, path) {
        let _ = std::fs::remove_file(temp);
        return Err(format!("can't swap the new rules file into place ({e}); nothing was changed"));
    }
    let displaced = std::fs::read(temp).ok();
    let backup = keep_as_backup(path, temp)?;
    if displaced.as_deref() == Some(expected.as_bytes()) {
        return Ok(Some(backup));
    }
    // Another program saved between the check and the swap: make its version active again.
    if let Err(e) = exchange(&backup, path) {
        return Err(format!("{} changed while the import was being added; the import is in place and the version it replaced is kept as {} ({e})", path.display(), backup.display()));
    }
    if std::fs::read(&backup).ok().as_deref() == Some(text.as_bytes()) {
        let _ = std::fs::remove_file(&backup);
        return Err(format!("{} changed while the import was being added; it was left as the other program saved it. Check it, then preview or run the command again.", path.display()));
    }
    Err(format!("{} changed while the import was being added; it was left as the other program saved it, and what was there in between is kept as {}", path.display(), backup.display()))
}

/// A new file next to `path` holding `text`, named `.<name>.<tag>-<pid>-<n><ext>`; never reuses an existing path.
/// `permissions` are applied before the text is written.
fn new_sibling(path: &Path, tag: &str, ext: &str, text: &str, permissions: Option<std::fs::Permissions>) -> Result<PathBuf, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "controller-rules.json".into());
    for n in 0..1000 {
        let candidate = path.with_file_name(format!(".{name}.{tag}-{}-{n}{ext}", std::process::id()));
        let file = match std::fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("can't write {}: {e}", candidate.display())),
        };
        let written = permissions.clone().map_or(Ok(()), |p| file.set_permissions(p)).and_then(|()| {
            let mut file = file;
            file.write_all(text.as_bytes())?;
            file.sync_all()
        });
        return match written {
            Ok(()) => Ok(candidate),
            Err(e) => {
                let _ = std::fs::remove_file(&candidate);
                Err(format!("can't write {}: {e}", candidate.display()))
            }
        };
    }
    Err(format!("can't find a free temporary name next to {}", path.display()))
}

/// Gives `file` a free `<name>.before-import-<unix time>[-n]` name next to `path`, without replacing anything.
fn keep_as_backup(path: &Path, file: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "controller-rules.json".into());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    for n in 0..1000 {
        let candidate = path.with_file_name(if n == 0 { format!("{name}.before-import-{stamp}") } else { format!("{name}.before-import-{stamp}-{n}") });
        match std::fs::hard_link(file, &candidate) {
            Ok(()) => {
                let _ = std::fs::remove_file(file);
                return Ok(candidate);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("the previous rules are kept as {} (can't rename it: {e})", file.display())),
        }
    }
    Err(format!("the previous rules are kept as {} (no free backup name)", file.display()))
}

/// Atomically swaps two paths (Linux `renameat2` with `RENAME_EXCHANGE`).
#[cfg(target_os = "linux")]
fn exchange(a: &Path, b: &Path) -> std::io::Result<()> {
    use rustix::fs::{renameat_with, RenameFlags, CWD};
    Ok(renameat_with(CWD, a, CWD, b, RenameFlags::EXCHANGE)?)
}

#[cfg(not(target_os = "linux"))]
fn exchange(_a: &Path, _b: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "an atomic swap needs Linux"))
}

/// Small synthetic mappings for tests (here and in the app): no real exports, no vendor files.
#[doc(hidden)]
pub mod fixtures {
    use retraktor_formats::tsi::base64_encode;
    use retraktor_formats::TsiChunk;

    fn chunk(tag: &[u8; 4], payload: Vec<u8>, children: Vec<TsiChunk>) -> TsiChunk {
        TsiChunk { fourcc: *tag, payload, children }
    }

    fn wstr(s: &str) -> Vec<u8> {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut b = (units.len() as u32).to_be_bytes().to_vec();
        units.iter().for_each(|u| b.extend(u.to_be_bytes()));
        b
    }

    fn words(values: &[u32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_be_bytes()).collect()
    }

    /// One assignment: MIDI address, Traktor command, control type, interaction mode, target, set value, comment
    /// and up to two (modifier command, value) conditions.
    pub struct Row<'a> {
        pub address: &'a str,
        pub command: u32,
        pub control: u32,
        pub mode: u32,
        pub target: i32,
        pub set: u32,
        pub comment: &'a str,
        pub when: &'a [(u32, u32)],
    }

    fn device(name: &str, comment: &str, port: &str, definitions: &[&str], rows: &[Row]) -> TsiChunk {
        let defs: Vec<TsiChunk> = definitions.iter().map(|d| chunk(b"DCDT", [wstr(d), words(&[7, 0, 0x42FE_0000, 1, u32::MAX])].concat(), vec![])).collect();
        let mut cmai = Vec::new();
        let mut bindings = Vec::new();
        for (i, r) in rows.iter().enumerate() {
            let mut cmad = words(&[4, r.control, r.mode, r.target as u32, 0, 0, 0, 0x40A0_0000, 0, 0, 1, r.set]);
            cmad.extend(wstr(r.comment));
            for k in 0..2 {
                let (id, value) = r.when.get(k).copied().unwrap_or((0, 0));
                cmad.extend(words(&[id, 0, value]));
            }
            cmad.extend(words(&[0; 11]));
            cmai.push(chunk(b"CMAI", words(&[i as u32, 0, r.command]), vec![chunk(b"CMAD", cmad, vec![])]));
            bindings.push(chunk(b"DCBM", [words(&[i as u32]), wstr(r.address)].concat(), vec![]));
        }
        let ddat = vec![
            chunk(b"DDIC", wstr(comment), vec![]),
            chunk(b"DDPT", [wstr(port), wstr(port)].concat(), vec![]),
            chunk(b"DDDC", vec![], vec![chunk(b"DDCI", words(&[defs.len() as u32]), defs), chunk(b"DDCO", words(&[0]), vec![])]),
            chunk(b"DDCB", vec![], vec![chunk(b"CMAS", words(&[rows.len() as u32]), cmai), chunk(b"DCBM", words(&[rows.len() as u32]), bindings)]),
        ];
        chunk(b"DEVI", wstr(name), vec![chunk(b"DDAT", vec![], ddat)])
    }

    /// A Traktor settings file holding the given devices: (name, comment, port, definitions, rows).
    pub fn tsi(devices: &[(&str, &str, &str, &[&str], &[Row])]) -> String {
        let devs: Vec<TsiChunk> = devices.iter().map(|(n, c, p, d, r)| device(n, c, p, d, r)).collect();
        let diom = chunk(b"DIOM", vec![], vec![chunk(b"DIOI", words(&[1]), vec![]), chunk(b"DEVS", words(&[devs.len() as u32]), devs)]);
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n<NIXML><TraktorSettings><Entry Name=\"DeviceIO.Config.Controller\" Type=\"3\" Value=\"{}\"></Entry>\n<Entry Name=\"Gui.Layouts2\" Type=\"3\" Value=\"Regular2 0 22 0;Regular4 1 22 1;Stacked2 2 22 0;Stacked4 3 22 1;Custom 4 22 1\"></Entry>\n</TraktorSettings></NIXML>\n",
            base64_encode(&diom.to_bytes())
        )
    }

    const M1: u32 = 2548;
    const M2: u32 = 2549;

    fn row<'a>(address: &'a str, command: u32, control: u32, mode: u32, target: i32, set: u32, comment: &'a str, when: &'a [(u32, u32)]) -> Row<'a> {
        Row { address, command, control, mode, target, set, comment, when }
    }

    /// A Xone K2 style layout-and-browser mapping, shaped like a real one: a browser toggle, two encoder presses
    /// that flip M1 (2 / 4 decks) and M2 (regular / stacked) and pick the layout from both, one encoder that zooms
    /// every deck's waveform, plus a native controller with no assignments and a mixer device.
    pub fn traktor_three_devices() -> String {
        let k2 = [
            row("Ch14.Note.D0", 4209, 0, 1, 0, 0, "Browser toggle", &[]),
            row("Ch14.Note.C#0", M1, 0, 3, 0, 1, "2/4: 0 -> 1", &[(M1, 0)]),
            row("Ch14.Note.C#0", M1, 0, 3, 0, 0, "2/4: 1 -> 0", &[(M1, 1)]),
            row("Ch14.Note.C#0", 4208, 0, 3, 0, 1, "2/4: layout 1", &[(M1, 0), (M2, 0)]),
            row("Ch14.Note.C#0", 4208, 0, 3, 0, 0, "2/4: layout 0", &[(M1, 1), (M2, 0)]),
            row("Ch14.Note.C#0", 4208, 0, 3, 0, 3, "2/4: layout 3", &[(M1, 0), (M2, 1)]),
            row("Ch14.Note.C#0", 4208, 0, 3, 0, 2, "2/4: layout 2", &[(M1, 1), (M2, 1)]),
            row("Ch14.Note.D#0", M2, 0, 3, 0, 1, "stacked: 0 -> 1", &[(M2, 0)]),
            row("Ch14.Note.D#0", M2, 0, 3, 0, 0, "stacked: 1 -> 0", &[(M2, 1)]),
            row("Ch14.Note.D#0", 4208, 0, 3, 0, 2, "stacked: layout 2", &[(M1, 0), (M2, 0)]),
            row("Ch14.Note.D#0", 4208, 0, 3, 0, 3, "stacked: layout 3", &[(M1, 1), (M2, 0)]),
            row("Ch14.Note.D#0", 4208, 0, 3, 0, 0, "stacked: layout 0", &[(M1, 0), (M2, 1)]),
            row("Ch14.Note.D#0", 4208, 0, 3, 0, 1, "stacked: layout 1", &[(M1, 1), (M2, 1)]),
            row("Ch14.CC.020", 4162, 2, 4, 0, 0, "Zoom A", &[]),
            row("Ch14.CC.020", 4162, 2, 4, 1, 0, "Zoom B", &[]),
            row("Ch14.CC.020", 4162, 2, 4, 2, 0, "Zoom C", &[]),
            row("Ch14.CC.020", 4162, 2, 4, 3, 0, "Zoom D", &[]),
        ];
        let half = 0x3F00_0000;
        let mixer = [
            row("Ch01.CC.002", 303, 1, 3, 0, half, "Deck 1 high EQ", &[]),
            row("Ch01.CC.003", 302, 1, 3, 0, half, "Deck 1 upper mid EQ", &[]),
            row("Ch01.CC.004", 316, 1, 3, 0, half, "Deck 1 lower mid EQ", &[]),
            row("Ch01.CC.006", 320, 1, 3, 1, half, "Deck 2 filter", &[]),
            row("Ch01.CC.008", 100, 0, 1, 0, 0, "Deck 1 play", &[(M1, 0)]),
            row("Ch01.CC.009", 206, 0, 2, 0, 0, "Deck 1 cue", &[(M1, 0)]),
            row("Ch01.CC.008", 125, 0, 1, 0, 0, "Deck 1 shifted sync", &[(M1, 1)]),
            row("Ch01.CC.009", 2293, 0, 0, 0, 0, "Deck 1 shifted tempo master", &[(M1, 1)]),
            row("Ch01.CC.011", M1, 0, 2, 0, 1, "Shift layer", &[]),
            row("Ch01.CC.012", 102, 1, 3, -1, 0, "Focused deck volume", &[]),
            row("Ch01.Note.C3", 100, 65535, 8, 0, 0, "Play light", &[]),
        ];
        tsi(&[
            ("Traktor.Kontrol X1MK3", "", "X1", &["Shift Button", "Play Button"], &[]),
            ("Generic MIDI", "K2 layout and browser", "K2", &["Ch14.CC.020"], &k2),
            ("Generic MIDI", "Four deck mixer", "MIX", &[], &mixer),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::controller_rules::{RuleCommand, RuleEngine};
    use retraktor_core::SurfaceEvent;

    fn press(control: &str, pressed: bool) -> SurfaceEvent {
        SurfaceEvent::Button { control: control.into(), pressed }
    }

    fn source(text: &str) -> Source {
        parse_source(text, "test", None).unwrap_or_else(|e| panic!("{e}"))
    }

    fn item<'a>(c: &'a Conversion, location: &str) -> &'a Item {
        c.items.iter().find(|i| i.location == location).unwrap_or_else(|| panic!("no item {location} in {:#?}", c.items))
    }

    /// The K2-style device: all 17 rows convert, each control's rows that match the modifiers before the press
    /// run together, and the rules work through the live RuleEngine.
    #[test]
    fn traktor_multi_device_k2_layout_rows_run_together_in_the_rule_engine() {
        let src = source(&fixtures::traktor_three_devices());
        assert_eq!(src.devices.len(), 3);
        assert_eq!(src.devices[1].name, "Generic MIDI: K2 layout and browser");
        assert!(src.devices[1].detail.contains("17 assignments"), "{:?}", src.devices[1]);

        let c = convert(&src, 2, "xonek2").unwrap();
        assert_eq!(c.items.len(), 17);
        assert_eq!((c.count(Coverage::Translated), c.count(Coverage::Approximated), c.count(Coverage::Unsupported)), (4, 13, 0), "{}", c.report());
        assert!(c.summary().starts_with("17 source assignments, 17 report entries"), "{}", c.summary());
        assert_eq!(c.rule_count, 10, "{}", c.draft.as_deref().unwrap_or(""));
        let draft = c.draft.clone().unwrap();
        assert!(draft.contains("\"midi_encoders\": {\"midi.xonek2.ch14.cc20\":\"twos_complement\"}"), "{draft}");
        assert!(item(&c, "row 3").note.contains("layout \"Regular4\" (index 1)"), "{}", c.report());

        let mut e = RuleEngine::default();
        e.replace_rules(parse_rules(&draft).unwrap());
        let layout = |e: &mut RuleEngine, note: &str| {
            let commands = e.handle(&press(note, true)).unwrap();
            e.handle(&press(note, false));
            commands.into_iter().find_map(|c| match c {
                RuleCommand::Trigger { verb: "layout", arg, .. } => Some(arg),
                _ => None,
            })
        };
        let (two_four, stacked) = ("midi.xonek2.ch14.note13", "midi.xonek2.ch14.note15");
        assert_eq!(layout(&mut e, two_four).as_deref(), Some("regular4"), "M1=0, M2=0: to four decks");
        assert_eq!(layout(&mut e, two_four).as_deref(), Some("regular2"), "M1=1: back to two decks");
        assert_eq!(layout(&mut e, stacked).as_deref(), Some("stacked2"), "M2 0 -> 1 with two decks");
        assert_eq!(layout(&mut e, two_four).as_deref(), Some("stacked4"));
        assert_eq!(layout(&mut e, stacked).as_deref(), Some("regular4"), "M2 1 -> 0 with four decks");
        assert_eq!(e.active_modifiers().iter().map(|(_, n, v)| (n.as_str(), *v)).collect::<Vec<_>>(), vec![("m1", 1)]);
        assert_eq!(
            e.handle(&SurfaceEvent::Encoder { control: "midi.xonek2.ch14.cc20".into(), delta: 1 }).unwrap(),
            (1..=4).map(|d| RuleCommand::Trigger { verb: "zoom_in", index: d, arg: String::new() }).collect::<Vec<_>>()
        );
        assert_eq!(e.handle(&press("midi.xonek2.ch14.note14", true)).unwrap(), vec![RuleCommand::Trigger { verb: "browser_only", index: 0, arg: String::new() }]);
    }

    /// The native controller has no assignments: an actionable report, never an empty "successful" draft.
    #[test]
    fn a_native_controller_reports_its_qml_dependency_instead_of_an_empty_draft() {
        let c = convert(&source(&fixtures::traktor_three_devices()), 1, "x1").unwrap();
        assert!(c.draft.is_none());
        assert!(c.empty_reason.as_deref().unwrap().contains("native controller driven by Traktor's built-in QML script"));
        let report = c.report();
        assert!(report.contains("2 named inputs") && report.contains("separate port") && report.contains("Only its buttons"), "{report}");
        assert!(convert(&source(&fixtures::traktor_three_devices()), 4, "x1").unwrap_err().contains("device 4 doesn't exist"));
        assert!(convert(&source(&fixtures::traktor_three_devices()), 2, "XONE:K2").unwrap_err().starts_with("port:"));
    }

    /// The mixer device: verified EQ/filter actions, CC buttons, conditional play/sync and tempo master under a
    /// held Shift, and an honest list of what isn't there (Lo Mid, cue hold, focused-deck target).
    #[test]
    fn traktor_mixer_rows_translate_or_explain() {
        let c = convert(&source(&fixtures::traktor_three_devices()), 3, "mixer").unwrap();
        let report = c.report();
        assert!(item(&c, "row 1").coverage == Coverage::Approximated && item(&c, "row 1").note.contains("four-band Xone EQ"), "{report}");
        assert!(item(&c, "row 2").note.contains("no equivalent"), "{report}");
        assert!(item(&c, "row 5").note.contains("CUE"), "{report}");
        assert!(item(&c, "row 9").note.contains("focused deck"), "{report}");
        assert!(report.contains("DEPENDS ON") && report.contains("filter_on"), "{report}");
        let draft = c.draft.unwrap();
        assert!(draft.contains("\"midi_cc_buttons\": [\"midi.mixer.ch1.cc11\",\"midi.mixer.ch1.cc8\",\"midi.mixer.ch1.cc9\"]"), "{draft}");
        let mut e = RuleEngine::default();
        e.replace_rules(parse_rules(&draft).unwrap());
        let trigger = |verb, index| RuleCommand::Trigger { verb, index, arg: String::new() };
        assert_eq!(e.handle(&press("midi.mixer.ch1.cc8", true)), Some(vec![trigger("play", 1)]));
        e.handle(&press("midi.mixer.ch1.cc8", false));
        assert_eq!(e.handle(&press("midi.mixer.ch1.cc9", true)), None, "cue hold isn't imported, so plain CC 9 falls back");
        e.handle(&press("midi.mixer.ch1.cc9", false));
        e.handle(&press("midi.mixer.ch1.cc11", true));
        assert_eq!(e.handle(&press("midi.mixer.ch1.cc8", true)), Some(vec![trigger("sync", 1)]));
        assert_eq!(e.handle(&press("midi.mixer.ch1.cc9", true)), Some(vec![trigger("tempo_master", 1)]));
        e.handle(&press("midi.mixer.ch1.cc11", false));
        assert_eq!(e.handle(&SurfaceEvent::Fader { control: "midi.mixer.ch1.cc6".into(), value: 0.25 }), Some(vec![RuleCommand::Set { target: "filter", index: 2, value: 0.25 }]));
    }

    /// Too many modifier combinations on one control are reported quickly instead of expanded.
    #[test]
    fn modifier_combinations_are_bounded_and_reported() {
        // Eight modifiers, each tested for 1 and 2 on one note: 3^8 combinations.
        let mut rows = Vec::new();
        let conditions: Vec<[(u32, u32); 2]> = (0..8u32).map(|m| [(2548 + m, 1), (2548 + (m + 1) % 8, 2)]).collect();
        for (m, when) in conditions.iter().enumerate() {
            rows.push(fixtures::Row { address: "Ch01.Note.C3", command: 100, control: 0, mode: 1, target: 0, set: 0, comment: "", when });
            rows.push(fixtures::Row { address: "Ch01.Note.D3", command: 2548 + m as u32, control: 0, mode: 3, target: 0, set: 1, comment: "", when: &[] });
            rows.push(fixtures::Row { address: "Ch01.Note.E3", command: 2548 + m as u32, control: 0, mode: 3, target: 0, set: 2, comment: "", when: &[] });
        }
        let text = fixtures::tsi(&[("Generic MIDI", "many", "P", &[], &rows)]);
        let started = std::time::Instant::now();
        let c = convert(&source(&text), 1, "p").unwrap();
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(item(&c, "row 0").note.contains("combinations (limit 64)"), "{}", c.report());
    }

    #[test]
    fn malformed_or_unknown_sources_are_refused_with_a_reason() {
        assert!(parse_source("hello", "x", None).unwrap_err().contains("not a controller mapping"));
        assert!(parse_source("<NIXML><TraktorSettings><Entry Name=\"DeviceIO.Config.Controller\" Type=\"3\" Value=\"!!!\"></Entry></TraktorSettings></NIXML>", "x", None).is_err());
        assert!(parse_source("<NIXML><TraktorSettings></TraktorSettings></NIXML>", "x", None).unwrap_err().contains("no controller section"));
        assert!(parse_source("@file,2,X\n", "x", None).unwrap_err().contains("version 2 isn't supported"));
        assert!(parse_source("@file,1,X\nPlayPause,PlayPause,Button,900B,,,,,,,,,,,\n", "x", None).unwrap_err().contains("before the #name header"));
        assert!(parse_source("<MixxxControllerPreset><info></info></MixxxControllerPreset>", "x", None).unwrap_err().contains("no <controller>"));
    }

    const MIXXX: &str = r#"<?xml version='1.0' encoding='utf-8'?>
<!DOCTYPE mapping>
<MixxxControllerPreset mixxxVersion="2.4" schemaVersion="1">
  <info><name>Test Pad</name></info>
  <controller id="TEST">
    <scriptfiles><file filename="Test-scripts.js" functionprefix="Test"/></scriptfiles>
    <controls>
      <control><group>[Channel1]</group><key>play</key><status>0x90</status><midino>0x0B</midino><options><button/></options></control>
      <control><group>[Channel1]</group><key>play</key><status>0x80</status><midino>0x0B</midino><options><button/></options></control>
      <control><group>[Channel2]</group><key>volume</key><status>0xB1</status><midino>0x13</midino><options><fourteen-bit-msb/></options></control>
      <control><group>[Channel2]</group><key>volume</key><status>0xB1</status><midino>0x33</midino><options><fourteen-bit-lsb/></options></control>
      <control><group>[EqualizerRack1_[Channel1]_Effect1]</group><key>parameter1</key><status>0xB0</status><midino>0x0F</midino><options><Invert/></options></control>
      <control><group>[Library]</group><key>MoveVertical</key><status>0xB6</status><midino>0x40</midino><options><selectknob/></options></control>
      <control><group>[Channel1]</group><key>hotcue_1_clear</key><status>0x97</status><midino>0x08</midino><options><normal/></options></control>
      <control><group>[Channel1]</group><key>hotcue_2_activate</key><status>0x97</status><midino>0x01</midino><options><normal/></options></control>
      <control><group>[Channel1]</group><key>Test.jog</key><status>0xB0</status><midino>0x21</midino><options><script-binding/></options></control>
      <control><group>[Channel3]</group><key>rate</key><status>0xB2</status><midino>0x00</midino><options><spread64/></options></control>
      <control><group>[Channel4]</group><key>sync_enabled</key><status>0xB3</status><midino>0x58</midino><options><button/></options></control>
    </controls>
    <outputs><output><group>[Channel1]</group><key>play_indicator</key><status>0x90</status><midino>0x0B</midino></output></outputs>
  </controller>
</MixxxControllerPreset>"#;

    #[test]
    fn mixxx_declarative_rows_translate_and_scripts_outputs_and_deletes_are_explained() {
        let c = convert(&source(MIXXX), 1, "pad").unwrap();
        let report = c.report();
        assert_eq!(item(&c, "control 2").coverage, Coverage::Merged, "{report}");
        assert_eq!(item(&c, "control 4").coverage, Coverage::Merged, "{report}");
        assert!(item(&c, "control 3").note.contains("14-bit with CC 51"), "{report}");
        assert!(item(&c, "control 7").note.contains("never imported"), "{report}");
        assert_eq!(item(&c, "control 8").coverage, Coverage::Approximated, "{report}");
        assert!(item(&c, "control 9").note.contains("ReTraktor runs no scripts"), "{report}");
        assert!(item(&c, "control 10").note.contains("rate has no ReTraktor action"), "{report}");
        assert!(item(&c, "output 1").note.contains("LED output"), "{report}");
        let mut e = RuleEngine::default();
        e.replace_rules(parse_rules(c.draft.as_deref().unwrap()).unwrap());
        assert_eq!(e.handle(&press("midi.pad.ch1.note11", true)), Some(vec![RuleCommand::Trigger { verb: "play", index: 1, arg: String::new() }]));
        assert_eq!(e.handle(&SurfaceEvent::Fader { control: "midi.pad.ch1.cc15".into(), value: 1.0 }), Some(vec![RuleCommand::Set { target: "eq_low", index: 1, value: 0.0 }]), "inverted");
        assert_eq!(e.handle(&SurfaceEvent::Encoder { control: "midi.pad.ch7.cc64".into(), delta: -1 }), Some(vec![RuleCommand::Trigger { verb: "browse_up", index: 0, arg: String::new() }]));
        assert_eq!(e.handle(&press("midi.pad.ch4.cc88", true)), Some(vec![RuleCommand::Trigger { verb: "sync", index: 4, arg: String::new() }]), "CC button");
        assert_eq!(e.handle(&press("midi.pad.ch8.note1", true)), Some(vec![RuleCommand::Trigger { verb: "hotcue", index: 1, arg: "2".into() }]));
        let legacy = convert(&source(&MIXXX.replace("MixxxControllerPreset", "MixxxMIDIPreset")), 1, "pad").unwrap();
        assert_eq!(legacy.rule_count, c.rule_count, "the legacy root element reads the same");
    }

    /// The Xone K2 script rows: only with both known companions present and unmodified, and only the controls
    /// whose job never changes; deck columns follow the MIDI channel.
    #[test]
    fn known_mixxx_script_rows_need_unmodified_companions() {
        assert_eq!(xone_k2_row("XoneK2.controllers[0xE].columns[2].fader.input", 0xBE), Ok((1, "volume")));
        assert_eq!(xone_k2_row("XoneK2.controllers[0xA].columns[1].knobs[1].input", 0xBA), Ok((3, "eq_hi")));
        assert_eq!(xone_k2_row("XoneK2.controllers[0xB].columns[4].knobs[2].input", 0xBB), Ok((2, "eq_mid")));
        assert!(xone_k2_row("XoneK2.controllers[0xE].columns[1].fader.input", 0xBE).unwrap_err().contains("effect unit"));
        assert!(xone_k2_row("XoneK2.controllers[0xE].columns[2].knobs[3].input", 0xBE).unwrap_err().contains("Shift"));
        assert!(xone_k2_row("XoneK2.controllers[0xE].columns[2].fader.input", 0xBD).unwrap_err().contains("status 0xBD"));

        let dir = std::env::temp_dir().join(format!("retraktor-import-k2-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let xml = MIXXX.replace("Test-scripts.js", "Allen-and-Heath-Xone-K2-scripts.js").replace(
            "<controls>",
            "<controls><control><key>XoneK2.controllers[0xE].columns[2].fader.input</key><status>0xBE</status><midino>0x10</midino><options><Script-Binding/></options></control>",
        );
        let path = dir.join("k2.midi.xml");
        std::fs::write(&path, &xml).unwrap();
        let missing = convert(&read_source(&path).unwrap(), 1, "xonek2").unwrap();
        assert!(item(&missing, "control 1").note.contains("isn't next to the mapping"), "{}", missing.report());
        std::fs::write(dir.join("Allen-and-Heath-Xone-K2-scripts.js"), "var XoneK2 = {}; // altered").unwrap();
        std::fs::write(dir.join("midi-components-0.0.js"), "altered").unwrap();
        let altered = convert(&read_source(&path).unwrap(), 1, "xonek2").unwrap();
        assert!(item(&altered, "control 1").note.contains("differs from the known Mixxx version"), "{}", altered.report());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    const REKORDBOX: &str = "\u{feff}@file,1,Test DDJ\r\n#name,function,type,input,deck1,deck2,deck3,deck4,output,deck1,deck2,deck3,deck4,option,comment\r\n,,,,,,,,,,,,,,\r\n# Browser,,,,\r\nBrowse,Browse,Rotary,B640,,,,,,,,,,,\"Browse (Track, Folder)\"\r\nLoad,Load,Button,,9646,9647,,,,,,,,,Load\r\nShift,Shift,Button,903F,0,1,,,,,,,,RO,Shift\r\nPlayPause,PlayPause,Button,900B,0,1,,,900B,0,1,,,Fast;Priority=50,Play\r\nPlayPause,PlayPause+Shift,Button,9047,0,1,,,9047,0,1,,,Fast,Play\r\nCue,Cue,Button,900C,0,1,,,,,,,,,Cue\r\nChannelFader,ChannelFader,KnobSliderHiRes,B013,0,1,,,,,,,,,CH Fader\r\nPAD1_HotCue,PAD1_PadMode1,Pad,9000,7,9,,,,,,,,Fast,HOT CUE1\r\nPAD1_HotCue+Shift,PAD1_PadMode1+Shift,Pad,9000,8,10,,,,,,,,Fast,Delete HOT CUE1\r\nMasterLevel,MasterLevel,KnobSliderHiRes,B608,,,,,,,,,,,No function assigned here\r\nOdd,Odd,Button,9001,0,,,,,,,,,Dual,\r\nChannelLevel,ChannelLevel,Indicator,,,,,,B002,0,1,,,RO,Level\r\n";

    #[test]
    fn rekordbox_rows_resolve_decks_shift_inputs_and_skip_deletes() {
        let c = convert(&source(REKORDBOX), 1, "ddj").unwrap();
        let report = c.report();
        assert!(item(&c, "line 5").source.contains("\"Browse (Track, Folder)\""), "quoted field: {report}");
        assert_eq!(item(&c, "line 7").coverage, Coverage::Merged, "{report}");
        assert!(item(&c, "line 8 deck 2").note.contains("its LED output isn't sent"), "{report}");
        assert!(item(&c, "line 10").note.contains("CDJ-style cue"), "{report}");
        assert!(item(&c, "line 13").note.contains("never imported"), "{report}");
        assert!(item(&c, "line 14").note.contains("No function assigned"), "{report}");
        assert!(item(&c, "line 15").note.contains("option \"Dual\""), "{report}");
        assert!(item(&c, "line 16").note.contains("indicator"), "{report}");
        let mut e = RuleEngine::default();
        e.replace_rules(parse_rules(c.draft.as_deref().unwrap()).unwrap());
        let trigger = |verb, index, arg: &str| RuleCommand::Trigger { verb, index, arg: arg.into() };
        assert_eq!(e.handle(&press("midi.ddj.ch2.note11", true)), Some(vec![trigger("play", 2, "")]), "deck 2 = channel offset 1");
        assert_eq!(e.handle(&press("midi.ddj.ch1.note71", true)), Some(vec![trigger("play", 1, "")]), "the shifted play is its own input");
        assert_eq!(e.handle(&press("midi.ddj.ch7.note71", true)), Some(vec![trigger("load", 2, "")]), "per-deck address");
        assert_eq!(e.handle(&press("midi.ddj.ch10.note0", true)), Some(vec![trigger("hotcue", 2, "1")]));
        assert_eq!(e.handle(&press("midi.ddj.ch11.note0", true)), None, "the delete pad has no rule");
        assert_eq!(e.handle(&SurfaceEvent::Encoder { control: "midi.ddj.ch7.cc64".into(), delta: 1 }), Some(vec![trigger("browse_down", 0, "")]));
    }

    #[test]
    fn a_header_only_rekordbox_export_gives_no_draft() {
        let c = convert(&source("@file,1,Midi Through"), 1, "midi_through").unwrap();
        assert!(c.draft.is_none() && c.items.is_empty() && c.assignments == 0);
        assert!(c.empty_reason.as_deref().unwrap().contains("header-only export"));
        assert_eq!(csv_fields("a,\"b,\"\"c\"\"\",d"), vec!["a", "b,\"c\"", "d"]);
    }

    #[test]
    fn merging_refuses_collisions_and_keeps_existing_rules() {
        let draft = convert(&source(&fixtures::traktor_three_devices()), 2, "xonek2").unwrap().draft.unwrap();
        assert_eq!(merge_draft(None, &draft).unwrap(), draft);
        let existing = r#"{"format": "retraktor-controller-rules", "version": 1, "comment": "mine",
            "rules": [{"input": "surface.left.shift", "do": [{"action": "modifier", "name": "shift", "mode": "hold"}]},
                      {"input": "midi.other.ch1.note1", "do": [{"action": "play", "deck": 1}]}]}"#;
        let merged = merge_draft(Some(existing), &draft).unwrap();
        let set = parse_rules(&merged).unwrap();
        assert_eq!(set.rule_count(), 12);
        assert!(merged.contains("\"mine\"") && merged.find("surface.left.shift").unwrap() < merged.find("midi.xonek2.ch14.note14").unwrap());

        let clash = r#"{"format": "retraktor-controller-rules", "version": 1,
            "midi_cc_buttons": ["midi.xonek2.ch14.cc20"],
            "rules": [{"input": "midi.xonek2.ch14.note14", "do": [{"action": "play", "deck": 1}]},
                      {"input": "midi.xonek2.ch1.note1", "do": [{"action": "modifier", "name": "m1", "mode": "hold"}]}]}"#;
        let errors = merge_draft(Some(clash), &draft).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("midi.xonek2.ch14.note14 already has rules")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("modifier \"m1\" on midi.xonek2")), "{errors:?}");
        assert!(errors.iter().any(|e| e.contains("midi.xonek2.ch14.cc20 is read as a button in the rules file, but the draft reads it as twos_complement")), "{errors:?}");
        // A CC the rules file only declares (no rule uses it) still collides with the draft's absolute knob.
        let mixer = convert(&source(&fixtures::traktor_three_devices()), 3, "mixer").unwrap().draft.unwrap();
        let declared = r#"{"format": "retraktor-controller-rules", "version": 1, "midi_encoders": {"midi.mixer.ch1.cc2": "offset_64"},
            "rules": [{"input": "midi.other.ch1.note1", "do": [{"action": "play", "deck": 1}]}]}"#;
        let errors = merge_draft(Some(declared), &mixer).unwrap_err();
        assert!(errors.iter().any(|e| e.contains("midi.mixer.ch1.cc2 is read as offset_64 in the rules file, but the draft reads it as an absolute knob or fader")), "{errors:?}");
        assert!(merge_draft(Some("{\"format\": 1}"), &draft).unwrap_err()[0].contains("fix them"));
    }

    /// Pad lights from a draft are kept after the file's own; a pad the file already colours is refused.
    #[test]
    fn merging_keeps_draft_pad_lights_and_refuses_coloured_pads() {
        let file = |input: &str, leds: &str| format!(r#"{{"format": "retraktor-controller-rules", "version": 1, "rules": [{{"input": "{input}", "do": [{{"action": "play", "deck": 1}}]}}], "leds": [{leds}]}}"#);
        let existing = file("midi.a.ch1.note1", r#"{"control": "surface.left.pads.1", "colour": "red"}"#);
        let draft = file("midi.b.ch1.note1", r#"{"control": "surface.left.pads.2", "colour": "blue"}, {"control": "surface.left.pads.3", "colour": "green"}"#);
        let merged = merge_draft(Some(&existing), &draft).unwrap();
        assert_eq!(parse_rules(&merged).unwrap().led_count(), 3);
        let (red, blue, green) = (merged.find("\"red\"").unwrap(), merged.find("\"blue\"").unwrap(), merged.find("\"green\"").unwrap());
        assert!(red < blue && blue < green, "{merged}");
        let errors = merge_draft(Some(&merged), &file("midi.c.ch1.note1", r#"{"control": "surface.left.pads.2", "colour": "white"}"#)).unwrap_err();
        assert_eq!(errors, vec!["pad light surface.left.pads.2 is already coloured by the rules file".to_string()]);
    }

    #[test]
    fn rules_file_replacement_never_clobbers_and_keeps_a_backup() {
        let dir = std::env::temp_dir().join(format!("retraktor-import-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("controller-rules.json");
        assert_eq!(replace_rules_file(&path, None, "first"), Ok(None));
        assert!(replace_rules_file(&path, None, "again").unwrap_err().contains("changed on disk"));
        assert!(replace_rules_file(&path, Some("stale"), "x").unwrap_err().contains("changed on disk"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first");
        // Somebody else's file on the first temporary name is left alone.
        let foreign = dir.join(format!(".controller-rules.json.import-{}-0.tmp", std::process::id()));
        std::fs::write(&foreign, "not ours").unwrap();
        let backup = replace_rules_file(&path, Some("first"), "second").unwrap().unwrap();
        assert_eq!((std::fs::read_to_string(&path).unwrap(), std::fs::read_to_string(&backup).unwrap()), ("second".to_string(), "first".to_string()));
        assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "not ours");
        std::fs::remove_file(&foreign).unwrap();
        let draft = dir.join("draft.json");
        write_new_file(&draft, "one").unwrap();
        assert_eq!(write_new_file(&draft, "two").unwrap_err().kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read_to_string(&draft).unwrap(), "one");
        let leftovers: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert!(leftovers.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Another program saves after the check but before the swap: its version ends up active and nothing is lost.
    #[test]
    fn a_save_racing_the_swap_is_restored_and_kept() {
        let dir = std::env::temp_dir().join(format!("retraktor-import-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("controller-rules.json");
        std::fs::write(&path, "saved by someone else").unwrap();
        let temp = new_sibling(&path, "import", ".tmp", "merged", None).unwrap();
        let error = install(&path, "what the preview saw", &temp, "merged").unwrap_err();
        assert!(error.contains("left as the other program saved it"), "{error}");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "saved by someone else");
        let others: Vec<String> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n != "controller-rules.json").collect();
        assert!(others.is_empty(), "the regenerable import should be the only thing discarded: {others:?}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Real-corpus failures: an out-of-range CC number in one Mixxx row, a port whose name starts with "cc", and a
    /// condition whose setter was dropped. Each costs only its own rows; the rest of the draft stays valid.
    #[test]
    fn bad_rows_and_dropped_setters_cost_only_their_own_controls() {
        let xml = MIXXX.replace("<midino>0x13</midino><options><fourteen-bit-msb/>", "<midino>0xA4</midino><options><normal/>");
        let c = convert(&source(&xml), 1, "pad").unwrap();
        assert!(item(&c, "control 3").note.contains("isn't a MIDI message"), "{}", c.report());
        assert!(c.draft.is_some());
        let csv = REKORDBOX.replace("ChannelFader,ChannelFader,KnobSliderHiRes,B013", "ChannelFader,ChannelFader,KnobSliderHiRes,B0A4");
        let c = convert(&source(&csv), 1, "ddj").unwrap();
        assert!(item(&c, "line 11").note.contains("isn't a MIDI address"), "{}", c.report());

        let c = convert(&source(&fixtures::traktor_three_devices()), 2, "cc121").unwrap();
        let draft = c.draft.unwrap();
        assert!(!draft.contains("midi_cc_buttons"), "notes on a port named cc121 were declared as CC buttons: {draft}");
        assert!(draft.contains("\"midi.cc121.ch14.cc20\":\"twos_complement\""), "{draft}");

        // M1's only setter shares CC 5 with a knob, so both are dropped; the row that tests M1 goes too.
        let rows = [
            fixtures::Row { address: "Ch01.CC.005", command: 2548, control: 0, mode: 2, target: 0, set: 1, comment: "shift", when: &[] },
            fixtures::Row { address: "Ch01.CC.005", command: 303, control: 1, mode: 3, target: 0, set: 0, comment: "eq", when: &[] },
            fixtures::Row { address: "Ch01.Note.D3", command: 100, control: 0, mode: 1, target: 0, set: 0, comment: "shifted play", when: &[(2548, 1)] },
            fixtures::Row { address: "Ch01.Note.E3", command: 125, control: 0, mode: 1, target: 0, set: 0, comment: "sync", when: &[] },
            fixtures::Row { address: "Ch01.Note.F3", command: 125, control: 0, mode: 1, target: 0, set: 0, comment: "repeat", when: &[] },
        ];
        let c = convert(&source(&fixtures::tsi(&[("Generic MIDI", "setter", "P", &[], &rows)])), 1, "p").unwrap();
        assert!(item(&c, "row 2").note.contains("no imported row on this device sets"), "{}", c.report());
        assert!(!c.report().contains("shared by every controller"));
        assert_eq!(c.rule_count, 2, "{}", c.report());

        // M3 has no assignment anywhere in this mapping, so it is always 0: "M3 = 0" drops away, "M3 = 1" never runs.
        let rows = [
            fixtures::Row { address: "Ch01.Note.C3", command: 100, control: 0, mode: 1, target: 0, set: 0, comment: "play", when: &[(2550, 0)] },
            fixtures::Row { address: "Ch01.Note.D3", command: 125, control: 0, mode: 1, target: 0, set: 0, comment: "sync", when: &[(2550, 1)] },
        ];
        let c = convert(&source(&fixtures::tsi(&[("Generic MIDI", "constant", "P", &[], &rows)])), 1, "p").unwrap();
        assert_eq!(item(&c, "row 0").coverage, Coverage::Translated, "{}", c.report());
        assert!(item(&c, "row 1").note.contains("nothing in this mapping sets M3, so it stays 0"), "{}", c.report());
        let draft = c.draft.unwrap();
        assert!(draft.contains("\"input\": \"midi.p.ch1.note48\", \"do\""), "the always-true condition should be gone: {draft}");
    }

    /// A symlinked rules file stays a link and the target keeps its private permissions; a dangling link is refused.
    #[cfg(unix)]
    #[test]
    fn a_symlinked_private_rules_file_stays_a_link_with_its_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("retraktor-import-link-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).unwrap();
        let target = dir.join("real/rules.json");
        std::fs::write(&target, "old").unwrap();
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
        let link = dir.join("controller-rules.json");
        std::os::unix::fs::symlink("real/rules.json", &link).unwrap();
        let backup = replace_rules_file(&link, Some("old"), "new").unwrap().unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "the link was replaced by a file");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::metadata(&target).unwrap().permissions().mode() & 0o777, 0o600);
        assert_eq!((backup.parent().unwrap().file_name().unwrap(), std::fs::read_to_string(&backup).unwrap().as_str()), (std::ffi::OsStr::new("real"), "old"));

        let dangling = dir.join("dangling.json");
        std::os::unix::fs::symlink("real/missing.json", &dangling).unwrap();
        assert!(replace_rules_file(&dangling, None, "x").unwrap_err().contains("symbolic link"));
        assert!(!dir.join("real/missing.json").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn validator_problems_map_back_to_their_controls() {
        let owners = vec!["midi.p.ch1.note1".to_string(), "midi.p.ch1.note2".to_string()];
        let buttons = vec!["midi.p.ch1.cc8".to_string()];
        let problem = |location: &str, message: &str| Diagnostic { location: location.into(), message: message.into() };
        let rejected = rejected_inputs(
            &[problem("rules[1] \"x\".do[0]", "bad"), problem("midi_cc_buttons[0]", "twice"), problem("midi_encoders.\"midi.p.ch1.cc9\"", "odd"), problem("file", "whole file")],
            &owners,
            &buttons,
        );
        assert_eq!(rejected.into_iter().collect::<Vec<_>>(), vec![("midi.p.ch1.cc8".into(), "twice".into()), ("midi.p.ch1.cc9".into(), "odd".into()), ("midi.p.ch1.note2".into(), "bad".into())]);
    }

    /// Settings ReTraktor can't reproduce are reported, not converted into a different control: an auto-repeat or
    /// inverted Traktor button, an inverted zoom encoder, inverted or accumulating Mixxx library scrolling.
    #[test]
    fn reversed_repeating_and_accumulating_controls_are_reported() {
        let text = fixtures::tsi(&[(
            "Generic MIDI",
            "odd",
            "P",
            &["Ch01.CC.020"],
            &[
                fixtures::Row { address: "Ch01.Note.C3", command: 100, control: 0, mode: 1, target: 0, set: 0, comment: "", when: &[] },
                fixtures::Row { address: "Ch01.CC.020", command: 4162, control: 2, mode: 4, target: 0, set: 0, comment: "", when: &[] },
            ],
        )]);
        // Turn on auto-repeat for the button and invert for the encoder in the encoded settings.
        let src = source(&text);
        let Kind::Traktor { devices, .. } = &src.kind else { unreachable!() };
        let mut dev_rows: Vec<Vec<u8>> = devices[0].rows.iter().map(|r| r.cmad.clone()).collect();
        dev_rows[0][19] = 1;
        dev_rows[1][23] = 1;
        let mut c = Builder { port: "p".into(), ..Default::default() };
        let dev = TsiDev { rows: devices[0].rows.iter().zip(&dev_rows).map(|(r, cmad)| TsiRow { binding: r.binding, io: r.io, command: r.command, cmad: cmad.clone() }).collect(), name: "Generic MIDI".into(), bindings: devices[0].bindings.clone(), encoder_modes: devices[0].encoder_modes.clone(), ..Default::default() };
        convert_traktor(&mut c, &dev, &[]);
        assert!(c.items[0].note.contains("Auto Repeat"), "{:?}", c.items[0]);
        assert!(c.items[1].note.contains("inverted zoom encoder"), "{:?}", c.items[1]);

        for (option, why) in [("<invert/><selectknob/>", "inverted library scrolling"), ("<rot64/>", "<rot64/>")] {
            let xml = MIXXX.replace("<options><selectknob/></options>", &format!("<options>{option}</options>"));
            let c = convert(&source(&xml), 1, "pad").unwrap();
            assert!(item(&c, "control 6").note.contains(why), "{option}: {}", c.report());
        }
        let xml = MIXXX.replace("<options><selectknob/></options>", "<options><spread64/></options>");
        let c = convert(&source(&xml), 1, "pad").unwrap();
        assert!(item(&c, "control 6").coverage == Coverage::Approximated && item(&c, "control 6").note.contains("offset_64"), "{}", c.report());
    }

    #[test]
    fn rekordbox_device_title_is_the_third_field() {
        let c = convert(&source("@file,1,DDJ-FLX4,,,,,,,,,,,,\n"), 1, "ddj").unwrap();
        assert_eq!(c.device_name, "DDJ-FLX4");
    }
}
