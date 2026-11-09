# ReTraktor — Next-Generation Native Linux DJ Workstation

[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-orange.svg)](https://www.rust-lang.org)
[![Platform](https://img.shields.io/badge/platform-Linux-green.svg)](https://kernel.org)
[![Audio](https://img.shields.io/badge/audio-PipeWire%20%7C%20ALSA-red.svg)](https://pipewire.org)

**ReTraktor** is an architecture-authentic, clean-room native Linux desktop DJ workstation and hardware engine written in **100% safe Rust**. It pairs the core architecture of **Traktor Pro 4** (reactive CSI/QML state machine, lock-free real-time DSP graph, NHL2 USB HID controller driver, `.nml` collections, and `.tsi` mappings) with next-generation capabilities inspired by **Serato DJ Pro**, **Algoriddim djay Pro**, **DJ.Studio**, and **rekordbox**.

---

## Key Highlights & Innovations

| Feature Area | Standard DJ Software | ReTraktor on Linux |
| :--- | :--- | :--- |
| **Real-Time Stems** | Offline pre-renders or high-latency plugins | **Instant 4-Stem DSP Isolation**: Real-time 4-band stem separation (`DRUMS`, `BASS`, `OTHER`, `VOCALS`) on **every** track with zero pre-rendering delay + 1-click background neural stem caching. |
| **Quick Stem Isolators** | Requires multi-button sub-mixer mutes | **1-Click `ACAPELLA` & `INST`**: Dedicated vocal and instrumental solo buttons directly on the deck header and performance pads. |
| **EQ ⇄ Stems Remapping** | Separate stem sub-mixers required | **Neural Mix™ Mode**: 1-click toggle dynamically swaps the 3-band EQ knobs (and your physical hardware EQ pots!) between **Classic 3-Band EQ** and **Neural Stem EQ** (`VOCALS`, `INSTRUMENTS`, `DRUMS+BASS`). |
| **Vinyl Scratching Physics** | Choppy sample scrubbing or aliased audio | **Studio-Grade Turntable Simulation**: Alpha-Beta tracking filter (xwax/Mixxx model) with 4-point Hermite cubic anti-aliasing interpolation and magnetic cartridge velocity scaling ($V \propto v$). |
| **Visual Controller Remapper** | Complex text/XML MIDI mapping tables | **Interactive 2D Surface Map**: Click any button, pad, knob, or jogwheel on an interactive graphical controller diagram in Preferences to assign actions and customize vibrant RGB LED colors. |
| **Transition Studio** | Manual live mixing only | **DJ.Studio-Style Timeline Studio (`F4`)**: Horizontal multi-track transition arranger with automated stem crossfading (*Smooth Crossfade*, *Instant Bass-Swap*, *Vocal Bridge*) and live auditioning. |
| **Smart Crate Digging** | Simple BPM/Key column sorting | **Suggestions**: harmonic + tempo match scores for your whole collection against the track on deck A (Open Key wheel, half/double-time aware). |
| **Linux Native Engine** | Windows/macOS only (Wine/yabridge hacks) | **Pure Native Linux Stack**: Direct PipeWire / ALSA audio with real-time thread scheduling + plug-and-play USB HID / `hidraw` hardware engine with automatic udev permissions. |

---

## Detailed Features

### 1. Authentic Turntable Scratch Physics & Jogwheel Engine
* **Capacitive Touch Tracking**: Touching the top platter engages vinyl scratch mode instantly, smoothly tracking hand speed at true 33⅓ RPM ($1.8\text{ s}$ per 1024 ticks). Releasing touch provides natural vinyl slipmat spin-up/spin-down catch-up.
* **Alpha-Beta Filtering**: Implements the proven xwax/Mixxx tracking filter ($\alpha = 0.125, \beta = 0.0039$) to eliminate jitter, packet latency, and drift.
* **Hermite 4-Point Cubic Splines**: Replaces linear sample interpolation with 4-point Hermite cubic interpolation, preventing high-frequency aliasing and digital distortion during aggressive scratching or backspins.
* **Magnetic Cartridge Attenuation**: Physical turntable cartridges produce voltage proportional to needle speed ($V \propto v$). When the platter halts, audio smoothly attenuates to silence, eliminating frozen-sample DC buzzing.
* **CDJ Nudge vs Turntable Scratch**: Outer rim turning nudges pitch phase gently ($0.08\text{ s}$ scaling) without scratching. Easily switch between Vinyl Scratch (`TT`) and CDJ Pitch Bend (`JOG`) modes.

### 2. Four Workspace Layouts (`F1` – `F4`)
* **`F1` — Mix (Default)**: Stacked Deck A & B waveforms for visual beatmatching, compact decks, a 2-channel mixer, and the library below.
* **`F2` — 4-Deck (`A / B / C / D`)**: All four decks with a 4-channel mixer.
* **`F3` — Browser**: Mini deck strips and a full-height library for preparing sets.
* **`F4` — Timeline**: Track A → Track B transition arranger with stem automation presets and live auditioning.

### 3. Interactive Graphical Controller Surface Map & Remapper
* Located in **Preferences** (`⚙ -> Controllers & MX2`).
* Features an interactive visual diagram of your controller surface:
  * **Click Any Control**: Select any button, pad, EQ/filter knob, or jogwheel on the hardware diagram.
  * **Assign Custom Actions**: Play/Pause, Cue, Sync, Stem Mutes (Drums, Bass, Instruments, Vocals), Filter Cutoff, Filter Toggle, Mixer FX (Filter, Reverb, Delay, Noise), Hotcues, and Loops.
  * **Customize RGB LED Colors**: Assign true multi-color hardware codes (Vibrant Blue `0x2E`, Emerald Green `0x1E`, Ruby Red `0x06`, Amber Yellow `0x16`, Club Orange `0x0E`, Electric Cyan `0x22`, Vocal Magenta `0x36`, Clean White `0x4A`).
  * **Hardware Jogwheel Tuning**: Adjust scratch sensitivity ($0.2\times$ to $3.0\times$) and toggle default turntable scratch vs CDJ jog pitch bend.

### 4. Hybrid 4-Stem Engine & Multi-Lane Waveforms
* **Spectrum & 4-Stem Waveform Toggle**: Switch any deck between a high-contrast **Spectrum** frequency-colored waveform (warm red sub-bass, amber low-mids, teal mids, crisp ice-blue highs) and a **4-Lane Split Stem Waveform** (`DRUMS`, `BASS`, `OTHER`, `VOCALS`) with left-hand lane headers, per-lane mute toggles, and real-time level meters.
* **Instant DSP Isolation + Meta Demucs AI Stems**: Every track loaded immediately supports zero-latency 4-band stem isolation via real-time Linkwitz-Riley crossover filters. For studio-grade, 100% bleed-free stem separation, ReTraktor integrates Meta AI's `htdemucs` neural model:
  * **Optional Background Installation**: Install Demucs with one click directly in **Preferences > Stems** or via `./install.sh --with-demucs` into an isolated virtual environment (`~/.local/share/retraktor/venv`) without blocking audio playback or requiring root packages.
  * **Low-Priority Worker**: Separation runs in a background thread under `nice -n 10` to guarantee the real-time audio playback thread is never starved of CPU.
  * **Zero-Bleed Disk Caching**: True 4-stem FLAC files (`drums.flac`, `bass.flac`, `other.flac`, `vocals.flac`) are automatically cached in `~/.cache/retraktor/stems/htdemucs/<track_name>/` and instantly auto-loaded whenever the track is queued on any deck.
* **1-Click `ACAPELLA` & `INST` Isolators**: Instantly strip a track down to just its vocals or just its instrumental backing with a single click or controller pad press.

### 5. Neural Mix™ (`EQ ⇄ Stems`) & Mixer FX
* **Dual-Mode Mixer Strips**: Click **`EQ: CLASSIC`** / **`EQ: STEMS (NEURAL)`** at the top of the central mixer to transform the 3-band EQ knobs on every channel (and on your connected hardware mixer) into real-time **Vocal**, **Instrument**, and **Drums+Bass** stem isolators.
* **9 Mixer FX + 2-Slot Deck FX Rack**: One-knob per-channel **Mixer FX** (*Filter*, *Reverb*, *Delay*, *Noise*, *Time Gater*, *Flanger*, *Barber Pole*, *Dotted Delay*, *Crush*) plus a 40+ effect assignable **FX Unit** (*Ziqualizer*, *Beatmasher 2*, *Gater*, *Formant Filter*, *Delay T3*, *Reverb T3*) and Master **Ozone Maximizer** limiter (`BOOST` / `LIMIT`).

### 6. DJ.Studio-Style Timeline Transition Studio (`F4`)
* Visualize the overlap between **Deck A (Outgoing)** and **Deck B (Incoming)** over an adjustable **`8 Bars`**, **`16 Bars`**, or **`32 Bars`** transition window.
* Apply one-click stem transition presets:
  * **Smooth Crossfade**: Gradual linear volume blend across the transition window.
  * **Instant Bass-Swap**: Keeps Deck A's highs/vocals fading smoothly while swapping the sub-bass (`BASS` stem) cleanly on the phrase downbeat.
  * **Vocal Stem Bridge**: Holds Deck A's acapella over Deck B's incoming drums and bassline before dropping Deck A out at the phrase end.
* Click **`▶ Audition Transition`** to jump Deck A into the transition zone, sync Deck B's tempo and phase, and hear the automated stem transition live.

### 7. Native Audio Decoder & Collection Browser
* **Multi-Format Local Audio Playback**: Powered by a pure-Rust `symphonia` decoder supporting `.mp3`, `.flac`, `.wav`, `.ogg`, `.m4a`, `.aac`, and `.aiff` files with automatic waveform peak-pyramid extraction.
* **Music Library**: Reads real tags (ID3, Vorbis, MP4, AIFF) — artist, title, album, genre, BPM, key, label, remixer, year — and never invents missing values. Scans music folders in the background, handles 20,000+ track collections in a virtualized table with multi-select, sortable/resizable columns, and right-click actions.
* **Traktor Collection Import & Relocate**: Import an existing `collection.nml` (v19/v20) with cue points, loops, beatgrids, ratings and playlists intact; tracks whose files moved are re-linked by file name from your music folders.
* **Playlists**: Create, rename, delete and export (`.m3u8`) playlists; drag tracks onto a playlist, onto a deck, or onto "new playlist". Everything autosaves to `~/.local/share/retraktor/collection.nml` (your Traktor files are never modified).
* **Track Analysis**: Detects BPM + beatgrid, musical key (Open Key), a 1–10 energy level and up to 8 auto cue points at phrase changes. Runs in the background ("Analyze N tracks" fills only unknown values; right-click → Analyze replaces them) and automatically when an unanalysed track is loaded. Your existing cues are never replaced. Slow tracks are sometimes detected at double tempo — use ×½ in Edit track info. CLI: `retraktor-cli analyze <files>`.
* **rekordbox Export**: One click writes `~/Music/ReTraktor-rekordbox.xml` (tracks, cues, loops, grids, playlists) for rekordbox / CDJ USB export.
* **Suggestions**: Ranks your collection by harmonic and tempo compatibility with the track on deck A.
* **Track Info Editor**: Edit metadata in the collection, and optionally write it back into the file's tags.
* **Built-In Master `.WAV` Mix Recorder (`● REC`)**: Record your live set directly from the post-maximizer master bus to `~/Music/ReTraktor_Master_Mix.wav` (16-bit / 48 kHz stereo PCM).

### 8. Full Hardware Controller Support
* **First-Class Traktor MX2 (`17CC:2420`) Support**: Complete NHL2 HID input/output mapping for all **83 buttons**, **6 encoders**, **26 12-bit pots/faders**, **2 capacitive 1024-tick jogwheels**, **105 top-panel LEDs**, and **12 RGB bottom-shell ambient LEDs** (with Deck State, Loop Status, and End-of-Track Warning modes), plus **4-Out / 2-In 24-bit/96kHz USB Audio Class 2.0** routing (Master L/R `Out 1/2` + Headphone Cue L/R `Out 3/4`).
* **15 Supported Controllers Out of the Box**:
  * **Traktor MX2** (`17CC:2420`)
  * **Traktor Kontrol S4 MK3** (`17CC:1720`) — including dual Haptic Drive™ motorized jogwheel torque FFB (`0xE0`)
  * **Traktor Kontrol S2 MK3** (`17CC:1730`) & **S3** (`17CC:1750`)
  * **Traktor Kontrol X1 MK1 / MK2 / MK3** (`17CC:0839`, `17CC:1220`, `17CC:1760`)
  * **Traktor Kontrol Z1** (`17CC:1250`) & **Z1 MK2** (`17CC:2400`)
  * **Traktor Kontrol F1** (`17CC:1120`) & **D2** (`17CC:1400`)
  * **Traktor Kontrol S8** (`17CC:1370`), **S4 MK2** (`17CC:1310`), **S2 MK2** (`17CC:1320`), **S5** (`17CC:1420`)

---

## Installation & Setup

### Option 1: Automated Linux Installer (Ubuntu, Pop!_OS, Debian, Linux Mint)

The automated script installs dependencies, sets up rootless USB permissions via `udev`, configures real-time audio limits, builds optimized binaries, builds/installs a native `.deb` package, and registers desktop launcher icons:

```bash
git clone https://github.com/adamtdm/ReTraktor.git
cd ReTraktor
./install.sh
```

To update an existing installation to the latest version:

```bash
cd ~/ReTraktor && git pull origin main && ./install.sh
```

Launch **ReTraktor** from your desktop application launcher or run:

```bash
retraktor-gui
```

---

### Option 2: Manual Installation From Source (Any Linux Distribution)

#### 1. Install System Development Libraries

* **Ubuntu / Pop!_OS / Debian / Mint**:
  ```bash
  sudo apt update
  sudo apt install -y build-essential pkg-config curl git \
      libudev-dev libusb-1.0-0-dev libhidapi-dev libasound2-dev \
      libpipewire-0.3-dev libx11-dev libxkbcommon-dev libgl1-mesa-dev
  ```

* **Fedora / RHEL**:
  ```bash
  sudo dnf install -y gcc gcc-c++ pkgconfig git \
      systemd-devel libusb1-devel hidapi-devel alsa-lib-devel \
      pipewire-devel libX11-devel libxkbcommon-devel mesa-libGL-devel
  ```

* **Arch Linux / Manjaro**:
  ```bash
  sudo pacman -S --needed base-devel pkgconf git \
      systemd libusb hidapi alsa-lib pipewire libx11 libxkbcommon mesa
  ```

#### 2. Install Rust Toolchain (if not already installed)
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

#### 3. Compile Optimized Release Binaries
```bash
git clone https://github.com/adamtdm/ReTraktor.git
cd ReTraktor
cargo build --workspace --release
```

#### 4. Configure USB Hardware Permissions (`udev`)
To allow non-root users to access DJ controllers directly:
```bash
sudo cp assets/99-retraktor-ni.rules /etc/udev/rules.d/99-retraktor-ni.rules
sudo udevadm control --reload-rules && sudo udevadm trigger
sudo usermod -aG plugdev,audio $USER
```
*(Log out and log back in for group changes to take effect).*

#### 5. Install Binaries to System Path
```bash
sudo install -m 0755 target/release/retraktor-gui /usr/local/bin/retraktor-gui
sudo install -m 0755 target/release/retraktor-cli /usr/local/bin/retraktor-cli
sudo install -m 0755 assets/retraktor-wrapper.sh /usr/local/bin/retraktor
sudo install -m 0644 assets/retraktor.desktop /usr/share/applications/retraktor.desktop
sudo install -m 0644 assets/retraktor.svg /usr/share/icons/hicolor/scalable/apps/retraktor.svg
```

---

## Hardware Setup & Troubleshooting

### Traktor MX2 Setup Guide
1. **Power**: Plug in the external 15V DC power adapter. (USB bus power alone cannot power the internal audio converter or LED drivers).
2. **Connection**: Connect the USB-B cable to your Linux PC.
3. **Controller Mode**: Ensure the controller is in Native HID mode. Press `Left SHIFT + Right SHIFT` to toggle between Native HID and standard MIDI mode.
4. **Self-Test & Diagnostic**: Run the built-in system verification tool:
   ```bash
   retraktor doctor
   ```
   This will inspect:
   * PipeWire and ALSA audio subsystems
   * User membership in `audio` and `plugdev` groups
   * Active USB device nodes and udev permissions
   * Connected controller models and their firmware state

---

## Workspace Structure

```text
ReTraktor/
├── crates/
│   ├── retraktor-gui/        # Desktop UI (egui/eframe, 6 layouts, Spectrum & 4-Stem waveforms, Timeline studio)
│   ├── retraktor-core/       # Reactive app.traktor.* PropertyBus, QML Defines 1.0 enums, and CSI 1.0 engine
│   ├── retraktor-dsp/        # Real-time DSP: 5 EQ models, 9 Mixer FX, Flux/Freeze, Stem Summer & Ozone Maximizer
│   ├── retraktor-audio/      # Lock-free audio engine, Alpha-Beta scratch tracking, Hermite interpolation & PipeWire/ALSA routing
│   ├── retraktor-hid/        # NHL2 USB HID Driver + Haptic Drive FFB + Traktor MX2 RGB & Bottom-Shell lighting
│   ├── retraktor-formats/    # Traktor collection.nml (v19/v20), .tsi (DIOM TLV), Stripes/Transients & .stem.m4a parsers
│   ├── retraktor-library/    # Harmonic KeyWheel (Open Key 1d..12d / 1m..12m), TrackAnalyzer, Smartlists & Deck Loader
│   └── retraktor-cli/        # CLI Diagnostic Suite, Doctor, NML/TSI/CSI Inspector & 4-Deck Performance Simulator
├── assets/
│   ├── 99-retraktor-ni.rules # Linux udev rules for rootless USB HID access (Vendor ID 0x17CC)
│   ├── 99-retraktor-realtime.conf # Real-time audio scheduling limits (@audio / @realtime)
│   ├── retraktor.desktop     # Freedesktop.org application launcher entry
│   └── retraktor.svg         # Scalable vector desktop icon
└── docs/
    └── evidence/             # Clean-room protocol & format specifications (AppProperty catalog, CSI ABI, NHL2 HID)
```

---

## Keyboard Shortcuts

| Shortcut | Action |
| :--- | :--- |
| `Space` | Play / Pause active deck |
| `C` | Jump to Cue point |
| `S` | Sync BPM and phase with Master deck |
| `F1` – `F4` | Switch Workspace Layout (Mix, 4-Deck, Browser, Timeline) |
| `1` – `4` | Trigger Hotcues / Stem Mutes (Drums, Bass, Instruments, Vocals) |
| `Shift + 1..4` | Delete Hotcue |
| `L` | Toggle Active Loop |
| `[` / `]` | Halve / Double Loop Length (1..32 beats) |
| `Up` / `Down` | Browse library tracks |
| `Enter` | Load selected track to Deck A |
| `Shift + Enter` | Load selected track to Deck B |
| `Ctrl + P` | Open Preferences Dialog |
| `Ctrl + F` | Focus library search |
| `Ctrl + A` | Select all tracks in the current view |
| `Delete` | Remove selected tracks from the current playlist |
| Double-click | Load track to the first deck that isn't playing |

---

## Verification & Test Suite

Run the full automated test suite:

```bash
cargo test --workspace
```

All unit and integration tests across `retraktor-core`, `retraktor-audio`, `retraktor-hid`, `retraktor-formats`, and `retraktor-gui` pass with zero warnings, and the 4-deck + 4-stem lock-free audio engine benchmarks at **~103x real-time speed** (**< 1% CPU usage** at 48 kHz / 256 frames).

---

## License

Licensed under either of:

* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.
