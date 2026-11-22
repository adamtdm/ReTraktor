# Controller rules

Controller rules let you set up a controller the Traktor way: hold Shift to give the pads a second job, latch
a layer, give one control several actions, or make one knob a macro that drives several targets, each over its
own range. They work on the Traktor MX2 and on generic MIDI and learned NI HID controllers.

The rules live in a single plain JSON file, so you, a script or a coding agent can write them without the app
open. The app writes this file only when you ask: **Create example file** (only while there is none) and
**Import > Add to rules file**, and `retraktor-cli apply-mapping` writes it the same way (see
[Importing a mapping](#importing-a-mapping)). The visual MX2 map and the Other
controllers table keep saving to their own files.

## Quick start

1. In ReTraktor, open **Preferences > Controllers** (either tab) and press **Create example file** under
   *Rules file*. The button only appears while there is no file, so it never replaces your rules.
   Without the app, write the example to a new draft file instead, and copy it into place only if no file is
   there yet:

   ```sh
   retraktor-cli mapping-reference --example > ~/controller-rules-draft.json
   retraktor-cli validate-mapping ~/controller-rules-draft.json
   mkdir -p ~/.config/retraktor
   cp -n ~/controller-rules-draft.json ~/.config/retraktor/controller-rules.json   # -n: never overwrite
   ```

2. Edit the file in any editor.
3. `retraktor-cli validate-mapping` checks it without any hardware. It exits 0 when the file is valid. Otherwise it
   exits 1 and lists every problem (up to 100) with its location, for example
   `rules[7] "Loop layer: right pad 1 halves the loop".do[0].deck: 9 is out of range: use a whole number from 1 to 4`.
4. Press **Reload** under *Rules file*. Rules are also loaded at startup.
5. To find a control's ID, touch it on the hardware and copy it from **Last control** on the same panel.

`retraktor-cli mapping-reference` prints the grammar, every action with its parameters, every MX2 control ID
and the example. It is built from the same catalogue the validator and the app use, so it is always current.
Generic MIDI and NI HID IDs can't be listed in advance; they come from your hardware (see Control IDs).

## File

Path: `~/.config/retraktor/controller-rules.json` (UTF-8 JSON, at most 1 MiB; larger files are refused unread).

```json
{
  "format": "retraktor-controller-rules",
  "version": 1,
  "rules": [ RULE, ... ],

  "comment": "optional",
  "midi_encoders": { "midi.xonek2.ch15.cc0": "twos_complement" },
  "midi_cc_buttons": [ "midi.mixer.ch1.cc8" ],
  "leds": [ LED, ... ]
}
```

`format`, `version` and `rules` are required. `comment`, `midi_encoders`, `midi_cc_buttons` and `leds` are optional.

| Thing | Fields |
|---|---|
| RULE | `input` (control ID, required), `do` (list of actions, required; `[]` swallows the event), `when` (optional `{"modifier": value}`), `name`, `comment` |
| ACTION | `action` (name, required), then its parameters: `deck` 1–4, `unit` 1–2 (FX unit), `slot`, `mode`, `range`, `step`, `value`, `name`, plus `comment` |
| LED | `control` (an MX2 pad), `colour` (`off`, `blue`, `green`, `red`, `amber`, `orange`, `cyan`, `magenta`, `white`), `when`, `comment` |

A rule's `name` is only a label that appears in error messages. A modifier's name is the `name` inside a
`modifier` action, and that is what `when` refers to. Any unknown field is an error, so a typo like `"dekc"` is
caught instead of being ignored.

## Precedence: which mapping handles an event

1. **Rules file.** The app goes through the rules from the top. The first rule whose `input` is the control and
   whose `when` values all match the device's current modifiers runs every action in its `do`, in order.
   Only that rule runs.
2. **Repeats and the release follow the press.** A button's release goes to whatever handled its press, even if
   Shift changed in between. A second press before the release (MIDI controllers can send note-on twice) goes to
   the same place, and a rule does not run its actions again. For example, press a pad, then hold Shift, then
   let go of the pad: the release still goes to the plain mapping, so nothing gets stuck on.
3. **Fallback.** If no rule matches, the event goes to the simple mappings (the visual MX2 map, the Other
   controllers table) and the built-in controls, just as without a rules file. A control with only a Shift rule
   keeps its normal job when Shift is up.

Put the more specific rule first. A rule that can never run is reported as an error, for example a Shift rule
placed after a rule for the same control that has no `when`.

## Modifiers (layers)

```json
{ "action": "modifier", "name": "shift", "mode": "hold" }
```

- `name`: 1–32 characters from `a`–`z`, `0`–`9` and `_`.
- `value`: 1–7 (default 1). `set` may also use 0 to turn a layer off.
- `hold` sets the value while the button is down and 0 when it is released (Shift). `toggle` sets the value,
  or 0 if it already has that value (a latched layer). `set` sets the value (a layer selector).
- Several modifier actions in one `do` list run in order, and each one sees the result of the one before.
- Modifiers belong to one device: the MX2, each MIDI port, each NI HID model. A Shift on the MX2 never shifts
  your MIDI controller. A `when` can only name modifiers that some rule on the same device sets. A misspelled
  `"shfit"` is an error that suggests `"shift"`.
- Every modifier starts at 0. All modifiers go back to 0 when you Reload a valid file. A device's modifiers go
  back to 0 when it is unplugged, or when an MX2 connects again. MIDI and NI HID unplugs are noticed within
  about 2 seconds.
- A button you are holding while you Reload: if a rule took its press, its release does nothing. If the
  simple mappings took it, they still get its release.
- If two buttons hold the same modifier, releasing either one sets it to 0.
- Preferences > Controllers shows the active layers, for example `mx2 shift=1`.

## Actions and values

Run `retraktor-cli mapping-reference` for the full list. Each action needs the right kind of input:

- **Button actions** run once per press, for example:
  - `play`;
  - `cue`: jumps to hotcue 1, and if that slot is empty, stores the playhead there;
  - `hotcue` (`slot` 1–8): the same, for that slot;
  - `stem` (`slot` 1–4: drums, bass, other, vocals);
  - `loop_half`;
  - `pad_mode` (`mode`: `hotcues`, `stems`, `pattern`, `loop`);
  - `tempo_master`: makes the deck the tempo master. Unlike the deck's MASTER button, a second press keeps it master;
  - `layout` (`mode`): the four deck layouts the toolbar's Waveforms and Deck count choices make. `regular2` (2 Decks,
    Waveforms In Deck) and `regular4` (4 Decks, In Deck) give each deck its own waveform, with decks A and B, or A and
    B over C and D, either side of the mixer; `stacked2` (2 Decks, Stacked) and `stacked4` (4 Decks, Stacked) stack two
    or four waveforms full width, one lane per deck even while it is empty, above those decks.
    With four decks the mixer has four channels. These are ReTraktor's own panels, not Traktor's layouts redrawn;
  - `browser_only`: shows the Browser layout, and a second press goes back to the layout before.

  They need a button: an MX2 button, a MIDI note, a CC listed in `midi_cc_buttons` or an NI HID bit.
- **Value actions** use values from 0 to 1, for example:
  - `filter`: 0.5 is off, lower is low-pass, higher is high-pass. You hear it while the channel filter is on
    (`filter_on`, or the MX2 channel's FX ON button).
  - `volume`, and `eq_low` / `eq_mid` / `eq_hi` (0.5 is neutral). These are always heard.
  - `pitch`: 0 is −8 %, 0.5 is 0 %, 1 is +8 %.
  - `xfader`: 0 is left.
  - `fx_mix`, which only moves the on-screen FX panel (see Limits).

  How the value is set depends on the input:
  - On a knob or fader: `"range": [from, to]`, default `[0, 1]`. The knob's 0–1 position maps straight onto it,
    and `[1, 0]` inverts. Example: `"range": [0.5, 1.0]` on `filter` sweeps only the high-pass half.
  - On an encoder: `"step"` per tick, default 0.02, so 50 ticks cover the full range. A negative step inverts.
    The result stays within 0 to 1.
  - On a button: `"value"` is what a press jumps to, for example `{"action": "filter", "deck": 1, "value": 0.5}`
    resets the filter.
- **Encoder actions**: `browse` (turn right for the next track), `loop_size` (turn right to double, left to
  halve) and `waveform_zoom` (turn right to zoom the deck's scrolling waveform in; 4 ticks double or halve the
  seconds shown, from 1/4 to 4 times the normal width; beats, cues and loops stay aligned, and dragging the
  waveform scrubs at the zoomed scale). They need an encoder: an MX2 encoder, or a MIDI CC listed in
  `midi_encoders`.

## MIDI CCs: knob, fader or encoder

A MIDI CC can be an absolute knob or fader, or a relative encoder, and the bytes alone don't say which. So the
file decides:

- A CC is a **knob or fader** by default. The app reads value / 127 from the first message, so a fader
  starting at 0 or crossing the middle is never mistaken for an encoder. Use `range`.
- A CC listed in **`midi_encoders`** is a **relative encoder**, read as ticks from its first message. Use
  `step`, `browse` or `loop_size`. Pick the encoding from what one notch right / left sends:

  | encoding | right | left |
  |---|---|---|
  | `offset_64` | 65 | 63 |
  | `twos_complement` | 1 | 127 |
  | `sign_bit` | 1 | 65 |

  The controller's MIDI chart says which, or watch the bytes with `amidi -d -p hw:1,0`. A CC message is
  `B<channel-1> <cc> <value>` in hex.
- A CC listed in **`midi_cc_buttons`** is a **button**, for controllers whose buttons send a CC (127 on press,
  0 on release): any value above 0 presses it and 0 releases it. A CC can't be in both lists.
- A CC used or listed in the file is read the same way by the simple mappings. Any other CC keeps the app's old
  automatic guess, so existing Other-controllers mappings behave as before.
- 14-bit faders (a coarse CC 0–31 plus a fine CC 32–63) still combine into one fine value on the coarse CC's ID.

## Control IDs

- **Traktor MX2:** `surface.left.pads.1`, `surface.mixer.channels.1.volume`, `surface.right.loop_size` and so
  on. `mapping-reference` lists all of them with their kind.
- **Generic MIDI:** `midi.<port>.ch<1-16>.note<0-127>` is a button. `.cc<0-127>` is a knob, fader or encoder (see
  above). `.bend` is a fader.
  - `<port>` is the ALSA card id in lowercase. `cat /proc/asound/cards` shows `1 [XONEK2 ]`, so an Allen & Heath
    Xone:K2 is `midi.xonek2`.
  - A card's second and later MIDI ports add `_d<N>`, so `hw:1,1` is `midi.xonek2_d1`.
  - The ID stays the same when the card number changes, for example when you plug devices in a different order.
  - If two cards would get the same ID, or a card has no id, the port keeps its raw-node ID, such as
    `midi.midic1d0`.
- **Older raw-node IDs:** IDs learned or written before this version use the raw node (`midi.midic1d0…`). They
  still work as an alias while that node is the device, but only when nothing uses the stable ID for the same
  control, so one press never runs two mappings. Learn finds an existing raw-node entry instead of adding a
  duplicate. New IDs from Last control and Learn are the stable ones.
- **Generic NI HID:** `hid.<pid>.r<report>.b<byte>.<bit>` is a button and `hid.<pid>.r<report>.a<byte>` is a
  knob or fader.
- Numbers are written without leading zeros (`ch1`, `note3`). `ch01` or `note003` are errors, because the app
  never sends them.

Don't guess note or CC numbers. Touch the control and copy **Last control** in Preferences > Controllers, or
use Learn on the Other controllers tab.

## Pad lights (MX2)

`leds` is optional. Its entries colour the 16 MX2 pads while their `when` matches. For each pad, the first
matching entry wins. A pad with no matching entry keeps the app's normal colour.

The app writes these colours to the pad LED bytes of the MX2's own LED report. That needs write access to the
MX2 (see the udev rules in the README), and only a device whose USB IDs are the MX2's (`17cc:2420`) is treated
as an MX2. Other NI controllers stay read-only, and nothing is ever sent to MIDI or other HID devices.

## Errors and Reload

- A file with any problem is not loaded. Preferences shows the problems, and the rules loaded before stay
  active. Fix the file and press Reload.
- A missing file means no rules, so every control uses the simple mappings.
- Reload never changes the file. There is no auto-save, so edits from another tool are safe.

## Example

[`crates/retraktor-hid/src/controller_rules_example.json`](../crates/retraktor-hid/src/controller_rules_example.json)
(the same text as `retraktor-cli mapping-reference --example`) is a complete MX2 setup:

- Holding left **SHIFT** turns left pads 1–4 into stem mutes. The pads light up in stem colours while Shift is
  held. Without Shift, the pads keep their normal job.
- **Left FX knob 4** is a macro. Turning it up raises deck A's high-pass filter from off (0.5) towards 1.0
  (heard while channel 1's FX ON is lit), and cuts deck A's bass EQ the other way, from 0.5 down to 0.1
  (always heard).
- **Right FAV** latches a loop layer. While it is on, right pads 1–3 halve or double deck B's loop, or turn its
  filter on and start the loop.
- The **left Loop Move** encoder nudges deck A's filter by 0.02 a tick.

The same ideas on a Xone:K2. These IDs and the encoding are examples; copy yours from Last control and your
controller's MIDI chart.

```json
{
  "format": "retraktor-controller-rules",
  "version": 1,
  "midi_encoders": { "midi.xonek2.ch15.cc0": "twos_complement" },
  "rules": [
    { "name": "K2 layer button", "input": "midi.xonek2.ch15.note12",
      "do": [{ "action": "modifier", "name": "shift", "mode": "hold" }] },
    { "input": "midi.xonek2.ch15.note36", "when": { "shift": 1 }, "do": [{ "action": "hotcue", "deck": 1, "slot": 5 }] },
    { "input": "midi.xonek2.ch15.cc4", "do": [{ "action": "filter", "deck": 1, "range": [0.5, 1] }, { "action": "eq_low", "deck": 1, "range": [0.5, 0.1] }] },
    { "input": "midi.xonek2.ch15.cc0", "do": [{ "action": "browse" }] }
  ]
}
```

## Limits in this version

- The two FX units are on-screen only for now. `fx_on` and `fx_mix` move the FX panel's controls, but the units
  are not in the audio path yet, so you won't hear them. The channel filter and EQ are audible.
- Actions run on press. There are no actions that run on release, apart from a `hold` modifier.
- `pad_mode` switches what the pads do, but not the MX2's pad mode button lights.
- NI HID devices that aren't in the device list get the product ID `2200` in their IDs.
- Rules on `jogwheel.rotation` replace scratching on that wheel.
- This is not Traktor's `.tsi` format (but see [Importing a mapping](#importing-a-mapping)). There is no
  scripting (JS/QML), no LED or other output to generic MIDI/HID controllers, and no in-app text editor.

## Importing a mapping

Traktor `.tsi` files (Generic MIDI devices), Mixxx `.midi.xml` mappings and rekordbox `.midi.csv` mappings can be
converted into rules. The converter reads the file only: it runs nothing from it, and it never changes the
installed application, its mappings or your rules file until you add the result.

In the app: **Preferences > Controllers > Import a mapping**. Choose the file, the device (a Traktor settings file
can hold several) and the MIDI port, the `<port>` of `midi.<port>.…` (connected ports are offered; otherwise touch
a control and read **Last control**). **Preview** shows the coverage report. Then:

- **Save draft…** writes the draft to a new file. An existing file is never overwritten.
- **Add to rules file** puts the imported rules after your own and reloads. It refuses, and writes nothing, if
  a control already has rules, a modifier name is already used on the same device, or a CC is already read
  another way (including a CC that is a knob only because a rule uses it). It also refuses if the file changed
  since the preview. The new file is swapped in atomically. Whatever it replaced is kept, byte for byte, as
  `controller-rules.json.before-import-<time>`. If another program saved in that instant, its version is put back
  and the import isn't applied. The file is rewritten in a standard JSON layout.

Without the app:

```sh
retraktor-cli import-mapping "Traktor Settings.tsi" --list
retraktor-cli import-mapping "Traktor Settings.tsi" --device 2 --port xonek2 --out k2-draft.json --report k2-report.txt
retraktor-cli validate-mapping k2-draft.json
```

`--out` and `--report` refuse existing files, and refuse to name the same file. Without `--out` it only previews, and `--out -` prints the draft to
stdout and the report to stderr. The exit status is 0 when a draft was made, 2 when nothing could be converted
(the report says why), and 1 on errors.

To add drafts to the rules file without the app, for example two complementary maps for one controller:

```sh
retraktor-cli apply-mapping --dry-run k2-decks.json k2-browser.json   # check and count, write nothing
retraktor-cli apply-mapping k2-decks.json k2-browser.json
```

This works like **Add to rules file**, on `~/.config/retraktor/controller-rules.json`. The drafts' rules and pad
lights go after the file's own, in the order given. It is all or nothing: if any draft can't be read or has
problems, collides with the file or with an earlier draft (the same refusals as above, plus a pad that already
has a colour), or the file changes on disk meanwhile, nothing is written. Applying the same draft twice is
refused as a collision; there is no option to force it. `--json` prints one object with `outcome` (`applied`,
`preview`, `refused` or `usage`), `dry_run`, `path`, `drafts`, `counts` (`rules_before`, `leds_before`,
`rules_added`, `leds_added`, `rules_after`, `leds_after`, or `null` when refused), `backup` (or `null`) and
`diagnostics` (`{draft, message}`, `draft` `null` for the rules file). The exit status is 0 when applied (or,
with `--dry-run`, when it would apply), 1 when refused or failed, and 2 for bad usage.

The change is on disk only: a running app keeps its current rules until you press **Reload** in
Preferences > Controllers, and loads the file at its next startup.

The report lists every source row as **translated**, **approximated** (imported, with what differs), **merged**
(covered by another row's rule, such as a note-off, the fine half of a 14-bit control or a hardware Shift) or
**unsupported** (with why), plus what the rules depend on. Its summary counts the source's assignments and the
report entries separately. A rekordbox row with two deck columns is one assignment with two entries. A note
about a whole device, such as a native controller, is an entry with no assignment. A row with an impossible MIDI
address (a note or CC above 127) is reported on its own. A control the validator rejects is left out with the
reason, and the rest of the draft is kept. A draft is only made when at least one rule results, and it is checked
with the same validator as the rules file.

What converts, and the limits:

- **Traktor:** Generic MIDI notes and CCs, buttons, knobs, faders and relative encoders (7Fh/01h encoder mode), and
  modifier conditions. Traktor runs every row whose conditions match the modifiers *before* the event, so the rows of
  one control become one rule per combination of the modifiers they test (at most 64), with all matching rows'
  actions. Commands: play, sync, keylock, monitor cue, mixer FX on and adjust (the channel filter), load, tempo
  master, volume, gain, EQ high, mid and low, crossfader, main and monitor volume, monitor mix, modifiers 1–8,
  layout selection (only layouts named Regular2, Regular4, Stacked2 or Stacked4 in the same file, shown as 2 or 4
  Decks with In Deck or Stacked waveforms), only-browser
  and waveform zoom (approximated: ReTraktor zooms in fixed steps). The imported device must set every modifier
  value its rows test. A test for "= 0" on a modifier nothing in the mapping assigns always holds and is dropped,
  and a test for any other value on it never runs. A row that tests a value only an unimported row sets isn't
  imported. Not imported: LED outputs, buttons with Auto Repeat or Invert, an inverted zoom
  encoder, the four-band EQ's Mid Low band, CUE (ReTraktor's `cue` is not Traktor's cue), the focused-deck
  target, remix slots, and native NI
  controllers such as the X1 MK3, whose behaviour comes from Traktor's own controller scripts (CSI QML), which
  ReTraktor can't run and which the `.tsi` doesn't contain. Custom QML behaviour needs a separate port. Only
  their buttons can be mapped by hand today (Last control shows a pressed button's `hid.…` ID); generic HID can
  report unrelated bit changes for knobs, and there is no verified decoder for their encoders.
- **Mixxx:** declarative rows for decks 1–4 (play, sync, keylock, headphone cue, load, loop halve and double,
  hot cues, volume, pregain, EQ, quick effect, crossfader, headphone mix, library scrolling), with `normal`,
  `invert`, `button` and matching 14-bit pairs. Library scrolling needs `selectknob` (two's complement) or
  `spread64` (offset 64), not inverted. ReTraktor moves one track per message, so it is approximated. `rot64`,
  which turns steps into an accumulated position, isn't imported. Script rows aren't run. They are
  only translated for the Allen & Heath Xone K1/K2/K3 mapping shipped with Mixxx, when its script and the
  components library sit unmodified next to the mapping, and then only the deck columns' faders and high and mid
  knobs, whose job never changes with the script's Shift or layers. Outputs aren't imported.
- **rekordbox:** buttons, pads, knobs, faders and rotary encoders, per deck (a base address plus a channel offset
  per deck, or an address per deck). Shifted controls send their own messages and are imported as their own rows.
  Rows that delete cues are never imported. Rows marked "No function assigned here", indicators, jog wheels and
  options that change how rekordbox reads a control are skipped. LED outputs aren't sent.
