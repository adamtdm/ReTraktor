//! `.tsi` (Traktor Settings / Controller Mapping) XML + Base64 Big-Endian FourCC TLV parser and serializer.

use crate::xml::{escape, XmlDocument, XmlNode};
use std::collections::BTreeMap;

/// A generic Big-Endian FourCC TLV chunk inside `DeviceIO.Config.Controller`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsiChunk {
    pub fourcc: [u8; 4],
    pub payload: Vec<u8>,
    pub children: Vec<TsiChunk>,
}

impl TsiChunk {
    pub fn fourcc_str(&self) -> String {
        String::from_utf8_lossy(&self.fourcc).into_owned()
    }

    /// Encodes this chunk (or its `children` if non-empty) into Big-Endian FourCC `[tag:4][len:u32 BE][payload]`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let body = if !self.children.is_empty() {
            let mut b = self.payload.clone();
            for c in &self.children {
                b.extend_from_slice(&c.to_bytes());
            }
            b
        } else {
            self.payload.clone()
        };
        let mut out = Vec::with_capacity(8 + body.len());
        out.extend_from_slice(&self.fourcc);
        out.extend_from_slice(&(body.len() as u32).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    /// Parses a sequence of Big-Endian FourCC TLV chunks from `bytes`.
    pub fn parse_stream(bytes: &[u8]) -> Result<Vec<TsiChunk>, String> {
        let mut chunks = Vec::new();
        let mut offset = 0;
        while offset + 8 <= bytes.len() {
            let mut fourcc = [0u8; 4];
            fourcc.copy_from_slice(&bytes[offset..offset + 4]);
            if !fourcc.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
                return Err(format!(
                    "Invalid FourCC at offset {offset}: {:?}",
                    String::from_utf8_lossy(&fourcc)
                ));
            }
            let len = u32::from_be_bytes([
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]) as usize;
            offset += 8;
            if offset + len > bytes.len() {
                return Err(format!(
                    "TSI chunk {} length {len} exceeds remaining buffer ({})",
                    String::from_utf8_lossy(&fourcc),
                    bytes.len() - offset
                ));
            }
            let slice = &bytes[offset..offset + len];
            offset += len;

            let (prefix, children) = parse_container_children(&fourcc, slice);
            chunks.push(TsiChunk {
                fourcc,
                payload: prefix,
                children,
            });
        }
        if offset != bytes.len() {
            return Err(format!(
                "Trailing {} unparsed bytes in TSI stream",
                bytes.len() - offset
            ));
        }
        Ok(chunks)
    }
}

fn parse_container_children(fourcc: &[u8; 4], slice: &[u8]) -> (Vec<u8>, Vec<TsiChunk>) {
    match fourcc {
        // Pure 0-byte prefix container chunks: DIOM (-> DIOI + DEVS), DDAT, DDDC, DDCB
        b"DIOM" | b"DDAT" | b"DDDC" | b"DDCB" if slice.len() >= 8 => {
            if let Ok(children) = TsiChunk::parse_stream(slice) {
                if !children.is_empty() {
                    return (Vec::new(), children);
                }
            }
        }
        // 4-byte count prefix container chunks: DEVS, DDCI, DDCO, CMAS, DCBM
        b"DEVS" | b"DDCI" | b"DDCO" | b"CMAS" | b"DCBM" if slice.len() >= 12 => {
            if let Ok(children) = TsiChunk::parse_stream(&slice[4..]) {
                if !children.is_empty() {
                    return (slice[..4].to_vec(), children);
                }
            }
        }
        // DEVI has a UTF-16BE string prefix followed by child chunks (DDAT)
        b"DEVI" => {
            if let Some((_, consumed)) = decode_utf16be_string(slice) {
                if slice.len() >= consumed + 8 {
                    if let Ok(children) = TsiChunk::parse_stream(&slice[consumed..]) {
                        if !children.is_empty() {
                            return (slice[..consumed].to_vec(), children);
                        }
                    }
                }
            }
        }
        // CMAI has a 12-byte header (midi_binding_id, io_type, traktor_control_id) followed by CMAD
        b"CMAI" if slice.len() >= 20 => {
            if let Ok(children) = TsiChunk::parse_stream(&slice[12..]) {
                if !children.is_empty() {
                    return (slice[..12].to_vec(), children);
                }
            }
        }
        _ => {}
    }
    (slice.to_vec(), Vec::new())
}

/// High-level representation of a single `CMAI` + `CMAD` control mapping entry inside a `.tsi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsiControlMapping {
    pub midi_binding_id: u32,
    pub is_output_led: bool,
    pub traktor_control_id: u32,
    pub target_assignment: u32,
}

impl TsiControlMapping {
    /// Encodes a [`TsiControlMapping`] into a `CMAI` TLV chunk containing a child `CMAD` chunk.
    pub fn to_chunk(&self) -> TsiChunk {
        let mut cmad_payload = vec![0u8; 156];
        cmad_payload[0..4].copy_from_slice(&4u32.to_be_bytes());
        cmad_payload[4..8].copy_from_slice(&1u32.to_be_bytes()); // control_type
        cmad_payload[8..12]
            .copy_from_slice(&(if self.is_output_led { 8u32 } else { 1u32 }).to_be_bytes());
        cmad_payload[12..16].copy_from_slice(&self.target_assignment.to_be_bytes());
        let cmad = TsiChunk {
            fourcc: *b"CMAD",
            payload: cmad_payload,
            children: Vec::new(),
        };

        let mut cmai_prefix = Vec::with_capacity(12);
        cmai_prefix.extend_from_slice(&self.midi_binding_id.to_be_bytes());
        cmai_prefix.extend_from_slice(&(if self.is_output_led { 1u32 } else { 0u32 }).to_be_bytes());
        cmai_prefix.extend_from_slice(&self.traktor_control_id.to_be_bytes());

        TsiChunk {
            fourcc: *b"CMAI",
            payload: cmai_prefix,
            children: vec![cmad],
        }
    }

    /// Parses a `CMAI` chunk payload into a [`TsiControlMapping`].
    pub fn from_cmai_chunk(chunk: &TsiChunk) -> Option<Self> {
        if &chunk.fourcc != b"CMAI" || chunk.payload.len() < 12 {
            return None;
        }
        let midi_binding_id = u32::from_be_bytes(chunk.payload[0..4].try_into().ok()?);
        let interaction = u32::from_be_bytes(chunk.payload[4..8].try_into().ok()?);
        let traktor_control_id = u32::from_be_bytes(chunk.payload[8..12].try_into().ok()?);
        let mut target_assignment = 0;

        if let Some(cmad) = chunk.children.iter().find(|c| &c.fourcc == b"CMAD") {
            if cmad.payload.len() >= 16 {
                target_assignment = u32::from_be_bytes(cmad.payload[12..16].try_into().ok()?);
            }
        }

        Some(Self {
            midi_binding_id,
            is_output_led: interaction == 1,
            traktor_control_id,
            target_assignment,
        })
    }
}

/// High-level representation of a `DEVI` device target inside a `.tsi`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsiDevice {
    pub device_name: String,
    pub comment: Option<String>,
    pub in_port: Option<String>,
    pub out_port: Option<String>,
    pub mappings: Vec<TsiControlMapping>,
}

impl TsiDevice {
    /// Encodes a [`TsiDevice`] into a `DEVI` chunk with UTF-16BE device name and child `DDAT -> DDCB -> CMAS -> CMAI` chunks.
    pub fn to_chunk(&self) -> TsiChunk {
        let prefix = encode_utf16be_string(&self.device_name);

        let mut ddat_children = Vec::new();
        if let Some(comment) = &self.comment {
            ddat_children.push(TsiChunk {
                fourcc: *b"DDIC",
                payload: encode_utf16be_string(comment),
                children: Vec::new(),
            });
        }
        if self.in_port.is_some() || self.out_port.is_some() {
            let mut ddpt = encode_utf16be_string(self.in_port.as_deref().unwrap_or("None"));
            ddpt.extend_from_slice(&encode_utf16be_string(
                self.out_port.as_deref().unwrap_or("None"),
            ));
            ddat_children.push(TsiChunk {
                fourcc: *b"DDPT",
                payload: ddpt,
                children: Vec::new(),
            });
        }

        let cmas_children: Vec<TsiChunk> = self.mappings.iter().map(|m| m.to_chunk()).collect();
        let cmas = TsiChunk {
            fourcc: *b"CMAS",
            payload: (self.mappings.len() as u32).to_be_bytes().to_vec(),
            children: cmas_children,
        };
        let ddcb = TsiChunk {
            fourcc: *b"DDCB",
            payload: Vec::new(),
            children: vec![cmas],
        };
        ddat_children.push(ddcb);

        let ddat = TsiChunk {
            fourcc: *b"DDAT",
            payload: Vec::new(),
            children: ddat_children,
        };

        TsiChunk {
            fourcc: *b"DEVI",
            payload: prefix,
            children: vec![ddat],
        }
    }

    /// Parses a `DEVI` chunk into a [`TsiDevice`].
    pub fn from_devi_chunk(chunk: &TsiChunk) -> Option<Self> {
        if &chunk.fourcc != b"DEVI" {
            return None;
        }
        let (device_name, _) = decode_utf16be_string(&chunk.payload)?;
        let mut comment = None;
        let mut in_port = None;
        let mut out_port = None;
        let mut mappings = Vec::new();

        collect_devi_metadata(&chunk.children, &mut comment, &mut in_port, &mut out_port, &mut mappings);

        Some(Self {
            device_name,
            comment,
            in_port,
            out_port,
            mappings,
        })
    }
}

fn collect_devi_metadata(
    chunks: &[TsiChunk],
    comment: &mut Option<String>,
    in_port: &mut Option<String>,
    out_port: &mut Option<String>,
    mappings: &mut Vec<TsiControlMapping>,
) {
    for c in chunks {
        match &c.fourcc {
            b"DDIC" => {
                if let Some((s, _)) = decode_utf16be_string(&c.payload) {
                    *comment = Some(s);
                }
            }
            b"DDPT" => {
                if let Some((inp, consumed)) = decode_utf16be_string(&c.payload) {
                    *in_port = Some(inp);
                    if let Some((outp, _)) = decode_utf16be_string(&c.payload[consumed..]) {
                        *out_port = Some(outp);
                    }
                }
            }
            b"CMAI" => {
                if let Some(m) = TsiControlMapping::from_cmai_chunk(c) {
                    mappings.push(m);
                }
            }
            _ => {}
        }
        collect_devi_metadata(&c.children, comment, in_port, out_port, mappings);
    }
}

/// Complete `.tsi` file containing XML `<NIXML><TraktorSettings>` entries and decoded `DIOM` controller chunks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TsiFile {
    pub settings: BTreeMap<String, (u32, String)>,
    pub root_chunks: Vec<TsiChunk>,
    pub devices: Vec<TsiDevice>,
}

impl TsiFile {
    /// Parses a `.tsi` XML file (`<NIXML><TraktorSettings>` or `<TraktorSettings>`) and decodes its Base64 `DeviceIO.Config.Controller` TLV hierarchy.
    pub fn parse_xml(xml: &str) -> Result<Self, String> {
        let doc = XmlDocument::parse(xml)?;
        let mut settings = BTreeMap::new();
        let mut root_chunks = Vec::new();
        let mut devices = Vec::new();

        let mut entry_elements = Vec::new();
        collect_entry_elements(&doc.root, &mut entry_elements);

        for elem in entry_elements {
            if let (Some(name), Some(val)) = (elem.attr("Name"), elem.attr("Value")) {
                let entry_type = elem
                    .attr("Type")
                    .and_then(|t| t.parse::<u32>().ok())
                    .unwrap_or(3);
                if name == "DeviceIO.Config.Controller" || name == "DeviceIO.Config.Keyboard" {
                    let raw_bytes = base64_decode(val)?;
                    let chunks = TsiChunk::parse_stream(&raw_bytes)?;
                    extract_devices(&chunks, &mut devices);
                    root_chunks.extend(chunks);
                }
                settings.insert(name.to_string(), (entry_type, val.to_string()));
            }
        }

        Ok(Self {
            settings,
            root_chunks,
            devices,
        })
    }

    /// Builds a `.tsi` file from a list of [`TsiDevice`] controller definitions using the exact `DIOM -> DIOI + DEVS` layout.
    pub fn from_devices(devices: Vec<TsiDevice>) -> Self {
        let dioi = TsiChunk {
            fourcc: *b"DIOI",
            payload: 1u32.to_be_bytes().to_vec(),
            children: Vec::new(),
        };
        let devi_chunks: Vec<TsiChunk> = devices.iter().map(|d| d.to_chunk()).collect();
        let devs = TsiChunk {
            fourcc: *b"DEVS",
            payload: (devi_chunks.len() as u32).to_be_bytes().to_vec(),
            children: devi_chunks,
        };
        let diom = TsiChunk {
            fourcc: *b"DIOM",
            payload: Vec::new(),
            children: vec![dioi, devs],
        };
        let raw_bytes = diom.to_bytes();
        let b64 = base64_encode(&raw_bytes);
        let mut settings = BTreeMap::new();
        settings.insert("DeviceIO.Config.Controller".to_string(), (3, b64));
        Self {
            settings,
            root_chunks: vec![diom],
            devices,
        }
    }

    /// Serializes this [`TsiFile`] to a `.tsi` XML string matching Traktor Pro 4's `<NIXML><TraktorSettings>` envelope.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
        out.push_str("<NIXML><TraktorSettings>");
        for (name, (ty, val)) in &self.settings {
            out.push_str(&format!(
                "<Entry Name=\"{}\" Type=\"{ty}\" Value=\"{}\"></Entry>\n",
                escape(name),
                escape(val)
            ));
        }
        out.push_str("</TraktorSettings>\n</NIXML>\n");
        out
    }
}

fn collect_entry_elements<'a>(elem: &'a XmlNode, out: &mut Vec<&'a XmlNode>) {
    if elem.name == "Entry" {
        out.push(elem);
    }
    for child in &elem.children {
        collect_entry_elements(child, out);
    }
}

fn extract_devices(chunks: &[TsiChunk], out: &mut Vec<TsiDevice>) {
    for c in chunks {
        if &c.fourcc == b"DEVI" {
            if let Some(dev) = TsiDevice::from_devi_chunk(c) {
                out.push(dev);
            }
        }
        extract_devices(&c.children, out);
    }
}

fn encode_utf16be_string(s: &str) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let mut out = Vec::with_capacity(4 + units.len() * 2);
    out.extend_from_slice(&(units.len() as u32).to_be_bytes());
    for u in units {
        out.extend_from_slice(&u.to_be_bytes());
    }
    out
}

fn decode_utf16be_string(bytes: &[u8]) -> Option<(String, usize)> {
    if bytes.len() < 4 {
        return None;
    }
    let char_count = u32::from_be_bytes(bytes[0..4].try_into().ok()?) as usize;
    let byte_len = char_count.checked_mul(2)?;
    if bytes.len() < 4 + byte_len {
        return None;
    }
    let mut units = Vec::with_capacity(char_count);
    for i in 0..char_count {
        let start = 4 + i * 2;
        units.push(u16::from_be_bytes([bytes[start], bytes[start + 1]]));
    }
    Some((String::from_utf16_lossy(&units), 4 + byte_len))
}

const B64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(input: &[u8]) -> String {
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;

        out.push(B64_TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(B64_TABLE[((triple >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_TABLE[((triple >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_TABLE[(triple & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let clean: Vec<u8> = input
        .bytes()
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if clean.len() % 4 != 0 {
        return Err("Invalid base64 length".to_string());
    }
    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for quartet in clean.chunks(4) {
        let v0 = b64_val(quartet[0])?;
        let v1 = b64_val(quartet[1])?;
        let v2 = if quartet[2] == b'=' { 0 } else { b64_val(quartet[2])? };
        let v3 = if quartet[3] == b'=' { 0 } else { b64_val(quartet[3])? };
        let triple = (v0 << 18) | (v1 << 12) | (v2 << 6) | v3;
        out.push(((triple >> 16) & 0xFF) as u8);
        if quartet[2] != b'=' {
            out.push(((triple >> 8) & 0xFF) as u8);
        }
        if quartet[3] != b'=' {
            out.push((triple & 0xFF) as u8);
        }
    }
    Ok(out)
}

fn b64_val(b: u8) -> Result<u32, String> {
    match b {
        b'A'..=b'Z' => Ok((b - b'A') as u32),
        b'a'..=b'z' => Ok((b - b'a' + 26) as u32),
        b'0'..=b'9' => Ok((b - b'0' + 52) as u32),
        b'+' => Ok(62),
        b'/' => Ok(63),
        _ => Err(format!("Invalid base64 byte: 0x{b:02X}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tsi_diom_roundtrip() {
        let dev = TsiDevice {
            device_name: "Traktor.Kontrol S4 MK3".to_string(),
            comment: Some("Default S4 MK3 Mapping".to_string()),
            in_port: Some("Traktor Kontrol S4 MK3".to_string()),
            out_port: Some("Traktor Kontrol S4 MK3".to_string()),
            mappings: vec![
                TsiControlMapping {
                    midi_binding_id: 1,
                    is_output_led: false,
                    traktor_control_id: 102, // Deck Play/Pause
                    target_assignment: 1,    // Deck A
                },
                TsiControlMapping {
                    midi_binding_id: 1,
                    is_output_led: true,
                    traktor_control_id: 102,
                    target_assignment: 1,
                },
            ],
        };

        let tsi = TsiFile::from_devices(vec![dev.clone()]);
        let xml = tsi.to_xml();
        let parsed = TsiFile::parse_xml(&xml).expect("parse tsi xml");
        assert_eq!(parsed.devices.len(), 1);
        assert_eq!(parsed.devices[0], dev);
        assert_eq!(parsed.root_chunks[0].fourcc_str(), "DIOM");
        assert_eq!(parsed.root_chunks[0].children[0].fourcc_str(), "DIOI");
        assert_eq!(parsed.root_chunks[0].children[1].fourcc_str(), "DEVS");
        // Verify exact binary round-trip of the root DIOM chunk
        assert_eq!(parsed.root_chunks[0].to_bytes(), tsi.root_chunks[0].to_bytes());
    }

    #[test]
    fn test_parse_real_traktor4_ddj_flx4_tsi_if_present() {
        let path = "/Users/adammeek/Documents/Native Instruments/Traktor 4.5.1/Settings/Default Settings/Controller/AlphaTheta/02 - Controller - AlphaTheta - DDJ-FLX4.tsi";
        if let Ok(xml) = std::fs::read_to_string(path) {
            let parsed = TsiFile::parse_xml(&xml).expect("parse real DDJ-FLX4.tsi");
            assert_eq!(parsed.root_chunks.len(), 1);
            let diom = &parsed.root_chunks[0];
            assert_eq!(diom.fourcc_str(), "DIOM");
            assert_eq!(diom.children.len(), 2);
            assert_eq!(diom.children[0].fourcc_str(), "DIOI");
            assert_eq!(diom.children[1].fourcc_str(), "DEVS");
            assert_eq!(parsed.devices.len(), 6);
            assert_eq!(parsed.devices[0].device_name, "Generic MIDI");
            assert_eq!(
                parsed.devices[0].comment.as_deref(),
                Some("DDJ-FLX4 - Pads A (Play)")
            );
            assert!(parsed.devices[0].mappings.len() > 100);
            // Verify lossless byte-for-byte re-serialization of the entire 2.16 MB DIOM tree
            let orig_b64 = &parsed.settings.get("DeviceIO.Config.Controller").unwrap().1;
            let orig_bytes = base64_decode(orig_b64).unwrap();
            assert_eq!(diom.to_bytes(), orig_bytes);
        }
    }
}
