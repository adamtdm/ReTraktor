//! `.stem.m4a` MP4 `stem` atom extractor/builder and `.trak` archive inspector.

/// Compressor parameters inside the MP4 `stem` atom's `mastering_dsp`.
#[derive(Debug, Clone, PartialEq)]
pub struct StemCompressorDsp {
    pub enabled: bool,
    pub ratio: f64,
    pub output_gain: f64,
    pub threshold: f64,
    pub attack: f64,
    pub release: f64,
    pub hp_cutoff: u32,
    pub dry_wet: u32,
}

/// Limiter parameters inside the MP4 `stem` atom's `mastering_dsp`.
#[derive(Debug, Clone, PartialEq)]
pub struct StemLimiterDsp {
    pub enabled: bool,
    pub release: f64,
    pub threshold: f64,
    pub ceiling: f64,
}

/// Combined mastering DSP block (`compressor` + `limiter`) for 4-stem summing.
#[derive(Debug, Clone, PartialEq)]
pub struct StemMasteringDsp {
    pub compressor: StemCompressorDsp,
    pub limiter: StemLimiterDsp,
}

/// Metadata for one of the 4 stem tracks (`Drums`, `Bass`, `Synths`, `Vocals`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StemSlotMeta {
    pub name: String,
    pub color: String,
}

/// Complete `stem` MP4 user-data atom metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct StemMetadata {
    pub version: u32,
    pub mastering_dsp: StemMasteringDsp,
    pub stems: [StemSlotMeta; 4],
}

impl Default for StemMetadata {
    fn default() -> Self {
        Self {
            version: 1,
            mastering_dsp: StemMasteringDsp {
                compressor: StemCompressorDsp {
                    enabled: true,
                    ratio: 2.5,
                    output_gain: 0.5,
                    threshold: -6.0,
                    attack: 0.003,
                    release: 0.08,
                    hp_cutoff: 120,
                    dry_wet: 50,
                },
                limiter: StemLimiterDsp {
                    enabled: true,
                    release: 0.05,
                    threshold: -0.3,
                    ceiling: -0.1,
                },
            },
            stems: [
                StemSlotMeta {
                    name: "Drums".to_string(),
                    color: "#FF3B30".to_string(),
                },
                StemSlotMeta {
                    name: "Bass".to_string(),
                    color: "#5856D6".to_string(),
                },
                StemSlotMeta {
                    name: "Synths".to_string(),
                    color: "#34C759".to_string(),
                },
                StemSlotMeta {
                    name: "Vocals".to_string(),
                    color: "#FF9500".to_string(),
                },
            ],
        }
    }
}

impl StemMetadata {
    /// Encodes this [`StemMetadata`] into an ISO Base Media File Format (`MP4`/`M4A`) `stem` atom
    /// (`[size: u32 BE][b"stem"][json_utf8]`).
    pub fn to_mp4_atom_bytes(&self) -> Vec<u8> {
        let json = self.to_json();
        let total_len = (8 + json.len()) as u32;
        let mut out = Vec::with_capacity(total_len as usize);
        out.extend_from_slice(&total_len.to_be_bytes());
        out.extend_from_slice(b"stem");
        out.extend_from_slice(json.as_bytes());
        out
    }

    /// Scans an MP4 / `.stem.m4a` byte stream for the `stem` atom and parses its JSON payload.
    pub fn extract_from_mp4_bytes(bytes: &[u8]) -> Result<Self, String> {
        for i in 0..bytes.len().saturating_sub(8) {
            if &bytes[i + 4..i + 8] == b"stem" {
                let atom_len =
                    u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
                        as usize;
                if atom_len >= 8 && i + atom_len <= bytes.len() {
                    let payload = std::str::from_utf8(&bytes[i + 8..i + atom_len])
                        .map_err(|e| format!("Invalid UTF-8 in stem atom: {e}"))?;
                    return Self::from_json(payload);
                }
            }
        }
        Err("No 'stem' MP4 atom found in container".to_string())
    }

    /// Serializes [`StemMetadata`] to the canonical NI Stem JSON structure.
    pub fn to_json(&self) -> String {
        let c = &self.mastering_dsp.compressor;
        let l = &self.mastering_dsp.limiter;
        format!(
            concat!(
                "{{\"version\":{},",
                "\"mastering_dsp\":{{",
                "\"compressor\":{{\"enabled\":{},\"ratio\":{:.3},\"output_gain\":{:.3},\"threshold\":{:.3},\"attack\":{:.4},\"release\":{:.4},\"hp_cutoff\":{},\"dry_wet\":{}}},",
                "\"limiter\":{{\"enabled\":{},\"release\":{:.4},\"threshold\":{:.3},\"ceiling\":{:.3}}}",
                "}},",
                "\"stems\":[",
                "{{\"name\":\"{}\",\"color\":\"{}\"}},",
                "{{\"name\":\"{}\",\"color\":\"{}\"}},",
                "{{\"name\":\"{}\",\"color\":\"{}\"}},",
                "{{\"name\":\"{}\",\"color\":\"{}\"}}",
                "]}}"
            ),
            self.version,
            c.enabled,
            c.ratio,
            c.output_gain,
            c.threshold,
            c.attack,
            c.release,
            c.hp_cutoff,
            c.dry_wet,
            l.enabled,
            l.release,
            l.threshold,
            l.ceiling,
            self.stems[0].name,
            self.stems[0].color,
            self.stems[1].name,
            self.stems[1].color,
            self.stems[2].name,
            self.stems[2].color,
            self.stems[3].name,
            self.stems[3].color,
        )
    }

    /// Parses [`StemMetadata`] from a `stem` atom JSON string.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let mut meta = Self::default();
        if let Some(v) = extract_json_number(json, "\"version\":") {
            meta.version = v as u32;
        }
        if let Some(r) = extract_json_number(json, "\"ratio\":") {
            meta.mastering_dsp.compressor.ratio = r;
        }
        if let Some(og) = extract_json_number(json, "\"output_gain\":") {
            meta.mastering_dsp.compressor.output_gain = og;
        }
        if let Some(ceil) = extract_json_number(json, "\"ceiling\":") {
            meta.mastering_dsp.limiter.ceiling = ceil;
        }

        // Extract stem names & colors in order
        let mut search_pos = json.find("\"stems\"").unwrap_or(0);
        for slot in &mut meta.stems {
            if let Some(name_idx) = json[search_pos..].find("\"name\":") {
                let abs = search_pos + name_idx + 7;
                if let Some((val, next_off)) = extract_first_quoted(&json[abs..]) {
                    slot.name = val;
                    search_pos = abs + next_off;
                }
            }
            if let Some(col_idx) = json[search_pos..].find("\"color\":") {
                let abs = search_pos + col_idx + 8;
                if let Some((val, next_off)) = extract_first_quoted(&json[abs..]) {
                    slot.color = val;
                    search_pos = abs + next_off;
                }
            }
        }
        Ok(meta)
    }
}

fn extract_json_number(s: &str, key: &str) -> Option<f64> {
    let idx = s.find(key)?;
    let rest = s[idx + key.len()..].trim_start();
    let end = rest
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-')
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok()
}

fn extract_first_quoted(s: &str) -> Option<(String, usize)> {
    let q1 = s.find('"')?;
    let rest = &s[q1 + 1..];
    let q2 = rest.find('"')?;
    Some((rest[..q2].to_string(), q1 + 1 + q2 + 1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stem_mp4_atom_roundtrip() {
        let mut meta = StemMetadata::default();
        meta.stems[2].name = "Modular Lead".to_string();
        let atom = meta.to_mp4_atom_bytes();

        // Embed inside dummy ftyp + moov + udta header
        let mut fake_m4a = Vec::new();
        fake_m4a.extend_from_slice(&[0, 0, 0, 8, b'f', b't', b'y', b'p']);
        fake_m4a.extend_from_slice(&atom);

        let extracted = StemMetadata::extract_from_mp4_bytes(&fake_m4a).expect("extract stem atom");
        assert_eq!(extracted.stems[2].name, "Modular Lead");
        assert_eq!(extracted.mastering_dsp.compressor.ratio, 2.5);
    }
}
