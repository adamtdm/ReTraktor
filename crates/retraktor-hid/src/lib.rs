//! # `retraktor-hid`
//!
//! Native Instruments Kontrol (`VID 0x17CC`) USB HID, Haptic Drive, & `OffscreenView` Display protocol layer:
//! - [`NI_DEVICES`]: Complete USB VID/PID registry for Kontrol S4 MK3, S3, S2 MK3, S8, S5, D2, X1 MK3, Z1 MK2, and F1.
//! - [`HidReportDecoder`]: Decodes Report `0x01` (Buttons, Encoders, Jog Wheels) and Report `0x02`
//!   (12-bit/16-bit ADC Faders/Pots) into [`retraktor_core::SurfaceEvent`]s.
//! - [`S4Mk3HapticPlatterConfig`]: Encodes Report `0x30`/`0x31` for S4 MK3 Haptic Drive motorized platters
//!   (`JogwheelMode`, platter speed `33.3`/`45` RPM, tension `0..=255`, haptic tick density `0..=2`, LED ring color).
//! - [`KontrolScreenFramebuffer`]: Renders 3-band [`WaveformStripe`] + playhead + hotcues into an offscreen
//!   RGB565 framebuffer (`480x272` for S8/S5/D2 `OffscreenView.qml`, `320x240` for S4 MK3) and packetizes
//!   dirty rectangles into USB Bulk frames (`0x8400` header).
//! - [`AlsaMidiTsiRouter`]: Routes raw 3-byte MIDI messages through a [`TsiFile`] into [`PropertyBus`].
//! - [`generate_linux_udev_rules`]: Generates `/etc/udev/rules.d/99-retraktor-ni.rules`.

use retraktor_core::{JogwheelMode, PropValue, PropertyBus, ScreenFlavor, SurfaceEvent, TraktorColor};
use retraktor_formats::{TsiFile, WaveformStripe};

/// Native Instruments GmbH USB Vendor ID.
pub const NI_USB_VID: u16 = 0x17CC;

/// Supported Native Instruments Kontrol hardware models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NiDeviceDescriptor {
    pub model: &'static str,
    pub vid: u16,
    pub pid: u16,
    pub csi_folder: &'static str,
    pub has_color_screen: bool,
    pub has_motorized_jog: bool,
}

/// Registry of Native Instruments Traktor Kontrol hardware USB IDs.
pub const NI_DEVICES: [NiDeviceDescriptor; 9] = [
    NiDeviceDescriptor {
        model: "Traktor Kontrol S4 MK3",
        vid: NI_USB_VID,
        pid: 0x1720,
        csi_folder: "S4MK3",
        has_color_screen: true,
        has_motorized_jog: true,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S3",
        vid: NI_USB_VID,
        pid: 0x1900,
        csi_folder: "S3",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S2 MK3",
        vid: NI_USB_VID,
        pid: 0x1730,
        csi_folder: "S2MK3",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S8",
        vid: NI_USB_VID,
        pid: 0x1370,
        csi_folder: "S8",
        has_color_screen: true,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol S5",
        vid: NI_USB_VID,
        pid: 0x1420,
        csi_folder: "S5",
        has_color_screen: true,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol D2",
        vid: NI_USB_VID,
        pid: 0x1400,
        csi_folder: "D2",
        has_color_screen: true,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol X1 MK3",
        vid: NI_USB_VID,
        pid: 0x1CBC,
        csi_folder: "X1MK3",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol Z1 MK2",
        vid: NI_USB_VID,
        pid: 0x2120,
        csi_folder: "Z1MK2",
        has_color_screen: false,
        has_motorized_jog: false,
    },
    NiDeviceDescriptor {
        model: "Traktor Kontrol F1",
        vid: NI_USB_VID,
        pid: 0x1120,
        csi_folder: "F1",
        has_color_screen: false,
        has_motorized_jog: false,
    },
];

/// Looks up a Native Instruments controller descriptor by `(vid, pid)`.
pub fn lookup_ni_device(vid: u16, pid: u16) -> Option<&'static NiDeviceDescriptor> {
    NI_DEVICES.iter().find(|d| d.vid == vid && d.pid == pid)
}

/// Stateful NI HID input report decoder that detects button edges and ADC fader deltas.
#[derive(Debug, Clone)]
pub struct HidReportDecoder {
    pub surface_prefix: String,
    prev_buttons: Vec<u8>,
    prev_adc: Vec<u16>,
}

impl HidReportDecoder {
    pub fn new(surface_prefix: impl Into<String>, button_bytes: usize, adc_channels: usize) -> Self {
        Self {
            surface_prefix: surface_prefix.into(),
            prev_buttons: vec![0; button_bytes],
            prev_adc: vec![0; adc_channels],
        }
    }

    /// Decodes a raw NI HID input report (`0x01` buttons or `0x02` 12-bit Little-Endian ADCs)
    /// into [`SurfaceEvent`]s.
    pub fn decode_report(&mut self, report: &[u8]) -> Vec<SurfaceEvent> {
        let mut events = Vec::new();
        if report.is_empty() {
            return events;
        }
        match report[0] {
            0x01 => {
                let payload = &report[1..];
                for (byte_idx, &b) in payload.iter().enumerate() {
                    let prev = self.prev_buttons.get(byte_idx).copied().unwrap_or(0);
                    let diff = b ^ prev;
                    if diff != 0 {
                        for bit in 0..8 {
                            if (diff & (1 << bit)) != 0 {
                                let pressed = (b & (1 << bit)) != 0;
                                let btn_id = byte_idx * 8 + bit + 1;
                                events.push(SurfaceEvent::Button {
                                    control: format!("{}.buttons.{btn_id}", self.surface_prefix),
                                    pressed,
                                });
                            }
                        }
                        if byte_idx < self.prev_buttons.len() {
                            self.prev_buttons[byte_idx] = b;
                        }
                    }
                }
            }
            0x02 => {
                let payload = &report[1..];
                for (ch_idx, pair) in payload.chunks_exact(2).enumerate() {
                    let raw = u16::from_le_bytes([pair[0], pair[1]]) & 0x0FFF;
                    let prev = self.prev_adc.get(ch_idx).copied().unwrap_or(u16::MAX);
                    if raw != prev {
                        let norm = (raw as f64) / 4095.0;
                        events.push(SurfaceEvent::Fader {
                            control: format!("{}.faders.{}", self.surface_prefix, ch_idx + 1),
                            value: norm,
                        });
                        if ch_idx < self.prev_adc.len() {
                            self.prev_adc[ch_idx] = raw;
                        }
                    }
                }
            }
            _ => {}
        }
        events
    }
}

/// S4 MK3 Haptic Drive Motorized Platter Report (`0x30` / `0x31`) matching `CSI/S4MK3/S4MK3.qml` & `S4MK3Deck.qml`.
#[derive(Debug, Clone, PartialEq)]
pub struct S4Mk3HapticPlatterConfig {
    pub deck_side: u8,
    pub mode: JogwheelMode,
    pub motor_on: bool,
    pub platter_speed_45rpm: bool,
    pub tension: u8,
    pub ticks_density: u8,
    pub tempo_ratio: f32,
    pub led_color: TraktorColor,
}

impl Default for S4Mk3HapticPlatterConfig {
    fn default() -> Self {
        Self {
            deck_side: 0,
            mode: JogwheelMode::Jogwheel,
            motor_on: false,
            platter_speed_45rpm: false,
            tension: 50,
            ticks_density: 0,
            tempo_ratio: 1.0,
            led_color: TraktorColor::Blue,
        }
    }
}

impl S4Mk3HapticPlatterConfig {
    /// Packs the S4 MK3 Haptic Drive configuration into a 16-byte HID Output Report (`0x30`).
    pub fn to_hid_report(&self) -> [u8; 16] {
        let mut r = [0u8; 16];
        r[0] = 0x30;
        r[1] = self.deck_side;
        r[2] = self.mode as u8;
        r[3] = u8::from(self.motor_on);
        r[4] = u8::from(self.platter_speed_45rpm);
        r[5] = self.tension;
        r[6] = self.ticks_density.min(2);
        let rpm_base: f32 = if self.platter_speed_45rpm { 45.0 } else { 33.3333 };
        let effective_rpm = rpm_base * self.tempo_ratio;
        let rpm_bytes = effective_rpm.to_le_bytes();
        r[7..11].copy_from_slice(&rpm_bytes);
        let (red, green, blue) = self.led_color.to_rgb8();
        r[11] = red >> 1;
        r[12] = green >> 1;
        r[13] = blue >> 1;
        r[14] = self.led_color as u8;
        r
    }
}

/// Offscreen RGB565 framebuffer matching `Resources/qml/Screens/OffscreenView.qml`
/// (`480x272` for S8/S5/D2, `320x240` for S4 MK3) with USB Bulk packet framing (`0x8400`).
#[derive(Debug, Clone)]
pub struct KontrolScreenFramebuffer {
    pub flavor: ScreenFlavor,
    pub screen_index: u8,
    pub width: u16,
    pub height: u16,
    pub pixels_rgb565: Vec<u16>,
}

impl KontrolScreenFramebuffer {
    pub fn new(flavor: ScreenFlavor, screen_index: u8) -> Self {
        let (width, height) = match flavor {
            ScreenFlavor::S8 | ScreenFlavor::D2 => (480, 272),
            ScreenFlavor::S4Mk3 => (320, 240),
            ScreenFlavor::X1Mk3Mode | ScreenFlavor::Z1Mk2Main => (128, 64),
        };
        Self {
            flavor,
            screen_index,
            width,
            height,
            pixels_rgb565: vec![0; (width as usize) * (height as usize)],
        }
    }

    #[inline]
    pub fn rgb8_to_rgb565(r: u8, g: u8, b: u8) -> u16 {
        let r5 = ((r as u16) >> 3) & 0x1F;
        let g6 = ((g as u16) >> 2) & 0x3F;
        let b5 = ((b as u16) >> 3) & 0x1F;
        (r5 << 11) | (g6 << 5) | b5
    }

    pub fn set_pixel(&mut self, x: u16, y: u16, rgb565: u16) {
        if x < self.width && y < self.height {
            let idx = (y as usize) * (self.width as usize) + (x as usize);
            self.pixels_rgb565[idx] = rgb565;
        }
    }

    /// Renders a Deck HUD + 3-band [`WaveformStripe`] + Playhead cursor into the offscreen framebuffer.
    pub fn render_deck_waveform(
        &mut self,
        deck_idx: usize,
        stripe: &WaveformStripe,
        playhead_norm: f32,
    ) {
        // Clear background to deep charcoal
        let bg = Self::rgb8_to_rgb565(12, 14, 18);
        self.pixels_rgb565.fill(bg);

        // Top deck header bar (Blue for Decks 1/2, Orange for Decks 3/4)
        let header_color = if deck_idx <= 2 {
            TraktorColor::Blue
        } else {
            TraktorColor::LightOrange
        };
        let (hr, hg, hb) = header_color.to_rgb8();
        let header_px = Self::rgb8_to_rgb565(hr, hg, hb);
        let header_h = (self.height / 10).max(4);
        for y in 0..header_h {
            for x in 0..self.width {
                self.set_pixel(x, y, header_px);
            }
        }

        // Render waveform columns centered vertically
        if !stripe.columns.is_empty() {
            let center_y = (self.height / 2) as i32;
            let max_half_h = ((self.height as i32) * 3 / 8).max(8);
            for x in 0..self.width {
                let col_idx =
                    ((x as usize) * stripe.columns.len()) / (self.width as usize).max(1);
                let col = stripe.columns[col_idx.min(stripe.columns.len() - 1)];
                let amp = (col.low as u16 + col.mid as u16 + col.high as u16) / 3;
                let bar_h = ((amp as i32) * max_half_h) / 255;
                let wf_px = Self::rgb8_to_rgb565(col.low, col.mid, col.high);
                for dy in -bar_h..=bar_h {
                    let y = (center_y + dy).clamp(header_h as i32, (self.height - 1) as i32) as u16;
                    self.set_pixel(x, y, wf_px);
                }
            }
        }

        // Draw playhead vertical red/white cursor
        let play_x = ((playhead_norm.clamp(0.0, 1.0) * (self.width.saturating_sub(1) as f32))
            .round() as u16)
            .min(self.width.saturating_sub(1));
        let cursor_px = Self::rgb8_to_rgb565(255, 255, 255);
        for y in header_h..self.height {
            self.set_pixel(play_x, y, cursor_px);
        }
    }

    /// Encodes a rectangular region `(x, y, w, h)` of the framebuffer into a NI USB Bulk Display
    /// packet (`[0x84, 0x00, screen_index, 0x00, x_be, y_be, w_be, h_be, ...rgb565_be_payload]`).
    pub fn encode_usb_bulk_rect(&self, x: u16, y: u16, w: u16, h: u16) -> Vec<u8> {
        let w_clamped = w.min(self.width.saturating_sub(x));
        let h_clamped = h.min(self.height.saturating_sub(y));
        let pixel_count = (w_clamped as usize) * (h_clamped as usize);
        let mut pkt = Vec::with_capacity(12 + pixel_count * 2);
        pkt.extend_from_slice(&[0x84, 0x00, self.screen_index, 0x00]);
        pkt.extend_from_slice(&x.to_be_bytes());
        pkt.extend_from_slice(&y.to_be_bytes());
        pkt.extend_from_slice(&w_clamped.to_be_bytes());
        pkt.extend_from_slice(&h_clamped.to_be_bytes());

        for row in y..(y + h_clamped) {
            let row_start = (row as usize) * (self.width as usize) + (x as usize);
            for &px in &self.pixels_rgb565[row_start..row_start + (w_clamped as usize)] {
                pkt.extend_from_slice(&px.to_be_bytes());
            }
        }
        pkt
    }
}

/// Evaluates raw 3-byte ALSA MIDI messages against a loaded `.tsi` [`TsiFile`] and updates [`PropertyBus`].
pub struct AlsaMidiTsiRouter;

impl AlsaMidiTsiRouter {
    /// Routes a 3-byte MIDI message (`status`, `data1`, `data2`) through `tsi` into `bus`,
    /// returning the number of property updates applied.
    pub fn route_midi_message(midi: [u8; 3], tsi: &TsiFile, bus: &mut PropertyBus) -> usize {
        let msg_type = midi[0] & 0xF0;
        let binding_id = midi[1] as u32;
        let val_norm = (midi[2] as f64) / 127.0;
        let mut updates = 0;

        for dev in &tsi.devices {
            for m in &dev.mappings {
                if m.is_output_led || m.midi_binding_id != binding_id {
                    continue;
                }
                let deck = m.target_assignment.clamp(1, 4);
                match (m.traktor_control_id, msg_type) {
                    // 102 = Deck Play/Pause
                    (102, 0x90) if midi[2] > 0 => {
                        let path = format!("app.traktor.decks.{deck}.play");
                        let cur = bus.get(&path).and_then(|v| v.as_bool()).unwrap_or(false);
                        if bus.set(&path, PropValue::Bool(!cur)) {
                            updates += 1;
                        }
                    }
                    // 201 = Channel Volume CC
                    (201, 0xB0) => {
                        let path = format!("app.traktor.mixer.channels.{deck}.volume");
                        if bus.set(&path, PropValue::Float(val_norm)) {
                            updates += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
        updates
    }
}

/// Generates the Linux `udev` rules file (`/etc/udev/rules.d/99-retraktor-ni.rules`)
/// granting unprivileged user-space access (`TAG+="uaccess"`) to all NI Kontrol HID & USB display endpoints.
pub fn generate_linux_udev_rules() -> String {
    let mut rules = String::new();
    rules.push_str("# /etc/udev/rules.d/99-retraktor-ni.rules\n");
    rules.push_str("# Unprivileged hidraw and USB bulk display access for Native Instruments Traktor controllers\n\n");
    for dev in &NI_DEVICES {
        rules.push_str(&format!("# {}\n", dev.model));
        rules.push_str(&format!(
            "SUBSYSTEM==\"hidraw\", ATTRS{{idVendor}}==\"{:04x}\", ATTRS{{idProduct}}==\"{:04x}\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n",
            dev.vid, dev.pid
        ));
        rules.push_str(&format!(
            "SUBSYSTEM==\"usb\", ATTR{{idVendor}}==\"{:04x}\", ATTR{{idProduct}}==\"{:04x}\", MODE=\"0660\", GROUP=\"plugdev\", TAG+=\"uaccess\"\n\n",
            dev.vid, dev.pid
        ));
    }
    rules
}

#[cfg(test)]
mod tests {
    use super::*;
    use retraktor_formats::{StripeColumn, TsiControlMapping, TsiDevice};

    #[test]
    fn test_hid_haptic_screen_and_midi_tsi() {
        assert_eq!(
            lookup_ni_device(0x17CC, 0x1720).map(|d| d.model),
            Some("Traktor Kontrol S4 MK3")
        );

        // S4 MK3 Haptic Platter report
        let platter = S4Mk3HapticPlatterConfig {
            deck_side: 0,
            mode: JogwheelMode::Turntable,
            motor_on: true,
            platter_speed_45rpm: false,
            tension: 80,
            ticks_density: 1,
            tempo_ratio: 1.0,
            led_color: TraktorColor::Blue,
        };
        let report = platter.to_hid_report();
        assert_eq!(report[0], 0x30);
        assert_eq!(report[3], 1); // motor_on

        // OffscreenView 480x272 RGB565 waveform rendering & USB Bulk packetization
        let mut fb = KontrolScreenFramebuffer::new(ScreenFlavor::S8, 0);
        let stripe = WaveformStripe {
            header_tag: 0x06,
            version: 2,
            scale_factor: 3.5,
            block_size: 1024,
            block_count: 64,
            populated_columns: 64,
            duration_sec: 180.0,
            columns: vec![
                StripeColumn {
                    low: 200,
                    mid: 120,
                    high: 80,
                    total: 133,
                };
                64
            ],
            transients_sec: vec![0.0, 0.5, 1.0],
        };
        fb.render_deck_waveform(1, &stripe, 0.25);
        let bulk_pkt = fb.encode_usb_bulk_rect(0, 0, 480, 16);
        assert_eq!(&bulk_pkt[0..2], &[0x84, 0x00]);
        assert_eq!(bulk_pkt.len(), 12 + 480 * 16 * 2);

        // ALSA MIDI .tsi router test
        let tsi = TsiFile::from_devices(vec![TsiDevice {
            device_name: "Generic MIDI".to_string(),
            comment: None,
            in_port: None,
            out_port: None,
            mappings: vec![TsiControlMapping {
                midi_binding_id: 60, // Note C4
                is_output_led: false,
                traktor_control_id: 102, // Deck Play
                target_assignment: 1,
            }],
        }]);
        let mut bus = PropertyBus::new_traktor_pro4_default();
        let writes = AlsaMidiTsiRouter::route_midi_message([0x90, 60, 127], &tsi, &mut bus);
        assert_eq!(writes, 1);
        assert_eq!(
            bus.get("app.traktor.decks.1.play"),
            Some(&PropValue::Bool(true))
        );
    }
}
