# ReTraktor Evidence Ledger: `app.traktor.*` Property Bus Catalog

## 1. Provenance & REA Binary Evidence

All findings in this catalog are anchored to shipped artifacts from **Traktor Pro 4 (`4.5.1 21`)** inspected via **REA (`rea-agents@6.7.0`)** + **Hopper Disassembler v6.2.0**:

| Artifact / View | REA Evidence ID / Path | SHA-256 / UUID |
| :--- | :--- | :--- |
| `Traktor Pro 4` Universal Binary | `ev_fb958e256da8dbb4f77cb48853b8d825c706ee0e6c6c15656ae0de75af169bab` | `8a155de62bca89d2905e4aa7d1198b11540565437094dc71e529956f15a99294` |
| Mach-O Summary (`arm64` slice @ `0x7a70000`) | `ev_95f5ad86df4a2b343ad112771c43ef36b64aa3b8c67222d6b1d16a980a25e10d` | UUID `A5A811E6-CFF6-324D-9DEC-904A84921F88` |
| Mach-O Segments & `.qtmimedatabase` | `ev_a9ec5856a95be76b9a45a68cf735caff40617150b9173991fb1fff36f2b8679a` | `__TEXT` (`87.78 MB`), `__DATA_CONST` (`6.08 MB`) |
| Mach-O Build Metadata | `ev_532ba3d6f80ab59c4d3d09628dcd80d2acf56f9c4d817edfc02e001f5f77298b` | `platform: MACOS`, `min_os: 12.0`, `sdk: 13.3` |
| Mach-O Exports & Embedded Decoders | `ev_0db2f69bfbd1521bc1986118a7817103f09f53b4d8403ec86649ad28fb6b30b9` | `_IIS_MP3D_*`, `_IIS_Mp3sDec_*`, `_WebP*` |
| Mach-O Entrypoints | `ev_3323a851855472a3fcd662f7d31188ce237ba7679b87f20f71a5ece569cfd3f5` | `file_offset: 11086800` (`0x100a92b90`) |
| `iZTraktorTDSP.dylib` (iZotope Ozone Maximizer) | `ev_3bbfeea70b9b15fe32b11954527ae893063832e0b1995ec7af7d0812eb021803` / `ev_0f3b8a2c2c8b6708ecf63e17480e116ee99ab720358d61a5793eaf7d98730e0a` | `_TestableDSP_*`, `MOTHERSHIP_9_13_6` (`b21cf78b...`) |
| `libpcon.dylib` (Pioneer Pro DJ Link / CDJ Auth) | `ev_4ee84839e36b5c247471ffbd25ab7b030fb6b3faed06c61d0a05371d83051b14` / `ev_38d2bfbb15bf83d3a1830cde871d0058f55be88678098776d290e542767e1cbc` | `_PDJ_AuthInitialize`, `_PDJ_AuthSetup`, `_PDJ_AuthInput` (`108d02ea...`) |
| Shipped QML CSI Property Bindings | `Traktor Pro 4.app/Contents/Resources/qml/CSI/` | `S4MK3/`, `S8/`, `X1MK3/`, `Z1MK2/`, `Common/` |

---

## 2. Architecture of the `app.traktor.*` State Bus

Traktor exposes its entire internal engine state to both the Qt/QML GUI (`Screens/`) and the Controller Script Interface (`CSI/`) through a hierarchical, typed, observable property tree rooted at `app.traktor.`.

Each node in the tree is addressed by a dot-delimited path (1-indexed for deck, channel, FX unit, hotcue, remix slot, and stem indices) and holds a strongly-typed value:
- `Bool` (`true` / `false`)
- `Int` (`i64` — discrete enum index, integer count, or beat/loop size index)
- `Float` (`f64` — normalized `0.0..1.0`, bipolar `-1.0..1.0`, BPM, or seconds)
- `String` (UTF-8 metadata such as title, artist, musical key string)

In QML, `AppProperty { id: prop; path: "app.traktor.decks.1.play" }` subscribes to changes on that path and allows reading/writing `prop.value`, reading `prop.description`, and inspect min/max bounds (`prop.min`, `prop.max`).

---

## 3. Complete `app.traktor.*` Namespace Catalog

### 3.1 Decks (`app.traktor.decks.<N>.*`, where `N` in `1..=4`)

| Property Path | Value Type | Range / Enum | Description |
| :--- | :--- | :--- | :--- |
| `app.traktor.decks.<N>.type` | `Int` (`DeckType`) | `0=Track, 1=Remix, 2=Stem, 3=Live` | Active deck flavor |
| `app.traktor.decks.<N>.is_loaded` | `Bool` | `false..true` | True when a track, stem, or remix set is loaded |
| `app.traktor.decks.<N>.is_loaded_signal` | `Bool` | Trigger | Fires when deck load completes |
| `app.traktor.decks.<N>.play` | `Bool` | `false..true` | Deck transport play/pause state |
| `app.traktor.decks.<N>.cue` | `Bool` | `false..true` | Transport Cue button state |
| `app.traktor.decks.<N>.cup` | `Bool` | `false..true` | Cue Play (CUP) transport state |
| `app.traktor.decks.<N>.seek` | `Float` | `0.0..1.0` | Normalized playhead position within track |
| `app.traktor.decks.<N>.set_to_master` | `Bool` | `false..true` | Assigns this deck as the Tempo Master |
| `app.traktor.decks.<N>.sync.enabled` | `Bool` | `false..true` | Beat-sync lock state |
| `app.traktor.decks.<N>.sync.phase` | `Float` | `-0.5..0.5` | Phase offset relative to Master Clock |
| `app.traktor.decks.<N>.tempo.base_bpm` | `Float` | `20.0..300.0` | Analyzed track base BPM |
| `app.traktor.decks.<N>.tempo.true_bpm` | `Float` | `20.0..300.0` | Effective playback BPM after tempo adjust |
| `app.traktor.decks.<N>.tempo.adjust` | `Float` | `-1.0..1.0` | Pitch fader tempo adjustment |
| `app.traktor.decks.<N>.tempo.range_value` | `Float` | `0.02..1.0` | Tempo fader range (`6%`, `8%`, `10%`, `16%`, `100%`) |
| `app.traktor.decks.<N>.tempo.tempo_bend.up` | `Bool` | `false..true` | Momentary pitch bend up |
| `app.traktor.decks.<N>.tempo.tempo_bend.down` | `Bool` | `false..true` | Momentary pitch bend down |
| `app.traktor.decks.<N>.flux.enabled` | `Bool` | `false..true` | Flux / Slip mode enabled |
| `app.traktor.decks.<N>.flux.state` | `Int` | `0..2` | Active Flux timeline state |
| `app.traktor.decks.<N>.reverse` | `Bool` | `false..true` | Reverse playback active |

### 3.2 Track Sub-Namespace (`app.traktor.decks.<N>.track.*`)

| Property Path | Value Type | Range / Enum | Description |
| :--- | :--- | :--- | :--- |
| `app.traktor.decks.<N>.track.content.title` | `String` | UTF-8 | Loaded track title |
| `app.traktor.decks.<N>.track.content.artist` | `String` | UTF-8 | Loaded track artist |
| `app.traktor.decks.<N>.track.content.album` | `String` | UTF-8 | Loaded track album |
| `app.traktor.decks.<N>.track.content.bpm` | `Float` | `0.0..999.0` | Track tag BPM |
| `app.traktor.decks.<N>.track.content.musical_key` | `String` | Open Key (`1m..12d`) | Track key string |
| `app.traktor.decks.<N>.track.content.track_length` | `Float` | Seconds | Duration in seconds |
| `app.traktor.decks.<N>.track.player.elapsed_time` | `Float` | Seconds | Current playhead position in seconds |
| `app.traktor.decks.<N>.track.player.effective_pitch` | `Float` | `-1.0..1.0` | Effective pitch ratio |
| `app.traktor.decks.<N>.track.key.lock_enabled` | `Bool` | `false..true` | Keylock (Elastique time-stretch) enabled |
| `app.traktor.decks.<N>.track.key.lock_enabled_preserve_pitch` | `Bool` | `false..true` | Keylock preserving current pitch offset |
| `app.traktor.decks.<N>.track.key.adjust` | `Float` | `-12.0..12.0` | Semitone key transpose offset |
| `app.traktor.decks.<N>.track.key.final_id` | `Int` | `-1..23` | Resulting musical key ID (`0..23`), `-1` when off |
| `app.traktor.decks.<N>.track.grid.enable_tick` | `Bool` | `false..true` | Beatgrid metronome tick |
| `app.traktor.decks.<N>.track.grid.lock_bpm` | `Bool` | `false..true` | Beatgrid BPM lock |
| `app.traktor.decks.<N>.track.cue.active.type` | `Int` (`CueType`) | `0=Cue, 1=FadeIn, 2=FadeOut, 3=Load, 4=Grid, 5=Loop` | Active cue marker type |
| `app.traktor.decks.<N>.track.cue.hotcues.<H>.exists` | `Bool` | `H` in `1..=8` | Whether Hotcue slot `H` is populated |
| `app.traktor.decks.<N>.track.cue.hotcues.<H>.type` | `Int` (`CueType`) | `0..5` | Hotcue `H` marker type |
| `app.traktor.decks.<N>.track.cue.hotcues.<H>.start_pos` | `Float` | Seconds | Hotcue `H` start position |
| `app.traktor.decks.<N>.track.cue.hotcues.<H>.length` | `Float` | Seconds | Hotcue `H` loop length (`0.0` if non-loop) |

### 3.3 Loop, Move & Freeze Slicer (`app.traktor.decks.<N>.loop.*`, `move.*`, `freeze.*`)

| Property Path | Value Type | Range / Enum | Description |
| :--- | :--- | :--- | :--- |
| `app.traktor.decks.<N>.loop.active` | `Bool` | `false..true` | Loop currently active |
| `app.traktor.decks.<N>.loop.is_in_active_loop` | `Bool` | `false..true` | Playhead is currently inside active loop |
| `app.traktor.decks.<N>.loop.size` | `Int` (`LoopSize`) | `0..10` (`1/32` .. `32` beats) | Selected loop size index |
| `app.traktor.decks.<N>.loop.set` | `Bool` | Trigger | Activate loop of current `loop.size` |
| `app.traktor.decks.<N>.loop.backward` | `Bool` | Trigger | Move loop backward by loop length |
| `app.traktor.decks.<N>.loop.forward` | `Bool` | Trigger | Move loop forward by loop length |
| `app.traktor.decks.<N>.move.mode` | `Int` (`MoveMode`) | `0=Beatjump, 1=Loop, 2=LoopIn, 3=LoopOut` | Move mode selector |
| `app.traktor.decks.<N>.move.size` | `Int` (`JumpSize`) | `0..13` (`Fine`, `UltraFine`, `1/16`..`32`, `Loop`) | Beatjump step size |
| `app.traktor.decks.<N>.move.internal` | `Int` | `-1, 0, +1` | Direction trigger for move |
| `app.traktor.decks.<N>.freeze.enabled` | `Bool` | `false..true` | Freeze mode enabled |
| `app.traktor.decks.<N>.freeze.slice_size_in_beats` | `Float` | `0.25..4.0` | Freeze slice length in beats |
| `app.traktor.decks.<N>.freeze.slice_size_index` | `Int` | `0..4` (`1/4`, `1/2`, `1`, `2`, `4`) | Freeze slice size index |
| `app.traktor.decks.<N>.freeze.slice_count` | `Int` | `8` | Number of slices (`8`) |
| `app.traktor.decks.<N>.freeze.current_slice` | `Int` | `0..7` | Currently playing slice index |

### 3.4 Stem Deck & Remix Deck (`stems.*`, `remix.*`)

| Property Path | Value Type | Range / Enum | Description |
| :--- | :--- | :--- | :--- |
| `app.traktor.decks.<N>.stems.<S>.volume` | `Float` (`S` in `1..=4`) | `0.0..1.0` | Stem slot volume (`1=Drums, 2=Bass, 3=Other, 4=Vocals`) |
| `app.traktor.decks.<N>.stems.<S>.muted` | `Bool` | `false..true` | Stem slot mute state |
| `app.traktor.decks.<N>.stems.<S>.filter_value` | `Float` | `0.0..1.0` (`0.5` center) | Stem slot bipolar filter |
| `app.traktor.decks.<N>.stems.<S>.filter_on` | `Bool` | `false..true` | Stem slot filter bypass toggle |
| `app.traktor.decks.<N>.stems.<S>.fx_send` | `Float` | `0.0..1.0` | Stem slot FX send level |
| `app.traktor.decks.<N>.stems.<S>.fx_send_on` | `Bool` | `false..true` | Stem slot FX send enable |
| `app.traktor.decks.<N>.stems.<S>.name` | `String` | UTF-8 | Stem slot label (`Drums`, `Bass`, `Synths`, `Vocals`) |
| `app.traktor.decks.<N>.stems.<S>.color_id` | `Int` | `0..15` | Stem slot palette color |
| `app.traktor.decks.<N>.remix.quant` | `Bool` | `false..true` | Remix deck quantization enabled |
| `app.traktor.decks.<N>.remix.quant_index` | `Int` | `0..6` | Remix deck quantization beat value |
| `app.traktor.decks.<N>.remix.page` | `Int` | `0..3` | Active 4x4 Remix sample page (`1..4`) |
| `app.traktor.decks.<N>.remix.cell_Capturing_source` | `Int` | `0..3` | Capture source deck for Remix sampling |
| `app.traktor.decks.<N>.remix.sequencer.on` | `Bool` | `false..true` | Remix Step Sequencer enabled |
| `app.traktor.decks.<N>.remix.sequencer.swing` | `Float` | `0.0..1.0` | Step Sequencer swing amount |
| `app.traktor.decks.<N>.remix.players.<C>.volume` | `Float` (`C` in `1..=4`) | `0.0..1.0` | Remix column `C` volume |
| `app.traktor.decks.<N>.remix.players.<C>.filter_value` | `Float` | `0.0..1.0` | Remix column `C` filter |
| `app.traktor.decks.<N>.remix.players.<C>.filter_on` | `Bool` | `false..true` | Remix column `C` filter enable |
| `app.traktor.decks.<N>.remix.players.<C>.fx_send` | `Float` | `0.0..1.0` | Remix column `C` FX send |
| `app.traktor.decks.<N>.remix.players.<C>.fx_send_on` | `Bool` | `false..true` | Remix column `C` FX send enable |
| `app.traktor.decks.<N>.remix.players.<C>.sequencer.pattern_length` | `Int` | `8` or `16` | Step sequencer pattern length for slot `C` |

### 3.5 Mixer, Master Clock, FX Units & Browser

| Property Path | Value Type | Range / Enum | Description |
| :--- | :--- | :--- | :--- |
| `app.traktor.mixer.channels.<N>.volume` | `Float` (`N` in `1..=4`) | `0.0..1.0` | Channel line fader volume |
| `app.traktor.mixer.channels.<N>.gain` | `Float` | `0.0..1.0` | Channel rotary trim/gain |
| `app.traktor.mixer.channels.<N>.autogain` | `Float` | dB / linear | Analyzed autogain scalar |
| `app.traktor.mixer.channels.<N>.eq.high` | `Float` | `0.0..1.0` (`0.5` unity) | High EQ band |
| `app.traktor.mixer.channels.<N>.eq.mid` | `Float` | `0.0..1.0` (`0.5` unity) | Mid EQ band |
| `app.traktor.mixer.channels.<N>.eq.mid_low` | `Float` | `0.0..1.0` (`0.5` unity) | Mid-Low EQ band (Xone:92 4-band mode) |
| `app.traktor.mixer.channels.<N>.eq.low` | `Float` | `0.0..1.0` (`0.5` unity) | Low EQ band |
| `app.traktor.mixer.channels.<N>.fx.select` | `Int` | `0..4` (`0=Filter, 1..4=MixerFX`) | Selected Mixer FX slot |
| `app.traktor.mixer.channels.<N>.fx.adjust` | `Float` | `0.0..1.0` (`0.5` center) | Bipolar Mixer FX / Filter knob |
| `app.traktor.mixer.channels.<N>.fx.on` | `Bool` | `false..true` | Mixer FX / Filter on/off button |
| `app.traktor.mixer.channels.<N>.fx.assign.<U>` | `Bool` (`U` in `1..=4`) | `false..true` | Route channel `N` to FX Unit `U` |
| `app.traktor.mixer.channels.<N>.cue` | `Bool` | `false..true` | Headphone cue PFL toggle |
| `app.traktor.mixer.channels.<N>.xfader_assign.left` | `Bool` | `false..true` | Assign channel to Crossfader Left (A) |
| `app.traktor.mixer.channels.<N>.xfader_assign.right` | `Bool` | `false..true` | Assign channel to Crossfader Right (B) |
| `app.traktor.mixer.channels.<N>.level.prefader.linear.sum` | `Float` | `0.0..1.0` | Pre-fader VU meter level |
| `app.traktor.mixer.xfader.adjust` | `Float` | `0.0..1.0` (`0.5` center) | Crossfader position |
| `app.traktor.mixer.xfader.curve` | `Float` | `0.0..1.0` | Crossfader curve (Smooth .. Sharp) |
| `app.traktor.mixer.master.volume` | `Float` | `0.0..1.0` | Master output volume |
| `app.traktor.mixer.master.cue.mix` | `Float` | `0.0..1.0` | Headphone Cue / Master mix balance |
| `app.traktor.mixer.master.cue.volume` | `Float` | `0.0..1.0` | Headphone amplifier volume |
| `app.traktor.masterclock.mode` | `Int` | `0=Auto, 1=Manual/Internal, 2=External` | Master clock mode |
| `app.traktor.masterclock.tempo` | `Float` | `20.0..300.0` | Global Master Clock BPM |
| `app.traktor.masterclock.source_id` | `Int` | `-1=Internal, 0..3=Deck A..D` | Current tempo master source |
| `app.traktor.fx.<U>.type` | `Int` (`U` in `1..=4`) | `0=Group, 1=Single, 2=PatternPlayer` | FX Unit mode |
| `app.traktor.fx.<U>.routing` | `Int` | `0=Insert, 1=Send, 2=PostFader` | FX Unit audio routing |
| `app.traktor.fx.<U>.dry_wet` | `Float` | `0.0..1.0` | FX Unit Dry/Wet knob |
| `app.traktor.fx.<U>.knobs.<K>` | `Float` (`K` in `1..=3`) | `0.0..1.0` | FX Parameter Knob `1..3` |
| `app.traktor.fx.<U>.buttons.<B>` | `Bool` (`B` in `1..=3`) | `false..true` | FX Parameter Button `1..3` |
| `app.traktor.fx.<U>.enabled` | `Bool` | `false..true` | FX Unit master on/off button |
| `app.traktor.fx.<U>.select.<K>` | `Int` (`K` in `1..=3`) | Effect ID | Effect selection for slot `K` |
| `app.traktor.browser.full_screen` | `Bool` | `false..true` | Full-screen browser view toggle |
| `app.traktor.browser.list.select_up_down` | `Int` | Relative delta | Scroll browser track list |
| `app.traktor.browser.tree.select_up_down` | `Int` | Relative delta | Scroll browser playlist tree |
| `app.traktor.browser.favorites.select` | `Int` | `0..11` | Select favorite crate `1..12` |
| `app.traktor.browser.preparation.append` | `Bool` | Trigger | Append selected track to Preparation list |
| `app.traktor.browser.preview_player.is_loaded` | `Bool` | `false..true` | Preview Player loaded state |
| `app.traktor.browser.preview_player.play` | `Bool` | `false..true` | Preview Player play/pause |
| `app.traktor.browser.preview_player.seek` | `Float` | `0.0..1.0` | Preview Player seek position |
| `app.traktor.snap` | `Bool` | `false..true` | Global Snap (S) mode |
| `app.traktor.quant` | `Bool` | `false..true` | Global Quantize (Q) mode |
