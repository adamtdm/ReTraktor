# UI / UX review, November 2026

Claude and ChatGPT (GPT-6.1) reviewed every screen from real-app screenshots at 1440×900. Each round, ChatGPT checked the claimed fixes against the code diff, argued with the pushbacks and re-scored the app. Claude then fixed what it found. The bar: 8 = "happily use it in a club", 9 = "as good as the best commercial app in this area".

| Area | Round 1 | 2 | 3 | 4 | 5 |
|---|---|---|---|---|---|
| Library | 5 | 7 | 8 | 8 | 8 |
| Deck | 4 | 6 | 8 | 8 | 8 |
| Mixer | 3 | 5 | 7 | 7 | 8 |
| Stems | 5 | 7 | 8 | 8 | 8 |
| Layouts | 4 | 6 | 8 | 8 | 8 |
| Set / AI | 4 | 6 | 7 | 8 | 8 |
| Preferences | 3 | 6 | 8 | 8 | 8 |
| Controller mapping | 5 | 6 | 8 | 8 | 8 |
| Visual design | 4 | 6 | 7 | 7 | 8 |
| Feedback | 3 | 5 | 7 | 8 | 8 |
| Keyboard | 5 | 6 | 8 | 8 | 8 |

## What changed

- **Safety**
  - Set, Next-up and Shift+N loads respect deck protection.
  - Help, the next-track picker and Preferences now block every keyboard handler underneath them. All handlers run through `dispatch_keys()`.
  - Escape closes only the topmost layer.
- **Honesty**
  - Audio status shows the real output device and sample rate.
  - Settings the engine doesn't apply yet say so.
- **Deck**
  - Hotcue pads are always visible.
  - Play/CUE buttons are 30 px tall.
  - The loop label reads LOOP.
  - MASTER is a real exclusive state, and SYNC follows it.
  - Waveforms show bar numbers and the length of time on screen.
  - LOADING, LOAD FAILED, LOCK and LOCKED appear on the deck itself.
- **Mixer**
  - Labels and handles are drawn above the dot grid, at 11 px.
  - Each fader has a rail with a unity mark and a lettered handle.
  - Headphone CUE buttons are 30×24.
  - The MAIN meter has a 0 / −6 / −12 scale, and CLIP shows on overload.
  - Every channel reads LOW → HI; deck B is no longer mirrored.
  - Four-deck mode has letter labels that fit.
- **Stems**
  - The deck says whether it is playing saved Demucs stems or the live frequency split (LIVE SPLIT · BANDS).
  - Each stem has its own FX toggle.
  - Muted stems are labelled OFF.
- **Library**
  - The toolbar has a fixed action row: Load A/B/C/D, Prepare (analyse, stems, set, edit), and the double-click rules.
  - Search placeholder text in plain words, plus a Filters menu.
  - A new Export menu.
  - Neutral selection colour in the sidebar.
- **Set**
  - Clear asks before emptying the set.
  - Warnings appear in amber words.
  - The selected transition is spelled out.
  - The energy and BPM arc has axes, track numbers and the selected title.
- **Layouts**
  - Set and Timeline keep a compact transport bar.
  - In four-deck mode, the rows line up.
  - The first load switches to Mix only once it succeeds, and says how to return to the Browser.
- **Preferences and mapper**
  - Settings sit in a centred column.
  - The search box is labelled "Find a section…", which is what it does.
  - The MX2 editor sits beside the drawing.
  - Reset asks first.
  - A failed save is shown on screen with Retry.

## Disagreements and how they ended

| Point | Raised by | Outcome |
|---|---|---|
| Browser switches to Mix on the first load | ChatGPT: remove it | Kept as the owner's decision; it now switches only after a successful load and explains itself |
| Mirrored EQ on deck B | ChatGPT: use the same order on every channel | Claude conceded; every channel now reads LOW → HI |
| MASTER button | ChatGPT: rename it to SET CLOCK | Kept the MASTER name; made it a real exclusive state that drives SYNC |
| Red used for selection and for deck B | ChatGPT: neutral selection | Sidebar selection is now neutral; the top mode tabs stay red (accepted) |
| MAIN scale in four-channel mode | Claude: no room | Accepted by ChatGPT; the scale is two-channel only |
| Full settings search | Claude: relabel instead | Accepted |

## Still open

- Nothing scores 9 yet.
- Not checked: live audio, hardware, AI output quality, and window sizes other than 1440×900.
- A test that plays real audio through Set/Next with both decks protected needs an audio device, so it hasn't been written. The protection itself is covered by the shared `load_to_deck` test.
