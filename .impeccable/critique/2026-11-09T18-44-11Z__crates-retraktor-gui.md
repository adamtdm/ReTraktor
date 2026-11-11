---
target: ReTraktor GUI UX review
total_score: 21
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 3
target_identity: "file:/home/mcsbot/Documents/ReTraktor/crates/retraktor-gui"
timestamp: 2026-11-09T18-44-11Z
slug: crates-retraktor-gui
---
Method: design review only (detector not applicable, native egui desktop app; evidence = headless screenshots of the real build)

#### Design Health Score
| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 2 | Empty decks show "0.00 BPM" and a fake "-02:00"; MASTER 128.00 shows with nothing playing; job progress lives in an 11px status line |
| 2 | Match System / Real World | 3 | DJ vocabulary is right; but CUP, FLX, PAT, "KNOBS: 3-BAND EQ (HI / MID / LOW) / STEMS", "NML v20" leak implementation terms |
| 3 | User Control and Freedom | 2 | Delete playlist / remove from collection have no confirm and no undo |
| 4 | Consistency and Standards | 2 | Three visual dialects: monospace pill badges, unicode-glyph "icons" (▶ ◆ ★ ●), and a separate card style in Preferences |
| 5 | Error Prevention | 2 | Lock-playing-decks is good; destructive library actions unguarded; Remove sits next to everything in the context menu |
| 6 | Recognition Rather Than Recall | 2 | Loading a track (double-click / drag / Enter) is never signposted; deck abbreviations require memory |
| 7 | Flexibility and Efficiency | 3 | Keyboard browse, multi-select, drag-to-deck/playlist, F1–F4, sortable columns |
| 8 | Aesthetic and Minimalist Design | 2 | ~20 always-on controls in the top bar, ~30 per deck; in Mix the empty decks take ~60% of the screen before a track is loaded |
| 9 | Error Recovery | 2 | "File not found — use Relocate" is good; import/tag errors only appear in the tiny status line |
| 10 | Help and Documentation | 1 | Only a "?" shortcut overlay; tooltips sparse and some still describe old behaviour |
| **Total** | | **21/40** | **Acceptable** |

#### Design Specificity Verdict
**LLM assessment**: The deck/mixer half is authored for DJs (key colours, stem lanes, 270° knobs, deck-coloured accents) and reads as a Traktor-family tool. The library half and Preferences are generic egui: default buttons, default spacing, list-row cards with mono caps section headers that could belong to any settings app. The two halves don't share a type scale or a control style, so the product feels assembled. Unicode glyphs (▶ ◆ ★ ●) stand in for an icon system throughout.
**Deterministic scan**: not applicable (native platform).

#### Overall Impression
The bones are now right (real data, fast table, sane first run), but the performance surface still shows everything at once, all the time. The single biggest opportunity is progressive disclosure on the decks: show the 6 controls a DJ touches every few seconds and fold the rest away until asked for.

#### What's Working
- Library table: virtualized, real metadata, key colours around the wheel, red titles for missing files, sortable resizable columns. Scales to 20k tracks.
- New first run: finds Traktor collections automatically and offers one-click import; native file chooser for everything else.
- Deck colour language (blue A / orange B) is applied consistently to headers, waveforms, crossfader ends and mixer assign buttons.

#### Priority Issues
- **[P1] Deck strip overload** — Why: ~30 controls per deck (SYNC MASTER STEMS, CUE STEM PAT ROLL, CUE CUP FLX, loop sizes 1–16, 8 hotcues, pitch) compete equally; nothing says which matter. Fix: primary row = play, cue, sync, tempo, loop; hotcues/pad modes/flux behind a per-deck disclosure or only in the 4-Deck/Performance layout. Command: /impeccable distill
- **[P1] Empty decks lie and dominate** — Why: 0.00 BPM, -02:00, a beat grid with no audio, MASTER 128.00; the Mix layout gives 60% of the screen to nothing on first launch. Fix: empty deck = one line "Drop a track here or double-click in the library"; hide tempo/time/grid until loaded; Browser layout as the default until a track is loaded. Command: /impeccable onboard
- **[P1] Loading a track is undiscoverable** — Why: the core action (get a track onto a deck) has no visible affordance since row buttons were removed. Fix: hint in the empty deck, a drop-target highlight already exists, add "Load to A/B" as the first context-menu items (done) plus a hover hint on rows. Command: /impeccable clarify
- **[P2] Destructive actions unguarded** — Why: Delete playlist and Remove from collection act instantly with no undo. Fix: 5-second undo toast in the status bar, or confirm for playlists > 0 tracks. Command: /impeccable harden
- **[P2] Two visual dialects** — Why: Preferences (cards, mono caps headers, "Show Column" checkboxes ×15) and the main UI share no type scale or controls; glyph "icons" render inconsistently. Fix: one type scale (11/12.5/15/22), one button style, an SVG icon set via egui images; Preferences rows as plain aligned rows not cards. Command: /impeccable typeset then /impeccable polish

#### Persona Red Flags
**Jordan (first-timer, bedroom DJ)**: Opens app → two empty decks full of controls and "0.00 BPM" → library area at the bottom third. Doesn't know double-click loads. "FLX", "CUP", "PAT" mean nothing. Top bar "FX 1 D/W REV" knobs look clickable but do nothing without a track.
**Alex (working DJ, power user)**: Keyboard browse and drag-to-deck are good. Wants: load to a specific deck from keyboard beyond A/B (no C/D), BPM-range filter, sort by key-then-BPM, "prep" crate hotkey. Misses undo after an accidental Remove.
**Sam (Traktor migrant)**: Import is now smooth. 7,560 tracks marked missing with only a red title and a toolbar button; wants a clear "x tracks missing — locate" banner in Missing view.

#### Minor Observations
- Status bar text is 11px grey on dark grey; job progress is easy to miss.
- "No controller" now honest; PipeWire label shows sample rate even when no device opened.
- Column header arrows fixed, but the leading "#" column wastes width in Collection view (order is meaningless there; useful in playlists/history).
- Track editor is a floating window; edits are fine, but key field accepts free text (validated on save).
- Preferences tab strip leaves ~45% empty to the right; search box floats far from tabs.

#### Questions to Consider
- What if the app opened in Browser layout until the first track is loaded, then switched to Mix?
- Does a bedroom DJ ever need hotcues 5–8, flux and pattern player visible at the same time as the library?
- What would the deck look like if it only showed what changes while a track plays?
