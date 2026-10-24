//! Exact enumerations matching Traktor Pro 4's `Resources/qml/Defines/*.qml` singletons,
//! `CSI/Common/DeckHelpers.js`, and internal `app.traktor.*` integer property constants.

/// Active deck flavor (`app.traktor.decks.<N>.type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum DeckType {
    Track = 0,
    Remix = 1,
    Stem = 2,
    Live = 3,
}

impl DeckType {
    pub fn from_i64(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::Track),
            1 => Some(Self::Remix),
            2 => Some(Self::Stem),
            3 => Some(Self::Live),
            _ => None,
        }
    }
}

/// Cue marker type (`CUE_V2/@TYPE` and `app.traktor.decks.<N>.track.cue.active.type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum CueType {
    Cue = 0,
    FadeIn = 1,
    FadeOut = 2,
    Load = 3,
    Grid = 4,
    Loop = 5,
}

impl CueType {
    pub fn from_i64(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::Cue),
            1 => Some(Self::FadeIn),
            2 => Some(Self::FadeOut),
            3 => Some(Self::Load),
            4 => Some(Self::Grid),
            5 => Some(Self::Loop),
            _ => None,
        }
    }
}

/// Matches `Resources/qml/Defines/PadsMode.qml` (`0..=5`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum PadsMode {
    Disabled = 0,
    Hotcues = 1,
    Remix = 2,
    Stems = 3,
    Freeze = 4,
    Loop = 5,
}

impl PadsMode {
    pub fn from_i64(v: i64) -> Option<Self> {
        match v {
            0 => Some(Self::Disabled),
            1 => Some(Self::Hotcues),
            2 => Some(Self::Remix),
            3 => Some(Self::Stems),
            4 => Some(Self::Freeze),
            5 => Some(Self::Loop),
            _ => None,
        }
    }

    /// Exact implementation of `PadsMode.isPadsModeSupported(padMode, deckType)` in `PadsMode.qml`.
    pub fn is_supported(self, deck_type: DeckType) -> bool {
        match self {
            Self::Disabled => deck_type == DeckType::Live,
            Self::Hotcues => deck_type == DeckType::Stem || deck_type == DeckType::Track,
            Self::Remix => deck_type == DeckType::Remix,
            Self::Stems => deck_type == DeckType::Stem,
            Self::Freeze | Self::Loop => deck_type != DeckType::Live,
        }
    }

    /// Exact implementation of `PadsMode.defaultPadsModeForDeck(deckType)` in `PadsMode.qml`.
    pub fn default_for_deck(deck_type: DeckType) -> Self {
        match deck_type {
            DeckType::Stem => Self::Stems,
            DeckType::Remix => Self::Remix,
            DeckType::Track => Self::Hotcues,
            DeckType::Live => Self::Disabled,
        }
    }
}

/// Matches `Resources/qml/Defines/Overlay.qml` (`0..=9`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum Overlay {
    None = 0,
    Bpm = 1,
    Key = 2,
    Fx = 3,
    Quantize = 4,
    Slice = 5,
    Sorting = 6,
    Capture = 7,
    BrowserWarnings = 8,
    Swing = 9,
}

/// Matches `Resources/qml/Defines/FooterPage.qml` (`0..=10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum FooterPage {
    Empty = 0,
    Filter = 1,
    Pitch = 2,
    FxSend = 3,
    Fx = 4,
    Midi = 5,
    Volume = 6,
    Slot1 = 7,
    Slot2 = 8,
    Slot3 = 9,
    Slot4 = 10,
}

/// Matches `Resources/qml/Defines/FxOverlay.qml` (`0..=7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum FxOverlay {
    UpperButton1 = 0,
    UpperButton2 = 1,
    UpperButton3 = 2,
    UpperButton4 = 3,
    LowerButton1 = 4,
    LowerButton2 = 5,
    LowerButton3 = 6,
    LowerButton4 = 7,
}

/// Matches `Resources/qml/Defines/StemStyle.qml` (`0..=1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum StemStyle {
    Track = 0,
    Daw = 1,
}

/// Matches `Resources/qml/Defines/ScreenView.qml` (`0..=2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum ScreenView {
    Deck = 0,
    Browser = 1,
    Settings = 2,
}

/// Matches `Resources/qml/Defines/NavigationMode.qml` (`0..=1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum NavigationMode {
    Tree = 0,
    Playlist = 1,
}

/// Matches `JogwheelMode` in `CSI/S4MK3/S4MK3Deck.qml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum JogwheelMode {
    Jogwheel = 0,
    Turntable = 1,
    Cdj = 2,
}

/// Matches `ScreenFlavor` in `KontrolScreen` QML definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum ScreenFlavor {
    S8 = 0,
    D2 = 1,
    S4Mk3 = 2,
    X1Mk3Mode = 3,
    Z1Mk2Main = 4,
}

/// Matches `ScreenSide` in `KontrolScreen`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum ScreenSide {
    Left = 0,
    Right = 1,
}

/// Matches `DecksAssignment` in `CSI/S8/S8.qml` and `CSI/Z1MK2/Z1MK2.qml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum DecksAssignment {
    AC = 0,
    BD = 1,
    AB = 2,
    CD = 3,
}

/// Matches `Color.*` palette constants in Traktor `CSI 1.0`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum TraktorColor {
    Black = 0,
    Red = 1,
    DarkOrange = 2,
    LightOrange = 3,
    WarmYellow = 4,
    Yellow = 5,
    Lime = 6,
    Green = 7,
    Mint = 8,
    Cyan = 9,
    Turquoise = 10,
    Blue = 11,
    Plum = 12,
    Violet = 13,
    Purple = 14,
    Magenta = 15,
    Fuchsia = 16,
    White = 17,
}

impl TraktorColor {
    /// Returns 8-bit `(R, G, B)` values for LED pads and screen UI rendering.
    pub fn to_rgb8(self) -> (u8, u8, u8) {
        match self {
            Self::Black => (0, 0, 0),
            Self::Red => (255, 36, 24),
            Self::DarkOrange => (255, 92, 0),
            Self::LightOrange => (255, 149, 0),
            Self::WarmYellow => (255, 195, 0),
            Self::Yellow => (255, 230, 0),
            Self::Lime => (140, 235, 0),
            Self::Green => (0, 214, 50),
            Self::Mint => (0, 224, 145),
            Self::Cyan => (0, 210, 240),
            Self::Turquoise => (0, 165, 255),
            Self::Blue => (0, 102, 255),
            Self::Plum => (98, 70, 255),
            Self::Violet => (148, 55, 255),
            Self::Purple => (195, 45, 255),
            Self::Magenta => (255, 40, 190),
            Self::Fuchsia => (255, 50, 110),
            Self::White => (255, 255, 255),
        }
    }
}

/// Matches `CSI/Common/DeckHelpers.js`.
pub struct DeckHelpers;

impl DeckHelpers {
    /// Deck 1 & 2 are Blue; Deck 3 & 4 are LightOrange.
    pub fn color_for_deck(deck_idx: usize) -> TraktorColor {
        match deck_idx {
            1 | 2 => TraktorColor::Blue,
            3 | 4 => TraktorColor::LightOrange,
            _ => TraktorColor::Black,
        }
    }

    /// Primary decks (1, 2) default to Track; Secondary decks (3, 4) default to Remix.
    pub fn default_type_for_deck(deck_idx: usize) -> DeckType {
        if deck_idx > 2 {
            DeckType::Remix
        } else {
            DeckType::Track
        }
    }

    /// Only Track and Stem decks support beatgrid adjustment.
    pub fn deck_type_supports_grid_adjust(deck_type: DeckType) -> bool {
        matches!(deck_type, DeckType::Track | DeckType::Stem)
    }

    /// Deck A (0) <-> C (2), Deck B (1) <-> D (3).
    pub fn linked_deck_idx(zero_based_idx: usize) -> usize {
        match zero_based_idx {
            0 => 2,
            2 => 0,
            1 => 3,
            3 => 1,
            other => other,
        }
    }
}

/// Matches `Resources/qml/Defines/JumpSize.qml` (`app.traktor.decks.x.beatjump`, `-13..=-1` and `1..=13`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum JumpSize {
    BwdLoop = -13,
    Bwd32 = -12,
    Bwd16 = -11,
    Bwd8 = -10,
    Bwd4 = -9,
    Bwd2 = -8,
    Bwd1 = -7,
    Bwd1_2 = -6,
    Bwd1_4 = -5,
    Bwd1_8 = -4,
    Bwd1_16 = -3,
    Bwd1_32 = -2,
    BwdUf = -1,
    FwdUf = 1,
    Fwd1_32 = 2,
    Fwd1_16 = 3,
    Fwd1_8 = 4,
    Fwd1_4 = 5,
    Fwd1_2 = 6,
    Fwd1 = 7,
    Fwd2 = 8,
    Fwd4 = 9,
    Fwd8 = 10,
    Fwd16 = 11,
    Fwd32 = 12,
    FwdLoop = 13,
}

/// Matches `Resources/qml/Defines/LoopSize.qml` (`app.traktor.decks.x.loop.size`, `0..=10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum LoopSize {
    Loop1_32 = 0,
    Loop1_16 = 1,
    Loop1_8 = 2,
    Loop1_4 = 3,
    Loop1_2 = 4,
    Loop1 = 5,
    Loop2 = 6,
    Loop4 = 7,
    Loop8 = 8,
    Loop16 = 9,
    Loop32 = 10,
}

impl LoopSize {
    pub const NAMES: [&'static str; 11] = [
        "1/32", "1/16", "1/8", "1/4", "1/2", "1", "2", "4", "8", "16", "32",
    ];

    /// Returns the length in beats for a loop size index `0..=10`.
    pub fn beats_for_index(index: usize) -> Option<f64> {
        const BEATS: [f64; 11] = [
            0.03125, 0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0,
        ];
        BEATS.get(index).copied()
    }
}

/// Matches `Resources/qml/Defines/MoveSize.qml` (`app.traktor.decks.x.move.size`, `0..=12`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(i64)]
pub enum MoveSize {
    MoveUf = 0,
    Move1_32 = 1,
    Move1_16 = 2,
    Move1_8 = 3,
    Move1_4 = 4,
    Move1_2 = 5,
    Move1 = 6,
    Move2 = 7,
    Move4 = 8,
    Move8 = 9,
    Move16 = 10,
    Move32 = 11,
    MoveLoop = 12,
}
