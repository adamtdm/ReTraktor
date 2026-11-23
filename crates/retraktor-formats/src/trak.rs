//! `.stem.m4a` MP4 `stem` atom extractor/builder and the Traktor Remix Set (`.trak`) reader
//! ([`read_remix_set`]).

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

// ---------------------------------------------------------------- Traktor Remix Sets (.trak)

/// One file stored in a `.trak` archive: its exact name and where its bytes are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrakMember {
    pub name: String,
    pub offset: u64,
    pub size: u64,
}

/// One filled cell of a Remix Set, as ReTraktor plays it.
#[derive(Debug, Clone, PartialEq)]
pub struct TrakCell {
    /// 0-based slot (column) and cell (row 0–15: four pages of four), as Traktor places it.
    pub column: usize,
    pub row: usize,
    /// The [`TrakMember`] holding its audio, by exact name: always a plain file name, FLAC or WAV.
    pub file: String,
    /// The cell's display name (the file's name without extension when the set gives none), cut to
    /// [`MAX_CELL_NAME_CHARS`] with control characters made spaces.
    pub name: String,
    /// The set's name for the cell was longer or had control characters, so `name` isn't exactly it.
    pub name_shortened: bool,
    /// `PlayType` 0 loops, 1 plays once.
    pub looped: bool,
    /// `Synced`: follows the master tempo at `bpm`.
    pub synced: bool,
    pub bpm: f64,
    /// The cell's Traktor settings that differ from their defaults and that a plain sample player doesn't carry.
    pub unapplied: Vec<&'static str>,
}

/// A Traktor Remix Set: Q, its filled cells and the files the archive stores.
#[derive(Debug, Clone, PartialEq)]
pub struct TrakRemixSet {
    pub quantize: bool,
    pub cells: Vec<TrakCell>,
    pub members: Vec<TrakMember>,
}

const NI_CONTAINER_MAGIC: &[u8; 16] = b"/\\ NI FC MTD  /\\";
const NI_TOC_MAGIC: &[u8; 16] = b"/\\ NI FC TOC  /\\";
const NI_HEADER_BYTES: u64 = 256;
const NI_RECORD_HEADER_BYTES: usize = 1920;
/// The record holding the set's cell list.
const NISOUND: &str = "NISOUND";
/// Real sets store at most 65 files and a 250 000-byte cell list; far more isn't a Remix Set.
const MAX_TRAK_MEMBERS: usize = 1024;
const MAX_NISOUND_BYTES: u64 = 8 << 20;
/// The longest cell name taken; real sets' names are far shorter.
pub const MAX_CELL_NAME_CHARS: usize = 128;
/// Slots (columns) and cells per slot in every Remix Set.
const TRAK_SLOTS: usize = 4;
const TRAK_CELLS: usize = 16;

/// Reads a Traktor Remix Set (`.trak`): an NI file container of named records (one per sample, FLAC or WAV, and
/// `NISOUND`, the set's tree of four slots of sixteen cells, each an XML document). Only `NISOUND` and a few bytes
/// of each sample are read; every length is checked against the file. Refused, saying why, unless the container,
/// the tree's shape and every cell are whole and as Traktor writes them: each slot and cell at its own position,
/// and each filled cell naming one stored file exactly. Errors read after the file's name.
pub fn read_remix_set<R: std::io::Read + std::io::Seek>(r: &mut R) -> Result<TrakRemixSet, String> {
    use std::io::SeekFrom;
    let io = |e: std::io::Error| format!("can't be read ({e})");
    let len = r.seek(SeekFrom::End(0)).map_err(io)?;
    let mut read_at = |at: u64, buf: &mut [u8]| r.seek(SeekFrom::Start(at)).and_then(|_| r.read_exact(buf)).map_err(io);
    let mut magic = [0u8; 16];
    if len < NI_HEADER_BYTES || read_at(0, &mut magic).is_err() || &magic != NI_CONTAINER_MAGIC {
        return Err("isn't a Traktor Remix Set (no NI file container header)".into());
    }
    let mut members: Vec<TrakMember> = Vec::new();
    let mut nisound = None;
    let (mut pos, mut records) = (NI_HEADER_BYTES, 0u64);
    while pos < len {
        records += 1;
        if records as usize > MAX_TRAK_MEMBERS + 1 {
            return Err(format!("stores more than {MAX_TRAK_MEMBERS} files, more than any Remix Set"));
        }
        let mut h = [0u8; NI_RECORD_HEADER_BYTES];
        if len - pos < NI_RECORD_HEADER_BYTES as u64 {
            return Err(format!("is cut short (the file entry at byte {pos} is incomplete)"));
        }
        read_at(pos, &mut h)?;
        let word = |at: usize| u64::from_le_bytes(h[at..at + 8].try_into().unwrap());
        let size = word(24);
        let whole = h[8..16] == [0xF0; 8] && &h[32..48] == NI_TOC_MAGIC && word(1280) == size && h[1288..1296] == [0xF1; 8] && &h[1312..1328] == NI_TOC_MAGIC;
        if !whole {
            return Err(format!("has a damaged file entry at byte {pos}"));
        }
        if word(648) != records {
            return Err(format!("has its file entries out of order at byte {pos}"));
        }
        let name = utf16_name(&h[672..1272]).ok_or(format!("has a file entry with an unreadable name at byte {pos}"))?;
        let offset = pos + NI_RECORD_HEADER_BYTES as u64;
        let end = offset.checked_add(size).filter(|&end| end <= len).ok_or(format!("is cut short ('{name}' runs past the end of the file)"))?;
        if name == NISOUND {
            if nisound.is_some() || size > MAX_NISOUND_BYTES {
                return Err("has a damaged cell list (NISOUND twice, or too big)".into());
            }
            let mut data = vec![0u8; size as usize];
            read_at(offset, &mut data)?;
            nisound = Some(data);
        } else if members.iter().any(|m| m.name == name) {
            return Err(format!("stores '{name}' twice"));
        } else {
            members.push(TrakMember { name, offset, size });
        }
        pos = end;
    }
    let nisound = nisound.ok_or("has no cell list (NISOUND)")?;
    let (quantize, cells) = read_nisound(&nisound, &members)?;
    // Only FLAC and WAV, the formats Traktor's own sets store, are taken: anything else is refused, not guessed at.
    let mut checked: Vec<&str> = Vec::new();
    for cell in &cells {
        if checked.contains(&cell.file.as_str()) {
            continue;
        }
        checked.push(&cell.file);
        let member = members.iter().find(|m| m.name == cell.file).expect("read_nisound matched every file");
        let mut head = [0u8; 12];
        let audio = member.size >= 12 && read_at(member.offset, &mut head).is_ok() && (&head[..4] == b"fLaC" || (&head[..4] == b"RIFF" && &head[8..] == b"WAVE"));
        if !audio {
            return Err(format!("{}: '{}' isn't FLAC or WAV audio", trak_cell_label(cell.column, cell.row), cell.file));
        }
    }
    Ok(TrakRemixSet { quantize, cells, members })
}

fn trak_cell_label(column: usize, row: usize) -> String {
    format!("cell {}.{}", column + 1, row + 1)
}

/// A record's name: UTF-16LE up to a NUL, with only zeros after it, and not empty.
fn utf16_name(field: &[u8]) -> Option<String> {
    let units: Vec<u16> = field.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
    let end = units.iter().position(|&u| u == 0)?;
    if end == 0 || units[end..].iter().any(|&u| u != 0) {
        return None;
    }
    String::from_utf16(&units[..end]).ok()
}

/// One item of `NISOUND`'s tree: its kind, id, XML (a `kart` item's) and children by position.
struct NiItem<'a> {
    kind: [u8; 4],
    id: u32,
    xml: Option<&'a str>,
    children: Vec<(u32, NiItem<'a>)>,
}

/// Reads the item at `pos`, which must end by `limit`, and its children (`depth` levels down at most 3: set,
/// slot, cell). Returns it and where it ends.
fn ni_item(d: &[u8], pos: usize, limit: usize, depth: usize) -> Result<(NiItem<'_>, usize), String> {
    let bad = || format!("has a damaged cell list (NISOUND, byte {pos})");
    let u32_at = |at: usize| d.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap())).ok_or_else(bad);
    let u64_at = |at: usize| d.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).ok_or_else(bad);
    let span = |start: usize, size: u64, limit: usize| usize::try_from(size).ok().and_then(|size| start.checked_add(size)).filter(|&end| end <= limit).ok_or_else(bad);
    let end = span(pos, u64_at(pos)?, limit)?;
    if d.get(pos + 12..pos + 16) != Some(b"hsin".as_slice()) {
        return Err(bad());
    }
    let frame = pos + 40;
    let frame_end = span(frame, u64_at(frame)?, end)?;
    if frame_end < frame + 20 {
        return Err(bad());
    }
    let kind: [u8; 4] = d[frame + 8..frame + 12].try_into().unwrap();
    let id = u32_at(frame + 12)?;
    let xml = if &kind == b"kart" {
        // The frame ends in the XML, its length given twice just before it.
        let length = u32_at(frame + 60)? as usize;
        if frame_end < frame + 68 || u32_at(frame + 64)? as usize != length || frame + 68 + length != frame_end {
            return Err(bad());
        }
        Some(std::str::from_utf8(&d[frame + 68..frame_end]).map_err(|_| bad())?)
    } else {
        None
    };
    let count = u32_at(frame_end + 4)? as usize;
    if frame_end + 8 > end || count > TRAK_CELLS || (depth >= 3 && count > 0) {
        return Err(bad());
    }
    let mut children = Vec::with_capacity(count);
    let mut at = frame_end + 8;
    for _ in 0..count {
        let (position, kind, id) = (u32_at(at)?, d.get(at + 4..at + 8).ok_or_else(bad)?, u32_at(at + 8)?);
        let (child, child_end) = ni_item(d, at + 12, end, depth + 1)?;
        if kind != child.kind || id != child.id {
            return Err(bad());
        }
        children.push((position, child));
        at = child_end;
    }
    if at != end {
        return Err(bad());
    }
    Ok((NiItem { kind, id, xml, children }, end))
}

/// `item`'s `count` children, each a `kart` item `id` with XML whose root is `root`, in position order: every
/// position 0..count exactly once.
fn ni_children<'a>(item: &'a NiItem<'a>, count: usize, id: u32, root: &str, what: &str) -> Result<Vec<(&'a NiItem<'a>, crate::xml::XmlNode)>, String> {
    if item.children.len() != count {
        return Err(format!("has {} {what}s where a Remix Set has {count}", item.children.len()));
    }
    let mut placed: Vec<Option<(&NiItem, crate::xml::XmlNode)>> = (0..count).map(|_| None).collect();
    for (position, child) in &item.children {
        let slot = placed.get_mut(*position as usize).ok_or(format!("has a {what} at position {}, outside 1–{count}", u64::from(*position) + 1))?;
        if slot.is_some() {
            return Err(format!("has two {what}s at position {}", u64::from(*position) + 1));
        }
        let xml = child.xml.filter(|_| &child.kind == b"kart" && child.id == id).ok_or(format!("has a {what} of an unknown kind"))?;
        let doc = crate::xml::XmlDocument::parse(xml).map_err(|e| format!("has a {what} with broken XML ({e})"))?;
        if doc.root.name != root {
            return Err(format!("has a {what} that isn't a {root}"));
        }
        *slot = Some((child, doc.root));
    }
    Ok(placed.into_iter().map(|p| p.expect("every position filled once")).collect())
}

/// Q and the filled cells from `NISOUND`'s tree: root → set → 4 slots → 16 cells each. Each filled cell's file is
/// one of `members`, by its exact name.
fn read_nisound(d: &[u8], members: &[TrakMember]) -> Result<(bool, Vec<TrakCell>), String> {
    // The tree's root item says how long it is; opaque filler (not always zeros) follows it in the record.
    let (root, _) = ni_item(d, 0, d.len(), 0)?;
    if &root.kind != b"DSIN" {
        return Err("has a damaged cell list (NISOUND has no root)".into());
    }
    let set = ni_children(&root, 1, 1, "TraktorSet", "set")?;
    let (set, set_xml) = &set[0];
    let quantize = match set_xml.child("QuantState").map(|n| n.text.as_str()) {
        Some("true") => true,
        Some("false") => false,
        _ => return Err("has no quantize setting (QuantState)".into()),
    };
    let mut cells = Vec::new();
    for (column, (slot, _)) in ni_children(set, TRAK_SLOTS, 2, "TraktorSlot", "slot")?.into_iter().enumerate() {
        for (row, (_, cell)) in ni_children(slot, TRAK_CELLS, 3, "TraktorCell", "cell")?.into_iter().enumerate() {
            let at = trak_cell_label(column, row);
            let text = |tag: &str| cell.child(tag).map(|n| n.text.trim());
            let string = |tag: &str| -> Result<String, String> {
                let bytes = crate::tsi::base64_decode(text(tag).unwrap_or_default()).map_err(|e| format!("{at} has an unreadable {tag} ({e})"))?;
                if bytes.len() % 2 != 0 {
                    return Err(format!("{at} has an unreadable {tag}"));
                }
                let units: Vec<u16> = bytes.chunks_exact(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect();
                String::from_utf16(&units).map_err(|_| format!("{at} has an unreadable {tag}"))
            };
            if text("RelativeFilename").is_none() {
                return Err(format!("{at} has no file entry (RelativeFilename)"));
            }
            let file = string("RelativeFilename")?;
            if file.is_empty() {
                continue; // an empty cell
            }
            let plain = !(file.starts_with('.') || file.len() > 255 || file.chars().any(|c| c.is_control() || c == '/' || c == '\\'));
            if !plain {
                return Err(format!("{at} plays '{file}', which isn't a plain file name"));
            }
            if !members.iter().any(|m| m.name == file) {
                return Err(format!("{at} plays '{file}', which the set doesn't store"));
            }
            let looped = match text("PlayType") {
                Some("0") => true,
                Some("1") => false,
                other => return Err(format!("{at} has play type {}, which ReTraktor doesn't play (only loop 0 and one-shot 1)", other.unwrap_or("none"))),
            };
            let synced = match text("Synced") {
                Some("true") => true,
                Some("false") => false,
                _ => return Err(format!("{at} has no sync setting (Synced)")),
            };
            let bpm = text("BPM").and_then(|t| t.parse::<f64>().ok()).filter(|b| b.is_finite() && *b >= 0.0).ok_or(format!("{at} has no valid BPM"))?;
            // A name is drawn on its pad every frame and saved in sets: at most MAX_CELL_NAME_CHARS, no control characters.
            let full_name = string("Name")?;
            let mut name: String = full_name.chars().take(MAX_CELL_NAME_CHARS).map(|c| if c.is_control() { ' ' } else { c }).collect();
            let name_shortened = name != full_name;
            if name.trim().is_empty() {
                name = std::path::Path::new(&file).file_stem().map_or(file.clone(), |s| s.to_string_lossy().into_owned());
            }
            // Missing settings are taken as Traktor's defaults; any other value is named, not applied.
            let differs = |tag: &str, default: f64| text(tag).is_some_and(|t| t.parse::<f64>() != Ok(default));
            let mut unapplied = Vec::new();
            for (setting, on) in [
                ("gain", differs("Gain", 0.5)),
                ("reverse", text("Reverse").is_some_and(|t| t != "false")),
                ("speed", differs("Speed", 1.0)),
                ("pitch", differs("Pitch", 0.0)),
                ("markers", differs("Offset", 0.0) || differs("StartMarker", 0.0) || differs("EndMarker", 0.0) || differs("BeatGridOffset", 0.0)),
                ("trigger", differs("TriggerType", 1.0)),
                ("one-shot sync", synced && !looped),
            ] {
                if on {
                    unapplied.push(setting);
                }
            }
            cells.push(TrakCell { column, row, file, name, name_shortened, looped, synced, bpm, unapplied });
        }
    }
    Ok((quantize, cells))
}

/// Test-only `.trak` builder, shared with the GUI's tests (not built into the library).
#[cfg(test)]
#[path = "../tests/support/trak_fixture.rs"]
mod trak_fixture;

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

    use super::trak_fixture::{archive, grid, nisound, Cell, Slots};

    fn read(bytes: &[u8]) -> Result<TrakRemixSet, String> {
        read_remix_set(&mut std::io::Cursor::new(bytes))
    }

    const FLAC: &[u8] = b"fLaC\0\0\0\x22 kick bytes";
    const WAV: &[u8] = b"RIFF\x10\0\0\0WAVE vocal bytes";

    fn three_cells() -> Vec<(usize, usize, Cell<'static>)> {
        vec![
            (0, 0, Cell { file: "B vocal.wav", name: "Vocal", play_type: 0, synced: true, bpm: 126.0 }),
            (2, 6, Cell { file: "A kick.flac", name: "Kick", play_type: 1, synced: false, bpm: 0.0 }),
            (3, 15, Cell { file: "B vocal.wav", name: "", play_type: 0, synced: false, bpm: 98.5 }),
        ]
    }

    /// A sparse set, its files stored in another order than its cells and one file feeding two cells, with a loop
    /// and a one-shot and the last cell (4.16), in both header variants: each filled cell reads back in its place
    /// with its file, name (the file's when it has none), loop, sync and BPM, the holes stay empty, and each stored
    /// file is found at its exact bytes. Settings a sample player doesn't carry are named.
    #[test]
    fn a_remix_set_reads_back_its_cells_and_files() {
        for word in [2, 3] {
            let mut slots = grid(&three_cells());
            slots[2].1[6].1 = slots[2].1[6].1.replace("<Gain>0.5</Gain>", "<Gain>0.8</Gain>").replace("<Synced>false</Synced>", "<Synced>true</Synced>");
            let set = nisound(true, &slots, word);
            let bytes = archive(&[("A kick.flac", FLAC), ("unused.wav", WAV), ("B vocal.wav", WAV), ("NISOUND", &set)]);
            let read = read(&bytes).unwrap_or_else(|e| panic!("word {word}: {e}"));
            assert!(read.quantize);
            let cells: Vec<_> = read.cells.iter().map(|c| (c.column, c.row, c.file.as_str(), c.name.as_str(), c.looped, c.synced, c.bpm)).collect();
            assert_eq!(cells, [(0, 0, "B vocal.wav", "Vocal", true, true, 126.0), (2, 6, "A kick.flac", "Kick", false, true, 0.0), (3, 15, "B vocal.wav", "B vocal", true, false, 98.5)]);
            assert_eq!(read.cells.iter().map(|c| c.unapplied.clone()).collect::<Vec<_>>(), [vec![], vec!["gain", "one-shot sync"], vec![]]);
            assert_eq!(read.members.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(), ["A kick.flac", "unused.wav", "B vocal.wav"]);
            for m in &read.members {
                assert_eq!(&bytes[m.offset as usize..(m.offset + m.size) as usize], if m.name.ends_with("flac") { FLAC } else { WAV });
            }
            assert!(read.cells.iter().all(|c| !c.name_shortened));
        }
    }

    /// A cell name with control characters, or longer than a pad can show, is cut to MAX_CELL_NAME_CHARS with its
    /// control characters made spaces, and says so; a huge one never leaves the parser.
    #[test]
    fn cell_names_are_bounded_and_printable() {
        let long = format!("Kick\n{}", "é".repeat(1 << 20));
        let named = |name: &str| {
            let slots = grid(&[(0, 0, Cell { file: "A kick.flac", name, play_type: 1, synced: false, bpm: 0.0 })]);
            let read = read(&archive(&[("A kick.flac", FLAC), ("NISOUND", &nisound(false, &slots, 3))])).unwrap();
            let cell = read.cells[0].clone();
            (cell.name, cell.name_shortened)
        };
        let (name, shortened) = named(&long);
        assert!(shortened && name.chars().count() == MAX_CELL_NAME_CHARS && name.starts_with("Kick é") && !name.chars().any(char::is_control), "{name:?}");
        assert_eq!(named("Kick\tdry"), ("Kick dry".to_string(), true));
        assert_eq!(named("Trance Kick"), ("Trance Kick".to_string(), false));
    }

    /// Anything but a whole set as Traktor writes it is refused, saying what is wrong: cut short anywhere, a damaged
    /// container, a tree of the wrong shape, cells at wrong or repeated positions, a cell naming a file the set doesn't
    /// store by that exact name (or a path), an unknown play type, and audio that isn't FLAC or WAV. Damage anywhere
    /// in the cell list is refused or read, never a panic.
    #[test]
    fn incomplete_or_unexpected_sets_are_refused_with_the_reason() {
        let good_set = nisound(false, &grid(&three_cells()), 3);
        let good = archive(&[("A kick.flac", FLAC), ("B vocal.wav", WAV), ("NISOUND", &good_set)]);
        assert!(read(&good).is_ok());
        for cut in (0..good.len()).step_by(61).chain([good.len() - 1]) {
            assert!(read(&good[..cut]).is_err(), "cut at {cut}");
        }
        let with = |slots: Slots| archive(&[("A kick.flac", FLAC), ("B vocal.wav", WAV), ("NISOUND", &nisound(false, &slots, 3))]);
        let one = |file: &'static str, play_type| grid(&[(1, 1, Cell { file, name: "x", play_type, synced: false, bpm: 120.0 })]);
        let mut five = grid(&three_cells());
        five.push((4, five[0].1.clone()));
        let mut twice = grid(&three_cells());
        twice[1].0 = 0;
        let mut fifteen = grid(&three_cells());
        fifteen[2].1.pop();
        let mut outside = grid(&three_cells());
        outside[3].1[15].0 = 16;
        let mut last_position = grid(&three_cells());
        last_position[3].1[15].0 = u32::MAX;
        let mut not_ni = good.clone();
        not_ni[3] = b'X';
        let mut past_end = good.clone();
        let huge = (1u64 << 40).to_le_bytes();
        past_end[256 + 24..256 + 32].copy_from_slice(&huge);
        past_end[256 + 1280..256 + 1288].copy_from_slice(&huge);
        let mut order = good.clone();
        order[256 + 648] = 5;
        let mut tree = good_set.clone();
        tree[0] ^= 0x40;
        let cases: Vec<(&str, Vec<u8>, &str)> = vec![
            ("not NI", not_ni, "isn't a Traktor Remix Set"),
            ("size past end", past_end, "runs past the end"),
            ("entry order", order, "out of order"),
            ("no NISOUND", archive(&[("A kick.flac", FLAC)]), "no cell list"),
            ("file twice", archive(&[("A kick.flac", FLAC), ("A kick.flac", FLAC), ("NISOUND", &good_set)]), "stores 'A kick.flac' twice"),
            ("tree size", archive(&[("A kick.flac", FLAC), ("B vocal.wav", WAV), ("NISOUND", &tree)]), "damaged cell list"),
            ("five slots", with(five), "has 5 slots"),
            ("slot twice", with(twice), "two slots at position 1"),
            ("fifteen cells", with(fifteen), "has 15 cells"),
            ("cell 17", with(outside), "cell at position 17"),
            ("last position", with(last_position), "cell at position 4294967296"),
            ("path", with(one("../A kick.flac", 1)), "isn't a plain file name"),
            ("absent", with(one("C hat.flac", 1)), "doesn't store"),
            ("other case", with(one("a kick.flac", 1)), "doesn't store"),
            ("play type", with(one("A kick.flac", 2)), "play type 2"),
            ("not audio", archive(&[("A kick.flac", b"ID3\x04 not a flac"), ("B vocal.wav", WAV), ("NISOUND", &good_set)]), "cell 3.7: 'A kick.flac' isn't FLAC or WAV"),
        ];
        for (name, bytes, want) in cases {
            let e = read(&bytes).err().unwrap_or_else(|| panic!("{name}: read"));
            assert!(e.contains(want), "{name}: wanted '{want}', got '{e}'");
        }
        let start = good.len() - good_set.len();
        for at in (start..good.len() - 1000).step_by(37) {
            let mut bad = good.clone();
            bad[at] ^= 0xA5;
            let _ = read(&bad);
        }
    }
}
