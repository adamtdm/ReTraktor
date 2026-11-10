# ReTraktor design brief: a companion display for the hardware

One user: a bedroom DJ with a Traktor MX2 controller. The hands are on the hardware; the
screen is the display the hardware does not have. Think Teenage Engineering OP-1 / EP-133
screens and the Nothing dot-matrix glyphs: playful, legible at arm's length, alive with the music.

## Principles
1. **Show, don't mirror.** A control on screen is first a readout of the hardware's state.
   Mouse use is secondary (dragging may still work), but **every action must have a keyboard
   shortcut**, listed in the shortcuts HUD (`?`).
2. **Dot matrix is the house style** for anything live: levels, EQ, stems, tempo, phase, time,
   crossfader. Dots are circles on a regular grid; unlit dots stay faintly visible
   (`CONTROL_BG`-like) so the grid always reads. Numbers use `theme::dot_font`.
3. **Library is the exception.** Max readability and fast navigation: plain text table,
   dot matrix only as small accents (e.g. energy, a status lamp).
4. **Symmetry.** Deck A on the left, B mirrored on the right, shared things in the centre.
5. **Motion with meaning**, in the OP-1/EP spirit: things move because the music or the
   hardware moved. Beat-driven sweeps, level-driven glow, staged dot wipes when state
   changes (track load, loop on, stem toggled, layout switch). Every animation goes through
   `theme::animations()` / `theme::fade` so the Look → Animations switch turns it off
   (off = final state immediately). Never block input on an animation.
6. **Tokens only.** No `Color32::from_rgb` literals in UI code: use `TraktorTheme::*()`,
   `theme::deck_color`, `theme::mix`, `theme::on`. Must look right in all three presets
   (Night, Field (light), Charcoal). Existing contrast tests must keep passing.
7. Nothing clickable smaller than 24 px (`HIT`). Nothing may overflow at 1024x640.

## Target for the Mix view (see `mixer-v2.png`, source `mixer-v2.html`)
- **Top bar:** left = deck A strip, right = deck B strip (mirrored), centre = master tempo in
  dot font. A strip = three dot rings with the value in the middle: GAIN, FILTER, FX
  (amount; name of effect as label; lamp for on/off). Layout tabs, REC, MAIN level, help and
  settings stay but compact. The old FX 1 / FX 2 units fold into the strips (FX1 = deck A,
  FX2 = deck B). PATTERN keeps its button. No A/B badges or CUE buttons in the mixer.
- **Mixer panel** (between the decks), symmetric:
  - Per deck a **dot-matrix display** (dark inset). EQ mode: three band columns/blocks
    LOW/MID/HI (B mirrored so LOW is on the outer edge for both) whose lit height is the
    EQ knob value and whose brightness/flicker follows that band's live level; the filter
    sweeps across it (dims the dots it cuts). A killed band shows as hollow dots.
    STEMS mode (deck has stems on): four blocks DRUMS / BASS / OTHER / VOCALS, each
    animated from that stem's live level; stems are **on/off** (click or key toggles mute),
    a muted stem collapses to a dim outline with a wipe animation.
  - Per deck one **combined fader + meter**: a two-column dot meter (L/R) showing live
    level; the channel fader is a bracket riding on that column; dots above the bracket
    shrink. dB readout below in dot font. A small headphone lamp shows cue on/off.
  - Centre: MAIN meter in the same dot language.
  - Bottom: **dot-ribbon crossfader**: 4 rows of dots across the panel; deck A colour left
    of the handle, deck B colour right; each side's lit height follows that deck's live
    level; the handle is a bright 2-dot-wide block. Drag still works; double-click centres.
- **Deck display** (the readout strip on each deck): a wide dot-matrix panel replacing the
  small beat lamp: four-beat bar counter that sweeps with the beat (uses the track's
  `grid_offset_sec`), a phase meter showing how far this deck's beat is from the other
  deck's (centre = locked), BPM / pitch / key / remaining in dot font, and a dot wipe when a
  track loads or a loop engages.

## How to check your work
`scripts/screenshot.sh <out-dir> "<steps>"` renders the real app headlessly (xvfb) to PNGs:
steps like `Mix,FourDeck,Browser,Timeline,prefs:Look,loadA,expandA,wait`. Use
`SHOT_SIZE=1024x640` for the minimum window and `SHOT_HOME=<dir>` for a sandbox home.
Look at the PNGs yourself and fix what is ugly, clipped or overlapping before you finish.
`cargo test --workspace` must pass with no new warnings.

## Rules
- Follow `CLAUDE.md` (smallest complete change; one small test for non-trivial logic).
- Do not copy code from GPL projects (Mixxx etc.). Never write under `/DATA`.
- Commit on your branch with a clear message ending `Co-Authored-By: Gemini <noreply@google.com>`.
  Do not push, do not merge.
