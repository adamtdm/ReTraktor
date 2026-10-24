# ReTraktor Evidence Ledger: Native Instruments Kontrol HID & Display Protocol (`0x17CC`)

## 1. Provenance & Evidence

Anchored to:
- `Traktor Pro 4` Mach-O binary (`ev_fb958e256da8dbb4f77cb48853b8d825c706ee0e6c6c15656ae0de75af169bab`) and `IOKit.framework` / `CoreMIDI.framework` imports (`ev_0db2f69bfbd1521bc1986118a7817103f09f53b4d8403ec86649ad28fb6b30b9`).
- Shipped QML hardware controller definitions in `/Applications/Native Instruments/Traktor Pro 4/Traktor Pro 4.app/Contents/Resources/qml/CSI/`:
  - `S4MK3/`, `S3/`, `S2MK3/`, `S8/`, `S5/`, `D2/`, `X1MK3/`, `Z1MK2/`, `MX2/`.

---

## 2. USB Vendor & Product ID Registry

All Native Instruments Traktor hardware controllers use **USB Vendor ID `0x17CC`** (*Native Instruments GmbH*):

| Controller Model | USB VID | USB PID | CSI QML Folder | Audio Channels (In / Out) | Display / Haptics |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Traktor Kontrol S4 MK3** | `0x17CC` | `0x1720` | `CSI/S4MK3` | 8 In / 4 Out (24-bit 96 kHz USB Audio Class 2) | Dual 320x240 RGB LCDs + Haptic Drive Motorized Platters |
| **Traktor Kontrol S3** | `0x17CC` | `0x1900` | `CSI/S3` | 2 In / 4 Out (24-bit 48/96 kHz UAC2) | None (RGB Pads + LED Rings) |
| **Traktor Kontrol S2 MK3** | `0x17CC` | `0x1730` | `CSI/S2MK3` | 0 In / 4 Out (24-bit 48 kHz UAC2) | None (RGB Pads) |
| **Traktor Kontrol S8** | `0x17CC` | `0x1370` | `CSI/S8` | 8 In / 4 Out (24-bit 96 kHz UAC2) | Dual 480x272 High-Res RGB LCDs + Touchstrips |
| **Traktor Kontrol S5** | `0x17CC` | `0x1420` | `CSI/S5` | 2 In / 4 Out (24-bit 48 kHz UAC2) | Dual 480x272 High-Res RGB LCDs + Touchstrips |
| **Traktor Kontrol D2** | `0x17CC` | `0x1400` | `CSI/D2` | None (USB Hub + HID + Display) | Single 480x272 High-Res RGB LCD + Touchstrip |
| **Traktor Kontrol X1 MK3** | `0x17CC` | `0x1CBC` | `CSI/X1MK3` | None (USB Hub + HID + OLEDs) | 5 Monochrome 128x64 OLEDs + Under-chassis RGB LEDs |
| **Traktor Kontrol Z1 MK2** | `0x17CC` | `0x2120` | `CSI/Z1MK2` | 0 In / 4 Out (24-bit 96 kHz UAC2) | 3 Monochrome 128x64 OLEDs + VU Meters |
| **Traktor Kontrol F1** | `0x17CC` | `0x1120` | Built-in NHL/CSI | None (HID) | Dual 7-Segment LED + 16 RGB Matrix Pads |

---

## 3. HID Input & Output Report Framing

### 3.1 Input Reports (Device -> Host)
- **Report ID `0x01` (Buttons, Encoders & Jog Wheels)**:
  - Byte `0`: Report ID (`0x01`)
  - Bytes `1..N`: Bit-packed digital button states (transport buttons, cue/play/sync, shift, pad presses, FX buttons, encoder push switches) and 4-bit / 8-bit wrapping rotary encoder counters (`0x00..0x0F` or `0x00..0xFF`).
  - Jog Wheel / Platter High-Resolution Encoders (`S4 MK3`, `S3`, `S2 MK3`): 32-bit Little-Endian angular position (`u32 LE`) and touch capacitance flag.
- **Report ID `0x02` (12-bit / 16-bit ADC Analog Controls)**:
  - Byte `0`: Report ID (`0x02`)
  - Array of Little-Endian `u16` ADC readings (`0x0000..0x0FFF` for 12-bit ADCs) for channel faders, crossfader, 3/4-band EQ pots, filter/MixerFX pots, stem/remix volume faders, and capacitive touchstrip position/pressure.

### 3.2 Output Reports (Host -> Device LEDs & Haptic Motors)
- **Report ID `0x80` / `0x81` (LED Brightness & RGB Pad Matrix)**:
  - Each monochromatic button LED accepts a 7-bit brightness scalar (`0x00..0x7F`, covering dim backlit idle state vs full on state).
  - RGB Performance Pads (`16` pads per controller or `8` per deck) accept indexed Traktor color palette bytes or 3-byte `[R, G, B]` tuples (`0x00..0x7F`).
- **Report ID `0x30` / `0x31` (S4 MK3 Haptic Drive Platter Configuration)**:
  - Controls platter mode (`Off / Freewheel`, `TT / Motorized 33.3 or 45 RPM`, `Jog Haptic Detents`), cue point haptic tick positions, and platter LED ring rotation angle.

---

## 4. High-Speed USB Bulk Display Streaming (`KontrolScreen`)

For controllers with color LCDs (`S8`, `S5`, `D2`, `S4 MK3`) or OLEDs (`X1 MK3`, `Z1 MK2`):
1. The QML screen scene (`Resources/qml/Screens/S8/`, `Screens/S4MK3/`, etc.) renders into an offscreen framebuffer at 60 FPS:
   - `480x272` RGB565 (16-bit per pixel, Big-Endian byte order) for `S8`, `S5`, and `D2` (`KontrolScreen` `ScreenFlavor.S8` / `ScreenFlavor.D2`).
   - `320x240` RGB565 for `S4 MK3` (`ScreenFlavor.S4MK3`).
   - `128x64` 1-bit/4-bit monochrome buffers for `X1 MK3` and `Z1 MK2` OLEDs.
2. Frame updates are transmitted over a dedicated vendor-specific USB Bulk Out endpoint using rectangular region headers (`[magic: 0x8400, screen_index, x, y, width, height, payload_len]`) followed by the RGB565 pixel payload and a commit footer.

---

## 5. Linux `udev` & PipeWire Integration

On Linux:
1. **Audio Interface**: All Kontrol audio interfaces (`S4 MK3`, `S3`, `S2 MK3`, `S8`, `S5`, `Z1 MK2`) are **USB Audio Class 2.0 (UAC2) compliant** and bind natively to the Linux kernel `snd-usb-audio` driver with zero proprietary kernel modules required. PipeWire exposes them under the `pro-audio` profile for bit-perfect multi-channel routing (`Master L/R`, `Booth L/R`, `Headphone Cue L/R`, and `4x Stereo Phono/Line Inputs` for DVS/Live Input).
2. **HID & Bulk Display Access**: A single `/etc/udev/rules.d/99-retraktor-ni.rules` rule grants `uaccess` and `plugdev`/`audio` group permissions to `/dev/hidraw*` and `/dev/bus/usb/*/*` nodes matching `ATTRS{idVendor}=="17cc"`, enabling unprivileged user-space access for ReTraktor's HID and display threads.
