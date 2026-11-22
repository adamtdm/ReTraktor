//! Controller rules: one versioned JSON file of creative mappings (held Shift, latched layers, conditions,
//! several actions per control, macro knobs, MX2 pad colours) that the app runs before its simple mappings.
//! The app, `retraktor-cli validate-mapping` and `retraktor-cli mapping-reference` all use this parser and
//! catalogue. The author's contract is `docs/controller-rules.md`.

use crate::{MX2_BUTTON_NAMES, MX2_ENCODER_NAMES, MX2_POT_FADER_NAMES};
use retraktor_core::SurfaceEvent;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt::{self, Write as _};
use std::io::Read as _;
use std::path::{Path, PathBuf};

pub const RULES_FORMAT: &str = "retraktor-controller-rules";
pub const RULES_VERSION: u64 = 1;
/// A complete, valid MX2 rules file (`retraktor-cli mapping-reference --example`).
pub const EXAMPLE_RULES: &str = include_str!("controller_rules_example.json");
/// Modifier values run 0..=7 like Traktor's modifiers; 0 means off.
pub const MAX_MODIFIER_VALUE: u64 = 7;
pub const DECKS: (u64, u64) = (1, 4);
pub const FX_UNITS: (u64, u64) = (1, 2);
pub const PAD_MODES: [&str; 4] = ["hotcues", "stems", "pattern", "loop"];
/// Deck count and waveforms for `layout`: regular = In Deck, each deck shows its own waveform; stacked = Stacked, the
/// waveforms run full width above the decks.
pub const LAYOUTS: [&str; 4] = ["regular2", "regular4", "stacked2", "stacked4"];
/// MX2 pad colour codes, the same bytes the app writes in Output Report `0x80`.
pub const LED_COLOURS: [(&str, u8); 9] = [
    ("off", 0x00),
    ("blue", 0x2E),
    ("green", 0x1E),
    ("red", 0x06),
    ("amber", 0x16),
    ("orange", 0x0E),
    ("cyan", 0x22),
    ("magenta", 0x36),
    ("white", 0x4A),
];
const MX2_JOG_NAMES: [&str; 2] = ["surface.left.jogwheel.rotation", "surface.right.jogwheel.rotation"];
const DEFAULT_STEP: f32 = 0.02;
const MAX_FILE_BYTES: usize = 1 << 20;
/// Problems listed before validation stops, so a badly broken file still answers at once.
const MAX_DIAGNOSTICS: usize = 100;
const NAME_RULE: &str = "modifier names use 1 to 32 lowercase letters, digits or _";

/// `~/.config/retraktor/controller-rules.json`.
pub fn default_rules_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/retraktor/controller-rules.json")
}

/// Reads a rules file, refusing one over the 1 MiB limit without reading the rest of it into memory.
pub fn read_rules_file(path: &Path) -> std::io::Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?.take(MAX_FILE_BYTES as u64 + 1).read_to_string(&mut text)?;
    if text.len() > MAX_FILE_BYTES {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "the file is larger than 1 MiB"));
    }
    Ok(text)
}

/// How a MIDI CC's 7-bit values are read. A CC is an absolute knob or fader unless the file's `midi_encoders`
/// names its relative encoding; the app reads it that way from the very first message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcMode {
    /// Knob or fader position: value / 127.
    Absolute,
    /// Relative, centred on 64: 65 is +1, 63 is -1.
    Offset64,
    /// Relative, two's complement: 1 is +1, 127 is -1.
    TwosComplement,
    /// Relative, sign bit: 1 is +1, 65 is -1.
    SignBit,
    /// A button sent as a CC (listed in `midi_cc_buttons`): any value above 0 is a press, 0 the release.
    Button,
}

pub const CC_ENCODINGS: [(&str, CcMode); 3] = [("offset_64", CcMode::Offset64), ("twos_complement", CcMode::TwosComplement), ("sign_bit", CcMode::SignBit)];

impl CcMode {
    /// Signed ticks in one relative CC value (0 for `Absolute`).
    pub fn ticks(self, value: u8) -> i64 {
        let v = i64::from(value & 0x7F);
        match self {
            CcMode::Absolute | CcMode::Button => 0,
            CcMode::Offset64 => v - 64,
            CcMode::TwosComplement if v < 64 => v,
            CcMode::TwosComplement => v - 128,
            CcMode::SignBit if v < 64 => v,
            CcMode::SignBit => -(v - 64),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionKind {
    /// Runs once per button press.
    Trigger,
    /// A 0..=1 value: follows a knob or fader through `range`, moves by `step` per encoder tick, or jumps to `value` on a press.
    Continuous,
    /// Needs a relative encoder: one move per tick, direction from the turn.
    Encoder,
    /// Changes a per-device modifier (layer) value.
    Modifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    Deck,
    FxUnit,
}

#[derive(Debug, Clone, Copy)]
pub struct ActionSpec {
    pub name: &'static str,
    /// The app's own action id it runs (a button verb or a value target).
    pub verb: &'static str,
    pub kind: ActionKind,
    pub scope: Scope,
    /// Inclusive range of `slot` for actions that take one.
    pub slots: Option<(u64, u64)>,
    pub about: &'static str,
}

const fn spec(name: &'static str, verb: &'static str, kind: ActionKind, scope: Scope, slots: Option<(u64, u64)>, about: &'static str) -> ActionSpec {
    ActionSpec { name, verb, kind, scope, slots, about }
}

use ActionKind::{Continuous, Encoder, Modifier, Trigger};
use Scope::{Deck, FxUnit, Global};

/// Every action a rule can use; the app executes each of these.
pub const ACTIONS: &[ActionSpec] = &[
    spec("play", "play", Trigger, Deck, None, "Play / pause"),
    spec("cue", "cue", Trigger, Deck, None, "Jump to hotcue 1; if it is empty, store the playhead there"),
    spec("cueplay", "cueplay", Trigger, Deck, None, "Like cue, then play"),
    spec("sync", "sync", Trigger, Deck, None, "Sync on / off"),
    spec("flux", "flux", Trigger, Deck, None, "Flux (slip) on / off"),
    spec("keylock", "keylock", Trigger, Deck, None, "Key lock on / off"),
    spec("loop", "loop", Trigger, Deck, None, "Loop on / off at the current loop size"),
    spec("loop_half", "loop_half", Trigger, Deck, None, "Halve the loop size"),
    spec("loop_double", "loop_double", Trigger, Deck, None, "Double the loop size"),
    spec("hotcue", "hotcue", Trigger, Deck, Some((1, 8)), "Jump to hotcue `slot`; if it is empty, store the playhead there"),
    spec("stem", "stem", Trigger, Deck, Some((1, 4)), "Mute / unmute stem `slot`: 1 drums, 2 bass, 3 other, 4 vocals"),
    spec("stem_view", "stem_view", Trigger, Deck, None, "Show / hide the deck's stems"),
    spec("pad_mode", "pads", Trigger, Deck, None, "Switch the deck's pads to `mode`: hotcues, stems, pattern or loop"),
    spec("load", "load", Trigger, Deck, None, "Load the selected library track"),
    spec("filter_on", "filter_on", Trigger, Deck, None, "Channel filter on / off"),
    spec("headphone", "headphone", Trigger, Deck, None, "Headphone cue on / off"),
    spec("browse_up", "browse_up", Trigger, Global, None, "Select the previous library track"),
    spec("browse_down", "browse_down", Trigger, Global, None, "Select the next library track"),
    spec("fx_on", "fx_on", Trigger, FxUnit, None, "FX unit on / off (on-screen FX panel only, see fx_mix)"),
    spec("tempo_master", "tempo_master", Trigger, Deck, None, "Make this deck the tempo master (it stays master when pressed again)"),
    spec("layout", "layout", Trigger, Global, None, "Switch to `mode`: regular2, regular4 (2 or 4 Decks, Waveforms In Deck), stacked2, stacked4 (2 or 4 Decks, Waveforms Stacked)"),
    spec("browser_only", "browser_only", Trigger, Global, None, "Show the browser layout; press again to go back to the layout before"),
    spec("volume", "volume", Continuous, Deck, None, "Channel fader: 0 silent, 1 full"),
    spec("gain", "gain", Continuous, Deck, None, "Channel gain knob: 0.5 is the centre"),
    spec("eq_hi", "eq_hi", Continuous, Deck, None, "EQ high: 0.5 is neutral"),
    spec("eq_mid", "eq_mid", Continuous, Deck, None, "EQ mid: 0.5 is neutral"),
    spec("eq_low", "eq_low", Continuous, Deck, None, "EQ low: 0.5 is neutral"),
    spec("filter", "filter", Continuous, Deck, None, "Channel filter: 0.5 off, lower low-pass, higher high-pass (heard while filter_on)"),
    spec("pitch", "pitch", Continuous, Deck, None, "Tempo fader: 0 is -8 %, 0.5 is 0 %, 1 is +8 %"),
    spec("xfader", "xfader", Continuous, Global, None, "Crossfader: 0 left, 1 right"),
    spec("master_volume", "master_volume", Continuous, Global, None, "Main volume"),
    spec("cue_mix", "cue_mix", Continuous, Global, None, "Headphone mix: 0 cue, 1 main"),
    spec("cue_vol", "cue_vol", Continuous, Global, None, "Headphone volume"),
    spec("fx_mix", "fx_mix", Continuous, FxUnit, None, "FX unit dry / wet knob (on-screen FX panel; the FX units are not in the audio path yet)"),
    spec("browse", "browse", Encoder, Global, None, "Scroll the library: turning right selects the next track"),
    spec("loop_size", "loop_size", Encoder, Deck, None, "Turning right doubles the loop size, left halves it"),
    spec("waveform_zoom", "waveform_zoom", Encoder, Deck, None, "Turning right zooms the deck's scrolling waveform in (fewer seconds), left out"),
    spec("modifier", "modifier", Modifier, Global, None, "Change a modifier (layer) on this device, see MODIFIERS"),
];

/// What a control sends, which decides the actions it can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputKind {
    Button,
    Fader,
    Encoder,
    /// A MIDI CC: a knob or fader unless the file's `midi_encoders` makes it an encoder.
    Cc,
}

impl InputKind {
    fn describe(self) -> &'static str {
        match self {
            InputKind::Button => "button",
            InputKind::Fader => "knob or fader",
            InputKind::Encoder => "encoder",
            InputKind::Cc => "MIDI CC (knob, fader or encoder)",
        }
    }
}

/// The device a control ID belongs to: `mx2`, `midi.<port>` or `hid.<pid>`. Modifiers are kept per device.
pub fn device_of(control: &str) -> Option<String> {
    if control.starts_with("surface.") {
        return Some("mx2".into());
    }
    let mut parts = control.splitn(3, '.');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(kind @ ("midi" | "hid")), Some(dev), Some(_)) => Some(format!("{kind}.{dev}")),
        _ => None,
    }
}

/// Whether a control ID is a MIDI CC (`midi.<port>.ch<N>.cc<N>`), judged by its control part, so a port named
/// `cc121` doesn't make its notes look like CCs.
pub fn is_midi_cc(id: &str) -> bool {
    id.starts_with("midi.") && id.rsplit('.').next().is_some_and(|control| control.starts_with("cc"))
}

/// Every Traktor MX2 control ID with its kind, in hardware order.
pub fn mx2_controls() -> impl Iterator<Item = (&'static str, InputKind)> {
    let buttons = MX2_BUTTON_NAMES.iter().map(|n| (*n, InputKind::Button));
    let faders = MX2_POT_FADER_NAMES.iter().map(|n| (*n, InputKind::Fader));
    let encoders = MX2_ENCODER_NAMES.iter().chain(MX2_JOG_NAMES.iter()).map(|n| (*n, InputKind::Encoder));
    buttons.chain(faders).chain(encoders)
}

/// The kind of control `id` is, and its device; the error says how to fix the ID.
pub fn classify_input(id: &str) -> Result<(InputKind, String), String> {
    if id.starts_with("surface.") {
        return match mx2_controls().find(|(n, _)| *n == id) {
            Some((_, kind)) => Ok((kind, "mx2".into())),
            None => Err(format!("\"{id}\" is not a Traktor MX2 control{}; `retraktor-cli mapping-reference` lists them", did_you_mean(id, mx2_controls().map(|(n, _)| n)))),
        };
    }
    let parts: Vec<&str> = id.split('.').collect();
    let device = |dev: &str| -> Result<(), String> {
        if !dev.is_empty() && dev.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            Ok(())
        } else {
            Err(format!("device \"{dev}\" must be lowercase letters, digits or _ (copy the ID from Preferences > Controllers)"))
        }
    };
    match parts.as_slice() {
        ["midi", dev, ch, ctl] => {
            device(dev)?;
            number(ch, "ch", 1, 16, "MIDI channel")?;
            let kind = if ctl.starts_with("note") {
                number(ctl, "note", 0, 127, "note")?;
                InputKind::Button
            } else if ctl.starts_with("cc") {
                number(ctl, "cc", 0, 127, "CC")?;
                InputKind::Cc
            } else if *ctl == "bend" {
                InputKind::Fader
            } else {
                return Err(format!("\"{ctl}\" must be note0..note127, cc0..cc127 or bend"));
            };
            Ok((kind, format!("midi.{dev}")))
        }
        ["hid", pid, report, rest @ ..] => {
            if pid.len() != 4 || !pid.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
                return Err(format!("HID product ID \"{pid}\" must be 4 lowercase hex digits, e.g. 2200"));
            }
            number(report, "r", 0, 255, "report ID")?;
            let kind = match rest {
                [byte, bit] => {
                    number(byte, "b", 0, 255, "byte")?;
                    number(&format!("bit{bit}"), "bit", 0, 7, "bit")?;
                    InputKind::Button
                }
                [byte] => {
                    number(byte, "a", 0, 255, "byte")?;
                    InputKind::Fader
                }
                _ => return Err("NI HID IDs look like hid.2200.r1.b0.2 (button) or hid.2200.r1.a2 (knob or fader)".into()),
            };
            Ok((kind, format!("hid.{pid}")))
        }
        _ => Err(format!(
            "\"{id}\" is not a control ID: use surface.* (Traktor MX2), midi.<port>.ch<1-16>.<noteN|ccN|bend> or hid.<pid>.r<report>.<bN.bit|aN>"
        )),
    }
}

/// Parses `<prefix><n>` with `lo <= n <= hi`.
fn number(part: &str, prefix: &str, lo: u32, hi: u32, what: &str) -> Result<u32, String> {
    // No leading zeros: the app writes ch1 and note3, so ch01 or note003 could never match.
    part.strip_prefix(prefix)
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) && (n.len() == 1 || !n.starts_with('0')))
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (lo..=hi).contains(n))
        .ok_or_else(|| format!("{what} in \"{part}\" must be {prefix}{lo}..{prefix}{hi}, without leading zeros"))
}

/// One problem in a rules file, with where it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub location: String,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// What the app must do for an event a rule took. `index` is the deck or FX unit (1-based), 0 when global.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleCommand {
    /// A button action, with its slot number or pad mode in `arg`.
    Trigger { verb: &'static str, index: u8, arg: String },
    /// Set a value target to `value` (0..=1).
    Set { target: &'static str, index: u8, value: f32 },
    /// Move a value target by `delta`, kept within 0..=1.
    Adjust { target: &'static str, index: u8, delta: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModifierMode {
    Hold,
    Toggle,
    Set,
}

#[derive(Debug, Clone)]
enum Action {
    Trigger { verb: &'static str, index: u8, arg: String },
    Continuous { target: &'static str, index: u8, range: (f32, f32), step: f32, value: Option<f32> },
    Encoder { name: &'static str, index: u8 },
    Modifier { name: String, mode: ModifierMode, value: u8 },
}

type Conditions = Vec<(String, u8)>;

#[derive(Debug, Clone)]
struct Rule {
    location: String,
    input: String,
    /// The input's kind as validated (a MIDI CC resolved to fader or encoder).
    kind: InputKind,
    device: String,
    when: Conditions,
    actions: Vec<Action>,
}

#[derive(Debug, Clone)]
struct Led {
    location: String,
    /// Index in `MX2_BUTTON_NAMES`, which is also the LED byte for buttons 0..=73.
    index: usize,
    when: Conditions,
    code: u8,
}

/// A validated rules file.
#[derive(Debug, Clone, Default)]
pub struct RuleSet {
    rules: Vec<Rule>,
    leds: Vec<Led>,
    by_input: HashMap<String, Vec<usize>>,
    /// Every MIDI CC the file uses or declares, with how to read it.
    midi_cc_modes: Vec<(String, CcMode)>,
}

impl RuleSet {
    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }

    pub fn led_count(&self) -> usize {
        self.leds.len()
    }

    /// MIDI CC control IDs the file uses or declares, with how the MIDI reader must decode them.
    pub fn midi_cc_modes(&self) -> &[(String, CcMode)] {
        &self.midi_cc_modes
    }
}

#[derive(Default)]
struct Checker {
    errors: Vec<Diagnostic>,
}

impl Checker {
    fn err(&mut self, location: &str, message: impl Into<String>) {
        self.errors.push(Diagnostic { location: location.to_string(), message: message.into() });
    }

    fn fields(&mut self, obj: &Map<String, Value>, loc: &str, allowed: &[&str]) {
        for key in obj.keys().filter(|k| !allowed.contains(&k.as_str())) {
            self.err(&format!("{loc}.{key}"), format!("unknown field{}; allowed here: {}", did_you_mean(key, allowed.iter().copied()), allowed.join(", ")));
        }
    }

    /// A whole number in `lo..=hi`; `default` is used when the field is missing, otherwise it is required.
    fn whole(&mut self, obj: &Map<String, Value>, loc: &str, key: &str, (lo, hi): (u64, u64), default: Option<u64>) -> Option<u64> {
        match obj.get(key) {
            None if default.is_some() => default,
            None => {
                self.err(loc, format!("needs \"{key}\" ({lo} to {hi})"));
                None
            }
            Some(v) => {
                let n = v.as_u64().filter(|n| (lo..=hi).contains(n));
                if n.is_none() {
                    self.err(&format!("{loc}.{key}"), format!("{v} is out of range: use a whole number from {lo} to {hi}"));
                }
                n
            }
        }
    }

    fn choice<'a>(&mut self, obj: &Map<String, Value>, loc: &str, key: &str, options: &[&'a str]) -> Option<&'a str> {
        let Some(given) = obj.get(key) else {
            self.err(loc, format!("needs \"{key}\": one of {}", options.join(", ")));
            return None;
        };
        let found = given.as_str().and_then(|s| options.iter().find(|o| **o == s).copied());
        if found.is_none() {
            self.err(&format!("{loc}.{key}"), format!("{given} is not one of {}{}", options.join(", "), did_you_mean(given.as_str().unwrap_or(""), options.iter().copied())));
        }
        found
    }

    fn unit(&mut self, loc: &str, v: &Value) -> Option<f32> {
        let x = v.as_f64().filter(|x| (0.0..=1.0).contains(x)).map(|x| x as f32);
        if x.is_none() {
            self.err(loc, format!("{v} must be a number from 0 to 1"));
        }
        x
    }
}

fn valid_name(name: &str) -> bool {
    (1..=32).contains(&name.len()) && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Parses and validates a rules file; every problem found is reported, each with its location.
pub fn parse_rules(text: &str) -> Result<RuleSet, Vec<Diagnostic>> {
    let mut c = Checker::default();
    if text.len() > MAX_FILE_BYTES {
        c.err("file", "is larger than 1 MiB");
        return Err(c.errors);
    }
    let root: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            let message = e.to_string();
            let message = message.split(" at line ").next().unwrap_or(&message).to_string();
            c.err(&format!("line {}, column {}", e.line(), e.column()), format!("not valid JSON: {message}"));
            return Err(c.errors);
        }
    };
    let Some(top) = root.as_object() else {
        c.err("file", "must be a JSON object with \"format\", \"version\" and \"rules\"");
        return Err(c.errors);
    };
    c.fields(top, "file", &["format", "version", "comment", "midi_encoders", "midi_cc_buttons", "rules", "leds"]);
    if top.get("format").and_then(Value::as_str) != Some(RULES_FORMAT) {
        c.err("format", format!("must be \"{RULES_FORMAT}\""));
    }
    match top.get("version") {
        Some(v) if v.as_u64() == Some(RULES_VERSION) => {}
        Some(v) => c.err("version", format!("{v} is not supported; this build reads version {RULES_VERSION}")),
        None => c.err("version", format!("is required (use {RULES_VERSION})")),
    }
    let mut encoders: HashMap<String, CcMode> = HashMap::new();
    match top.get("midi_encoders") {
        None => {}
        Some(Value::Object(map)) => {
            let names: Vec<&str> = CC_ENCODINGS.iter().map(|(n, _)| *n).collect();
            for (id, encoding) in map {
                let at = format!("midi_encoders.\"{id}\"");
                match classify_input(id) {
                    Ok((InputKind::Cc, _)) => {}
                    Ok(_) => c.err(&at, "only MIDI CC IDs (midi.<port>.ch<N>.cc<N>) can be encoders"),
                    Err(e) => c.err(&at, e),
                }
                match CC_ENCODINGS.iter().find(|(n, _)| Some(*n) == encoding.as_str()) {
                    Some((_, mode)) => {
                        encoders.insert(id.clone(), *mode);
                    }
                    None => c.err(&at, format!("{encoding} is not one of {}{}", names.join(", "), did_you_mean(encoding.as_str().unwrap_or(""), names.iter().copied()))),
                }
            }
        }
        Some(_) => c.err("midi_encoders", "must be an object like {\"midi.xonek2.ch15.cc0\": \"twos_complement\"}"),
    }
    // CCs read as buttons share the encoders' map, so every declared CC has exactly one reading.
    match top.get("midi_cc_buttons") {
        None => {}
        Some(Value::Array(ids)) => {
            for (i, id) in ids.iter().enumerate() {
                let at = format!("midi_cc_buttons[{i}]");
                let Some(id) = id.as_str() else {
                    c.err(&at, "must be a MIDI CC ID string such as \"midi.mixer.ch1.cc8\"");
                    continue;
                };
                match classify_input(id) {
                    Ok((InputKind::Cc, _)) if encoders.contains_key(id) => c.err(&at, format!("\"{id}\" is listed twice: a CC is a button or an encoder, not both")),
                    Ok((InputKind::Cc, _)) => {
                        encoders.insert(id.to_string(), CcMode::Button);
                    }
                    Ok(_) => c.err(&at, "only MIDI CC IDs (midi.<port>.ch<N>.cc<N>) can be CC buttons"),
                    Err(e) => c.err(&at, e),
                }
            }
        }
        Some(_) => c.err("midi_cc_buttons", "must be a list like [\"midi.mixer.ch1.cc8\"]"),
    }
    let mut set = RuleSet::default();
    let mut stopped = false;
    match top.get("rules").and_then(Value::as_array) {
        Some(rules) => {
            for (i, rule) in rules.iter().enumerate() {
                if c.errors.len() >= MAX_DIAGNOSTICS {
                    stopped = true;
                    break;
                }
                set.rules.extend(parse_rule(&mut c, i, rule, &encoders));
            }
        }
        None => c.err("rules", "is required and must be a list of rules"),
    }
    match top.get("leds") {
        None => {}
        Some(Value::Array(leds)) => set.leds = leds.iter().enumerate().filter_map(|(i, l)| parse_led(&mut c, i, l)).collect(),
        Some(_) => c.err("leds", "must be a list"),
    }

    // A condition on a modifier that nothing on its device ever sets is almost always a typo.
    let mut set_on: HashMap<&str, Vec<&str>> = HashMap::new();
    for rule in &set.rules {
        for action in &rule.actions {
            if let Action::Modifier { name, .. } = action {
                set_on.entry(&rule.device).or_default().push(name);
            }
        }
    }
    let conditions = set.rules.iter().map(|r| (&r.location, r.device.as_str(), &r.when)).chain(set.leds.iter().map(|l| (&l.location, "mx2", &l.when)));
    for (loc, device, when) in conditions {
        let known = set_on.get(device).cloned().unwrap_or_default();
        for (name, _) in when.iter().filter(|(n, _)| !known.contains(&n.as_str())) {
            c.err(&format!("{loc}.when.{name}"), format!("no rule on device {device} sets modifier \"{name}\"{}", did_you_mean(name, known.iter().copied())));
        }
    }
    for (i, rule) in set.rules.iter().enumerate() {
        set.by_input.entry(rule.input.clone()).or_default().push(i);
    }
    // First match wins, so a rule after one with the same input and a subset of its conditions never runs.
    let mut hidden: Vec<(usize, Diagnostic)> = Vec::new();
    for indices in set.by_input.values() {
        for (k, &j) in indices.iter().enumerate() {
            let later = &set.rules[j];
            if let Some(&i) = indices[..k].iter().find(|&&i| set.rules[i].when.iter().all(|cond| later.when.contains(cond))) {
                let message = format!("never runs: {} has the same input and matches whenever this one does; put the more specific rule first", set.rules[i].location);
                hidden.push((j, Diagnostic { location: later.location.clone(), message }));
            }
        }
    }
    hidden.sort_by_key(|(j, _)| *j);
    c.errors.extend(hidden.into_iter().map(|(_, d)| d));

    if stopped || c.errors.len() > MAX_DIAGNOSTICS {
        c.errors.truncate(MAX_DIAGNOSTICS);
        c.err("file", format!("stopped after {MAX_DIAGNOSTICS} problems; fix these and check again"));
    }
    if !c.errors.is_empty() {
        return Err(c.errors);
    }
    let mut cc_modes: Vec<(String, CcMode)> = set.rules.iter().filter(|r| is_midi_cc(&r.input)).map(|r| (r.input.clone(), encoders.get(&r.input).copied().unwrap_or(CcMode::Absolute))).collect();
    cc_modes.extend(encoders);
    cc_modes.sort_by(|a, b| a.0.cmp(&b.0));
    cc_modes.dedup_by(|a, b| a.0 == b.0);
    set.midi_cc_modes = cc_modes;
    Ok(set)
}

fn parse_when(c: &mut Checker, loc: &str, v: Option<&Value>) -> Conditions {
    let Some(v) = v else { return Vec::new() };
    let Some(obj) = v.as_object() else {
        c.err(loc, "must be an object like {\"shift\": 1}");
        return Vec::new();
    };
    let mut when = Vec::new();
    for (name, value) in obj {
        let at = format!("{loc}.{name}");
        if !valid_name(name) {
            c.err(&at, NAME_RULE);
        } else if let Some(n) = value.as_u64().filter(|n| *n <= MAX_MODIFIER_VALUE) {
            when.push((name.clone(), n as u8));
        } else {
            c.err(&at, format!("{value} must be a whole number from 0 to {MAX_MODIFIER_VALUE}"));
        }
    }
    when
}

fn parse_rule(c: &mut Checker, i: usize, v: &Value, encoders: &HashMap<String, CcMode>) -> Option<Rule> {
    let Some(obj) = v.as_object() else {
        c.err(&format!("rules[{i}]"), "must be an object with \"input\" and \"do\"");
        return None;
    };
    let name = obj.get("name").and_then(Value::as_str).unwrap_or_default();
    let loc = if name.is_empty() { format!("rules[{i}]") } else { format!("rules[{i}] \"{name}\"") };
    c.fields(obj, &loc, &["name", "comment", "input", "when", "do"]);
    let before = c.errors.len();
    let input = match obj.get("input") {
        Some(Value::String(s)) => s.clone(),
        _ => {
            c.err(&loc, "needs \"input\": a control ID such as \"surface.left.pads.1\"");
            return None;
        }
    };
    let (kind, device) = classify_input(&input).map_err(|e| c.err(&format!("{loc}.input"), e)).ok()?;
    let is_cc = kind == InputKind::Cc;
    let kind = match (kind, encoders.get(&input)) {
        (InputKind::Cc, Some(CcMode::Button)) => InputKind::Button,
        (InputKind::Cc, Some(_)) => InputKind::Encoder,
        (InputKind::Cc, None) => InputKind::Fader,
        (other, _) => other,
    };
    let when = parse_when(c, &format!("{loc}.when"), obj.get("when"));
    let Some(list) = obj.get("do").and_then(Value::as_array) else {
        c.err(&loc, "needs \"do\": a list of actions ([] does nothing)");
        return None;
    };
    let actions: Vec<Action> = list.iter().enumerate().filter_map(|(j, a)| parse_action(c, &format!("{loc}.do[{j}]"), a, kind, is_cc)).collect();
    (c.errors.len() == before).then_some(Rule { location: loc, input, kind, device, when, actions })
}

fn parse_action(c: &mut Checker, loc: &str, v: &Value, input: InputKind, is_cc: bool) -> Option<Action> {
    let Some(obj) = v.as_object() else {
        c.err(loc, "must be an object like {\"action\": \"play\", \"deck\": 1}");
        return None;
    };
    let Some(name) = obj.get("action").and_then(Value::as_str) else {
        c.err(loc, "needs \"action\": one of the actions listed by `retraktor-cli mapping-reference`");
        return None;
    };
    let Some(spec) = ACTIONS.iter().find(|s| s.name == name) else {
        c.err(&format!("{loc}.action"), format!("unknown action \"{name}\"{}; `retraktor-cli mapping-reference` lists them", did_you_mean(name, ACTIONS.iter().map(|s| s.name))));
        return None;
    };
    let mut allowed = vec!["action", "comment"];
    allowed.extend(match spec.scope {
        Deck => Some("deck"),
        FxUnit => Some("unit"),
        Global => None,
    });
    allowed.extend(spec.slots.map(|_| "slot"));
    match spec.kind {
        Continuous => allowed.extend(["range", "step", "value"]),
        Modifier => allowed.extend(["name", "mode", "value"]),
        Trigger if name == "pad_mode" || name == "layout" => allowed.push("mode"),
        _ => {}
    }
    let before = c.errors.len();
    c.fields(obj, loc, &allowed);
    let needs = match spec.kind {
        Trigger | Modifier if input != InputKind::Button => Some("a button"),
        Encoder if input != InputKind::Encoder => Some("an encoder (or a MIDI CC listed in \"midi_encoders\")"),
        _ => None,
    };
    let cc_hint = if is_cc { " (a MIDI CC is a knob or fader unless \"midi_encoders\" names its encoding)" } else { "" };
    if let Some(needs) = needs {
        let hint = if is_cc && needs == "a button" { " (list the CC in \"midi_cc_buttons\" if it sends a value above 0 on press and 0 on release)" } else { cc_hint };
        c.err(loc, format!("\"{name}\" needs {needs}, but this input is a {}{hint}", input.describe()));
    }
    let index = match spec.scope {
        Deck => c.whole(obj, loc, "deck", DECKS, None),
        FxUnit => c.whole(obj, loc, "unit", FX_UNITS, None),
        Global => Some(0),
    }
    .unwrap_or(0) as u8;
    let slot = spec.slots.and_then(|range| c.whole(obj, loc, "slot", range, None));
    let action = match spec.kind {
        Trigger => {
            let arg = match name {
                "pad_mode" => c.choice(obj, loc, "mode", &PAD_MODES).unwrap_or_default().to_string(),
                "layout" => c.choice(obj, loc, "mode", &LAYOUTS).unwrap_or_default().to_string(),
                _ => slot.map(|s| s.to_string()).unwrap_or_default(),
            };
            Action::Trigger { verb: spec.verb, index, arg }
        }
        Continuous => {
            let range = match obj.get("range") {
                None => (0.0, 1.0),
                Some(Value::Array(pair)) if pair.len() == 2 => {
                    let from = c.unit(&format!("{loc}.range[0]"), &pair[0]).unwrap_or(0.0);
                    (from, c.unit(&format!("{loc}.range[1]"), &pair[1]).unwrap_or(1.0))
                }
                Some(_) => {
                    c.err(&format!("{loc}.range"), "must be [from, to] with numbers from 0 to 1, e.g. [1, 0] to invert");
                    (0.0, 1.0)
                }
            };
            let step = match obj.get("step") {
                None => DEFAULT_STEP,
                Some(s) => s.as_f64().filter(|s| *s != 0.0 && s.abs() <= 1.0).map(|s| s as f32).unwrap_or_else(|| {
                    c.err(&format!("{loc}.step"), format!("{s} must be a number from -1 to 1, not 0"));
                    DEFAULT_STEP
                }),
            };
            let value = obj.get("value").and_then(|v| c.unit(&format!("{loc}.value"), v));
            let has = |k: &str| obj.contains_key(k);
            let misuse = match input {
                InputKind::Button if !has("value") => Some(format!("\"{name}\" on a button needs \"value\": the 0 to 1 setting a press jumps to")),
                InputKind::Button if has("range") || has("step") => Some("on a button only \"value\" applies".into()),
                InputKind::Fader | InputKind::Encoder | InputKind::Cc if has("value") => Some("\"value\" is for buttons; knobs and faders use \"range\", encoders \"step\"".into()),
                InputKind::Fader if has("step") => Some(format!("knobs and faders use \"range\", not \"step\"{cc_hint}")),
                InputKind::Encoder if has("range") => Some("encoders use \"step\", not \"range\"".into()),
                _ => None,
            };
            if let Some(m) = misuse {
                c.err(loc, m);
            }
            Action::Continuous { target: spec.verb, index, range, step, value }
        }
        Encoder => Action::Encoder { name: spec.name, index },
        Modifier => {
            let modifier = match obj.get("name").and_then(Value::as_str) {
                Some(n) if valid_name(n) => n.to_string(),
                Some(_) => {
                    c.err(&format!("{loc}.name"), NAME_RULE);
                    String::new()
                }
                None => {
                    c.err(loc, "needs \"name\": the modifier (layer) to change, e.g. \"shift\"");
                    String::new()
                }
            };
            let mode = match c.choice(obj, loc, "mode", &["hold", "toggle", "set"]) {
                Some("toggle") => ModifierMode::Toggle,
                Some("set") => ModifierMode::Set,
                _ => ModifierMode::Hold,
            };
            let value = c.whole(obj, loc, "value", (0, MAX_MODIFIER_VALUE), Some(1)).unwrap_or(1) as u8;
            if value == 0 && mode != ModifierMode::Set {
                c.err(&format!("{loc}.value"), "0 means off; hold and toggle need 1 to 7");
            }
            Action::Modifier { name: modifier, mode, value }
        }
    };
    (c.errors.len() == before).then_some(action)
}

fn parse_led(c: &mut Checker, i: usize, v: &Value) -> Option<Led> {
    let loc = format!("leds[{i}]");
    let Some(obj) = v.as_object() else {
        c.err(&loc, "must be an object with \"control\" and \"colour\"");
        return None;
    };
    c.fields(obj, &loc, &["control", "when", "colour", "comment"]);
    let before = c.errors.len();
    let pads = || MX2_BUTTON_NAMES.iter().copied().filter(|n| n.contains(".pads."));
    let control = obj.get("control").and_then(Value::as_str).unwrap_or_default();
    let index = MX2_BUTTON_NAMES.iter().position(|n| *n == control).filter(|_| control.contains(".pads."));
    if index.is_none() {
        c.err(&format!("{loc}.control"), format!("\"{control}\" has no known colour LED: use an MX2 pad, surface.left.pads.1..8 or surface.right.pads.1..8{}", did_you_mean(control, pads())));
    }
    let when = parse_when(c, &format!("{loc}.when"), obj.get("when"));
    let names: Vec<&str> = LED_COLOURS.iter().map(|(n, _)| *n).collect();
    let code = c.choice(obj, &loc, "colour", &names).and_then(|n| LED_COLOURS.iter().find(|(name, _)| *name == n)).map(|(_, code)| *code);
    (c.errors.len() == before).then(|| Led { location: loc, index: index.unwrap_or(0), when, code: code.unwrap_or(0) })
}

fn edit_distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for j in 0..b.len() {
            let above = row[j + 1];
            row[j + 1] = (diagonal + usize::from(ca != b[j])).min(row[j] + 1).min(above + 1);
            diagonal = above;
        }
    }
    row[b.len()]
}

fn did_you_mean<'a>(word: &str, options: impl IntoIterator<Item = &'a str>) -> String {
    // Nothing worth suggesting is this long, and comparing a huge pasted value would stall the UI.
    if word.len() > 64 {
        return String::new();
    }
    options
        .into_iter()
        .map(|o| (edit_distance(word, o), o))
        .filter(|(d, o)| *d > 0 && *d <= (o.len() / 3).max(2))
        .min_by_key(|(d, _)| *d)
        .map(|(_, o)| format!(" (did you mean \"{o}\"?)"))
        .unwrap_or_default()
}

fn conditions_hold(when: &[(String, u8)], modifiers: Option<&HashMap<String, u8>>) -> bool {
    when.iter().all(|(name, value)| modifiers.and_then(|m| m.get(name)).copied().unwrap_or(0) == *value)
}

/// Who handles a held button's release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owner {
    /// The simple mappings and built-in controls took the press.
    Legacy,
    /// This rule took the press.
    Rule(usize),
    /// A rule took the press before a reload; the release does nothing (its modifiers are already 0).
    Swallow,
}

/// Runs a rule set against hardware events and keeps the modifier state per device.
#[derive(Debug, Default)]
pub struct RuleEngine {
    rules: RuleSet,
    /// Non-zero modifier values per device (`mx2`, `midi.<port>`, `hid.<pid>`).
    modifiers: HashMap<String, HashMap<String, u8>>,
    /// Who took each held button's press; its repeats and its release go to the same place.
    held: HashMap<String, Owner>,
}

impl RuleEngine {
    pub fn rules(&self) -> &RuleSet {
        &self.rules
    }

    /// Swaps in new rules and resets every modifier to 0. A button held through the swap keeps its owner kind: if
    /// the simple mappings took its press they still get its release (so a held jog touch can't leave scratching
    /// on), and if an old rule took it the release does nothing.
    pub fn replace_rules(&mut self, rules: RuleSet) {
        self.rules = rules;
        self.modifiers.clear();
        for owner in self.held.values_mut() {
            if *owner != Owner::Legacy {
                *owner = Owner::Swallow;
            }
        }
    }

    /// Forgets a device's modifiers and held buttons, e.g. when it is unplugged.
    pub fn reset_device(&mut self, device: &str) {
        self.modifiers.remove(device);
        self.held.retain(|control, _| device_of(control).as_deref() != Some(device));
    }

    /// Resets every device that has state and is no longer `connected`.
    pub fn forget_disconnected(&mut self, connected: impl Fn(&str) -> bool) {
        let mut gone: Vec<String> = self.modifiers.keys().cloned().chain(self.held.keys().filter_map(|c| device_of(c))).filter(|d| !connected(d)).collect();
        gone.sort();
        gone.dedup();
        for device in gone {
            self.reset_device(&device);
        }
    }

    /// Non-zero modifiers as (device, name, value), sorted, for display.
    pub fn active_modifiers(&self) -> Vec<(String, String, u8)> {
        let mut active: Vec<_> = self.modifiers.iter().flat_map(|(d, m)| m.iter().map(move |(n, v)| (d.clone(), n.clone(), *v))).collect();
        active.sort();
        active
    }

    fn first_match(&self, control: &str) -> Owner {
        let rules = &self.rules;
        let found = rules.by_input.get(control).and_then(|candidates| {
            candidates.iter().copied().find(|&i| conditions_hold(&rules.rules[i].when, self.modifiers.get(&rules.rules[i].device)))
        });
        found.map_or(Owner::Legacy, Owner::Rule)
    }

    /// Runs the rules for one hardware event. `None` means no rule matched, so the simple mappings and
    /// built-in controls handle the event exactly as they would without a rules file.
    pub fn handle(&mut self, ev: &SurfaceEvent) -> Option<Vec<RuleCommand>> {
        let owner = match ev {
            SurfaceEvent::Button { control, pressed: true } => match self.held.get(control) {
                // A repeated press before the release (MIDI can send note-on twice) keeps the first press's owner;
                // a rule doesn't run its press actions again.
                Some(Owner::Legacy) => return None,
                Some(_) => return Some(Vec::new()),
                None => {
                    let owner = self.first_match(control);
                    self.held.insert(control.clone(), owner);
                    owner
                }
            },
            SurfaceEvent::Button { control, pressed: false } => self.held.remove(control).unwrap_or_else(|| self.first_match(control)),
            SurfaceEvent::Fader { control, .. } | SurfaceEvent::Encoder { control, .. } => self.first_match(control),
        };
        let index = match owner {
            Owner::Legacy => return None,
            Owner::Swallow => return Some(Vec::new()),
            Owner::Rule(index) => index,
        };
        let rule = &self.rules.rules[index];
        let mut commands = Vec::new();
        for action in &rule.actions {
            match (action, ev) {
                (Action::Trigger { verb, index, arg }, SurfaceEvent::Button { pressed: true, .. }) => {
                    commands.push(RuleCommand::Trigger { verb: *verb, index: *index, arg: arg.clone() });
                }
                (Action::Continuous { target, index, value: Some(value), .. }, SurfaceEvent::Button { pressed: true, .. }) => {
                    commands.push(RuleCommand::Set { target: *target, index: *index, value: *value });
                }
                // A CC read the other way (only before the reader has this file's CC readings) is ignored, never misapplied.
                (Action::Continuous { target, index, range: (from, to), .. }, SurfaceEvent::Fader { value, .. }) if rule.kind != InputKind::Encoder => {
                    let value = (*value as f32).clamp(0.0, 1.0);
                    commands.push(RuleCommand::Set { target: *target, index: *index, value: from + (to - from) * value });
                }
                (Action::Continuous { target, index, step, .. }, SurfaceEvent::Encoder { delta, .. }) if rule.kind == InputKind::Encoder => {
                    commands.push(RuleCommand::Adjust { target: *target, index: *index, delta: *delta as f32 * step });
                }
                (Action::Encoder { name, index }, SurfaceEvent::Encoder { delta, .. }) if *delta != 0 && rule.kind == InputKind::Encoder => {
                    let verb = match (*name, *delta > 0) {
                        ("browse", true) => "browse_down",
                        ("browse", false) => "browse_up",
                        ("waveform_zoom", true) => "zoom_in",
                        ("waveform_zoom", false) => "zoom_out",
                        (_, true) => "loop_double",
                        (_, false) => "loop_half",
                    };
                    commands.push(RuleCommand::Trigger { verb, index: *index, arg: String::new() });
                }
                (Action::Modifier { name, mode, value }, SurfaceEvent::Button { pressed, .. }) => {
                    // Applied at once, so a later action in the same list sees it, like the other ordered actions.
                    let modifiers = self.modifiers.entry(rule.device.clone()).or_default();
                    let now = modifiers.get(name).copied().unwrap_or(0);
                    let next = match (mode, pressed) {
                        (ModifierMode::Hold | ModifierMode::Set, true) => *value,
                        (ModifierMode::Hold, false) => 0,
                        (ModifierMode::Toggle, true) if now == *value => 0,
                        (ModifierMode::Toggle, true) => *value,
                        _ => now,
                    };
                    if next == 0 {
                        modifiers.remove(name);
                    } else {
                        modifiers.insert(name.clone(), next);
                    }
                }
                _ => {}
            }
        }
        Some(commands)
    }

    /// Paints the file's pad colours over an MX2 LED report (`[0x80, 105 LED bytes]`). The first entry whose
    /// conditions match wins per pad; pads with none keep the app's own colours.
    pub fn apply_mx2_led_overlay(&self, report: &mut [u8; 106]) {
        let modifiers = self.modifiers.get("mx2");
        let mut painted = [false; 74];
        for led in &self.rules.leds {
            if !painted[led.index] && conditions_hold(&led.when, modifiers) {
                report[1 + led.index] = led.code;
                painted[led.index] = true;
            }
        }
    }
}

/// The author's reference (`retraktor-cli mapping-reference`), built from the catalogue the validator uses.
pub fn reference_text() -> String {
    let mut s = String::new();
    let mut line = |text: &str| {
        s.push_str(text);
        s.push('\n');
    };
    line(&format!("ReTraktor controller rules: format \"{RULES_FORMAT}\", version {RULES_VERSION}"));
    line("File: ~/.config/retraktor/controller-rules.json (plain JSON; the app writes it only for Create example file");
    line("and Import > Add to rules file). Convert a Traktor, Mixxx or rekordbox mapping: `retraktor-cli import-mapping`;");
    line("add drafts to the file: `retraktor-cli apply-mapping`.");
    line("Check it with `retraktor-cli validate-mapping [file]`, then press Reload in Preferences > Controllers.");
    line("Full contract: docs/controller-rules.md. Example: `retraktor-cli mapping-reference --example`.");
    line("");
    line("FILE    { \"format\": \"retraktor-controller-rules\", \"version\": 1, \"rules\": [RULE...] }");
    line("        optional: \"comment\", \"midi_encoders\": {MIDI_CC_ID: ENCODING, ...}, \"midi_cc_buttons\": [MIDI_CC_ID...],");
    line("        \"leds\": [LED...]");
    line("RULE    { \"input\": CONTROL_ID, \"do\": [ACTION...] }   optional: \"when\": {\"modifier\": 0-7, ...}, \"name\", \"comment\"");
    line("        \"name\" is only a label for messages; modifiers are named inside the modifier action.");
    line("ACTION  { \"action\": NAME, parameters... }   deck 1-4, unit 1-2 (FX unit), slot as listed");
    line("LED     { \"control\": MX2_PAD, \"when\": {...}, \"colour\": COLOUR }");
    line("\"comment\" is allowed on the file, rules, actions and LEDs. Unknown fields are errors.");
    line("");
    line("PRECEDENCE");
    line("  For each hardware event, the first rule from the top whose \"input\" is the control and whose \"when\"");
    line("  values all equal that device's modifiers runs every action in \"do\", in order. A button release goes");
    line("  to whatever took its press, and a repeated press before the release does not run the rule again.");
    line("  If no rule matches, the simple mappings (Traktor MX2 map, Other controllers)");
    line("  and the built-in controls handle the event as before. Rules hidden behind an earlier rule are errors.");
    line("");
    line("VALUES");
    line("  Value actions take 0 to 1. On a knob or fader: \"range\": [from, to] (default [0, 1]; [1, 0] inverts).");
    line(&format!("  On an encoder: \"step\" per tick (default {DEFAULT_STEP}; negative inverts). On a button: \"value\" to jump to."));
    line("");
    line("MIDI CC");
    line("  A MIDI CC is an absolute knob or fader (value / 127, \"range\") unless \"midi_encoders\" lists it as a");
    line("  relative encoder (\"step\", browse, loop_size). It is read that way from its first message. Encodings, by");
    line("  what one notch right / left sends: offset_64 = 65 / 63, twos_complement = 1 / 127, sign_bit = 1 / 65.");
    line("  `amidi -d -p hw:1,0` shows the bytes: B<ch-1> <cc> <value>. A listed or used CC is read the same way");
    line("  by the simple mappings; other CCs keep the app's automatic guess. A CC listed in \"midi_cc_buttons\" is a");
    line("  button: a value above 0 presses it, 0 releases it (for controllers whose buttons send 127 / 0 as CCs).");
    line("");
    line("MODIFIERS");
    line("  {\"action\": \"modifier\", \"name\": \"shift\", \"mode\": \"hold\" | \"toggle\" | \"set\", \"value\": 1-7 (default 1; set may use 0)}");
    line("  hold: value while pressed, 0 on release. toggle: value, or 0 if it already has it. set: value.");
    line("  Several modifier actions in one \"do\" list run in order, each seeing the one before.");
    line("  Modifiers belong to one device (the MX2, each MIDI port, each NI HID model), start at 0, and go back");
    line("  to 0 on Reload and when the device is unplugged. A \"when\" may only name modifiers set on its device.");
    line("");
    line("ACTIONS");
    line(&format!("  {:<14}{:<10}{:<22}{}", "name", "input", "parameters", "what it does"));
    for a in ACTIONS {
        let mut params: Vec<String> = Vec::new();
        params.extend(match a.scope {
            Deck => Some("deck".to_string()),
            FxUnit => Some("unit".to_string()),
            Global => None,
        });
        params.extend(a.slots.map(|(lo, hi)| format!("slot {lo}-{hi}")));
        match a.kind {
            Continuous => params.push("range|step|value".into()),
            Modifier => params.push("name mode value".into()),
            Trigger if a.name == "pad_mode" || a.name == "layout" => params.push("mode".into()),
            _ => {}
        }
        let input = match a.kind {
            Trigger | Modifier => "button",
            Continuous => "any",
            Encoder => "encoder",
        };
        line(&format!("  {:<14}{:<10}{:<22}{}", a.name, input, params.join(" "), a.about));
    }
    line("");
    line("CONTROL IDS");
    for (kind, title) in [(InputKind::Button, "Traktor MX2 buttons"), (InputKind::Fader, "Traktor MX2 knobs and faders"), (InputKind::Encoder, "Traktor MX2 encoders")] {
        line(&format!("  {title}:"));
        let names: Vec<&str> = mx2_controls().filter(|(_, k)| *k == kind).map(|(n, _)| n).collect();
        for chunk in names.chunks(3) {
            line(&format!("    {}", chunk.join("  ")));
        }
    }
    line("  Generic MIDI: midi.<port>.ch<1-16>.note<0-127> (button), .cc<0-127> (knob/fader, or encoder), .bend (fader).");
    line("    <port> is the ALSA card id in lowercase (`cat /proc/asound/cards`: [XONEK2] is midi.xonek2), plus _d<N>");
    line("    for the card's second and later MIDI ports (hw:1,1 is midi.xonek2_d1). It stays the same when the card");
    line("    number changes. Older raw-node IDs (midi.midic1d0) still work as an alias while that node is the device.");
    line("    Numbers have no leading zeros (ch1, note3).");
    line("  Generic NI HID: hid.<pid>.r<report>.b<byte>.<bit> (button), hid.<pid>.r<report>.a<byte> (knob or fader).");
    line("  Don't guess IDs: in Preferences > Controllers, touch the control and copy \"Last control\" (or use Learn).");
    line("");
    line(&format!("PAD MODES    {}", PAD_MODES.join(", ")));
    line(&format!("LAYOUTS      {}", LAYOUTS.join(", ")));
    line(&format!("LED COLOURS  {}", LED_COLOURS.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ")));
    line("MX2_PAD      surface.left.pads.1 .. surface.left.pads.8, surface.right.pads.1 .. surface.right.pads.8");
    line("");
    line("EXAMPLE");
    let _ = write!(s, "{EXAMPLE_RULES}");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(control: &str, pressed: bool) -> SurfaceEvent {
        SurfaceEvent::Button { control: control.into(), pressed }
    }

    fn engine(text: &str) -> RuleEngine {
        let mut e = RuleEngine::default();
        e.replace_rules(parse_rules(text).unwrap_or_else(|d| panic!("{d:?}")));
        e
    }

    fn file(rules: &str) -> String {
        format!(r#"{{"format": "retraktor-controller-rules", "version": 1, "rules": [{rules}]}}"#)
    }

    const SHIFT: &str = r#"{"input": "surface.left.shift", "do": [{"action": "modifier", "name": "shift", "mode": "hold"}]}"#;

    #[test]
    fn example_is_valid_and_the_reference_covers_the_catalogue() {
        let set = parse_rules(EXAMPLE_RULES).unwrap_or_else(|d| panic!("{d:?}"));
        assert_eq!((set.rule_count(), set.led_count()), (11, 7));
        let reference = reference_text();
        for a in ACTIONS {
            assert!(reference.contains(&format!("  {:<14}", a.name)), "{} missing from the reference", a.name);
        }
        for (control, _) in mx2_controls() {
            assert!(reference.contains(control), "{control} missing from the reference");
        }
    }

    #[test]
    fn diagnostics_name_the_place_and_the_fix() {
        let bad = file(
            r#"{"name": "typo", "input": "surface.left.pads.1", "do": [{"action": "hotcu", "deck": 1}]},
               {"input": "surface.left.pads.2", "do": [{"action": "hotcue", "deck": 5, "slot": 9, "dekc": 1}]},
               {"input": "surface.mixer.xfader", "do": [{"action": "play", "deck": 1}]},
               {"input": "surface.left.pad.3", "do": []},
               {"input": "midi.ddj.ch17.note200", "do": []},
               {"input": "surface.left.pads.4", "when": {"shfit": 1}, "do": []},
               {"input": "surface.left.fx.knobs.1", "do": [{"action": "filter", "deck": 1, "range": [0, 2]}]},
               {"input": "surface.left.play", "do": [{"action": "volume", "deck": 1}]},
               {"input": "surface.left.sync", "do": []},
               {"input": "surface.left.sync", "when": {"shift": 1}, "do": []}"#,
        )
        .replace("\"rules\": [", &format!("\"rules\": [{SHIFT},"));
        let errors: Vec<String> = parse_rules(&bad).unwrap_err().iter().map(|d| d.to_string()).collect();
        let has = |needle: &str| assert!(errors.iter().any(|e| e.contains(needle)), "no diagnostic with {needle:?} in {errors:#?}");
        has("rules[1] \"typo\".do[0].action: unknown action \"hotcu\" (did you mean \"hotcue\"?)");
        has("rules[2].do[0].deck: 5 is out of range: use a whole number from 1 to 4");
        has("rules[2].do[0].slot: 9 is out of range");
        has("rules[2].do[0].dekc: unknown field (did you mean \"deck\"?)");
        has("rules[3].do[0]: \"play\" needs a button, but this input is a knob or fader");
        has("rules[4].input: \"surface.left.pad.3\" is not a Traktor MX2 control (did you mean \"surface.left.pads.3\"?)");
        has("rules[5].input: MIDI channel in \"ch17\" must be ch1..ch16");
        has("rules[6].when.shfit: no rule on device mx2 sets modifier \"shfit\" (did you mean \"shift\"?)");
        has("rules[7].do[0].range[1]: 2 must be a number from 0 to 1");
        has("rules[8].do[0]: \"volume\" on a button needs \"value\"");
        has("rules[10]: never runs: rules[9] has the same input");
        let syntax = parse_rules("{\n  \"format\": \"retraktor-controller-rules\",\n  \"version\": 1\n  \"rules\": []\n}").unwrap_err();
        assert_eq!(syntax[0].location, "line 4, column 3");
        let version = parse_rules(r#"{"format": "retraktor-controller-rules", "version": 2, "rules": []}"#).unwrap_err();
        assert!(version[0].to_string().contains("version: 2 is not supported"));
    }

    #[test]
    fn held_shift_changes_a_pad_and_a_release_follows_its_press() {
        let pad = r#"{"input": "surface.left.pads.1", "when": {"shift": 1}, "do": [{"action": "stem", "deck": 1, "slot": 2}]}"#;
        let mut e = engine(&file(&format!("{SHIFT}, {pad}")));
        assert_eq!(e.handle(&press("surface.left.pads.1", true)), None, "plain pad goes to the simple mappings");
        assert_eq!(e.handle(&press("surface.left.shift", true)), Some(vec![]));
        // The pad was pressed before Shift, so its release still goes to the simple mappings.
        assert_eq!(e.handle(&press("surface.left.pads.1", false)), None);
        let stem = RuleCommand::Trigger { verb: "stem", index: 1, arg: "2".into() };
        assert_eq!(e.handle(&press("surface.left.pads.1", true)), Some(vec![stem]));
        assert_eq!(e.handle(&press("surface.left.shift", false)), Some(vec![]));
        assert!(e.active_modifiers().is_empty());
        // Pressed under Shift: the release goes to the rule even though Shift is up now.
        assert_eq!(e.handle(&press("surface.left.pads.1", false)), Some(vec![]));
        assert_eq!(e.handle(&press("surface.left.pads.1", true)), None);
    }

    #[test]
    fn latched_layers_macros_encoders_and_leds() {
        let text = r#"{"format": "retraktor-controller-rules", "version": 1, "rules": [
            {"input": "surface.right.browse.favorite", "do": [{"action": "modifier", "name": "layer", "mode": "toggle", "value": 2}]},
            {"input": "surface.right.pads.1", "when": {"layer": 2}, "do": [{"action": "loop_half", "deck": 2}, {"action": "xfader", "value": 0.5}]},
            {"input": "surface.left.fx.knobs.4", "do": [{"action": "filter", "deck": 1, "range": [0.5, 1]}, {"action": "fx_mix", "unit": 1, "range": [1, 0]}]},
            {"input": "surface.left.loop_move", "do": [{"action": "filter", "deck": 1, "step": -0.05}]},
            {"input": "surface.right.loop_size", "do": [{"action": "loop_size", "deck": 2}]}],
          "leds": [{"control": "surface.right.pads.1", "when": {"layer": 2}, "colour": "magenta"},
                   {"control": "surface.right.pads.1", "colour": "red"}]}"#;
        let mut e = engine(text);
        let mut report = [0x7Cu8; 106];
        e.apply_mx2_led_overlay(&mut report);
        assert_eq!(report[1 + 50], 0x06, "no layer: second entry (red)");
        e.handle(&press("surface.right.browse.favorite", true));
        e.handle(&press("surface.right.browse.favorite", false));
        assert_eq!(e.active_modifiers(), vec![("mx2".into(), "layer".into(), 2)], "toggle latches past the release");
        e.apply_mx2_led_overlay(&mut report);
        assert_eq!(report[1 + 50], 0x36, "layer on: magenta on right pad 1's LED byte");
        assert_eq!(report[1 + 20], 0x7C, "pads without entries keep the app's colour");
        assert_eq!(
            e.handle(&press("surface.right.pads.1", true)),
            Some(vec![RuleCommand::Trigger { verb: "loop_half", index: 2, arg: String::new() }, RuleCommand::Set { target: "xfader", index: 0, value: 0.5 }])
        );
        e.handle(&press("surface.right.browse.favorite", true));
        assert!(e.active_modifiers().is_empty(), "second press unlatches");

        let knob = e.handle(&SurfaceEvent::Fader { control: "surface.left.fx.knobs.4".into(), value: 0.25 }).unwrap();
        assert_eq!(knob, vec![RuleCommand::Set { target: "filter", index: 1, value: 0.625 }, RuleCommand::Set { target: "fx_mix", index: 1, value: 0.75 }]);
        assert_eq!(e.handle(&SurfaceEvent::Encoder { control: "surface.left.loop_move".into(), delta: 2 }), Some(vec![RuleCommand::Adjust { target: "filter", index: 1, delta: -0.1 }]));
        assert_eq!(
            e.handle(&SurfaceEvent::Encoder { control: "surface.right.loop_size".into(), delta: -1 }),
            Some(vec![RuleCommand::Trigger { verb: "loop_half", index: 2, arg: String::new() }])
        );
    }

    #[test]
    fn modifiers_stay_on_their_device_and_reset_on_unplug_and_reload() {
        let text = file(&format!(
            r#"{SHIFT},
            {{"input": "midi.midic1d0.ch1.note1", "do": [{{"action": "modifier", "name": "shift", "mode": "hold"}}]}},
            {{"input": "midi.midic1d0.ch1.note60", "when": {{"shift": 1}}, "do": [{{"action": "play", "deck": 2}}]}},
            {{"input": "surface.left.pads.1", "when": {{"shift": 1}}, "do": [{{"action": "play", "deck": 1}}]}}"#
        ));
        let mut e = engine(&text);
        e.handle(&press("surface.left.shift", true));
        assert_eq!(e.handle(&press("midi.midic1d0.ch1.note60", true)), None, "MX2 Shift doesn't shift the MIDI controller");
        e.handle(&press("midi.midic1d0.ch1.note60", false));
        e.handle(&press("midi.midic1d0.ch1.note1", true));
        assert!(e.handle(&press("midi.midic1d0.ch1.note60", true)).is_some());

        // Unplugging the MIDI controller drops its held Shift; the MX2's stays.
        e.forget_disconnected(|d| d == "mx2");
        assert_eq!(e.active_modifiers(), vec![("mx2".into(), "shift".into(), 1)]);
        assert_eq!(e.handle(&press("midi.midic1d0.ch1.note60", true)), None);
        e.reset_device("mx2");
        assert_eq!(e.handle(&press("surface.left.pads.1", true)), None, "unplugged MX2 Shift no longer shifts");

        // Reload while Shift is held and a pad press went to the simple mappings: modifiers reset, and the
        // pad's release still goes to the simple mappings even though the new file now has a rule for it.
        e.handle(&press("surface.left.shift", true));
        e.handle(&press("surface.left.play", true));
        let plain_play = r#"{"input": "surface.left.play", "do": [{"action": "cue", "deck": 1}]}"#;
        e.replace_rules(parse_rules(&file(&format!("{SHIFT}, {plain_play}"))).unwrap());
        assert!(e.active_modifiers().is_empty());
        assert_eq!(e.handle(&press("surface.left.play", false)), None);
        assert_eq!(e.handle(&press("surface.left.shift", false)), Some(vec![]), "Shift's release after a reload is harmless");
        assert!(e.active_modifiers().is_empty());
    }

    #[test]
    fn midi_ccs_are_faders_unless_declared_encoders_and_ids_are_canonical() {
        let good = r#"{"format": "retraktor-controller-rules", "version": 1,
            "midi_encoders": {"midi.xonek2.ch15.cc0": "twos_complement"},
            "rules": [{"input": "midi.xonek2.ch15.cc0", "do": [{"action": "browse"}]},
                      {"input": "midi.xonek2.ch15.cc4", "do": [{"action": "volume", "deck": 1, "range": [1, 0]}]}]}"#;
        let set = parse_rules(good).unwrap_or_else(|d| panic!("{d:?}"));
        assert_eq!(set.midi_cc_modes(), &[("midi.xonek2.ch15.cc0".to_string(), CcMode::TwosComplement), ("midi.xonek2.ch15.cc4".to_string(), CcMode::Absolute)]);

        let bad = r#"{"format": "retraktor-controller-rules", "version": 1,
            "midi_encoders": {"midi.xonek2.ch15.cc1": "twos_complment", "midi.xonek2.ch15.note1": "offset_64"},
            "rules": [{"input": "midi.xonek2.ch15.cc2", "do": [{"action": "browse"}]},
                      {"input": "midi.xonek2.ch15.cc3", "do": [{"action": "filter", "deck": 1, "step": 0.1}]},
                      {"input": "midi.xonek2.ch01.note3", "do": []},
                      {"input": "midi.xonek2.ch1.note003", "do": []}]}"#;
        let errors: Vec<String> = parse_rules(bad).unwrap_err().iter().map(ToString::to_string).collect();
        let has = |needle: &str| assert!(errors.iter().any(|e| e.contains(needle)), "no diagnostic with {needle:?} in {errors:#?}");
        has("midi_encoders.\"midi.xonek2.ch15.cc1\": \"twos_complment\" is not one of offset_64, twos_complement, sign_bit (did you mean \"twos_complement\"?)");
        has("midi_encoders.\"midi.xonek2.ch15.note1\": only MIDI CC IDs");
        has("rules[0].do[0]: \"browse\" needs an encoder (or a MIDI CC listed in \"midi_encoders\"), but this input is a knob or fader (a MIDI CC is a knob or fader unless \"midi_encoders\" names its encoding)");
        has("rules[1].do[0]: knobs and faders use \"range\", not \"step\"");
        has("rules[2].input: MIDI channel in \"ch01\" must be ch1..ch16, without leading zeros");
        has("rules[3].input: note in \"note003\" must be note0..note127, without leading zeros");

        // Before the reader thread has these readings, a guessed event of the wrong kind changes nothing.
        let mut e = RuleEngine::default();
        e.replace_rules(set);
        assert_eq!(e.handle(&SurfaceEvent::Encoder { control: "midi.xonek2.ch15.cc4".into(), delta: 1 }), Some(vec![]));
        assert_eq!(e.handle(&SurfaceEvent::Fader { control: "midi.xonek2.ch15.cc0".into(), value: 0.0 }), Some(vec![]));
        assert_eq!(e.handle(&SurfaceEvent::Fader { control: "midi.xonek2.ch15.cc4".into(), value: 0.0 }), Some(vec![RuleCommand::Set { target: "volume", index: 1, value: 1.0 }]));

        assert_eq!([65, 63, 64].map(|v| CcMode::Offset64.ticks(v)), [1, -1, 0]);
        assert_eq!([1, 127, 3].map(|v| CcMode::TwosComplement.ticks(v)), [1, -1, 3]);
        assert_eq!([1, 65, 67].map(|v| CcMode::SignBit.ticks(v)), [1, -1, -3]);
    }

    /// A CC listed in midi_cc_buttons drives button actions, and the layout, browser, tempo master and waveform zoom
    /// actions reach the app as triggers.
    #[test]
    fn cc_buttons_and_the_layout_zoom_and_tempo_master_actions() {
        let text = r#"{"format": "retraktor-controller-rules", "version": 1,
            "midi_encoders": {"midi.mixer.ch1.cc20": "twos_complement"},
            "midi_cc_buttons": ["midi.mixer.ch1.cc8"],
            "rules": [
              {"input": "midi.mixer.ch1.cc8", "do": [{"action": "tempo_master", "deck": 2}, {"action": "layout", "mode": "stacked4"}, {"action": "browser_only"}]},
              {"input": "midi.mixer.ch1.cc20", "do": [{"action": "waveform_zoom", "deck": 1}, {"action": "waveform_zoom", "deck": 3}]}]}"#;
        let set = parse_rules(text).unwrap_or_else(|d| panic!("{d:?}"));
        assert_eq!(set.midi_cc_modes(), &[("midi.mixer.ch1.cc20".to_string(), CcMode::TwosComplement), ("midi.mixer.ch1.cc8".to_string(), CcMode::Button)]);
        let mut e = RuleEngine::default();
        e.replace_rules(set);
        let trigger = |verb, index, arg: &str| RuleCommand::Trigger { verb, index, arg: arg.into() };
        assert_eq!(
            e.handle(&press("midi.mixer.ch1.cc8", true)),
            Some(vec![trigger("tempo_master", 2, ""), trigger("layout", 0, "stacked4"), trigger("browser_only", 0, "")])
        );
        assert_eq!(e.handle(&press("midi.mixer.ch1.cc8", false)), Some(vec![]));
        assert_eq!(e.handle(&SurfaceEvent::Encoder { control: "midi.mixer.ch1.cc20".into(), delta: -1 }), Some(vec![trigger("zoom_out", 1, ""), trigger("zoom_out", 3, "")]));
        assert_eq!(e.handle(&SurfaceEvent::Encoder { control: "midi.mixer.ch1.cc20".into(), delta: 2 }), Some(vec![trigger("zoom_in", 1, ""), trigger("zoom_in", 3, "")]));

        let bad = r#"{"format": "retraktor-controller-rules", "version": 1,
            "midi_encoders": {"midi.mixer.ch1.cc20": "twos_complement"},
            "midi_cc_buttons": ["midi.mixer.ch1.cc20", "midi.mixer.ch1.note3", 7],
            "rules": [{"input": "midi.mixer.ch1.cc9", "do": [{"action": "play", "deck": 1}]},
                      {"input": "midi.mixer.ch1.note1", "do": [{"action": "layout", "mode": "stacked3"}]}]}"#;
        let errors: Vec<String> = parse_rules(bad).unwrap_err().iter().map(ToString::to_string).collect();
        let has = |needle: &str| assert!(errors.iter().any(|e| e.contains(needle)), "no diagnostic with {needle:?} in {errors:#?}");
        has("midi_cc_buttons[0]: \"midi.mixer.ch1.cc20\" is listed twice");
        has("midi_cc_buttons[1]: only MIDI CC IDs");
        has("midi_cc_buttons[2]: must be a MIDI CC ID string");
        has("rules[0].do[0]: \"play\" needs a button, but this input is a knob or fader (list the CC in \"midi_cc_buttons\"");
        has("rules[1].do[0].mode: \"stacked3\" is not one of regular2, regular4, stacked2, stacked4 (did you mean \"stacked2\"?)");
    }

    /// MIDI can send note-on twice before note-off. The first press's owner keeps the button, including a
    /// conditional held Shift whose condition has changed since, and a rule never re-runs its press actions.
    #[test]
    fn repeated_presses_keep_their_owner_and_modifiers_apply_in_order() {
        let mut e = engine(&file(
            r#"{"input": "surface.left.browse.favorite", "do": [{"action": "modifier", "name": "layer", "mode": "toggle"}]},
               {"input": "surface.left.shift", "when": {"layer": 1}, "do": [{"action": "modifier", "name": "shift", "mode": "hold", "value": 2}]},
               {"input": "surface.left.shift", "do": [{"action": "modifier", "name": "shift", "mode": "hold"}]},
               {"input": "surface.left.pads.1", "do": [{"action": "modifier", "name": "page", "mode": "set", "value": 2}, {"action": "modifier", "name": "page", "mode": "toggle", "value": 2}]}"#,
        ));
        let active = |e: &RuleEngine| e.active_modifiers().into_iter().map(|(_, n, v)| (n, v)).collect::<Vec<_>>();
        e.handle(&press("surface.left.browse.favorite", true));
        e.handle(&press("surface.left.browse.favorite", false));
        e.handle(&press("surface.left.shift", true));
        assert_eq!(active(&e), vec![("layer".into(), 1), ("shift".into(), 2)]);
        e.handle(&press("surface.left.browse.favorite", true));
        e.handle(&press("surface.left.browse.favorite", false));
        assert_eq!(e.handle(&press("surface.left.shift", true)), Some(vec![]), "a repeated press is swallowed");
        assert_eq!(active(&e), vec![("shift".into(), 2)], "the repeat must not reselect the plain Shift rule");
        e.handle(&press("surface.left.shift", false));
        assert!(active(&e).is_empty(), "the original conditional owner releases Shift");

        e.handle(&press("surface.left.browse.favorite", true));
        e.handle(&press("surface.left.browse.favorite", true));
        e.handle(&press("surface.left.browse.favorite", false));
        assert_eq!(active(&e), vec![("layer".into(), 1)], "a repeated press must not toggle back");

        // set 2 then toggle 2 in one list: the toggle sees the 2 just set, so the result is 0.
        e.handle(&press("surface.left.pads.1", true));
        assert_eq!(active(&e), vec![("layer".into(), 1)]);

        // A rule-owned button held through a reload: its repeats and its release do nothing.
        e.handle(&press("surface.left.shift", true));
        e.replace_rules(parse_rules(&file(SHIFT)).unwrap());
        assert_eq!(e.handle(&press("surface.left.shift", true)), Some(vec![]));
        assert_eq!(e.handle(&press("surface.left.shift", false)), Some(vec![]));
        assert!(active(&e).is_empty());
    }

    #[test]
    fn oversized_files_and_huge_identifiers_are_refused_quickly() {
        let path = std::env::temp_dir().join(format!("retraktor-rules-oversize-{}.json", std::process::id()));
        std::fs::write(&path, vec![b' '; MAX_FILE_BYTES + 10]).unwrap();
        let err = read_rules_file(&path).unwrap_err();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
        let huge = format!("surface.{}", "x".repeat(200_000));
        let started = std::time::Instant::now();
        assert!(!classify_input(&huge).unwrap_err().contains("did you mean"));
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        let many: Vec<String> = (0..5000).map(|i| format!(r#"{{"input": "surface.left.pad.{i}", "do": []}}"#)).collect();
        let errors = parse_rules(&file(&many.join(","))).unwrap_err();
        assert_eq!(errors.len(), MAX_DIAGNOSTICS + 1);
        assert!(errors.last().unwrap().message.starts_with("stopped after 100 problems"));
    }
}
