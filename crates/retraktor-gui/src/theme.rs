use eframe::egui::Color32;

/// Traktor Pro 4 Signature Color Palette
pub struct TraktorTheme;

impl TraktorTheme {
    // Window & Panels
    pub const BG_WINDOW: Color32 = Color32::from_rgb(14, 15, 18);
    pub const BG_PANEL: Color32 = Color32::from_rgb(24, 25, 29);
    pub const BG_PANEL_ALT: Color32 = Color32::from_rgb(20, 21, 25);
    pub const BG_SECTION: Color32 = Color32::from_rgb(32, 34, 40);
    pub const BG_INPUT: Color32 = Color32::from_rgb(18, 19, 23);
    pub const BORDER_COLOR: Color32 = Color32::from_rgb(46, 50, 60);
    pub const BORDER_ACCENT: Color32 = Color32::from_rgb(70, 75, 90);

    // Text & Labels
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(245, 245, 250);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 165, 175);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(105, 110, 122);
    pub const TEXT_HIGHLIGHT: Color32 = Color32::from_rgb(255, 255, 255);

    // Deck Identity Colors (Authentic Traktor Pro 4)
    pub const DECK_A: Color32 = Color32::from_rgb(0, 175, 240); // Cyan / Electric Blue
    pub const DECK_A_MUTED: Color32 = Color32::from_rgb(0, 75, 105);
    pub const DECK_B: Color32 = Color32::from_rgb(255, 115, 20); // Warm Tangerine / Orange
    pub const DECK_B_MUTED: Color32 = Color32::from_rgb(115, 50, 10);
    pub const DECK_C: Color32 = Color32::from_rgb(165, 80, 245); // Lavender / Purple
    pub const DECK_C_MUTED: Color32 = Color32::from_rgb(70, 35, 110);
    pub const DECK_D: Color32 = Color32::from_rgb(0, 225, 160); // Mint Emerald
    pub const DECK_D_MUTED: Color32 = Color32::from_rgb(0, 95, 70);

    // Transport & Performance Buttons
    pub const PLAY_ACTIVE: Color32 = Color32::from_rgb(0, 230, 118); // Emerald Neon
    pub const PLAY_INACTIVE: Color32 = Color32::from_rgb(35, 55, 42);
    pub const CUE_ACTIVE: Color32 = Color32::from_rgb(0, 210, 255);
    pub const CUE_INACTIVE: Color32 = Color32::from_rgb(30, 48, 58);
    pub const SYNC_ACTIVE: Color32 = Color32::from_rgb(0, 229, 255);
    pub const FLUX_ACTIVE: Color32 = Color32::from_rgb(255, 145, 0);
    pub const LOOP_ACTIVE: Color32 = Color32::from_rgb(118, 255, 3);
    pub const KEYLOCK_ACTIVE: Color32 = Color32::from_rgb(255, 64, 129);

    // Metering
    pub const METER_BG: Color32 = Color32::from_rgb(15, 16, 18);
    pub const METER_GREEN: Color32 = Color32::from_rgb(0, 230, 118);
    pub const METER_YELLOW: Color32 = Color32::from_rgb(255, 214, 0);
    pub const METER_RED: Color32 = Color32::from_rgb(255, 23, 68);

    // Waveform Bands (Traktor Pro 4 3-Band Spectrum)
    pub const WAVEFORM_BG: Color32 = Color32::from_rgb(12, 13, 16);
    pub const WAVEFORM_LOW: Color32 = Color32::from_rgb(235, 60, 60); // Red / Bass
    pub const WAVEFORM_MID: Color32 = Color32::from_rgb(255, 180, 40); // Amber / Vocals
    pub const WAVEFORM_HIGH: Color32 = Color32::from_rgb(0, 225, 255); // Cyan / Hats
    pub const WAVEFORM_GRID: Color32 = Color32::from_rgba_premultiplied(255, 255, 255, 70);
    pub const WAVEFORM_PLAYHEAD: Color32 = Color32::from_rgb(255, 255, 255);

    // Hotcue Pad Colors
    pub const HOTCUE_GRID: Color32 = Color32::from_rgb(41, 121, 255); // 1: Blue Grid
    pub const HOTCUE_CUE: Color32 = Color32::from_rgb(255, 145, 0); // 2: Orange Cue
    pub const HOTCUE_LOAD: Color32 = Color32::from_rgb(255, 234, 0); // 3: Yellow Load
    pub const HOTCUE_FADEIN: Color32 = Color32::from_rgb(0, 230, 118); // 4: Green Fade In
    pub const HOTCUE_FADEOUT: Color32 = Color32::from_rgb(255, 23, 68); // 5: Red Fade Out
    pub const HOTCUE_LOOP: Color32 = Color32::from_rgb(0, 229, 255); // 6: Cyan Loop
    pub const HOTCUE_GENERIC: Color32 = Color32::from_rgb(170, 0, 255); // 7: Purple
    pub const HOTCUE_EMPTY: Color32 = Color32::from_rgb(45, 48, 56);

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
            1 => Self::HOTCUE_GRID,
            2 => Self::HOTCUE_CUE,
            3 => Self::HOTCUE_LOAD,
            4 => Self::HOTCUE_FADEIN,
            5 => Self::HOTCUE_FADEOUT,
            6 => Self::HOTCUE_LOOP,
            7 => Self::HOTCUE_GENERIC,
            _ => Self::HOTCUE_CUE,
        }
    }
}
