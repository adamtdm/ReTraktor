//! Test-only: builds small synthetic Traktor Remix Sets (`.trak`) in the layout `trak::read_remix_set` reads, so
//! tests need no NI audio. Included with `#[path]` by the tests of `retraktor-formats` and `retraktor-gui`; never
//! part of a shipped crate.
#![allow(dead_code)]

use retraktor_formats::tsi::base64_encode;

/// A filled cell as the set's XML gives it.
pub struct Cell<'a> {
    pub file: &'a str,
    pub name: &'a str,
    pub play_type: u8,
    pub synced: bool,
    pub bpm: f64,
}

/// Slots by position, each its cells by position as XML.
pub type Slots = Vec<(u32, Vec<(u32, String)>)>;

/// Text as the set stores it: Base64 of UTF-16LE.
fn text(s: &str) -> String {
    base64_encode(&s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<u8>>())
}

pub fn cell_xml(cell: Option<&Cell>) -> String {
    let head = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";
    match cell {
        None => format!("{head}<TraktorCell><RelativeFilename/><Name/><PlayType>1</PlayType><BPM>0</BPM><Synced>false</Synced><Active>false</Active></TraktorCell>"),
        Some(c) => format!(
            "{head}<TraktorCell><RelativeFilename>{}</RelativeFilename><Name>{}</Name><Color>1</Color><TriggerType>1</TriggerType><PlayType>{}</PlayType><Gain>0.5</Gain><Offset>0</Offset><Speed>1</Speed><Pitch>0</Pitch><StartMarker>0</StartMarker><EndMarker>0</EndMarker><BPM>{}</BPM><BeatGridOffset>0</BeatGridOffset><Reverse>false</Reverse><Synced>{}</Synced><Active>true</Active></TraktorCell>",
            text(c.file),
            text(c.name),
            c.play_type,
            c.bpm,
            c.synced
        ),
    }
}

/// Four slots of sixteen cells: `cells` at their (column, row), the rest empty.
pub fn grid(cells: &[(usize, usize, Cell)]) -> Slots {
    (0..4u32)
        .map(|s| (s, (0..16u32).map(|r| (r, cell_xml(cells.iter().find(|c| (c.0, c.1) == (s as usize, r as usize)).map(|c| &c.2)))).collect()))
        .collect()
}

/// One `hsin` item: header, frame (`kind`, `id`, `payload`) and children (position, kind, id, item bytes).
fn item(kind: &[u8; 4], id: u32, payload: &[u8], children: &[(u32, [u8; 4], u32, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    body.extend(((20 + payload.len()) as u64).to_le_bytes());
    body.extend(kind);
    body.extend(id.to_le_bytes());
    body.extend(1u32.to_le_bytes());
    body.extend(payload);
    body.extend(1u32.to_le_bytes());
    body.extend((children.len() as u32).to_le_bytes());
    for (position, kind, id, child) in children {
        body.extend(position.to_le_bytes());
        body.extend(kind);
        body.extend(id.to_le_bytes());
        body.extend(child);
    }
    let mut out = Vec::new();
    out.extend(((40 + body.len()) as u64).to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(b"hsin");
    out.extend(1u64.to_le_bytes());
    out.extend([0x5a; 16]);
    out.extend(body);
    out
}

/// A `kart` item: its XML, after the word that is 2 in older sets and 3 in newer ones.
fn kart(id: u32, xml: &str, word: u32, children: &[(u32, [u8; 4], u32, Vec<u8>)]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend(24u64.to_le_bytes());
    payload.extend(b"DSIN");
    for w in [1u32, 1, 1, word, 1, 0, 0, xml.len() as u32, xml.len() as u32] {
        payload.extend(w.to_le_bytes());
    }
    payload.extend(xml.as_bytes());
    item(b"kart", id, &payload, children)
}

/// The `NISOUND` record: a set with Q `quantize` and `slots`, then zero filler.
pub fn nisound(quantize: bool, slots: &Slots, word: u32) -> Vec<u8> {
    let slot_xml = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<TraktorSlot><FxEnable>true</FxEnable><Keylock>false</Keylock><PunchEnable>false</PunchEnable></TraktorSlot>";
    let slot_items: Vec<_> = slots
        .iter()
        .map(|(position, cells)| {
            let cell_items: Vec<_> = cells.iter().map(|(row, xml)| (*row, *b"kart", 3, kart(3, xml, word, &[]))).collect();
            (*position, *b"kart", 2, kart(2, slot_xml, word, &cell_items))
        })
        .collect();
    let set_xml = format!("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<TraktorSet><QuantValue>4</QuantValue><QuantState>{quantize}</QuantState><Title>{}</Title><BPM>120</BPM></TraktorSet>", text("Test set"));
    let set = kart(1, &set_xml, word, &slot_items);
    let mut out = item(b"DSIN", 118, &[0; 130], &[(0, *b"kart", 1, set)]);
    out.extend([0; 1000]);
    out
}

/// An NI file container holding `records` (name, bytes) in that order.
pub fn archive(records: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = b"/\\ NI FC MTD  /\\".to_vec();
    out.resize(256, 0);
    out[132] = 2;
    out[144] = 1;
    for (i, (name, data)) in records.iter().enumerate() {
        let mut h = vec![0u8; 1920];
        let size = (data.len() as u64).to_le_bytes();
        h[0..8].copy_from_slice(&1u64.to_le_bytes());
        h[8..16].fill(0xF0);
        h[16..24].copy_from_slice(&1u64.to_le_bytes());
        h[24..32].copy_from_slice(&size);
        h[32..48].copy_from_slice(b"/\\ NI FC TOC  /\\");
        h[648..656].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        let name: Vec<u8> = name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        h[672..672 + name.len()].copy_from_slice(&name);
        h[1280..1288].copy_from_slice(&size);
        h[1288..1296].fill(0xF1);
        h[1312..1328].copy_from_slice(b"/\\ NI FC TOC  /\\");
        out.extend(h);
        out.extend(*data);
    }
    out
}
