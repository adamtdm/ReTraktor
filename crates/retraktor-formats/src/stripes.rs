//! Binary waveform Stripe (`Stripes/000/...`) and Transient (`Transients/000/...`) cache formats
//! used by Traktor Pro 4's overview and high-resolution scrolling waveforms (`OffscreenView` & desktop decks).
//!
//! ## Binary Layout Verified Against `Traktor 4.5.1/`
//!
//! ### 1. `Stripes/<dir>/<hash>` (`262,169` bytes)
//! - Byte `0`: `header_tag: u8` (`0x06`)
//! - Bytes `1..5`: `magic: [u8; 4]` (`b"PRTS"`, i.e. `"STRP"` in Little-Endian)
//! - Bytes `5..9`: `version: u32 LE` (`2`)
//! - Bytes `9..13`: `scale_factor: f32 LE` (`3.5`)
//! - Bytes `13..17`: `block_size: u32 LE` (`1024`)
//! - Bytes `17..21`: `block_count: u32 LE` (`64`, so `1024 * 64 = 65,536` column slots)
//! - Bytes `21..25`: `populated_columns: u32 LE` (active column count for this track)
//! - Bytes `25..262169`: `65,536` 4-byte tuples `[low: u8, mid: u8, high: u8, total: u8]` (`262,144` bytes).
//!
//! ### 2. `Transients/<dir>/<hash>` (`5 + count * 16` bytes)
//! - Byte `0`: `version: u8` (`0x05`)
//! - Bytes `1..5`: `count: u32 LE`
//! - Bytes `5..`: `count` 16-byte Little-Endian records:
//!   - `position_sec: f64 LE` (onset timestamp in seconds)
//!   - `weight: f32 LE` (transient onset strength `0.0..1.0`)
//!   - `metric: f32 LE` (local BPM candidate / spectral flux metric)

/// A single 4-byte amplitude column (`[low, mid, high, total]`) in a Traktor `PRTS` (`STRP`) stripe file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StripeColumn {
    pub low: u8,
    pub mid: u8,
    pub high: u8,
    pub total: u8,
}

/// A 16-byte transient onset record inside a Traktor `Transients/` file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransientRecord {
    pub position_sec: f64,
    pub weight: f32,
    pub metric: f32,
}

/// Complete Traktor `Transients/` cache file (`version = 5`, `count: u32 LE`, `records: Vec<TransientRecord>`).
#[derive(Debug, Clone, PartialEq)]
pub struct TransientFile {
    pub version: u8,
    pub records: Vec<TransientRecord>,
}

impl TransientFile {
    /// Parses a binary Traktor `Transients/` file (`0x05` + `u32 LE count` + `16B * count`).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 5 {
            return Err("Transient file shorter than 5-byte header".to_string());
        }
        let version = bytes[0];
        let count = u32::from_le_bytes(bytes[1..5].try_into().unwrap()) as usize;
        let expected_len = 5usize
            .checked_add(count.checked_mul(16).ok_or("Transient count overflow")?)
            .ok_or("Transient size overflow")?;
        if bytes.len() < expected_len {
            return Err(format!(
                "Transient file truncated: expected {expected_len} bytes for {count} records, got {}",
                bytes.len()
            ));
        }
        let mut records = Vec::with_capacity(count);
        for i in 0..count {
            let off = 5 + i * 16;
            let position_sec = f64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
            let weight = f32::from_le_bytes(bytes[off + 8..off + 12].try_into().unwrap());
            let metric = f32::from_le_bytes(bytes[off + 12..off + 16].try_into().unwrap());
            records.push(TransientRecord {
                position_sec,
                weight,
                metric,
            });
        }
        Ok(Self { version, records })
    }

    /// Encodes this [`TransientFile`] into Traktor's exact `Transients/` binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.records.len() * 16);
        out.push(self.version);
        out.extend_from_slice(&(self.records.len() as u32).to_le_bytes());
        for rec in &self.records {
            out.extend_from_slice(&rec.position_sec.to_le_bytes());
            out.extend_from_slice(&rec.weight.to_le_bytes());
            out.extend_from_slice(&rec.metric.to_le_bytes());
        }
        out
    }
}

/// Complete waveform overview stripe (`0x06` + `b"PRTS"` header + 4-byte columns) and optional transient timestamps.
#[derive(Debug, Clone, PartialEq)]
pub struct WaveformStripe {
    pub header_tag: u8,
    pub version: u32,
    pub scale_factor: f32,
    pub block_size: u32,
    pub block_count: u32,
    pub populated_columns: u32,
    pub duration_sec: f32,
    pub columns: Vec<StripeColumn>,
    pub transients_sec: Vec<f32>,
}

impl WaveformStripe {
    pub const PRTS_MAGIC: &'static [u8; 4] = b"PRTS";

    /// Encodes this [`WaveformStripe`] into Traktor's `PRTS` (`STRP` LE) binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let total_slots = (self.block_size as usize) * (self.block_count as usize);
        let slot_count = total_slots.max(self.columns.len());
        let mut out = Vec::with_capacity(25 + slot_count * 4);
        out.push(self.header_tag);
        out.extend_from_slice(Self::PRTS_MAGIC);
        out.extend_from_slice(&self.version.to_le_bytes());
        out.extend_from_slice(&self.scale_factor.to_le_bytes());
        out.extend_from_slice(&self.block_size.to_le_bytes());
        out.extend_from_slice(&self.block_count.to_le_bytes());
        out.extend_from_slice(&self.populated_columns.to_le_bytes());
        for i in 0..slot_count {
            let col = self.columns.get(i).copied().unwrap_or_default();
            out.push(col.low);
            out.push(col.mid);
            out.push(col.high);
            out.push(col.total);
        }
        out
    }

    /// Parses a real Traktor `Stripes/` binary file (`0x06` + `b"PRTS"` Little-Endian header + 4-byte columns).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 25 || &bytes[1..5] != Self::PRTS_MAGIC {
            return Err("Invalid Traktor PRTS (STRP) stripe header".to_string());
        }
        let header_tag = bytes[0];
        let version = u32::from_le_bytes(bytes[5..9].try_into().unwrap());
        let scale_factor = f32::from_le_bytes(bytes[9..13].try_into().unwrap());
        let block_size = u32::from_le_bytes(bytes[13..17].try_into().unwrap());
        let block_count = u32::from_le_bytes(bytes[17..21].try_into().unwrap());
        let populated_columns = u32::from_le_bytes(bytes[21..25].try_into().unwrap());

        let total_slots = (block_size as usize)
            .checked_mul(block_count as usize)
            .ok_or("Stripe slot count overflow")?;
        let data_bytes = bytes.len() - 25;
        let available_slots = (data_bytes / 4).min(total_slots);
        let mut columns = Vec::with_capacity(available_slots);
        for i in 0..available_slots {
            let off = 25 + i * 4;
            columns.push(StripeColumn {
                low: bytes[off],
                mid: bytes[off + 1],
                high: bytes[off + 2],
                total: bytes[off + 3],
            });
        }

        Ok(Self {
            header_tag,
            version,
            scale_factor,
            block_size,
            block_count,
            populated_columns,
            duration_sec: (populated_columns as f32) * 0.05,
            columns,
            transients_sec: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_waveform_stripe_and_transient_roundtrip() {
        let stripe = WaveformStripe {
            header_tag: 0x06,
            version: 2,
            scale_factor: 3.5,
            block_size: 4,
            block_count: 1,
            populated_columns: 2,
            duration_sec: 0.1,
            columns: vec![
                StripeColumn {
                    low: 12,
                    mid: 4,
                    high: 2,
                    total: 26,
                },
                StripeColumn {
                    low: 19,
                    mid: 7,
                    high: 2,
                    total: 31,
                },
                StripeColumn::default(),
                StripeColumn::default(),
            ],
            transients_sec: Vec::new(),
        };
        let encoded = stripe.to_bytes();
        let decoded = WaveformStripe::from_bytes(&encoded).expect("decode PRTS stripe");
        assert_eq!(stripe, decoded);

        let tf = TransientFile {
            version: 5,
            records: vec![
                TransientRecord {
                    position_sec: 0.284444,
                    weight: 0.667,
                    metric: 174.5246,
                },
                TransientRecord {
                    position_sec: 0.397642,
                    weight: 0.205,
                    metric: 0.0,
                },
            ],
        };
        let t_bytes = tf.to_bytes();
        let t_dec = TransientFile::from_bytes(&t_bytes).expect("decode transients");
        assert_eq!(tf, t_dec);
    }

    #[test]
    fn test_real_traktor_stripes_and_transients_on_disk() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let stripe_path = PathBuf::from(&home)
            .join("Documents/Traktor 4.5.1/Stripes/104/ILFBD3BZNWLA3AMJMZPRDMDCOCRB");
        if stripe_path.exists() {
            let raw = fs::read(&stripe_path).expect("read real stripe file");
            assert_eq!(raw.len(), 262_169);
            let parsed = WaveformStripe::from_bytes(&raw).expect("parse real Traktor stripe");
            assert_eq!(parsed.header_tag, 0x06);
            assert_eq!(parsed.version, 2);
            assert_eq!(parsed.block_size, 1024);
            assert_eq!(parsed.block_count, 64);
            assert_eq!(parsed.columns.len(), 65_536);
            assert_eq!(parsed.to_bytes(), raw);
        }
    }
}
