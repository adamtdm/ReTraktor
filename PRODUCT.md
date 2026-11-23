# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

## Stack

Static HTML/CSS/JS in `site/` deployed directly to Vercel (zero-build static bundle with direct `.deb` download, `./install.sh`, and GitHub repo links).

## Users

Linux DJs — especially bedroom and club DJs running Native Instruments Traktor controllers (led by the Traktor MX2, S4 MK3, S2/S3, X1/Z1/F1/D2/S8/S5) or class-compliant MIDI hardware (including Pioneer DDJ-FLX4) — who want a native Linux DJ workstation without Wine, dual-booting, or subscriptions.

## Product Purpose

Explain and demonstrate ReTraktor, an architecture-authentic, clean-room native Linux desktop DJ workstation written in 100% safe Rust, and get visitors from evaluation to installation (`retraktor_latest_amd64.deb`, `./install.sh`, or building from GitHub source).

## Positioning

ReTraktor is the only native Linux DJ workstation written in pure safe Rust with direct PipeWire/ALSA real-time audio, plug-and-play Native Instruments NHL2 USB HID hardware control (LEDs, screens, haptic platters), 4-stem separation (instant Linkwitz-Riley DSP split + offline Meta Demucs `htdemucs` cache + Neural Mix EQ swap), a hardware-companion dot-matrix UI, a Set Builder with energy/key/tempo arcs, a DJ.Studio-style Transition Timeline, and bring-your-own-key AI set generation — with no subscription and no Wine.

## Operating Context

Visitors inspect ReTraktor on desktop or mobile before installing on x86_64 Linux (Ubuntu, Pop!_OS, Debian, Linux Mint, Fedora, Arch). They evaluate real UI screenshots across Mix, 4-Deck, Browser, Timeline, Set, and Preferences/MX2 Mapper views, verify hardware/audio compatibility, copy install commands, or download the `.deb` package.

## Capabilities and Constraints

Confirmed capabilities from the ReTraktor workspace (`crates/*`, `README.md`, `docs/`):
- **Audio & DSP (`retraktor-audio`, `retraktor-dsp`)**: Lock-free 4-deck engine, direct PipeWire (0.3.53+) with RealtimeKit (`rtkit`) scheduling and ALSA fallback, per-card stereo pair output routing + split mono cue, 24 dB/oct 3/4-band EQ models (Classic, Nuo, Xone92, Classic 3-Band), Neural Mix (EQ ⇄ Stems), Key Lock (WSOLA time-stretch), 9 Mixer FX (Filter, Reverb, Dual Delay, Noise Sweep, Time Gater, Flanger, Barber Pole Phaser, Dotted Eighth Delay, Bit Crush), 40+ Deck FX rack, Ozone Maximizer master limiter (`BOOST`/`LIMIT`), Auto-Gain loudness matching (`ebur128`), Pattern Player, and Alpha-Beta turntable scratch physics with 4-point Hermite cubic interpolation.
- **4-Stem Engine (`retraktor-dsp`, `demucs.rs`)**: Zero-latency 4-band DSP split (`LIVE SPLIT · BANDS`) on every track, 1-click `ACAPELLA` & `INST` isolators, plus background `htdemucs` neural stem separation cached as FLACs (`drums`, `bass`, `other`, `vocals`).
- **5 Workspace Layouts (`F1`–`F5` / top tabs)**: Mix (`F1` parallel decks + 2-ch dot-matrix mixer), 4-Deck (`F2` A/B/C/D + 4-ch mixer), Browser (`F3` maximized library + mini decks + headphone preview + 256 KB Traktor stripe overviews), Timeline (`F4` 8/16/32-bar transition studio with Smooth Crossfade, Instant Bass-Swap, Vocal Stem Bridge presets & live audition), Set (`F5` Build → Perform → Record setlist with harmonic/tempo/energy arc & Next Up strip).
- **Hardware & Mapping**: First-class Traktor MX2 (`17CC:2420`) NHL2 HID driver (83 buttons, 6 encoders, 26 12-bit pots/faders, 2 capacitive 1024-tick jogwheels, 105 top-panel LEDs, 12 RGB bottom-shell ambient LEDs, 4-out/2-in 24-bit/96kHz audio), 15 NI HID devices (incl. S4 MK3 Haptic Drive™ FFB & Kontrol color screens), interactive 2D MX2 visual controller remapper, class-compliant MIDI learn mapper + Pioneer DDJ-FLX4 preset, and Linux `udev` rules.
- **Library, Formats & AI**: Native Rust `symphonia` decoder (MP3, FLAC, WAV, OGG, M4A, AAC, AIFF), background BPM/beatgrid/Open Key/1–10 energy/phrase/8-auto-cue analyzer, Traktor `.nml` (v19/v20) import & relocate, `.tsi` (DIOM TLV) import, rekordbox XML export (`ReTraktor-rekordbox.xml`), CSV/text playlist import with missing-track list, Suggestions harmonic/tempo ranker, BYOK AI assistant (Gemini, OpenAI, Anthropic, or local Ollama/OpenAI-compatible — metadata only, never audio), and post-maximizer timestamped 16-bit WAV + `.cue` mix recorder (`● REC`, RF64 >4 GiB).
- **House UI & Themes**: Companion dot-matrix interface in Night, Field (light), and Charcoal themes; 4 waveform styles (Classic, Dots, Bars, Line); 3 track overview looks (Traktor, Native, rekordbox); full keyboard navigation (`?` shortcuts HUD).
- **CLI (`retraktor-cli`)**: `doctor`, `inspect-nml`, `inspect-tsi`, `export-rekordbox`, `analyze`, `tui`, `simulate-mix`.

Constraints:
- Do not fabricate commercial claims, fake benchmarks, or testimonials. Only cite measured numbers present in the repo (`~103x real-time speed`, `< 1% CPU usage` at 48 kHz / 256 frames, `15` NI devices, `83` MX2 buttons, `9` Mixer FX, etc.).
- Real-time neural stems are offline/cached Demucs + instant DSP split (not live neural separation on CPU). Streaming services (Spotify/TIDAL/Apple Music) are metadata/tracklist import only, not direct audio streaming.
- `.deb` download is x86_64 Linux (`retraktor_latest_amd64.deb`).

## Brand Commitments

- **Name**: ReTraktor
- **Visual identity**: Hardware-companion dot-matrix aesthetic from `docs/design/COMPANION.md` (Night dark studio ground `#0a0a0a`, crisp off-white `#ececec`, signal red `#e83034`, dot-matrix numerals/glyphs, symmetric A/B deck accents `#00a8ff` / `#ff7b00` from `assets/retraktor.svg` and Deck A/B tokens).
- **Assets on hand**: `assets/retraktor.svg`, `assets/retraktor.png`, `scripts/screenshot.sh` (live app screenshot generator), `scripts/page-install.sh`, `dist/retraktor_*_amd64.deb`, `README.md`, `docs/`.

## Evidence on Hand

- Live Rust workspace (`crates/*`), real headless GUI screenshots captured via `scripts/screenshot.sh`, real `.deb` installer binary (`retraktor_latest_amd64.deb`), real install script (`install.sh` / `scripts/page-install.sh`), and real GitHub repository (`https://github.com/adamtdm/ReTraktor`).

## Product Principles

1. **Show the real workstation**: Real screenshots of the running Linux app lead the story rather than generic stock DJ photography.
2. **Hardware-first clarity**: Present software features through the lens of a working DJ booth — hands on the MX2/MIDI controller, eyes on the companion screen.
3. **Honest Linux engineering**: State exact compatibility, pipewire/ALSA routing, offline Demucs vs live DSP split, and x86_64 `.deb` / source install paths cleanly.
4. **Zero-friction installation**: A visitor should be able to download the `.deb`, copy the `curl` or `git clone` command, or inspect `retraktor doctor` within one click.
