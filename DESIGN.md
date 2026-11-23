---
name: ReTraktor Web
description: Hardware-companion dot-matrix specification surface for the ReTraktor Linux DJ workstation.
colors:
  bg-window: "#0a0a0a"
  bg-panel: "#141414"
  bg-panel-alt: "#0f0f0f"
  bg-section: "#1e1e1e"
  bg-input: "#060606"
  border: "#222222"
  border-accent: "#3c3c3c"
  text-primary: "#ececec"
  text-secondary: "#b0b4bc"
  text-muted: "#8a8f98"
  signal-red: "#c41e22"
  signal-red-hover: "#a8181c"
  signal-red-led: "#e83034"
  signal-red-deep: "#781218"
  deck-a: "#00a8ff"
  deck-b: "#ff7b00"
  dot-off: "#242424"
typography:
  display:
    fontFamily: "Archivo, system-ui, -apple-system, sans-serif"
    fontWeight: 800
    lineHeight: 1.03
    letterSpacing: "-0.035em"
  body:
    fontFamily: "Golos Text, system-ui, -apple-system, sans-serif"
    fontSize: "16px"
    fontWeight: 400
    lineHeight: 1.6
  mono:
    fontFamily: "JetBrains Mono, ui-monospace, monospace"
    fontSize: "12.5px"
    fontWeight: 400
    lineHeight: 1.55
rounded:
  sm: "4px"
  md: "6px"
spacing:
  sm: "8px"
  md: "16px"
  lg: "24px"
  xl: "64px"
components:
  button-primary:
    backgroundColor: "{colors.signal-red}"
    textColor: "#ffffff"
    rounded: "{rounded.sm}"
    padding: "8px 15px"
  button-primary-hover:
    backgroundColor: "{colors.signal-red-hover}"
---

## Overview

ReTraktor's public web showcase translates the desktop workstation's `Night` palette and dot-matrix instrumentation (`docs/design/COMPANION.md`) into a static specification and download surface (`site/`).

## Colors

- **Grounds & Surfaces:** Deep `#0a0a0a` window ground, `#141414` hardware chassis panels, `#0f0f0f` recessed alt panels, and `#060606` terminal/meter wells.
- **Signals & Decks:** `#c41e22` accessible filled signal red (`#ffffff` text >= 5.3:1 contrast), `#e83034` 7px LED indicators and focus rings, `#00a8ff` Deck A, `#ff7b00` Deck B.
- **Text:** `#ececec` primary ink, `#b0b4bc` secondary copy (> 7:1 contrast), `#8a8f98` muted table headers.

## Typography

- **Display (`Archivo`):** Tight industrial grotesk headings without kickers or eyebrow labels above them.
- **Body (`Golos Text`):** High-legibility technical prose capped at 58–65ch.
- **Telemetry & CLI (`JetBrains Mono`):** Used strictly for CLI commands, USB VID:PID tables, BPM/timing readouts, and shortcut keys with `tabular-nums`.

## Layout

Asymmetric 2-column first viewport pairing installation actions with an interactive 1440×900 companion screen bay, followed by an interactive 5-layout switcher, symmetric A/B Stems & Neural Mix simulator, hardware compatibility matrix, library/AI showcase, and CLI/install sections.

## Elevation & Depth

1px hairline structural borders (`#222222` and `#3c3c3c`) with a single soft drop shadow on the primary companion screen bay; zero zero-offset neon glows or hard offset shadows.

## Shapes

4px inner controls (`--radius-sm`) and 6px outer chassis panels (`--radius`), separated cleanly by 10px dot-matrix rules.

## Components

- **Screen Bay & Layout Switcher:** Interactive `role="tablist"` switchers wired to real lossless WebP screenshots and a native `<dialog>` 1440×900 inspector.
- **Interactive Stems & Neural Mix Lab:** Live JS toggle between `EQ: CLASSIC` and `EQ: STEMS (NEURAL)`, per-stem mutes, `ACAPELLA`/`INST` isolators, 5% deadzone crossfader, and dot-matrix response columns.
- **Hardware Table & CLI Switcher:** Client-side category filter over the 11 NI HID models + MIDI Learn, and copy-ready CLI examples.

## Do's and Don'ts

- **Do** lead with real application captures and verified crate/hardware specs.
- **Do not** add eyebrow/kicker labels above headings, gradient text, or generic SaaS metric cards.
- **Do not** place thick colored top/left borders on rounded panels.
