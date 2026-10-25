# ReTraktor

**ReTraktor** is an evidence-backed, clean-room Linux re-implementation of **Native Instruments Traktor Pro 4 (`4.5.1`)**, built in Rust.

It provides native Linux interoperability for Traktor music libraries (`.nml` v19/v20), controller mappings (`.tsi` Big-Endian FourCC `DIOM` TLV), binary waveform & transient caches (`Stripes/` `PRTS` and `Transients/`), Stem containers (`.stem.m4a` / `.trak`), the complete `app.traktor.*` reactive property bus (`1,797` properties across `190` QML templates), the `CSI 1.0` QML Controller Script Interface (`Wire`, `WiresGroup`, `Defines 1.0`), a 4-deck real-time DSP/audio engine, and Native Instruments Kontrol (`VID 0x17CC`) USB HID, Haptic Drive motorized platters, and `OffscreenView` RGB565 USB Bulk display streaming.

---

## Table of Contents

1. [Key Features](#key-features)
2. [Workspace Architecture](#workspace-architecture)
3. [Supported Native Instruments Hardware](#supported-native-instruments-hardware)
4. [Installation Guide](#installation-guide)
   - [1. Install System Dependencies (Linux & macOS)](#1-install-system-dependencies-linux--macos)
   - [2. Install the Rust Toolchain](#2-install-the-rust-toolchain)
   - [3. Clone, Build & Install ReTraktor](#3-clone-build--install-retraktor)
   - [4. Configure Linux `udev` Rules for NI Kontrol Hardware](#4-configure-linux-udev-rules-for-ni-kontrol-hardware)
   - [5. Configure Linux Low-Latency PipeWire `pro-audio` & Real-Time Privileges](#5-configure-linux-low-latency-pipewire-pro-audio--real-time-privileges)
   - [6. Using Your Existing Traktor Pro 4 Library & Controller Scripts](#6-using-your-existing-traktor-pro-4-library--controller-scripts)
5. [CLI Usage & Examples (`retraktor-cli`)](#cli-usage--examples-retraktor-cli)
6. [Running the Test Suite](#running-the-test-suite)
7. [Reverse-Engineering Evidence Ledger (`docs/evidence/`)](#reverse-engineering-evidence-ledger-docsevidence)
8. [Clean-Room Scope & Legal Notice](#clean-room-scope--legal-notice)

---

## Key Features

- **Native Hardware-Accelerated Desktop Graphical Application (`retraktor-gui`)**:
  - Full-blown desktop GUI interface matching Native Instruments Traktor Pro 4 on macOS/Windows, built on `eframe`/`egui` with native Wayland and X11 acceleration.
  - **Top Bar**: Master Clock BPM display with Tap/Sync, FX Units 1 & 2 racks with Dry/Wet and 3 parameter knobs, Ozone Maximizer limiter meter, and PipeWire `pro-audio` 48kHz status indicator.
  - **Decks A & B (and toggleable C & D)**: Authentic Traktor Pro 4 dark palette (Cyan for Deck A, Orange for Deck B), Track header with Open Key badge and harmonic match indicator, 60 FPS parallel scrolling 3-band frequency spectrum waveforms, full-track Stripe minimap overview (`PRTS`) with draggable scrubber, Loop strip (`/2`, `x2`, Active), 8 backlit Hotcue pads (`[1]`..`[8]`), and transport buttons (`[PLAY]`, `[CUE]`, `[SYNC]`, `[FLUX]`, `[PITCH]`).
  - **Center 4-Channel Mixer**: Rotary gain knobs with autogain, 3-band EQ knobs with individual kill buttons, Bipolar Filter / Mixer FX knobs with selector, Headphone Cue listen buttons, smooth vertical volume faders with dB scale, stereo LED VU meter towers (Green/Yellow/Red), and horizontal Crossfader with curve selector.
  - **Track Collection Browser**: Left tree navigation (Collection, Playlists, Smartlists, History, Explorer, NI Controllers), instant search bar, sortable track table with live Circle of Fifths harmonic compatibility highlighting, and one-click `[LOAD A]` / `[LOAD B]` buttons.
  - **Real-Time Sound Engine**: Real-time stereo audio playback through PipeWire / ALSA (`cpal`), synthesis demo tracks, and audio file loading.
  - **Preferences Modal (`Ctrl+,`)**: Audio backend (PipeWire / ALSA), sample rate (44.1k/48k/96k), buffer size / latency (64, 128, 256, 512 frames), output routing (Master L/R 1/2, Headphone Cue 3/4), mixer EQ emulations, and NI controller hardware detection.
- **100% Observed `app.traktor.*` Property Bus (`retraktor-core`)**:
  - Pre-registers `1,797` typed properties covering all `190` unique `app.traktor.*` property path templates extracted across all `510` QML files shipped in Traktor Pro `4.5.1` (`Decks 1..4`, `Stems 1..4`, `Remix Decks 4x16`, `Step Sequencer`, `4-Channel Mixer`, `4 FX Units`, `Ozone Maximizer`, `Master Clock`, and `Browser`).
  - Implements all 10 `Defines 1.0` QML singletons with exact integer parity (`PadsMode`, `Overlay`, `FooterPage`, `FxOverlay`, `StemStyle`, `ScreenView`, `NavigationMode`, `JumpSize`, `LoopSize`, `MoveSize`).
  - Evaluates `CSI 1.0` reactive controller graphs (`Module`, `AppProperty`, `MappingPropertyDescriptor`, `Wire`, `WiresGroup`, `Or`, `And`, and all 9 property adapter types).
- **Lossless Traktor File Formats (`retraktor-formats`)**:
  - **`.nml` (v19 & v20)**: Full XML DOM parser/serializer supporting Traktor Pro 4 Flexible Beatgrids (`<CUE_V2 TYPE="4"><GRID BPM="..."/></CUE_V2>`), Hotcues (`0..7`), Loops, Loudness, Musical Keys (`0..23`), Playlists, Smartlists, multiline attributes (`KEY_LYRICS`), and automatic Linux POSIX path (`/home/dj/Music/track.flac`) `<->` Traktor `/:` delimiter (`/:home/:dj/:Music/:`) conversion. Verified on a real `16,160`-track Traktor 4.5.1 `collection.nml`.
  - **`.tsi` Controller Mappings**: Parses `<NIXML><TraktorSettings>` and decodes the Base64 Big-Endian FourCC TLV binary tree (`DIOM -> DIOI + DEVS -> DEVI -> DDAT -> DDIF/DDIV/DDIC/DDPT/DDDC/DDCB/DVST`, including `CMAS -> CMAI -> CMAD` and `DCBM` bindings) with byte-for-byte round-trip fidelity. Verified on Traktor 4.5.1's `2.88 MB` `DDJ-FLX4.tsi`.
  - **`Stripes/` & `Transients/` Binary Caches**: Reads and writes Traktor's exact `262,169`-byte `PRTS` (`STRP` LE) 4-channel (`[low, mid, high, total]`) waveform stripes (`1024 x 64 = 65,536` columns) and `0x05` 16-byte transient onset caches (`[position_sec: f64 LE, weight: f32 LE, metric: f32 LE]`).
  - **`.stem.m4a` & `.trak`**: Extracts and serializes MP4 `udta -> stem` JSON compressor/limiter DSP metadata and 4-stem slot definitions (`Drums`, `Bass`, `Synths`, `Vocals`).
- **Harmonic Key Wheel, Analyzer & Track Library (`retraktor-library`)**:
  - Circle-of-Fifths `KeyWheel` converting Traktor `MUSICAL_KEY VALUE="0..23"` to Open Key (`1d..12d`, `1m..12m`) and evaluating harmonic compatibility (same key, `+/- 1` step on the wheel, or relative major/minor).
  - `TrackAnalyzer` computing RMS/Peak loudness (`-12 dBFS` autogain reference), tempo, `AutoGrid` beatgrid cues, and `PRTS` waveform stripes from stereo PCM audio.
- **4-Deck Real-Time DSP & Audio Engine (`retraktor-dsp`, `retraktor-audio`)**:
  - All 5 Traktor Mixer EQ models (`Classic`, `P600`, `Nuo`, `Z ISO`, `Xone:92` 4-band).
  - All 9 Channel Mixer FX (`Filter`, `Reverb`, `DualDelay`, `Noise`, `TimeGater`, `Flanger`, `BarberPole`, `DottedDelay`, `Crush`).
  - `DeckTransportDsp` with **Flux / Slip Mode** background timeline tracking, **Loop / Loop Roll**, and **8-Slice Freeze Mode**.
  - **Ozone Maximizer (`OzoneMaximizerDsp`)** master bus brickwall limiter & loudness maximizer modeled on `iZTraktorTDSP.dylib`.
  - Crossfader curves (`Smooth`, `Steep`, `Scratch`), Master/Cue bus summing, and PipeWire `pro-audio` UAC2 multi-channel routing.
- **Native Instruments Kontrol HID, Haptic Drive & USB Bulk Screens (`retraktor-hid`)**:
  - USB VID/PID registry (`0x17CC`) for `S4 MK3`, `S3`, `S2 MK3`, `S8`, `S5`, `D2`, `X1 MK3`, `Z1 MK2`, and `F1`.
  - Stateful HID Input Report decoder (`0x01` button/encoder bitmasks, `0x02` 12-bit Little-Endian ADC faders/pots).
  - `S4Mk3HapticPlatterConfig` Report `0x30` encoder for S4 MK3 Haptic Drive motorized platters (`JogwheelMode::Turntable` / `Jogwheel` / `Cdj`, `33.3`/`45` RPM motor drive, platter tension, haptic tick density, LED ring color).
  - `KontrolScreenFramebuffer` offscreen RGB565 renderer (`480x272` for S8/S5/D2 `OffscreenView.qml`, `320x240` for S4 MK3) with USB Bulk `0x8400` rectangular packet framing.
  - `AlsaMidiTsiRouter` for routing standard 3-byte MIDI messages through loaded `.tsi` files directly into the `PropertyBus`.

---

## Workspace Architecture

```text
ReTraktor/
├── Cargo.toml                  # Rust workspace manifest
├── crates/
│   ├── retraktor-core/         # app.traktor.* PropertyBus (1,797 props), Defines 1.0 enums, CSI 1.0 graph engine
│   ├── retraktor-formats/      # .nml (v19/v20), .tsi (DIOM TLV), Stripes (PRTS), Transients, .stem.m4a / .trak
│   ├── retraktor-library/      # KeyWheel (Open Key 1d..12d / 1m..12m), TrackAnalyzer, Smartlists, Deck Loader
│   ├── retraktor-dsp/          # 5 EQ models, 9 Mixer FX, Flux/Loop/Freeze transport, Stem summer, Ozone Maximizer
│   ├── retraktor-audio/        # 4-Deck real-time mixer, Crossfader curves, PipeWire pro-audio UAC2 routing
│   ├── retraktor-hid/          # NI Kontrol (0x17CC) HID reports, S4 MK3 Haptic Drive, RGB565 USB Bulk screens, udev
│   ├── retraktor-gui/          # Full-blown 60 FPS Traktor Pro 4 native desktop GUI (Wayland & X11)
│   └── retraktor-cli/          # CLI inspector, udev rule generator, and 4-deck performance simulator
└── docs/
    └── evidence/               # REA static analysis specifications & extracted QML property catalogs
        ├── app-property-catalog.md
        ├── csi-qml-abi.md
        ├── file-formats-spec.md
        ├── ni-hid-protocol.md
        └── generated/
            └── app-traktor-paths.observed.txt
```

---

## Supported Native Instruments Hardware

| Controller Model | USB `VID:PID` | CSI Module Folder | Color Display (`OffscreenView`) | Haptic Drive Motorized Platters |
| :--- | :--- | :--- | :--- | :--- |
| **Traktor Kontrol S4 MK3** | `17cc:1720` | `CSI/S4MK3` | Yes (`2x 320x240` RGB565) | Yes (Report `0x30`) |
| **Traktor Kontrol S8** | `17cc:1370` | `CSI/S8` | Yes (`2x 480x272` RGB565) | No (Touchstrip) |
| **Traktor Kontrol S5** | `17cc:1420` | `CSI/S5` | Yes (`2x 480x272` RGB565) | No (Touchstrip) |
| **Traktor Kontrol D2** | `17cc:1400` | `CSI/D2` | Yes (`1x 480x272` RGB565) | No (Touchstrip) |
| **Traktor Kontrol S3** | `17cc:1900` | `CSI/S3` | No | No |
| **Traktor Kontrol S2 MK3** | `17cc:1730` | `CSI/S2MK3` | No | No |
| **Traktor Kontrol X1 MK3** | `17cc:1cbc` | `CSI/X1MK3` | Yes (`OLED 128x64`) | No |
| **Traktor Kontrol Z1 MK2** | `17cc:2120` | `CSI/Z1MK2` | Yes (`OLED 128x64`) | No |
| **Traktor Kontrol F1** | `17cc:1120` | `CSI/F1` | 7-Segment | No |

---

## Installation Guide

### ⚡ 1-Command Universal Linux Installation

You can install ReTraktor and all required audio libraries, Native Instruments Kontrol `udev` rules, real-time PAM audio permissions, desktop launcher, and PATH binaries with a single command:

#### Option A: 1-Line Universal Installer (Ubuntu, Debian, Fedora, Arch, openSUSE)
```bash
curl -fsSL https://raw.githubusercontent.com/adamtdm/ReTraktor/main/install.sh | bash
```

Once installed, simply run:
```bash
retraktor
```
*(or click the **ReTraktor** icon in your application launcher!)*

#### Option B: Single Debian / Ubuntu Package (`.deb`)
If you prefer native packages, install the single `.deb` file with `apt` (which automatically installs dependencies, configures NI udev rules, sets up user groups, and registers the desktop launcher):
```bash
sudo apt install ./retraktor_0.1.0_amd64.deb
```
Then run:
```bash
retraktor
```

#### Option C: Universal Standalone AppImage (Zero Dependencies)
Download the single-file executable and run directly on any Linux distribution:
```bash
chmod +x ReTraktor-x86_64.AppImage
./ReTraktor-x86_64.AppImage
```

---

### Manual / Developer Build Instructions

#### 1. Install System Dependencies (Linux & macOS)

#### Ubuntu / Debian (22.04 / 24.04+)
```bash
sudo apt update
sudo apt install -y \
  build-essential pkg-config git curl \
  libudev-dev libusb-1.0-0-dev libhidapi-dev \
  libasound2-dev libpipewire-0.3-dev \
  pipewire pipewire-alsa pipewire-pulse wireplumber
```

#### Fedora / RHEL (39 / 40+)
```bash
sudo dnf install -y \
  gcc gcc-c++ make pkgconf-pkg-config git curl \
  systemd-devel libusb1-devel hidapi-devel \
  alsa-lib-devel pipewire-devel pipewire-alsa wireplumber
```

#### Arch Linux
```bash
sudo pacman -S --needed \
  base-devel pkgconf git curl \
  systemd-libs libusb hidapi \
  alsa-lib pipewire pipewire-alsa wireplumber
```

#### macOS (Development & Verification Host)
```bash
xcode-select --install
brew install pkg-config hidapi libusb
```

---

### 2. Install the Rust Toolchain

ReTraktor requires **Rust `1.80+`** (tested with `rustc 1.99.1`). If you do not have Rust installed, install it via `rustup`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
rustc --version
```

---

### 3. Clone, Build & Install ReTraktor

```bash
git clone https://github.com/adamtdm/ReTraktor.git
cd ReTraktor

# Build the entire workspace in release mode
cargo build --workspace --release

# Install the retraktor-cli binary into ~/.cargo/bin
cargo install --path crates/retraktor-cli
```

Verify the installation:
```bash
retraktor-cli list-devices
retraktor-cli simulate-mix
```

---

### 4. Configure Linux `udev` Rules for NI Kontrol Hardware

On Linux, unprivileged user access to Native Instruments `hidraw` control surfaces and USB Bulk display endpoints requires `udev` rules. ReTraktor generates the exact rule file automatically:

```bash
# Generate and install /etc/udev/rules.d/99-retraktor-ni.rules
retraktor-cli udev-rules | sudo tee /etc/udev/rules.d/99-retraktor-ni.rules > /dev/null

# Ensure the plugdev group exists and add your user
sudo groupadd -f plugdev
sudo usermod -aG plugdev,audio "$USER"

# Reload udev rules and trigger device re-enumeration
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Unplug and reconnect your Native Instruments controller. Both `/dev/hidraw*` and `/dev/bus/usb/*/*` endpoints for `VID 0x17cc` will now be accessible without root privileges (`TAG+="uaccess"`).

---

### 5. Configure Linux Low-Latency PipeWire `pro-audio` & Real-Time Privileges

1. **Grant Real-Time Audio Scheduling (`SCHED_FIFO`)**:
   ```bash
   sudo tee /etc/security/limits.d/99-retraktor-realtime.conf > /dev/null << 'EOF'
   @audio   -  rtprio     95
   @audio   -  memlock    unlimited
   @audio   -  nice      -19
   EOF
   ```
   *(Log out and back in for group and PAM limits to take effect.)*

2. **Switch Your NI Kontrol USB Audio Interface to PipeWire `pro-audio` Profile**:
   Native Instruments controllers expose multi-channel USB Audio Class 2.0 (UAC2) interfaces (e.g., Master L/R on Out 1/2, Headphone Monitor Cue L/R on Out 3/4). Enable the `pro-audio` profile so all discrete input/output channels are exposed without consumer surround upmixing:
   ```bash
   # List audio cards to find your Traktor Kontrol card ID
   wpctl status

   # Set quantum to 128 frames @ 48000 Hz (2.67 ms buffer latency)
   pw-metadata -n settings 0 clock.force-rate 48000
   pw-metadata -n settings 0 clock.force-quantum 128
   ```

---

### 6. Using Your Existing Traktor Pro 4 Library & Controller Scripts

ReTraktor reads native Traktor Pro 4 files directly without conversion:
- **Music Library (`collection.nml`)**: Copy or mount your `Traktor 4.x/collection.nml`, `Stripes/`, and `Transients/` directories. ReTraktor's `NmlLocation` automatically normalizes macOS/Windows `/:` volume paths and Linux POSIX paths.
- **MIDI & Controller Mappings (`.tsi`)**: Pass any `.tsi` file from `Settings/Default Settings/Controller/` or your custom mappings to `retraktor-cli inspect-tsi` or `AlsaMidiTsiRouter`.
- **Shipped QML Controller Scripts (`CSI/` & `Screens/`)**: Point `retraktor-cli inspect-csi` at `/Applications/Native Instruments/Traktor Pro 4/Traktor Pro 4.app/Contents/Resources/qml` (or a Linux copy) to audit and bind all `CSI 1.0` `Wire` and `WiresGroup` graphs.

---

## CLI Usage & Examples (`retraktor-cli`)

```text
ReTraktor CLI — Linux Port of Native Instruments Traktor Pro 4

Commands:
  inspect-nml <file.nml>       Parse and summarize a Traktor .nml collection
  inspect-tsi <file.tsi>       Decode and summarize a Traktor .tsi controller mapping
  inspect-stripe <file>        Inspect a Traktor PRTS binary waveform stripe cache
  inspect-transients <file>    Inspect a Traktor binary transient cache
  inspect-csi <file-or-dir>    Analyze Traktor CSI 1.0 QML controller script(s)
  dump-prop-bus [filter]       List registered app.traktor.* state bus properties
  list-devices                 List supported NI Kontrol USB VID/PID controllers
  udev-rules                   Output Linux /etc/udev/rules.d/99-retraktor-ni.rules
  simulate-mix                 Run end-to-end 4-deck audio/library/CSI/screen simulation
```

### Example 1: Inspect a Traktor Pro 4 `.nml` Collection
```bash
cargo run -p retraktor-cli -- inspect-nml "$HOME/Documents/Native Instruments/Traktor 4.5.1/collection.nml"
```
```text
NML Version: 20
Program:     Traktor Pro 4
Entries:     16160
  [1] James Brown - The Boss (97.84 BPM, 2 cues) -> /Audio Tracks/R&B & Soul/01. James Brown - The Boss.flac
  [2] 10cc - The Worst Band in the World (154.09 BPM, 1 cues) -> /Audio Tracks/R&B & Soul/10cc - Sheet Music - The Worst Band in the World.flac
  ...
```

### Example 2: Decode a `.tsi` Controller Mapping (`DIOM` Binary TLV)
```bash
cargo run -p retraktor-cli -- inspect-tsi "$HOME/Documents/Native Instruments/Traktor 4.5.1/Settings/Default Settings/Controller/AlphaTheta/02 - Controller - AlphaTheta - DDJ-FLX4.tsi"
```
```text
Settings Entries: 3
Root FourCC Chunks: 1
Devices: 6
  Device: Generic MIDI [DDJ-FLX4 - Pads A (Play)] (213 control mappings, In="DDJ-FLX4", Out="DDJ-FLX4")
  Device: Generic MIDI [DDJ-FLX4 - Pads B (Play)] (213 control mappings, In="DDJ-FLX4", Out="DDJ-FLX4")
  Device: Generic MIDI [DDJ-FLX4 - Deck FX (Play)] (16 control mappings, In="DDJ-FLX4", Out="DDJ-FLX4")
  Device: Generic MIDI [DDJ-FLX4 - Deck A (Play)] (38 control mappings, In="DDJ-FLX4", Out="DDJ-FLX4")
  Device: Generic MIDI [DDJ-FLX4 - Deck B (Play)] (38 control mappings, In="DDJ-FLX4", Out="DDJ-FLX4")
  Device: Generic MIDI [DDJ-FLX4 - Mixer (Play)] (50 control mappings, In="DDJ-FLX4", Out="DDJ-FLX4")
```

### Example 3: Inspect Binary `Stripes/` (`PRTS`) and `Transients/` Cache Files
```bash
cargo run -p retraktor-cli -- inspect-stripe "$HOME/Documents/Native Instruments/Traktor 4.5.1/Stripes/104/ILFBD3BZNWLA3AMJMZPRDMDCOCRB"
```
```text
Stripe Header Tag:   0x06 ('PRTS')
Version:             2
Scale Factor:        3.50
Block Grid:          1024 x 64 (65536 total columns)
Populated Columns:   9900
```

### Example 4: Audit Shipped `CSI 1.0` QML Controller Scripts
```bash
cargo run -p retraktor-cli -- inspect-csi "/Applications/Native Instruments/Traktor Pro 4/Traktor Pro 4.app/Contents/Resources/qml"
```
```text
CSI QML Directory:          /Applications/Native Instruments/Traktor Pro 4/Traktor Pro 4.app/Contents/Resources/qml
QML Files Scanned:          510
Total WiresGroup Blocks:    426
Total Wire Connections:     2796
Unique AppProperty Paths:   190
Unique Mapping Properties:  205
```

### Example 5: Run the End-to-End 4-Deck Performance Simulation
```bash
cargo run -p retraktor-cli -- simulate-mix
```
```text
=== ReTraktor End-to-End 4-Deck Linux Performance Simulation ===
[1/6] Initialized PropertyBus (1797 properties) & PipeWire pro-audio (NI Kontrol UAC2) @ 48000 Hz (128 frames/period)
[2/6] Built NML v20 Collection (2 tracks, 2 harmonic matches for 4m on KeyWheel, 120 stripe columns)
[3/6] Loaded Deck A ('Berlin Midnight (Stem Edit)' @ 128 BPM, 4m) & Deck B ('Detroit Dawn' @ 130 BPM, 5m) -> Harmonic Match: true
[4/6] Dispatched S4 MK3 CSI Events -> Deck A Play=true, Flux=true, Deck B Play=true, Sync Ratio=0.9846x
[5/6] Processed 48,000 Real-Time Audio Frames -> Master Peak=(0.591, 0.591), Deck A Playhead=1.000s
[6/6] Built S4 MK3 Haptic Report 0x30 (16 bytes) & OffscreenView 480x272 RGB565 USB Bulk Frame (261132 bytes)
=== Simulation Completed Successfully ===
```

---

## Running the Test Suite

Run all unit and real-data integration tests across the workspace:

```bash
cargo test --workspace
```

---

## Reverse-Engineering Evidence Ledger (`docs/evidence/`)

All behavioral specifications and binary/QML layouts were recovered using **REA (`rea-agents`)** + **Hopper Disassembler** against shipped **Traktor Pro 4 (`4.5.1 21`)** artifacts:

- [`docs/evidence/app-property-catalog.md`](docs/evidence/app-property-catalog.md): Complete `app.traktor.*` reactive property bus specification.
- [`docs/evidence/generated/app-traktor-paths.observed.txt`](docs/evidence/generated/app-traktor-paths.observed.txt): All `190` unique `app.traktor.*` property templates extracted across `510` QML files.
- [`docs/evidence/csi-qml-abi.md`](docs/evidence/csi-qml-abi.md): `CSI 1.0` QML C++ plugin types, adapters, and `Defines 1.0` singleton constants.
- [`docs/evidence/file-formats-spec.md`](docs/evidence/file-formats-spec.md): `.nml` (v19/v20), `.tsi` (`DIOM` Big-Endian FourCC TLV), `Stripes/` (`PRTS`), `Transients/` (`0x05`), `.stem.m4a`, and `.trak` specifications.
- [`docs/evidence/ni-hid-protocol.md`](docs/evidence/ni-hid-protocol.md): Native Instruments Kontrol (`0x17CC`) USB HID reports, S4 MK3 Haptic Drive Report `0x30`, and `OffscreenView` RGB565 USB Bulk display protocol (`0x8400`).

---

## Clean-Room Scope & Legal Notice

ReTraktor is an independent, clean-room open-source interoperability project designed to enable DJs to use their music libraries and hardware controllers on Linux.
- No proprietary source code, binaries, or copyrighted assets from Native Instruments are distributed in this repository.
- DRM, license verification, and `NTKDaemon` activation routines are strictly out of scope and are not touched or bypassed.
- "Traktor", "Traktor Kontrol", and "Native Instruments" are trademarks of Native Instruments GmbH, referenced here solely for hardware and file-format interoperability descriptions.
