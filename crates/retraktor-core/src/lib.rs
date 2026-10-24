//! # `retraktor-core`
//!
//! Core state engine for **ReTraktor** (clean-room Linux port of Traktor Pro 4):
//! - [`enums`]: Exact QML `Defines 1.0` singleton enumerations (`PadsMode`, `Overlay`, `FooterPage`,
//!   `FxOverlay`, `StemStyle`, `ScreenView`, `NavigationMode`, `JumpSize`, `LoopSize`, `MoveSize`,
//!   `DeckType`, `CueType`, `JogwheelMode`, `ScreenFlavor`, `TraktorColor`, `DeckHelpers`).
//! - [`property_bus`]: Hierarchical, observable `app.traktor.*` property tree backing both UI and
//!   controller scripts.
//! - [`csi_graph`]: `CSI 1.0` Controller Script Interface reactive `Wire` & `WiresGroup` evaluation
//!   and QML CSI parser/inspector.

pub mod csi_graph;
pub mod enums;
pub mod property_bus;

pub use csi_graph::{
    AdapterKind, CsiDirectoryReport, CsiModuleSummary, CsiWire, CsiWiresGroup, LogicGateKind,
    SurfaceEvent, WireEndpoint,
};
pub use enums::{
    CueType, DeckHelpers, DeckType, DecksAssignment, FooterPage, FxOverlay, JogwheelMode, JumpSize,
    LoopSize, MoveSize, NavigationMode, Overlay, PadsMode, ScreenFlavor, ScreenSide, ScreenView,
    StemStyle, TraktorColor,
};
pub use property_bus::{PropMeta, PropValue, PropertyBus};
