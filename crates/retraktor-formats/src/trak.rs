//! `.stem.m4a` MP4 `stem` atom extractor/builder and the Traktor Remix Set (`.trak`) reader
//! ([`read_remix_set`]) and writer pieces ([`container_header`], [`record_header`], [`write_nisound`]).

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
    /// `Pitch` as the set stores it (0 when absent, NaN when it isn't a number).
    pub pitch: f64,
    /// `pitch` in semitones by [`semitones_from_pitch`], or `None` when it isn't a whole semitone.
    pub tune: Option<i8>,
    /// The cell's Traktor settings that differ from their defaults and that a plain sample player doesn't carry.
    pub unapplied: Vec<&'static str>,
}

/// A Traktor Remix Set: Q, each slot's Key Lock, its filled cells and the files the archive stores.
#[derive(Debug, Clone, PartialEq)]
pub struct TrakRemixSet {
    pub quantize: bool,
    /// `TraktorSlot`'s `Keylock` per slot (column), off when a slot doesn't say.
    pub keylock: [bool; TRAK_SLOTS],
    pub cells: Vec<TrakCell>,
    pub members: Vec<TrakMember>,
}

/// The most Traktor tunes a cell either way, in semitones, at its `Pitch` control's ends (-1 and +1).
const FULL_SCALE_SEMITONES: i8 = 12;
/// How far a parsed `Pitch` may be from n/12 and still be n semitones: NI writes six decimals (up to 5e-7 off), plus
/// a little for the decimal-to-binary conversion. No two twelfths are anywhere near this close.
const PITCH_TOLERANCE: f64 = 5.1e-7;
/// Drop Squad's Shaker (page 2, slot 2, cell 7) stores this, and Traktor Pro 4.4.1.22's Pitch readout shows it as -0
/// st, its knob a hair left of centre; ReTraktor's whole-semitone tuning takes it as 0.
const LEGACY_NEUTRAL_PITCH: f64 = -0.0117647;

/// A cell's `Pitch` in semitones: n/12 is n semitones, for every whole semitone from -12 to +12, as Traktor Pro
/// 4.4.1.22's own Pitch readout shows for each one written with six decimals (`-0.333333` is -4 st, `0.916667` is +11
/// st), and NI's [`LEGACY_NEUTRAL_PITCH`] is 0. `None` for any other value (between twelfths, past either end or not a
/// number), which the importer names by cell and raw value rather than rounding.
pub fn semitones_from_pitch(pitch: f64) -> Option<i8> {
    let twelfths = (pitch * f64::from(FULL_SCALE_SEMITONES)).round();
    if (pitch - LEGACY_NEUTRAL_PITCH).abs() <= PITCH_TOLERANCE {
        Some(0)
    } else if (-1.0..=1.0).contains(&pitch) && (pitch - twelfths / f64::from(FULL_SCALE_SEMITONES)).abs() <= PITCH_TOLERANCE {
        Some(twelfths as i8)
    } else {
        None
    }
}

/// The `Pitch` written for a tuning of `semitones` (-12 to +12; beyond is taken as the end): n/12 with six decimals,
/// as NI writes it, which [`semitones_from_pitch`] and Traktor read as the same tuning.
pub fn pitch_from_semitones(semitones: i8) -> f64 {
    let semitones = semitones.clamp(-FULL_SCALE_SEMITONES, FULL_SCALE_SEMITONES);
    (f64::from(semitones) / f64::from(FULL_SCALE_SEMITONES) * 1e6).round() / 1e6
}

const NI_CONTAINER_MAGIC: &[u8; 16] = b"/\\ NI FC MTD  /\\";
const NI_TOC_MAGIC: &[u8; 16] = b"/\\ NI FC TOC  /\\";
const NI_HEADER_BYTES: u64 = 256;
const NI_RECORD_HEADER_BYTES: usize = 1920;
/// The record holding the set's cell list.
pub const NISOUND: &str = "NISOUND";
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
    let (quantize, keylock, cells) = read_nisound(&nisound, &members)?;
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
    Ok(TrakRemixSet { quantize, keylock, cells, members })
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

/// Q, each slot's Key Lock and the filled cells from `NISOUND`'s tree: root → set → 4 slots → 16 cells each. Each
/// filled cell's file is one of `members`, by its exact name.
fn read_nisound(d: &[u8], members: &[TrakMember]) -> Result<(bool, [bool; TRAK_SLOTS], Vec<TrakCell>), String> {
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
    let mut keylock = [false; TRAK_SLOTS];
    for (column, (slot, slot_xml)) in ni_children(set, TRAK_SLOTS, 2, "TraktorSlot", "slot")?.into_iter().enumerate() {
        // Every slot, filled or empty, says it; a slot that doesn't is taken as off, Traktor's default.
        keylock[column] = match slot_xml.child("Keylock").map(|n| n.text.trim()) {
            None | Some("false") => false,
            Some("true") => true,
            Some(other) => return Err(format!("has slot {} with a Key Lock setting '{other}' (only true or false)", column + 1)),
        };
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
            if !plain_file_name(&file) {
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
                ("markers", differs("Offset", 0.0) || differs("StartMarker", 0.0) || differs("EndMarker", 0.0) || differs("BeatGridOffset", 0.0)),
                ("trigger", differs("TriggerType", 1.0)),
                ("one-shot sync", synced && !looped),
            ] {
                if on {
                    unapplied.push(setting);
                }
            }
            let pitch = text("Pitch").map_or(0.0, |t| t.parse::<f64>().unwrap_or(f64::NAN));
            let tune = semitones_from_pitch(pitch);
            cells.push(TrakCell { column, row, file, name, name_shortened, looped, synced, bpm, pitch, tune, unapplied });
        }
    }
    Ok((quantize, keylock, cells))
}

/// A name a cell may play: a file name on its own, never a path, hidden or with control characters.
fn plain_file_name(file: &str) -> bool {
    !(file.is_empty() || file.starts_with('.') || file.len() > 255 || file.chars().any(|c| c.is_control() || c == '/' || c == '\\'))
}

// ---------------------------------------------------------------- writing a Remix Set

/// Items in a written `NISOUND` tree, each with its own id: the root, the set, 4 slots and 64 cells.
pub const NISOUND_ITEMS: usize = 2 + TRAK_SLOTS + TRAK_SLOTS * TRAK_CELLS;
/// Every `NISOUND` record in NI's sets is this long; the tree is followed by zeros up to it.
pub const NISOUND_BYTES: usize = 250_000;
/// The root item's payload, copied byte for byte from the Remix Deck Tutorial and Drop Squad sets, where it is the same
/// (two *Step Sequencer Essentials* sets differ at its bytes 76–79); its meaning is unknown, so it is never composed.
const NISOUND_ROOT: [u8; 130] = [
    0x48, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x44, 0x53, 0x49, 0x4e, 0x6a, 0x00, 0x00, 0x00, //
    0x01, 0x00, 0x00, 0x00, 0x18, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x44, 0x53, 0x49, 0x4e, //
    0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, //
    0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x0d, 0x62, 0x65, 0x85, 0x01, 0x00, 0x00, 0x00, 0x00, 0x40, 0x10, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x30, 0x00, 0x30, 0x00, //
    0x30, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, //
    0x00, 0x00,
];
/// The root item's kind id, and the word before each `kart` item's XML length that both reference sets carry.
const NISOUND_ROOT_ID: u32 = 118;
const KART_WORD: u32 = 2;
/// Traktor's quantize length for one beat, as ReTraktor's Q launches on the next beat: a position in Traktor's
/// eight-step Quantize menu. Traktor Pro 4.4.1.22 shows an exported set's 2 as 1 (one beat), 1 as 1/2 and 4 as 4.
const QUANT_VALUE: u32 = 2;
/// The cell colour written for every filled cell (NI's sets use 1–11 by hand; ReTraktor has no cell colours).
const CELL_COLOR: u32 = 1;
const XML_HEAD: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";

/// The 256-byte NI file container header every Remix Set starts with.
pub fn container_header() -> [u8; NI_HEADER_BYTES as usize] {
    let mut h = [0u8; NI_HEADER_BYTES as usize];
    h[..16].copy_from_slice(NI_CONTAINER_MAGIC);
    h[132] = 2;
    h[144] = 1;
    h
}

/// The 1920-byte header of the `number`th record (1-based, in file order) named `name`, whose `size` bytes follow it.
/// Refused for a name that isn't plain (at most 255 bytes, so it fits the header's 299 UTF-16 units).
pub fn record_header(number: u64, name: &str, size: u64) -> Result<Vec<u8>, String> {
    if !(plain_file_name(name) || name == NISOUND) {
        return Err(format!("'{name}' can't name a file in a Remix Set"));
    }
    let units: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut h = vec![0u8; NI_RECORD_HEADER_BYTES];
    h[0] = 1;
    h[8..16].fill(0xF0);
    h[16] = 1;
    h[24..32].copy_from_slice(&size.to_le_bytes());
    h[32..48].copy_from_slice(NI_TOC_MAGIC);
    h[648..656].copy_from_slice(&number.to_le_bytes());
    h[672..672 + units.len()].copy_from_slice(&units);
    h[1280..1288].copy_from_slice(&size.to_le_bytes());
    h[1288..1296].fill(0xF1);
    h[1312..1328].copy_from_slice(NI_TOC_MAGIC);
    Ok(h)
}

/// The `NISOUND` record ([`NISOUND_BYTES`] long) of a set titled `title` at `bpm`: `set`'s Q, slot Key Locks and
/// cells in a tree of four slots of sixteen cells, empty ones as Traktor writes them. Of each cell it writes the
/// place, file, name, loop or one-shot, sync, BPM and pitch; every other cell setting is Traktor's default. `ids` are
/// random bytes, one per item ([`NISOUND_ITEMS`]), made into version 4 UUIDs as NI's sets have. `set.members` isn't
/// read. Refused for a cell outside the grid or on another's place, a file name that isn't plain, a pitch outside
/// -1..=1 or a negative BPM, or a tree too big for the record.
pub fn write_nisound(title: &str, bpm: f64, set: &TrakRemixSet, ids: &[[u8; 16]; NISOUND_ITEMS]) -> Result<Vec<u8>, String> {
    let mut grid: [[Option<&TrakCell>; TRAK_CELLS]; TRAK_SLOTS] = Default::default();
    for cell in &set.cells {
        let at = trak_cell_label(cell.column, cell.row);
        let place = grid.get_mut(cell.column).and_then(|slot| slot.get_mut(cell.row)).ok_or(format!("{at} is outside a Remix Set's grid"))?;
        if place.replace(cell).is_some() {
            return Err(format!("{at} is filled twice"));
        }
        if !plain_file_name(&cell.file) {
            return Err(format!("{at} plays '{}', which isn't a plain file name", cell.file));
        }
        if !(-1.0..=1.0).contains(&cell.pitch) || !(cell.bpm.is_finite() && cell.bpm >= 0.0) {
            return Err(format!("{at} has a pitch or BPM a Remix Set can't hold"));
        }
    }
    let mut ids = ids.iter().map(|id| {
        let mut uuid = *id;
        uuid[6] = (uuid[6] & 0x0F) | 0x40;
        uuid[8] = (uuid[8] & 0x3F) | 0x80;
        uuid
    });
    let mut next_id = || ids.next().expect("one id per item");
    let text = |s: &str| crate::tsi::base64_encode(&s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>());
    // Ids are taken in tree order (root, set, then each slot before its cells), so each item has its own.
    let (root_id, set_id) = (next_id(), next_id());
    let mut slots = Vec::with_capacity(TRAK_SLOTS);
    for (position, (column, keylock)) in grid.iter().zip(set.keylock).enumerate() {
        let slot_id = next_id();
        let mut cells = Vec::with_capacity(TRAK_CELLS);
        for (row, cell) in column.iter().enumerate() {
            let xml = match cell {
                None => format!("{XML_HEAD}<TraktorCell><RelativeFilename/><Name/><Color>0</Color><TriggerType>1</TriggerType><PlayType>0</PlayType><Gain>0.5</Gain><Offset>0</Offset><Speed>1</Speed><Pitch>0</Pitch><StartMarker>0</StartMarker><EndMarker>0</EndMarker><BPM>0</BPM><BeatGridOffset>0</BeatGridOffset><Reverse>false</Reverse><Synced>true</Synced><Active>false</Active></TraktorCell>"),
                Some(c) => format!(
                    "{XML_HEAD}<TraktorCell><RelativeFilename>{}</RelativeFilename><Name>{}</Name><Color>{CELL_COLOR}</Color><TriggerType>1</TriggerType><PlayType>{}</PlayType><Gain>0.5</Gain><Offset>0</Offset><Speed>1</Speed><Pitch>{}</Pitch><StartMarker>0</StartMarker><EndMarker>0</EndMarker><BPM>{}</BPM><BeatGridOffset>0</BeatGridOffset><Reverse>false</Reverse><Synced>{}</Synced><Active>false</Active></TraktorCell>",
                    text(&c.file),
                    text(&c.name),
                    if c.looped { 0 } else { 1 },
                    c.pitch,
                    c.bpm,
                    c.synced
                ),
            };
            cells.push((row as u32, *b"kart", 3, kart_item(next_id(), 3, &xml, &[])));
        }
        let xml = format!("{XML_HEAD}<TraktorSlot><FxEnable>true</FxEnable><Keylock>{keylock}</Keylock><PunchEnable>false</PunchEnable></TraktorSlot>");
        slots.push((position as u32, *b"kart", 2, kart_item(slot_id, 2, &xml, &cells)));
    }
    let tags = ["Artist", "Album", "Label", "Comment", "Comment2", "Genre", "Key", "KeyLyrics", "Remixer", "Producer", "Mix", "CatalogNo"].map(|tag| format!("<{tag}/>")).concat();
    let dates = ["LastPlayed", "ReleaseDate", "ImportDate"].map(|when| format!("<{when}Year>0</{when}Year><{when}Month>0</{when}Month><{when}Day>0</{when}Day>")).concat();
    let set_xml = format!(
        "{XML_HEAD}<TraktorSet><QuantValue>{QUANT_VALUE}</QuantValue><QuantState>{}</QuantState><Title>{}</Title><BPM>{bpm}</BPM>{tags}{dates}<TrackNum>0</TrackNum></TraktorSet>",
        set.quantize,
        text(title)
    );
    let set_item = kart_item(set_id, 1, &set_xml, &slots);
    let mut out = ni_tree_item(root_id, b"DSIN", NISOUND_ROOT_ID, &NISOUND_ROOT, &[(0, *b"kart", 1, set_item)]);
    if out.len() > NISOUND_BYTES {
        return Err(format!("the set's cell list would be {} bytes, more than a Remix Set holds ({NISOUND_BYTES}): shorten the cell names", out.len()));
    }
    out.resize(NISOUND_BYTES, 0);
    Ok(out)
}

/// A child of a tree item: its position, kind, kind id and item bytes.
type NiChild = (u32, [u8; 4], u32, Vec<u8>);

/// One `hsin` item as [`ni_item`] reads it: header with its `uuid`, frame (`kind`, `kind_id`, `payload`), children.
fn ni_tree_item(uuid: [u8; 16], kind: &[u8; 4], kind_id: u32, payload: &[u8], children: &[NiChild]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend((20 + payload.len() as u64).to_le_bytes());
    body.extend(kind);
    body.extend(kind_id.to_le_bytes());
    body.extend(1u32.to_le_bytes());
    body.extend(payload);
    body.extend(1u32.to_le_bytes());
    body.extend((children.len() as u32).to_le_bytes());
    for (position, kind, kind_id, child) in children {
        body.extend(position.to_le_bytes());
        body.extend(kind);
        body.extend(kind_id.to_le_bytes());
        body.extend(child);
    }
    let mut out = Vec::with_capacity(40 + body.len());
    out.extend((40 + body.len() as u64).to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(b"hsin");
    out.extend(1u64.to_le_bytes());
    out.extend(uuid);
    out.extend(body);
    out
}

/// A `kart` item: an XML document (set 1, slot 2 or cell 3) after the frame words NI's sets carry.
fn kart_item(uuid: [u8; 16], kind_id: u32, xml: &str, children: &[NiChild]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(48 + xml.len());
    payload.extend(24u64.to_le_bytes());
    payload.extend(b"DSIN");
    for word in [1u32, 1, 1, KART_WORD, 1, 0, 0, xml.len() as u32, xml.len() as u32] {
        payload.extend(word.to_le_bytes());
    }
    payload.extend(xml.as_bytes());
    ni_tree_item(uuid, b"kart", kind_id, &payload, children)
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

    use super::trak_fixture::{archive, grid, nisound, nisound_keylock, Cell, Slots};

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

    /// Pitch values with the semitones Traktor Pro 4.4.1.22's Pitch readout showed for them, written out by hand: every
    /// six-decimal twelfth from -1 to +1, a seven-decimal one, and NI's -0.0117647 as 0. Everything else stays
    /// unplaced: between twelfths, just past rounding (6e-7 off), the mirrored +0.0117647, out of range or not a number.
    #[test]
    fn cell_pitch_becomes_semitones_only_for_known_values() {
        let readouts = [
            (-1.0, -12), (-0.916667, -11), (-0.833333, -10), (-0.75, -9), (-0.666667, -8), (-0.583333, -7), (-0.5, -6), (-0.416667, -5), (-0.333333, -4),
            (-0.25, -3), (-0.166667, -2), (-0.083333, -1), (0.0, 0), (0.083333, 1), (0.166667, 2), (0.25, 3), (0.333333, 4), (0.416667, 5), (0.5, 6),
            (0.583333, 7), (0.666667, 8), (0.75, 9), (0.833333, 10), (0.916667, 11), (1.0, 12), (0.0833333, 1), (-0.0117647, 0),
        ];
        for (pitch, want) in readouts {
            assert_eq!(semitones_from_pitch(pitch), Some(want), "Pitch {pitch}");
        }
        for pitch in [0.2, -0.3, 0.0833339, -0.9166676, 0.0117647, 0.9999, 1.0000004, -1.0000004, 1.5, -1.0001, f64::NAN, f64::INFINITY] {
            assert_eq!(semitones_from_pitch(pitch), None, "Pitch {pitch} was placed");
        }
    }

    /// Each slot's Key Lock reads back on or off, empty slots too, and a slot without one is off; anything but true
    /// or false is refused. A cell's pitch reads back raw with its semitones, or none when unplaced, also with the
    /// slot's Key Lock off.
    #[test]
    fn slot_key_lock_and_cell_pitch_read_back() {
        let cells = three_cells();
        let mut slots = grid(&cells);
        slots[0].1[0].1 = slots[0].1[0].1.replace("<Pitch>0</Pitch>", "<Pitch>-0.333333</Pitch>");
        slots[2].1[6].1 = slots[2].1[6].1.replace("<Pitch>0</Pitch>", "<Pitch>-0.0117647</Pitch>");
        slots[3].1[15].1 = slots[3].1[15].1.replace("<Pitch>0</Pitch>", "<Pitch>0.2</Pitch>");
        let files = |set: &[u8]| archive(&[("A kick.flac", FLAC), ("B vocal.wav", WAV), ("NISOUND", set)]);
        // Slot 2 holds no cells; slot 4 has no Keylock at all.
        let set = read(&files(&nisound_keylock(false, &slots, 3, [Some("false"), Some("true"), Some("true"), None]))).unwrap();
        assert_eq!(set.keylock, [false, true, true, false]);
        let pitches: Vec<_> = set.cells.iter().map(|c| (c.column, c.row, c.pitch, c.tune)).collect();
        assert_eq!(pitches, [(0, 0, -0.333333, Some(-4)), (2, 6, -0.0117647, Some(0)), (3, 15, 0.2, None)]);
        assert!(set.cells.iter().all(|c| !c.unapplied.contains(&"pitch")), "pitch is carried, not listed as unapplied");
        let off = read(&files(&nisound(false, &slots, 3))).unwrap();
        assert_eq!(off.keylock, [false; 4]);
        assert_eq!(off.cells[0].tune, Some(-4), "the tuning is kept with Key Lock off");
        let e = read(&files(&nisound_keylock(false, &slots, 3, [Some("false"), Some("yes"), Some("false"), Some("false")]))).unwrap_err();
        assert!(e.contains("slot 2") && e.contains("Key Lock"), "{e}");
        let mut broken = slots.clone();
        broken[0].1[0].1 = broken[0].1[0].1.replace("-0.333333", "low");
        let bad = read(&files(&nisound(false, &broken, 3))).unwrap();
        assert!(bad.cells[0].pitch.is_nan() && bad.cells[0].tune.is_none());
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

    /// A written set in the framing of NI's own: the container and record headers byte for byte as the shared fixture
    /// (itself checked against NI's sets) builds them, a 250 000-byte NISOUND whose root payload is NI's and whose tail
    /// is zeros, and 70 items each with its own version 4 UUID. It reads back with every cell in its place (1.1 a synced
    /// loop tuned -4, 3.7 a one-shot, 4.16 sharing 1.1's file), Q, slot Key Locks and the rest empty. A cell off the
    /// grid or on another's place, a path for a file, or a name too long for the record is refused.
    #[test]
    fn a_written_set_has_ni_framing_and_reads_back() {
        let cell = |column, row, file: &str, name: &str, looped, synced, bpm, pitch| {
            let tune = semitones_from_pitch(pitch);
            TrakCell { column, row, file: file.into(), name: name.into(), name_shortened: false, looped, synced, bpm, pitch, tune, unapplied: vec![] }
        };
        let cells = vec![
            cell(0, 0, "A1-Groove.wav", "Groove loop", true, true, 123.5, -0.333333),
            cell(2, 6, "C7-Bell.wav", "Bell", false, false, 0.0, 0.0),
            cell(3, 15, "A1-Groove.wav", "Groove again", true, false, 0.0, 1.0),
        ];
        let set = TrakRemixSet { quantize: true, keylock: [true, false, false, true], cells, members: vec![] };
        let ids: [[u8; 16]; NISOUND_ITEMS] = std::array::from_fn(|i| [i as u8; 16]);
        let nisound = write_nisound("Friday", 123.5, &set, &ids).unwrap();
        assert_eq!(nisound.len(), NISOUND_BYTES);
        let mut bytes = container_header().to_vec();
        for (number, (name, data)) in [("A1-Groove.wav", WAV), ("C7-Bell.wav", FLAC), ("NISOUND", &nisound[..])].into_iter().enumerate() {
            bytes.extend(record_header(number as u64 + 1, name, data.len() as u64).unwrap());
            bytes.extend(data);
        }
        assert_eq!(bytes, archive(&[("A1-Groove.wav", WAV), ("C7-Bell.wav", FLAC), ("NISOUND", &nisound)]), "headers differ from NI's framing");

        let read_back = read(&bytes).unwrap();
        assert_eq!((read_back.quantize, read_back.keylock, &read_back.cells), (set.quantize, set.keylock, &set.cells));
        let (root, end) = ni_item(&nisound, 0, nisound.len(), 0).unwrap();
        assert_eq!((&root.kind, root.id, &nisound[60..190]), (b"DSIN", NISOUND_ROOT_ID, &NISOUND_ROOT[..]));
        assert!(nisound[end..].iter().all(|&b| b == 0), "filler isn't zeros");
        let mut uuids = Vec::new();
        let mut at = 0;
        while let Some(found) = nisound[at..end].windows(4).position(|w| w == b"hsin") {
            let uuid = &nisound[at + found + 12..at + found + 28];
            assert_eq!((uuid[6] >> 4, uuid[8] >> 6), (4, 2), "not a version 4 UUID");
            uuids.push(uuid.to_vec());
            at += found + 4;
        }
        uuids.sort();
        uuids.dedup();
        assert_eq!(uuids.len(), NISOUND_ITEMS);
        let xml = String::from_utf8_lossy(&nisound);
        assert!(xml.contains("<QuantValue>2</QuantValue><QuantState>true</QuantState>") && xml.contains("<BPM>123.5</BPM><Artist/>") && !xml.contains("CoverArt"), "{xml:.600}");

        let refused = |cells: Vec<TrakCell>| write_nisound("x", 120.0, &TrakRemixSet { cells, ..set.clone() }, &ids).unwrap_err();
        assert!(refused(vec![cell(4, 0, "a.wav", "a", true, false, 0.0, 0.0)]).contains("outside"));
        assert!(refused(vec![cell(1, 1, "a.wav", "a", true, false, 0.0, 0.0), cell(1, 1, "b.wav", "b", true, false, 0.0, 0.0)]).contains("twice"));
        assert!(refused(vec![cell(1, 1, "../a.wav", "a", true, false, 0.0, 0.0)]).contains("plain file name"));
        let long = "é".repeat(2000);
        assert!(refused((0..64).map(|i| cell(i / 16, i % 16, "a.wav", &long, true, false, 0.0, 0.0)).collect()).contains("shorten the cell names"));
        assert!(record_header(1, &"x".repeat(256), 0).is_err() && record_header(1, &"é".repeat(127), 0).is_ok() && record_header(1, "a/b.wav", 0).is_err());
    }

    /// Every tuning is written as NI writes its twelfth (six decimals) and reads back as the same tuning.
    #[test]
    fn every_tuning_writes_a_pitch_that_reads_back() {
        assert_eq!((pitch_from_semitones(-4), pitch_from_semitones(11), pitch_from_semitones(12)), (-0.333333, 0.916667, 1.0));
        for semitones in -12..=12 {
            assert_eq!(semitones_from_pitch(pitch_from_semitones(semitones)), Some(semitones), "{semitones}");
        }
    }
}
