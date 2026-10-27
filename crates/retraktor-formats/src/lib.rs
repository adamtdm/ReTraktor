//! # `retraktor-formats`
//!
//! Clean-room parsers and serializers for Traktor Pro 4 file formats:
//! - [`nml`]: `.nml` (v19 & v20) XML music library collections, `ENTRY` track metadata,
//!   Flexible Beatgrids (`CUE_V2` + `GRID`), Hotcues, Playlists, and Smartlists, including
//!   Linux POSIX path <-> Traktor `/:` path normalization.
//! - [`tsi`]: `.tsi` `TraktorSettings` XML + Base64-encoded Big-Endian FourCC TLV (`DIOM`, `DEVS`,
//!   `DEVI`, `DDCB`, `CMAS`, `CMAI`, `CMAD`) controller mapping parser and builder.
//! - [`trak`]: `.stem.m4a` MP4 `stem` atom JSON metadata parser/serializer and `.trak` archive
//!   manifest inspector.
//! - [`stripes`]: Binary waveform Stripe (`STRP`) and Transient (`TRNS`) cache reader/writer.

pub mod nml;
pub mod stripes;
pub mod trak;
pub mod tsi;
pub mod xml;

pub use nml::{
    NmlCollection, NmlCue, NmlEntry, NmlLocation, NmlLoudness, NmlPlaylistNode, NmlTempo,
};
pub use stripes::{StripeColumn, TransientFile, TransientRecord, WaveformStripe};
pub use trak::{StemCompressorDsp, StemLimiterDsp, StemMasteringDsp, StemMetadata, StemSlotMeta};
pub use tsi::{TsiChunk, TsiControlMapping, TsiDevice, TsiFile};
