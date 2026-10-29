---
target: crates/retraktor-gui
total_score: 31
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 1
target_identity: "file:/home/mcsbot/Documents/ReTraktor/crates/retraktor-gui"
timestamp: 2026-10-29T22-44-39Z
slug: crates-retraktor-gui
---
#### Design Health Score
| # | Heuristic | Score | Key Issue |
|---|-----------|-------|-----------|
| 1 | Visibility of System Status | 3 | Real-time waveforms, peak meters, and stem status are clear; no visual cue when background library search is active |
| 2 | Match System / Real World | 4 | Authentic DJ hardware layout: 270° rotary knobs, line faders, crossfader curves, and standard musical keys |
| 3 | User Control and Freedom | 3 | Instant layout switching (F1-F6) and full-page preferences; lacks track load undo or live deck lock |
| 4 | Consistency and Standards | 3 | Strict Traktor Pro 4 matte charcoal palette and consistent deck coloring; standard shortcuts |
| 5 | Error Prevention | 3 | Real-time audio safety prevents dropouts, but no guard prevents loading a track over an actively playing live deck |
| 6 | Recognition Rather Than Recall | 3 | Harmonic key matching highlights compatible keys; stem faders map directly to frequency stripes |
| 7 | Flexibility and Efficiency | 4 | Rich hotkeys, full-page searchable settings, 1000 Hz USB HID controller support, and multi-layout switching |
| 8 | Aesthetic and Minimalist Design | 3 | Authentic dark pro-audio aesthetic; 4-deck view is visually dense and utility rows compete with platters |
| 9 | Error Recovery | 3 | Non-blocking fallback to software clock and clear Demucs error diagnosis; no recovery toast on unreadable audio files |
| 10 | Help and Documentation | 2 | Good tooltips on controls; lacks an in-app keyboard shortcut HUD overlay (`?`) for live gig reference |
| **Total** | | **31/40** | **Solid / Professional Grade** |

#### Design Specificity Verdict
- **LLM assessment**: Highly authored and authentic to professional DJ equipment. Distinct from generic SaaS dashboards, using tactile rotary knobs, 3-band spectrum audio waveforms, and sage-lime/steel-blue status illumination.
- **Deterministic scan**: not applicable (native platform)

#### Priority Issues
- **[P1] No Deck Load Lock Protection**: Accidental track load onto a playing deck abruptly stops live music in a club setting.
- **[P2] Missing Quick Keyboard Cheat-Sheet (`?` Overlay)**: Keyboard-only laptop performers cannot quickly reference play/cue/loop/stem shortcuts mid-set without hovering.
- **[P3] Secondary Control Density in 4-Deck View**: Pattern player and utility controls compete for visual hierarchy against primary waveforms.
