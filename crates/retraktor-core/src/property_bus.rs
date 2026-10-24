//! Hierarchical, strongly-typed `app.traktor.*` reactive property bus.

use std::collections::BTreeMap;
use std::fmt;

use crate::enums::{CueType, DeckType};

/// Value stored in an `app.traktor.*` or `mapping.*` property node.
#[derive(Debug, Clone, PartialEq)]
pub enum PropValue {
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
}

impl PropValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropValue::Bool(b) => Some(*b),
            PropValue::Int(i) => Some(*i != 0),
            PropValue::Float(f) => Some(*f != 0.0),
            PropValue::String(_) => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            PropValue::Int(i) => Some(*i),
            PropValue::Bool(b) => Some(if *b { 1 } else { 0 }),
            PropValue::Float(f) => Some(*f as i64),
            PropValue::String(_) => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            PropValue::Float(f) => Some(*f),
            PropValue::Int(i) => Some(*i as f64),
            PropValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            PropValue::String(_) => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            PropValue::String(s) => Some(s.as_str()),
            _ => None,
        }
    }
}

impl fmt::Display for PropValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PropValue::Bool(b) => write!(f, "{b}"),
            PropValue::Int(i) => write!(f, "{i}"),
            PropValue::Float(v) => write!(f, "{v:.4}"),
            PropValue::String(s) => write!(f, "\"{s}\""),
        }
    }
}

/// Metadata & bounds associated with a property node (`AppProperty` / `MappingPropertyDescriptor`).
#[derive(Debug, Clone)]
pub struct PropMeta {
    pub path: String,
    pub value: PropValue,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub description: String,
}

type SubscriberCallback = Box<dyn FnMut(&str, &PropValue) + Send>;

/// Central observable state store modeling Traktor Pro 4's `app.traktor.*` property tree.
pub struct PropertyBus {
    props: BTreeMap<String, PropMeta>,
    subscribers: BTreeMap<String, Vec<SubscriberCallback>>,
}

impl Default for PropertyBus {
    fn default() -> Self {
        Self::new_traktor_pro4_default()
    }
}

impl PropertyBus {
    /// Creates an empty property bus.
    pub fn empty() -> Self {
        Self {
            props: BTreeMap::new(),
            subscribers: BTreeMap::new(),
        }
    }

    /// Creates a `PropertyBus` pre-populated with all 190 observed Traktor Pro 4 `app.traktor.*`
    /// property templates across 4 Decks (Track, Stem, Remix, Freeze, Loop, Hotcues),
    /// 4-Channel Mixer, 4 FX Units (including Pattern Player), Master Clock, Informer, MIDI controls,
    /// Settings, and Browser.
    pub fn new_traktor_pro4_default() -> Self {
        let mut bus = Self::empty();

        // Global properties
        bus.register("app.traktor.flavour", PropValue::Int(1), None, None, "Traktor Flavour (1 = Pro 4)");
        bus.register("app.traktor.snap", PropValue::Bool(true), None, None, "Snap Mode");
        bus.register("app.traktor.quant", PropValue::Bool(true), None, None, "Quantize Mode");
        bus.register("app.traktor.debug.take_screenshot", PropValue::Bool(false), None, None, "Debug Screenshot Trigger");
        bus.register(
            "app.traktor.masterclock.mode",
            PropValue::Int(0),
            Some(0.0),
            Some(2.0),
            "Master Clock Mode (0=Auto, 1=Internal, 2=External)",
        );
        bus.register(
            "app.traktor.masterclock.tempo",
            PropValue::Float(128.0),
            Some(20.0),
            Some(300.0),
            "Master Clock Tempo (BPM)",
        );
        bus.register(
            "app.traktor.masterclock.source_id",
            PropValue::Int(0),
            Some(-1.0),
            Some(3.0),
            "Master Clock Source Deck",
        );

        // Settings properties
        bus.register("app.traktor.settings.deckheader.top.left", PropValue::Int(0), None, None, "Deck Header Top Left");
        bus.register("app.traktor.settings.deckheader.top.mid", PropValue::Int(1), None, None, "Deck Header Top Mid");
        bus.register("app.traktor.settings.deckheader.top.right", PropValue::Int(2), None, None, "Deck Header Top Right");
        bus.register("app.traktor.settings.deckheader.mid.left", PropValue::Int(3), None, None, "Deck Header Mid Left");
        bus.register("app.traktor.settings.deckheader.mid.mid", PropValue::Int(4), None, None, "Deck Header Mid Mid");
        bus.register("app.traktor.settings.deckheader.mid.right", PropValue::Int(5), None, None, "Deck Header Mid Right");
        bus.register("app.traktor.settings.waveform.color", PropValue::Int(0), None, None, "Waveform Color Scheme");

        // Informer messages & warnings (1..=4)
        for i in 1..=4 {
            bus.register(&format!("app.traktor.informer.deck_loading_warnings.{i}.active"), PropValue::Bool(false), None, None, "Deck Loading Warning Active");
            bus.register(&format!("app.traktor.informer.deck_loading_warnings.{i}.long"), PropValue::String(String::new()), None, None, "Deck Loading Warning Long");
            bus.register(&format!("app.traktor.informer.deck_loading_warnings.{i}.short"), PropValue::String(String::new()), None, None, "Deck Loading Warning Short");
            bus.register(&format!("app.traktor.informer.deckheader_message.{i}.active"), PropValue::Bool(false), None, None, "Deck Header Message Active");
            bus.register(&format!("app.traktor.informer.deckheader_message.{i}.long"), PropValue::String(String::new()), None, None, "Deck Header Message Long");
            bus.register(&format!("app.traktor.informer.deckheader_message.{i}.short"), PropValue::String(String::new()), None, None, "Deck Header Message Short");
            bus.register(&format!("app.traktor.informer.deckheader_message.{i}.type"), PropValue::Int(0), None, None, "Deck Header Message Type");
        }

        // Virtual MIDI controls (1..=8)
        for m in 1..=8 {
            bus.register(&format!("app.traktor.midi.buttons.{m}"), PropValue::Bool(false), None, None, "MIDI Button");
            bus.register(&format!("app.traktor.midi.faders.{m}"), PropValue::Float(0.0), Some(0.0), Some(1.0), "MIDI Fader");
            bus.register(&format!("app.traktor.midi.knobs.{m}"), PropValue::Float(0.0), Some(0.0), Some(1.0), "MIDI Knob");
        }

        // Browser properties
        bus.register("app.traktor.browser.full_screen", PropValue::Bool(false), None, None, "Browser Full Screen View");
        bus.register("app.traktor.browser.sort_id", PropValue::Int(0), None, None, "Browser Sort Column ID");
        bus.register("app.traktor.browser.flip_sort_up_down", PropValue::Bool(false), None, None, "Browser Flip Sort Direction");
        bus.register("app.traktor.browser.list.select_up_down", PropValue::Int(0), None, None, "Browser List Scroll");
        bus.register("app.traktor.browser.tree.select_up_down", PropValue::Int(0), None, None, "Browser Tree Scroll");
        bus.register("app.traktor.browser.favorites.select", PropValue::Int(0), Some(0.0), Some(11.0), "Browser Favorite Crate Index");
        bus.register("app.traktor.browser.preparation.append", PropValue::Bool(false), None, None, "Append to Preparation List");
        bus.register("app.traktor.browser.preparation.jump_to_list", PropValue::Bool(false), None, None, "Jump to Preparation List");
        bus.register("app.traktor.browser.preview_content.track_length", PropValue::Float(0.0), Some(0.0), None, "Preview Track Length");
        bus.register("app.traktor.browser.preview_player.elapsed_time", PropValue::Float(0.0), Some(0.0), None, "Preview Elapsed Time");
        bus.register("app.traktor.browser.preview_player.is_loaded", PropValue::Bool(false), None, None, "Preview Player Loaded");
        bus.register("app.traktor.browser.preview_player.load", PropValue::Bool(false), None, None, "Preview Player Load");
        bus.register("app.traktor.browser.preview_player.load_or_play", PropValue::Bool(false), None, None, "Preview Player Load or Play");
        bus.register("app.traktor.browser.preview_player.unload", PropValue::Bool(false), None, None, "Preview Player Unload");
        bus.register("app.traktor.browser.preview_player.play", PropValue::Bool(false), None, None, "Preview Player Play");
        bus.register("app.traktor.browser.preview_player.seek", PropValue::Float(0.0), Some(0.0), Some(1.0), "Preview Player Seek");

        // 4 Decks (1..=4)
        for d in 1..=4 {
            let prefix = format!("app.traktor.decks.{d}");
            bus.register(&format!("{prefix}.type"), PropValue::Int(DeckType::Track as i64), Some(0.0), Some(3.0), "Deck Flavor");
            bus.register(&format!("{prefix}.is_loaded"), PropValue::Bool(false), None, None, "Deck Loaded");
            bus.register(&format!("{prefix}.is_loaded_signal"), PropValue::Bool(false), None, None, "Deck Loaded Signal");
            bus.register(&format!("{prefix}.direct_thru"), PropValue::Bool(false), None, None, "Deck Direct Thru");
            bus.register(&format!("{prefix}.play"), PropValue::Bool(false), None, None, "Deck Play");
            bus.register(&format!("{prefix}.running"), PropValue::Bool(false), None, None, "Deck Running");
            bus.register(&format!("{prefix}.playback_mode"), PropValue::Int(0), None, None, "Deck Playback Mode");
            bus.register(&format!("{prefix}.cue"), PropValue::Bool(false), None, None, "Deck Cue");
            bus.register(&format!("{prefix}.cup"), PropValue::Bool(false), None, None, "Deck Cue Play");
            bus.register(&format!("{prefix}.seek"), PropValue::Float(0.0), Some(0.0), Some(1.0), "Deck Seek");
            bus.register(&format!("{prefix}.beatjump"), PropValue::Int(0), Some(-13.0), Some(13.0), "Deck Beatjump Trigger");
            bus.register(&format!("{prefix}.capture_source"), PropValue::Int(0), Some(0.0), Some(3.0), "Deck Capture Source");
            bus.register(&format!("{prefix}.set_as_master"), PropValue::Bool(d == 1), None, None, "Deck Set As Master");
            bus.register(&format!("{prefix}.set_to_master"), PropValue::Bool(d == 1), None, None, "Deck Tempo Master Alias");
            bus.register(&format!("{prefix}.load.selected"), PropValue::Bool(false), None, None, "Load Selected Track");
            bus.register(&format!("{prefix}.load.next"), PropValue::Bool(false), None, None, "Load Next Track");
            bus.register(&format!("{prefix}.load.previous"), PropValue::Bool(false), None, None, "Load Previous Track");
            bus.register(&format!("{prefix}.load.from_preview_player"), PropValue::Bool(false), None, None, "Load From Preview Player");
            bus.register(&format!("{prefix}.load_secondary.selected"), PropValue::Bool(false), None, None, "Load Secondary Selected");
            bus.register(&format!("{prefix}.sync.enabled"), PropValue::Bool(false), None, None, "Deck Beat Sync");
            bus.register(&format!("{prefix}.sync.phase"), PropValue::Float(0.0), Some(-0.5), Some(0.5), "Deck Sync Phase");
            bus.register(&format!("{prefix}.tempo.absolute"), PropValue::Float(1.0), Some(0.0), Some(2.0), "Tempo Absolute Ratio");
            bus.register(&format!("{prefix}.tempo.adjust"), PropValue::Float(0.0), Some(-1.0), Some(1.0), "Tempo Adjust");
            bus.register(&format!("{prefix}.tempo.adjust_bpm"), PropValue::Float(0.0), None, None, "Tempo Adjust BPM Delta");
            bus.register(&format!("{prefix}.tempo.base_bpm"), PropValue::Float(128.0), Some(20.0), Some(300.0), "Base BPM");
            bus.register(&format!("{prefix}.tempo.phase"), PropValue::Float(0.0), Some(-0.5), Some(0.5), "Tempo Beat Phase");
            bus.register(&format!("{prefix}.tempo.range_select"), PropValue::Int(1), Some(0.0), Some(5.0), "Tempo Range Index");
            bus.register(&format!("{prefix}.tempo.range_value"), PropValue::Float(0.08), Some(0.02), Some(1.0), "Tempo Range");
            bus.register(&format!("{prefix}.tempo.tempo_for_display"), PropValue::Float(128.0), None, None, "Tempo For Display");
            bus.register(&format!("{prefix}.tempo.true_bpm"), PropValue::Float(128.0), Some(20.0), Some(300.0), "True BPM");
            bus.register(&format!("{prefix}.tempo.true_tempo"), PropValue::Float(1.0), None, None, "True Tempo Ratio");
            bus.register(&format!("{prefix}.tempo.tempo_bend.up"), PropValue::Bool(false), None, None, "Tempo Bend Up");
            bus.register(&format!("{prefix}.tempo.tempo_bend.down"), PropValue::Bool(false), None, None, "Tempo Bend Down");
            bus.register(&format!("{prefix}.flux.enabled"), PropValue::Bool(false), None, None, "Flux Mode Enabled");
            bus.register(&format!("{prefix}.flux.state"), PropValue::Int(0), Some(0.0), Some(2.0), "Flux State");
            bus.register(&format!("{prefix}.reverse"), PropValue::Bool(false), None, None, "Reverse Playback");

            // Deck-level content properties (used in Screens/ & CSI/)
            bus.register(&format!("{prefix}.content.title"), PropValue::String(String::new()), None, None, "Deck Content Title");
            bus.register(&format!("{prefix}.content.artist"), PropValue::String(String::new()), None, None, "Deck Content Artist");
            bus.register(&format!("{prefix}.content.album"), PropValue::String(String::new()), None, None, "Deck Content Album");
            bus.register(&format!("{prefix}.content.genre"), PropValue::String(String::new()), None, None, "Deck Content Genre");
            bus.register(&format!("{prefix}.content.comment"), PropValue::String(String::new()), None, None, "Deck Content Comment");
            bus.register(&format!("{prefix}.content.comment2"), PropValue::String(String::new()), None, None, "Deck Content Comment 2");
            bus.register(&format!("{prefix}.content.label"), PropValue::String(String::new()), None, None, "Deck Content Label");
            bus.register(&format!("{prefix}.content.mix"), PropValue::String(String::new()), None, None, "Deck Content Mix");
            bus.register(&format!("{prefix}.content.remixer"), PropValue::String(String::new()), None, None, "Deck Content Remixer");
            bus.register(&format!("{prefix}.content.catalog_number"), PropValue::String(String::new()), None, None, "Deck Catalog Number");
            bus.register(&format!("{prefix}.content.cover_md5"), PropValue::String(String::new()), None, None, "Deck Cover MD5");
            bus.register(&format!("{prefix}.content.musical_key"), PropValue::String("1m".to_string()), None, None, "Deck Content Musical Key");
            bus.register(&format!("{prefix}.content.legacy_key"), PropValue::String("Am".to_string()), None, None, "Deck Legacy Key");
            bus.register(&format!("{prefix}.content.key_index"), PropValue::Int(21), Some(0.0), Some(23.0), "Deck Content Key Index");
            bus.register(&format!("{prefix}.content.bitrate"), PropValue::Int(1411200), None, None, "Deck Content Bitrate");
            bus.register(&format!("{prefix}.content.display_bpm"), PropValue::Float(128.0), None, None, "Deck Display BPM");
            bus.register(&format!("{prefix}.content.grid_offset"), PropValue::Float(0.0), None, None, "Deck Grid Offset");
            bus.register(&format!("{prefix}.content.total_gain"), PropValue::Float(1.0), None, None, "Deck Total Gain");

            // Track sub-namespace (`app.traktor.decks.<N>.track.*`)
            bus.register(&format!("{prefix}.track.content.title"), PropValue::String(String::new()), None, None, "Track Title");
            bus.register(&format!("{prefix}.track.content.artist"), PropValue::String(String::new()), None, None, "Track Artist");
            bus.register(&format!("{prefix}.track.content.album"), PropValue::String(String::new()), None, None, "Track Album");
            bus.register(&format!("{prefix}.track.content.bpm"), PropValue::Float(128.0), Some(0.0), Some(999.0), "Track BPM");
            bus.register(&format!("{prefix}.track.content.musical_key"), PropValue::String("1m".to_string()), None, None, "Musical Key");
            bus.register(&format!("{prefix}.track.content.entry_key"), PropValue::String(String::new()), None, None, "Track Collection Entry Key");
            bus.register(&format!("{prefix}.track.content.sample_count"), PropValue::Int(0), None, None, "Track Sample Count");
            bus.register(&format!("{prefix}.track.content.sample_rate"), PropValue::Int(48000), None, None, "Track Sample Rate");
            bus.register(&format!("{prefix}.track.content.track_length"), PropValue::Float(0.0), Some(0.0), Some(86400.0), "Track Length");
            bus.register(&format!("{prefix}.track.player.elapsed_time"), PropValue::Float(0.0), Some(0.0), Some(86400.0), "Elapsed Time");
            bus.register(&format!("{prefix}.track.player.effective_pitch"), PropValue::Float(0.0), Some(-1.0), Some(1.0), "Effective Pitch");
            bus.register(&format!("{prefix}.track.player.flux_position"), PropValue::Float(0.0), None, None, "Flux Playhead Position");
            bus.register(&format!("{prefix}.track.player.next_cue_point"), PropValue::Float(0.0), None, None, "Next Cue Point Position");
            bus.register(&format!("{prefix}.track.player.playhead_position"), PropValue::Float(0.0), None, None, "Playhead Position");
            bus.register(&format!("{prefix}.track.track_end_warning"), PropValue::Bool(false), None, None, "Track End Warning");
            bus.register(&format!("{prefix}.track.key.lock_enabled"), PropValue::Bool(true), None, None, "Keylock Enabled");
            bus.register(&format!("{prefix}.track.key.lock_enabled_preserve_pitch"), PropValue::Bool(false), None, None, "Keylock Preserve Pitch");
            bus.register(&format!("{prefix}.track.key.adjust"), PropValue::Float(0.0), Some(-12.0), Some(12.0), "Key Semitone Adjust");
            bus.register(&format!("{prefix}.track.key.transpose"), PropValue::Int(0), Some(-12.0), Some(12.0), "Key Transpose Semitones");
            bus.register(&format!("{prefix}.track.key.final_id"), PropValue::Int(21), Some(-1.0), Some(23.0), "Resulting Key ID");
            bus.register(&format!("{prefix}.track.key.resulting.precise"), PropValue::String("1m".to_string()), None, None, "Resulting Precise Key");
            bus.register(&format!("{prefix}.track.key.resulting.quantized"), PropValue::String("1m".to_string()), None, None, "Resulting Quantized Key");
            bus.register(&format!("{prefix}.track.grid.adjust_bpm"), PropValue::Float(0.0), None, None, "Grid Adjust BPM");
            bus.register(&format!("{prefix}.track.grid.enable_tick"), PropValue::Bool(false), None, None, "Grid Tick Enabled");
            bus.register(&format!("{prefix}.track.grid.lock_bpm"), PropValue::Bool(false), None, None, "Grid Lock BPM");
            bus.register(&format!("{prefix}.track.gridmarker.move"), PropValue::Float(0.0), None, None, "Grid Marker Move");
            bus.register(&format!("{prefix}.track.cue.active.type"), PropValue::Int(CueType::Cue as i64), Some(0.0), Some(5.0), "Active Cue Type");
            bus.register(&format!("{prefix}.track.cue.active.start_pos"), PropValue::Float(0.0), None, None, "Active Cue Start Pos");
            bus.register(&format!("{prefix}.track.cue.active.length"), PropValue::Float(0.0), None, None, "Active Cue Length");
            bus.register(&format!("{prefix}.track.cue.delete"), PropValue::Bool(false), None, None, "Cue Delete");
            bus.register(&format!("{prefix}.track.cue.jump_to_next_prev"), PropValue::Int(0), None, None, "Jump To Next/Prev Cue");
            bus.register(&format!("{prefix}.track.cue.store"), PropValue::Bool(false), None, None, "Cue Store");

            for h in 1..=8 {
                bus.register(&format!("{prefix}.track.cue.hotcues.{h}.exists"), PropValue::Bool(false), None, None, "Hotcue Exists");
                bus.register(&format!("{prefix}.track.cue.hotcues.{h}.delete"), PropValue::Bool(false), None, None, "Hotcue Delete");
                bus.register(&format!("{prefix}.track.cue.hotcues.{h}.type"), PropValue::Int(CueType::Cue as i64), Some(0.0), Some(5.0), "Hotcue Type");
                bus.register(&format!("{prefix}.track.cue.hotcues.{h}.start_pos"), PropValue::Float(0.0), Some(0.0), None, "Hotcue Start");
                bus.register(&format!("{prefix}.track.cue.hotcues.{h}.length"), PropValue::Float(0.0), Some(0.0), None, "Hotcue Length");
            }

            // Loop, Move, Freeze
            bus.register(&format!("{prefix}.loop.active"), PropValue::Bool(false), None, None, "Loop Active");
            bus.register(&format!("{prefix}.loop.is_in_active_loop"), PropValue::Bool(false), None, None, "In Active Loop");
            bus.register(&format!("{prefix}.loop.size"), PropValue::Int(7), Some(0.0), Some(10.0), "Loop Size (7 = 4 beats)");
            bus.register(&format!("{prefix}.loop.set"), PropValue::Bool(false), None, None, "Loop Set Trigger");
            bus.register(&format!("{prefix}.loop.set.auto_with_size"), PropValue::Int(7), Some(0.0), Some(10.0), "Auto Loop With Size");
            bus.register(&format!("{prefix}.move"), PropValue::Int(0), None, None, "Move Trigger");
            bus.register(&format!("{prefix}.move.mode"), PropValue::Int(0), Some(0.0), Some(3.0), "Move Mode");
            bus.register(&format!("{prefix}.move.size"), PropValue::Int(6), Some(0.0), Some(12.0), "Move Size");
            bus.register(&format!("{prefix}.freeze.enabled"), PropValue::Bool(false), None, None, "Freeze Enabled");
            bus.register(&format!("{prefix}.freeze.slice_size_in_beats"), PropValue::Float(1.0), Some(0.25), Some(4.0), "Freeze Slice Beats");
            bus.register(&format!("{prefix}.freeze.slice_size_in_measures"), PropValue::Float(0.25), None, None, "Freeze Slice Measures");
            bus.register(&format!("{prefix}.freeze.slice_size_index"), PropValue::Int(2), Some(0.0), Some(4.0), "Freeze Slice Size Index");
            bus.register(&format!("{prefix}.freeze.slice_count"), PropValue::Int(8), Some(8.0), Some(8.0), "Freeze Slice Count");
            bus.register(&format!("{prefix}.freeze.slice_start_in_sec"), PropValue::Float(0.0), None, None, "Freeze Slice Start Sec");
            bus.register(&format!("{prefix}.freeze.slice_width_in_sec"), PropValue::Float(0.5), None, None, "Freeze Slice Width Sec");

            // 4 Stem sub-channels
            let stem_names = ["Drums", "Bass", "Synths", "Vocals"];
            for s in 1..=4 {
                bus.register(&format!("{prefix}.stems.{s}.volume"), PropValue::Float(1.0), Some(0.0), Some(1.0), "Stem Volume");
                bus.register(&format!("{prefix}.stems.{s}.muted"), PropValue::Bool(false), None, None, "Stem Muted");
                bus.register(&format!("{prefix}.stems.{s}.filter_value"), PropValue::Float(0.5), Some(0.0), Some(1.0), "Stem Filter");
                bus.register(&format!("{prefix}.stems.{s}.filter_on"), PropValue::Bool(false), None, None, "Stem Filter On");
                bus.register(&format!("{prefix}.stems.{s}.fx_send"), PropValue::Float(0.0), Some(0.0), Some(1.0), "Stem FX Send");
                bus.register(&format!("{prefix}.stems.{s}.fx_send_on"), PropValue::Bool(false), None, None, "Stem FX Send On");
                bus.register(&format!("{prefix}.stems.{s}.name"), PropValue::String(stem_names[s - 1].to_string()), None, None, "Stem Name");
                bus.register(&format!("{prefix}.stems.{s}.color_id"), PropValue::Int(s as i64), Some(0.0), Some(17.0), "Stem Color ID");
            }

            // Remix deck & step sequencer
            bus.register(&format!("{prefix}.remix.quant"), PropValue::Bool(true), None, None, "Remix Quantize");
            bus.register(&format!("{prefix}.remix.quant_index"), PropValue::Int(2), Some(0.0), Some(6.0), "Remix Quantize Index");
            bus.register(&format!("{prefix}.remix.current_beat_pos"), PropValue::Int(0), None, None, "Remix Current Beat Pos");
            bus.register(&format!("{prefix}.remix.page"), PropValue::Int(0), Some(0.0), Some(3.0), "Remix Page");
            bus.register(&format!("{prefix}.remix.cell_Capturing_source"), PropValue::Int(0), Some(0.0), Some(3.0), "Remix Capture Source");
            bus.register(&format!("{prefix}.remix.sequencer.on"), PropValue::Bool(false), None, None, "Remix Sequencer On");
            bus.register(&format!("{prefix}.remix.sequencer.rec.on"), PropValue::Bool(false), None, None, "Remix Sequencer Record On");
            bus.register(&format!("{prefix}.remix.sequencer.swing"), PropValue::Float(0.0), Some(0.0), Some(1.0), "Remix Sequencer Swing");
            for c in 1..=4 {
                bus.register(&format!("{prefix}.remix.players.{c}.volume"), PropValue::Float(1.0), Some(0.0), Some(1.0), "Remix Slot Volume");
                bus.register(&format!("{prefix}.remix.players.{c}.muted"), PropValue::Bool(false), None, None, "Remix Slot Muted");
                bus.register(&format!("{prefix}.remix.players.{c}.filter_value"), PropValue::Float(0.5), Some(0.0), Some(1.0), "Remix Slot Filter");
                bus.register(&format!("{prefix}.remix.players.{c}.filter_on"), PropValue::Bool(false), None, None, "Remix Slot Filter On");
                bus.register(&format!("{prefix}.remix.players.{c}.fx_send"), PropValue::Float(0.0), Some(0.0), Some(1.0), "Remix Slot FX Send");
                bus.register(&format!("{prefix}.remix.players.{c}.fx_send_on"), PropValue::Bool(false), None, None, "Remix Slot FX Send On");
                bus.register(&format!("{prefix}.remix.players.{c}.sequencer.pattern_length"), PropValue::Int(8), Some(1.0), Some(16.0), "Sequencer Pattern Length");
                bus.register(&format!("{prefix}.remix.players.{c}.sequencer.selected_cell"), PropValue::Int(0), Some(0.0), Some(15.0), "Sequencer Selected Cell");

                // Remix cell matrix (columns 1..=4, rows 1..=4 for default registration; dynamic up to 16)
                for r in 1..=4 {
                    let cp = format!("{prefix}.remix.cell.columns.{c}.rows.{r}");
                    bus.register(&format!("{cp}.name"), PropValue::String(String::new()), None, None, "Remix Cell Name");
                    bus.register(&format!("{cp}.color_id"), PropValue::Int(0), None, None, "Remix Cell Color ID");
                    bus.register(&format!("{cp}.play_mode"), PropValue::Int(0), None, None, "Remix Cell Play Mode");
                    bus.register(&format!("{cp}.state"), PropValue::Int(0), None, None, "Remix Cell State");
                    bus.register(&format!("{cp}.animation.display_state"), PropValue::Int(0), None, None, "Remix Cell Anim Display State");
                    bus.register(&format!("{cp}.animation.color_ratio"), PropValue::Float(0.0), None, None, "Remix Cell Anim Color Ratio");
                    for a in 1..=2 {
                        bus.register(&format!("{cp}.animation.brightness.{a}"), PropValue::Float(1.0), None, None, "Remix Cell Anim Brightness");
                        bus.register(&format!("{cp}.animation.color_id.{a}"), PropValue::Int(0), None, None, "Remix Cell Anim Color ID");
                    }
                }
            }
        }

        // 4 Mixer channels + Master + Crossfader
        for ch in 1..=4 {
            let p = format!("app.traktor.mixer.channels.{ch}");
            bus.register(&format!("{p}.volume"), PropValue::Float(0.85), Some(0.0), Some(1.0), "Channel Volume");
            bus.register(&format!("{p}.gain"), PropValue::Float(0.5), Some(0.0), Some(1.0), "Channel Gain");
            bus.register(&format!("{p}.autogain"), PropValue::Float(0.0), Some(-24.0), Some(24.0), "Channel Autogain dB");
            bus.register(&format!("{p}.eq.high"), PropValue::Float(0.5), Some(0.0), Some(1.0), "EQ High");
            bus.register(&format!("{p}.eq.mid"), PropValue::Float(0.5), Some(0.0), Some(1.0), "EQ Mid");
            bus.register(&format!("{p}.eq.mid_low"), PropValue::Float(0.5), Some(0.0), Some(1.0), "EQ Mid-Low");
            bus.register(&format!("{p}.eq.low"), PropValue::Float(0.5), Some(0.0), Some(1.0), "EQ Low");
            bus.register(&format!("{p}.eq.kill_high"), PropValue::Bool(false), None, None, "EQ Kill High");
            bus.register(&format!("{p}.eq.kill_mid"), PropValue::Bool(false), None, None, "EQ Kill Mid");
            bus.register(&format!("{p}.eq.kill_low"), PropValue::Bool(false), None, None, "EQ Kill Low");
            bus.register(&format!("{p}.fx.select"), PropValue::Int(0), Some(0.0), Some(8.0), "Mixer FX Select");
            bus.register(&format!("{p}.fx.adjust"), PropValue::Float(0.5), Some(0.0), Some(1.0), "Mixer FX Bipolar Knob");
            bus.register(&format!("{p}.fx.on"), PropValue::Bool(false), None, None, "Mixer FX On");
            bus.register(&format!("{p}.cue"), PropValue::Bool(false), None, None, "Headphone Cue");
            bus.register(&format!("{p}.xfader_assign.left"), PropValue::Bool(ch % 2 == 1), None, None, "Crossfader Left Assign");
            bus.register(&format!("{p}.xfader_assign.right"), PropValue::Bool(ch % 2 == 0), None, None, "Crossfader Right Assign");
            bus.register(&format!("{p}.level.prefader.linear.sum"), PropValue::Float(0.0), Some(0.0), Some(1.0), "Channel Meter Level");
            bus.register(&format!("{p}.level.prefader.linear.meter"), PropValue::Float(0.0), Some(0.0), Some(1.0), "Channel Meter Segmented");
            for u in 1..=4 {
                bus.register(&format!("{p}.fx.assign.{u}"), PropValue::Bool(false), None, None, "FX Unit Assign");
            }
        }
        bus.register("app.traktor.mixer.xfader.adjust", PropValue::Float(0.5), Some(0.0), Some(1.0), "Crossfader Position");
        bus.register("app.traktor.mixer.xfader.curve", PropValue::Float(0.5), Some(0.0), Some(1.0), "Crossfader Curve");
        bus.register("app.traktor.mixer.master.volume", PropValue::Float(0.8), Some(0.0), Some(1.0), "Master Volume");
        bus.register("app.traktor.mixer.master_volume", PropValue::Float(0.8), Some(0.0), Some(1.0), "Master Volume Alias");
        bus.register("app.traktor.mixer.mic_volume", PropValue::Float(0.0), Some(0.0), Some(1.0), "Microphone Volume");
        bus.register("app.traktor.mixer.master.level.left", PropValue::Float(0.0), Some(0.0), Some(1.0), "Master Left Meter");
        bus.register("app.traktor.mixer.master.level.right", PropValue::Float(0.0), Some(0.0), Some(1.0), "Master Right Meter");
        bus.register("app.traktor.mixer.master.level.sum", PropValue::Float(0.0), Some(0.0), Some(1.0), "Master Sum Meter");
        bus.register("app.traktor.mixer.master.level.clip.left", PropValue::Bool(false), None, None, "Master Left Clip");
        bus.register("app.traktor.mixer.master.level.clip.right", PropValue::Bool(false), None, None, "Master Right Clip");
        bus.register("app.traktor.mixer.master.level.clip.sum", PropValue::Bool(false), None, None, "Master Sum Clip");
        bus.register("app.traktor.mixer.master.limiter_state", PropValue::Int(0), None, None, "Master Limiter State");
        bus.register("app.traktor.mixer.master.limiter.enabled", PropValue::Bool(true), None, None, "Ozone Maximizer Limiter Enable");
        bus.register("app.traktor.mixer.master.limiter.boost", PropValue::Float(0.0), Some(0.0), Some(12.0), "Ozone Maximizer Boost dB");
        bus.register("app.traktor.mixer.master.cue.mix", PropValue::Float(0.0), Some(0.0), Some(1.0), "Headphone Cue/Master Mix");
        bus.register("app.traktor.mixer.master.cue.volume", PropValue::Float(0.5), Some(0.0), Some(1.0), "Headphone Volume");
        bus.register("app.traktor.mixer.cue.mix", PropValue::Float(0.0), Some(0.0), Some(1.0), "Headphone Cue Mix");
        bus.register("app.traktor.mixer.cue.volume", PropValue::Float(0.5), Some(0.0), Some(1.0), "Headphone Cue Volume");

        // 4 FX Units + Pattern Player
        bus.register("app.traktor.fx.4fx_units", PropValue::Bool(true), None, None, "4 FX Units Enabled");
        for u in 1..=4 {
            let p = format!("app.traktor.fx.{u}");
            bus.register(&format!("{p}.type"), PropValue::Int(0), Some(0.0), Some(2.0), "FX Unit Type (0=Group, 1=Single, 2=PatternPlayer)");
            bus.register(&format!("{p}.name"), PropValue::String(format!("FX Unit {u}")), None, None, "FX Unit Name");
            bus.register(&format!("{p}.routing"), PropValue::Int(0), Some(0.0), Some(2.0), "FX Routing");
            bus.register(&format!("{p}.dry_wet"), PropValue::Float(0.0), Some(0.0), Some(1.0), "FX Dry/Wet");
            bus.register(&format!("{p}.enabled"), PropValue::Bool(false), None, None, "FX Master Enable");
            bus.register(&format!("{p}.store"), PropValue::Bool(false), None, None, "FX Store Snapshot");
            bus.register(&format!("{p}.kitSelect"), PropValue::Int(0), None, None, "Pattern Player Kit Select");
            bus.register(&format!("{p}.pattern_player.clear_pattern"), PropValue::Bool(false), None, None, "Pattern Player Clear");
            bus.register(&format!("{p}.pattern_player.current_sound"), PropValue::Int(0), None, None, "Pattern Player Current Sound");
            bus.register(&format!("{p}.pattern_player.current_step"), PropValue::Int(0), None, None, "Pattern Player Current Step");
            bus.register(&format!("{p}.pattern_player.kit_shortname"), PropValue::String("808".to_string()), None, None, "Pattern Player Kit Shortname");
            for k in 1..=3 {
                bus.register(&format!("{p}.knobs.{k}"), PropValue::Float(0.5), Some(0.0), Some(1.0), "FX Knob");
                bus.register(&format!("{p}.knobs.{k}.name"), PropValue::String(format!("Knob {k}")), None, None, "FX Knob Name");
                bus.register(&format!("{p}.parameters.{k}"), PropValue::Float(0.5), Some(0.0), Some(1.0), "FX Parameter");
                bus.register(&format!("{p}.buttons.{k}"), PropValue::Bool(false), None, None, "FX Button");
                bus.register(&format!("{p}.buttons.{k}.name"), PropValue::String(format!("Btn {k}")), None, None, "FX Button Name");
                bus.register(&format!("{p}.select.{k}"), PropValue::Int(k as i64), Some(0.0), Some(45.0), "FX Effect Select");
            }
        }

        bus
    }

    /// Registers a property path with metadata and initial value.
    pub fn register(
        &mut self,
        path: &str,
        value: PropValue,
        min: Option<f64>,
        max: Option<f64>,
        description: &str,
    ) {
        self.props.insert(
            path.to_string(),
            PropMeta {
                path: path.to_string(),
                value,
                min,
                max,
                description: description.to_string(),
            },
        );
    }

    /// Reads the current value of a property path.
    pub fn get(&self, path: &str) -> Option<&PropValue> {
        self.props.get(path).map(|m| &m.value)
    }

    /// Reads the full [`PropMeta`] of a property path.
    pub fn get_meta(&self, path: &str) -> Option<&PropMeta> {
        self.props.get(path)
    }

    /// Returns an ordered iterator over all registered properties.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &PropMeta)> {
        self.props.iter()
    }

    /// Returns the total count of registered properties.
    pub fn len(&self) -> usize {
        self.props.len()
    }

    /// Returns true if no properties are registered.
    pub fn is_empty(&self) -> bool {
        self.props.is_empty()
    }

    /// Updates a property value (clamping numeric values if `min`/`max` bounds exist)
    /// and notifies any registered subscribers when the value changes.
    pub fn set(&mut self, path: &str, mut new_value: PropValue) -> bool {
        if let Some(meta) = self.props.get_mut(path) {
            if let PropValue::Float(v) = &mut new_value {
                if let Some(lo) = meta.min {
                    *v = v.max(lo);
                }
                if let Some(hi) = meta.max {
                    *v = v.min(hi);
                }
            } else if let PropValue::Int(v) = &mut new_value {
                if let Some(lo) = meta.min {
                    *v = (*v).max(lo as i64);
                }
                if let Some(hi) = meta.max {
                    *v = (*v).min(hi as i64);
                }
            }

            if meta.value == new_value {
                return false;
            }
            meta.value = new_value.clone();
        } else {
            self.props.insert(
                path.to_string(),
                PropMeta {
                    path: path.to_string(),
                    value: new_value.clone(),
                    min: None,
                    max: None,
                    description: "Dynamic property".to_string(),
                },
            );
        }

        if let Some(callbacks) = self.subscribers.get_mut(path) {
            for cb in callbacks.iter_mut() {
                cb(path, &new_value);
            }
        }
        true
    }

    /// Subscribes a callback to value changes on `path`.
    pub fn subscribe<F>(&mut self, path: &str, callback: F)
    where
        F: FnMut(&str, &PropValue) + Send + 'static,
    {
        self.subscribers
            .entry(path.to_string())
            .or_default()
            .push(Box::new(callback));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observed_qml_property_templates_registered() {
        let bus = PropertyBus::new_traktor_pro4_default();
        let observed = include_str!("../../../docs/evidence/generated/app-traktor-paths.observed.txt");
        let mut missing = Vec::new();
        for line in observed.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let concrete = trimmed.replace("<N>", "1");
            if bus.get(&concrete).is_none() {
                missing.push(concrete);
            }
        }
        assert!(
            missing.is_empty(),
            "PropertyBus missing observed QML paths: {missing:?}"
        );
    }
}
