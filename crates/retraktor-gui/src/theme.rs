use eframe::egui::Color32;

/// Authentic Traktor Pro 4 Matte Charcoal & Toned-Down Spectrum Palette
pub struct TraktorTheme;

impl TraktorTheme {
    // Matte Charcoal & Slate Surfaces (Toned down to match Traktor Pro 4)
    pub const BG_WINDOW: Color32 = Color32::from_rgb(33, 35, 38);
    pub const BG_PANEL: Color32 = Color32::from_rgb(43, 45, 49);
    pub const BG_PANEL_ALT: Color32 = Color32::from_rgb(38, 40, 44);
    pub const BG_SECTION: Color32 = Color32::from_rgb(54, 57, 62);
    pub const BG_INPUT: Color32 = Color32::from_rgb(28, 30, 33);
    pub const BORDER_COLOR: Color32 = Color32::from_rgb(24, 25, 28);
    pub const BORDER_ACCENT: Color32 = Color32::from_rgb(62, 65, 71);
    pub const ROW_SELECTED: Color32 = Color32::from_rgb(46, 82, 128);

    // Typography Colors
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(232, 234, 238);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(156, 162, 170);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(106, 111, 120);
    pub const TEXT_HIGHLIGHT: Color32 = Color32::from_rgb(248, 249, 250);

    // Deck & Control Accents (Toned-down Steel Blue & Warm Amber from Traktor Pro 4)
    pub const DECK_A: Color32 = Color32::from_rgb(84, 164, 214); // Muted Traktor Steel-Sky Blue
    pub const DECK_A_MUTED: Color32 = Color32::from_rgb(44, 78, 102);
    pub const DECK_B: Color32 = Color32::from_rgb(214, 124, 46); // Muted Traktor Warm Amber
    pub const DECK_B_MUTED: Color32 = Color32::from_rgb(98, 60, 28);
    pub const DECK_C: Color32 = Color32::from_rgb(142, 116, 198); // Soft Slate Violet
    pub const DECK_C_MUTED: Color32 = Color32::from_rgb(66, 54, 92);
    pub const DECK_D: Color32 = Color32::from_rgb(92, 176, 148); // Soft Sage Teal
    pub const DECK_D_MUTED: Color32 = Color32::from_rgb(45, 84, 72);

    // Transport & Performance Buttons (Authentic Traktor Sage-Lime Play & Steel-Blue Sync)
    pub const PLAY_ACTIVE: Color32 = Color32::from_rgb(184, 208, 92); // Traktor Signature Sage-Lime
    pub const PLAY_INACTIVE: Color32 = Color32::from_rgb(56, 59, 65);
    pub const CUE_ACTIVE: Color32 = Color32::from_rgb(84, 164, 214);
    pub const CUE_INACTIVE: Color32 = Color32::from_rgb(56, 59, 65);
    pub const SYNC_ACTIVE: Color32 = Color32::from_rgb(84, 164, 214);
    pub const FLUX_ACTIVE: Color32 = Color32::from_rgb(214, 124, 46);
    pub const LOOP_ACTIVE: Color32 = Color32::from_rgb(114, 182, 102);
    pub const KEYLOCK_ACTIVE: Color32 = Color32::from_rgb(208, 92, 92);

    // Metering (Subtle LEDs like Traktor Pro 4 Header/Channel Meters)
    pub const METER_BG: Color32 = Color32::from_rgb(26, 28, 31);
    pub const METER_GREEN: Color32 = Color32::from_rgb(84, 164, 214); // Traktor uses cool blue-cyan for normal meter
    pub const METER_YELLOW: Color32 = Color32::from_rgb(218, 158, 52);
    pub const METER_RED: Color32 = Color32::from_rgb(212, 74, 68);

    // Waveform Bands (Traktor Pro 4 "Spectrum" Translucent Multi-Band Palette)
    pub const WAVEFORM_BG: Color32 = Color32::from_rgb(28, 30, 33);
    pub const WAVEFORM_LOW: Color32 = Color32::from_rgb(216, 96, 56); // Warm Coral-Orange Bass
    pub const WAVEFORM_MID: Color32 = Color32::from_rgb(188, 176, 68); // Warm Olive-Gold Mids
    pub const WAVEFORM_HIGH: Color32 = Color32::from_rgb(108, 148, 214); // Soft Periwinkle-Blue Highs
    pub const WAVEFORM_GRID: Color32 = Color32::from_rgba_premultiplied(210, 215, 222, 65);
    pub const WAVEFORM_PLAYHEAD: Color32 = Color32::from_rgb(212, 74, 68); // Traktor Red Playhead Line

    // Hotcue Colors (Toned down for subtle indicator bars)
    pub const HOTCUE_GRID: Color32 = Color32::from_rgb(220, 224, 230); // White Grid
    pub const HOTCUE_CUE: Color32 = Color32::from_rgb(84, 164, 214); // Steel Blue Cue
    pub const HOTCUE_LOAD: Color32 = Color32::from_rgb(214, 174, 64); // Muted Gold Load
    pub const HOTCUE_FADEIN: Color32 = Color32::from_rgb(214, 124, 46); // Muted Amber
    pub const HOTCUE_FADEOUT: Color32 = Color32::from_rgb(208, 84, 78); // Soft Red
    pub const HOTCUE_LOOP: Color32 = Color32::from_rgb(104, 182, 108); // Soft Green Loop
    pub const HOTCUE_GENERIC: Color32 = Color32::from_rgb(148, 118, 206); // Soft Violet
    pub const HOTCUE_EMPTY: Color32 = Color32::from_rgb(52, 55, 60);

    pub fn deck_color(deck_idx: usize) -> Color32 {
        match deck_idx {
            1 => Self::DECK_A,
            2 => Self::DECK_B,
            3 => Self::DECK_C,
            4 => Self::DECK_D,
            _ => Self::DECK_A,
        }
    }

    pub fn hotcue_color(cue_idx: usize) -> Color32 {
        match cue_idx {
            1 => Self::HOTCUE_CUE,
            2 => Self::HOTCUE_CUE,
            3 => Self::HOTCUE_LOAD,
            4 => Self::HOTCUE_LOOP,
            5 => Self::HOTCUE_FADEIN,
            6 => Self::HOTCUE_GENERIC,
            7 => Self::HOTCUE_FADEOUT,
            _ => Self::HOTCUE_CUE,
        }
    }
}
