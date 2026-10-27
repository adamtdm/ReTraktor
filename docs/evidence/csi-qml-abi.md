# ReTraktor Evidence Ledger: `CSI 1.0` QML Controller Script Interface & `Defines` ABI

## 1. Provenance & Shipped Evidence

Extracted from `Traktor Pro 4.app/Contents/Resources/qml/`:
- `Defines/qmldir` and all 10 QML singleton files (`PadsMode.qml`, `Overlay.qml`, `StemStyle.qml`, `FooterPage.qml`, `FxOverlay.qml`, `JumpSize.qml`, `LoopSize.qml`, `MoveSize.qml`, `NavigationMode.qml`, `ScreenView.qml`).
- `CSI/Common/Deck_S8Style.qml`, `CSI/Common/BrowserModule.qml`, `CSI/Common/ExtendedBrowserModule.qml`, `CSI/Common/HotcuesModule.qml`, and controller directories `S4MK3`, `S8`, `S5`, `D2`, `S2MK3`, `S3`, `X1MK3`, `Z1MK2`, `CDJ3000`, `XDJ1000MK2`, `XDJ700`, `MX2`.
- Binary registration symbols in `Traktor Pro 4` (`ev_fb958e256da8dbb4f77cb48853b8d825c706ee0e6c6c15656ae0de75af169bab`).

---

## 2. `Defines` QML Singleton Enumeration Exact Values

In `Resources/qml/Defines/qmldir`, Traktor registers 10 global QML singletons at version `1.0`. Every controller script imports `Defines 1.0` and relies on these exact integer constants:

### 2.1 `PadsMode` (`Defines/PadsMode.qml`)
| Constant | Integer Value | Semantics |
| :--- | :--- | :--- |
| `PadsMode.disabled` | `0` | Pad section inactive |
| `PadsMode.hotcues` | `1` | 8 Hotcue trigger/delete pads |
| `PadsMode.remix` | `2` | Remix Deck cell trigger mode |
| `PadsMode.stems` | `3` | Stem Deck mute/FX send pad mode |
| `PadsMode.freeze` | `4` | Freeze slicer mode (8 slices) |
| `PadsMode.loop` | `5` | Fixed-length loop / loop roll pads |
| `PadsMode.legacyRemix` | `2` | Alias for `PadsMode.remix` |
| `PadsMode.sequencerRec` | `6` | Remix Deck Step Sequencer pattern record |
| `PadsMode.sequencerSteps` | `7` | Remix Deck Step Sequencer 8-step editor |
| `PadsMode.advancedLoop` | `8` | Advanced loop control |
| `PadsMode.effects` | `9` | Pad FX trigger mode |
| `PadsMode.patternPlayer` | `10` | Traktor Pro 4 Pattern Player step pads |

### 2.2 `Overlay` (`Defines/Overlay.qml`)
| Constant | Integer Value |
| :--- | :--- |
| `Overlay.none` | `0` |
| `Overlay.bpm` | `1` |
| `Overlay.key` | `2` |
| `Overlay.fx` | `3` |
| `Overlay.quantize` | `4` |
| `Overlay.slice` | `5` |
| `Overlay.sorting` | `6` |
| `Overlay.capture` | `7` |
| `Overlay.browserWarnings` | `8` |
| `Overlay.swing` | `9` |

### 2.3 `FooterPage` (`Defines/FooterPage.qml`)
| Constant | Integer Value |
| :--- | :--- |
| `FooterPage.empty` | `0` |
| `FooterPage.filter` | `1` |
| `FooterPage.pitch` | `2` |
| `FooterPage.fxSend` | `3` |
| `FooterPage.fx` | `4` |
| `FooterPage.midi` | `5` |
| `FooterPage.volume` | `6` |
| `FooterPage.slot1` .. `FooterPage.slot4` | `7` .. `10` |
| `FooterPage.stems` | `11` |
| `FooterPage.slots` | `12` |

### 2.4 `FxOverlay` (`Defines/FxOverlay.qml`)
| Constant | Integer Value |
| :--- | :--- |
| `FxOverlay.upper_button_1` .. `FxOverlay.upper_button_4` | `0` .. `3` |
| `FxOverlay.lower_button_1` .. `FxOverlay.lower_button_4` | `4` .. `7` |

### 2.5 `StemStyle` (`Defines/StemStyle.qml`)
| Constant | Integer Value |
| :--- | :--- |
| `StemStyle.track` | `0` |
| `StemStyle.daw` | `1` |

### 2.6 `ScreenView` (`Defines/ScreenView.qml`)
| Constant | Integer Value |
| :--- | :--- |
| `ScreenView.deck` | `0` |
| `ScreenView.browser` | `1` |
| `ScreenView.settings` | `2` |

### 2.7 `NavigationMode` (`Defines/NavigationMode.qml`)
| Constant | Integer Value |
| :--- | :--- |
| `NavigationMode.tree` | `0` |
| `NavigationMode.playlist` | `1` |

### 2.8 `JumpSize`, `LoopSize`, `MoveSize`
- **`JumpSize`** (`Defines/JumpSize.qml`):
  - `bwd_loop = -13, bwd_32 = -12, bwd_16 = -11, bwd_8 = -10, bwd_4 = -9, bwd_2 = -8, bwd_1 = -7, bwd_1_2 = -6, bwd_1_4 = -5, bwd_1_8 = -4, bwd_1_16 = -3, bwd_f = -2, bwd_uf = -1`
  - `fwd_uf = 1, fwd_f = 2, fwd_1_16 = 3, fwd_1_8 = 4, fwd_1_4 = 5, fwd_1_2 = 6, fwd_1 = 7, fwd_2 = 8, fwd_4 = 9, fwd_8 = 10, fwd_16 = 11, fwd_32 = 12, fwd_loop = 13`
  - `JumpSize.names`: `["UltraFine", "Fine", "1/16", "1/8", "1/4", "1/2", "1", "2", "4", "8", "16", "32", "Loop"]` (13 entries)
- **`LoopSize`** (`Defines/LoopSize.qml`): `loop_1_32 = 0 .. loop_32 = 10`, `LoopSize.names`: `["1/32", "1/16", "1/8", "1/4", "1/2", "1", "2", "4", "8", "16", "32"]` (11 entries)
- **`MoveSize`** (`Defines/MoveSize.qml`): `move_uf = 0, move_1_32 = 1 .. move_32 = 11, move_loop = 12`, `MoveSize.names`: `["UltraFine", "1/32", "1/16", "1/8", "1/4", "1/2", "1", "2", "4", "8", "16", "32", "Loop"]` (13 entries)

---

## 3. `CSI 1.0` QML Plugin C++ Type Catalog

### 3.1 Core Reactive Graph Primitives
1. **`Module`**: Top-level container for a controller or sub-module (`property bool active: true`, `property string surface: ""`).
2. **`AppProperty`**: Reactive bridge to an `app.traktor.*` property (`path`, `value`, `description`, `min`, `max`, `onValueChanged`).
3. **`MappingPropertyDescriptor`**: Defines a local controller-scoped property (`id`, `path`, `type: MappingPropertyDescriptor.Boolean | Integer | Float | String`, `value`, `min`, `max`).
4. **`Wire`**: Reactive connection linking a hardware surface endpoint (`from`) to a target property/adapter (`to`), gated by `enabled: bool`.
   - `from`: Surface control path (e.g., `"s8.left.pads.1"`, `"%surface%.browse.encoder.push"`) or adapter output.
   - `to`: Adapter (`DirectPropertyAdapter`, `TriggerPropertyAdapter`, etc.) or hardware LED/display endpoint.
   - `enabled`: Dynamic boolean expression; when `enabled` transitions to `false`, the wire disconnects cleanly without firing spurious triggers, and restores the target's active binding when `enabled` becomes `true`.
5. **`WiresGroup`**: Hierarchical container of `Wire` and nested `WiresGroup` objects with a shared `enabled: bool` gate. A child `Wire` is active if and only if its own `enabled` is `true` AND every ancestor `WiresGroup.enabled` is `true`.
6. **`Group`**: Structural grouping node for child modules/properties.
7. **`Or` / `And`**: Combinatorial boolean gates over hardware surface signals (`inputs: [ ... ]`, output `output`).

### 3.2 Property Adapters
| Adapter Type | Properties | Evaluation Semantics |
| :--- | :--- | :--- |
| `DirectPropertyAdapter` | `path`, `input` (`bool`) | Directly binds hardware control value (0..1 fader/knob or bool button) to target property path. |
| `TriggerPropertyAdapter` | `path`, `value`, `output` (`bool`) | On rising edge of button press (`value > 0`), triggers action or writes `value` to `path`; optionally mirrors state to LED when `output: true`. |
| `SetPropertyAdapter` | `path`, `value`, `output` (`bool`), `color` | On button press, sets `path` to constant `value`. When `output: true`, lights the pad with `color` when `path.value == value`. |
| `TogglePropertyAdapter` | `path`, `value` (optional) | On rising button edge, toggles boolean property (`!current`) or toggles between default and `value`. |
| `HoldPropertyAdapter` | `path`, `value`, `defaultValue` | Sets `path` to `value` while button is held; restores `defaultValue` on release. |
| `RelativePropertyAdapter` | `path`, `step`, `mode`, `wrap` | Converts rotary encoder deltas (`+1` / `-1`) or step increments into relative additions on `path`, clamped or wrapped to `[min, max]`. |
| `ExpressionAdapter` | `type`, `expression`, `to` | Transforms an input signal via a QML expression before forwarding to `to`. |
| `ButtonScriptAdapter` | `onPress`, `onRelease`, `brightness`, `color` | Imperative QML callback adapter for custom button press/release handlers with LED feedback (`brightness`, `color`). |
| `EncoderScriptAdapter` | `onIncrement`, `onDecrement`, `onPress`, `onRelease` | Imperative QML callback adapter for rotary encoders. |

### 3.3 High-Level Domain Functional Modules
Registered in `CSI 1.0` C++ and instantiated inside `Deck_S8Style.qml`, `HotcuesModule.qml`, `BrowserModule.qml`, etc.:
- **`Browser`**: `name`, `fullScreenProp`, `prepListProp`, `horzScrollSpeed`, `vertScrollSpeed`, `scrollPageSize`, `navMode`, `sortingKnobMultiplier`, `fastScrollCenterOffset`. Exposes `.scrolling`, `.tree_navigation`, `.favorites_navigation`, `.sorting`, `.view`, `.load_into_deck.{A,B,C,D}`, `.add_remove_from_prep_list`.
- **`Hotcues`**: `name`, `channel` (`1..4`), `activeCueType`. Exposes `.1.trigger`..`.8.trigger`, `.1.delete`..`.8.delete`.
- **`Beatjump`**: `name`, `channel`, `size` (`-8..+8` etc.).
- **`FreezeSlicer`**: `name`, `channel`, `surface`, `current_slice`, `slice_size`, `slice_count`, `first_slice_beats`, `last_slice_beats`, `zoom_view`, `zoom_factor`, `zoom_offset`.
- **`DeckTempo`**: `name`, `channel`, `tempoBendStepless`, `shift`.
- **`KeyControl`**: `name`, `channel`.
- **`QuantizeControl`**: `name`, `channel`.
- **`TransportSection`**: `name`, `channel`, `master` (`bool`), `sync` (`bool`).
- **`Scratch`**: `name`, `channel`.
- **`TouchstripTempoBend` / `TouchstripTrackSeek`**: `name`, `channel`, `bendRange`, `sensitivity`, `invert`, `leds`.
- **`Loop`**: `name`, `channel`, `numberOfLeds`.
- **`RemixDeck` / `RemixDeckSlots` / `RemixDeckStepSequencer`**: `name`, `channel`, `size`, `page`, `captureSource`, `selectedCell`.
- **`StemDeckStreams`**: `name`, `channel`, `stems.1..4.{volume,muted,filter_value,filter_on,fx_send,fx_send_on}`.
- **`KontrolScreen`**: `name`, `side` (`ScreenSide.Left | ScreenSide.Right`), `flavor` (`ScreenFlavor.S8 | ScreenFlavor.D2 | ScreenFlavor.S4MK3 | ScreenFlavor.X1MK3_Mode | ScreenFlavor.Z1MK2_Main`), `leftDeckIdx`, `rightDeckIdx`, `propertiesPath`.
- **`Turntable`**: `name`, `channel` (`1..4`). Exposes `.mode`, `.pitch`, `.shift`, `.timeline`, `.motor_on` for S4 MK3 Haptic Drive motorized platters (`JogwheelMode.Jogwheel | JogwheelMode.Turntable | JogwheelMode.CDJ`).
- **`LEDLevelMeter`**: `name`, `segments` (`15`), `dBThresholds` (`[-15,-10,-7,-5,-3,-2,-1,0]`), `hasClipLED` (`bool`). Exposes `.level` and `.clip`.
- **`Traktor.Gui 1.0 OffscreenView`**: Root QML item in `Resources/qml/Screens/OffscreenView.qml` (`width: 480`, `height: 272`) that renders hardware display scenes offscreen for USB bulk transmission.
- **UI Utility Nodes**: `SoftTakeoverIndicator`, `SwitchTimer` (`name`, `setTimeout`, `resetTimeout`, `onSet`, `onReset`), `ButtonSection` (`buttons`, `channels`, `layer`, `leds`), `ButtonGestures` (`press_action`, `double_press_action`, `hold_action`, `timeout`), `Blinker`, `Lighter`.
