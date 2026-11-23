//! Serato Markers2 and BeatGrid import.
//!
//! Layouts: https://github.com/Holzhaus/serato-tags/tree/main/docs

use crate::{tsi::base64_decode, NmlCue};
use lofty::id3::v2::{Frame, GeneralEncapsulatedObject, Id3v2Tag};
use lofty::mp4::{AtomData, AtomIdent, Ilst};
use lofty::tag::{ItemKey, Tag, TagType};
use retraktor_core::CueType;

const MARKERS: &str = "Serato Markers2";
const GRID: &str = "Serato BeatGrid";
const ENVELOPE: &[u8] = b"application/octet-stream\0\0";

/// Reads cue data from the tags already loaded by Lofty.
pub fn read_cues(tags: impl IntoIterator<Item = Tag>) -> Vec<NmlCue> {
    // shortcut: only Markers2 supplies cues and loops, add Markers_ if older libraries need import.
    let mut markers = None;
    let mut grid = None;
    for tag in tags {
        match tag.tag_type() {
            TagType::Id3v2 if tag.has_format_specific_items() => {
                for frame in Id3v2Tag::from(tag) {
                    let Frame::Binary(frame) = frame else {
                        continue;
                    };
                    if frame.id().as_str() != "GEOB" {
                        continue;
                    }
                    let Ok(object) = GeneralEncapsulatedObject::parse(&frame.data, frame.flags())
                    else {
                        continue;
                    };
                    match object.descriptor.as_deref() {
                        Some(MARKERS) if markers.is_none() => markers = parse_markers(&object.data),
                        Some(GRID) if grid.is_none() => grid = parse_grid(&object.data),
                        _ => {}
                    }
                }
            }
            TagType::VorbisComments => {
                for item in tag.items() {
                    let (ItemKey::Unknown(key), Some(text)) = (item.key(), item.value().text())
                    else {
                        continue;
                    };
                    if markers.is_none()
                        && (key.eq_ignore_ascii_case("SERATO_MARKERS_V2")
                            || key.eq_ignore_ascii_case("SERATO_MARKERS2"))
                    {
                        markers = comment_markers(text.as_bytes());
                    } else if grid.is_none() && key.eq_ignore_ascii_case("SERATO_BEATGRID") {
                        grid = comment_grid(text.as_bytes()).or_else(|| ogg_grid(text));
                    }
                }
            }
            TagType::Mp4Ilst => {
                let ilst = Ilst::from(tag);
                let values = |name: &'static str| {
                    let id = AtomIdent::Freeform {
                        mean: "com.serato.dj".into(),
                        name: name.into(),
                    };
                    ilst.get(&id)
                        .into_iter()
                        .flat_map(|atom| atom.data())
                        .filter_map(|data| match data {
                            AtomData::UTF8(text) | AtomData::UTF16(text) => Some(text.as_bytes()),
                            AtomData::Unknown { data, .. } => Some(data.as_slice()),
                            _ => None,
                        })
                };
                markers = markers.or_else(|| values("markersv2").find_map(comment_markers));
                grid = grid.or_else(|| values("beatgrid").find_map(comment_grid));
            }
            _ => {}
        }
    }
    let mut cues = markers.unwrap_or_default();
    if let Some(mut grid) = grid {
        grid.displ_order = cues.len() as i64;
        cues.push(grid);
    }
    cues
}

fn comment_markers(data: &[u8]) -> Option<Vec<NmlCue>> {
    let decoded = decode(data, false)?;
    if decoded.starts_with(ENVELOPE) {
        parse_markers(unwrap_comment(&decoded, MARKERS)?)
    } else {
        parse_marker_entries(&decoded)
    }
}

fn comment_grid(data: &[u8]) -> Option<NmlCue> {
    let decoded = decode(data, false)?;
    parse_grid(unwrap_comment(&decoded, GRID)?)
}

fn unwrap_comment<'a>(data: &'a [u8], name: &str) -> Option<&'a [u8]> {
    data.strip_prefix(ENVELOPE)?
        .strip_prefix(name.as_bytes())?
        .strip_prefix(&[0])
}

fn decode(data: &[u8], inner: bool) -> Option<Vec<u8>> {
    let data = std::str::from_utf8(data).ok()?;
    let mut encoded: String = data
        .trim_end_matches(|c: char| c == '\0' || c.is_ascii_whitespace())
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    encoded.truncate(encoded.trim_end_matches('=').len());
    if !encoded
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
    {
        return None;
    }
    if encoded.len() % 4 == 1 {
        // Serato's inner Markers2 payload can be missing a sextet. The outer envelope can have an extra one.
        if inner {
            encoded.push('A');
        } else {
            encoded.pop();
        }
    }
    while encoded.len() % 4 != 0 {
        encoded.push('=');
    }
    base64_decode(&encoded).ok()
}

fn parse_markers(data: &[u8]) -> Option<Vec<NmlCue>> {
    parse_marker_entries(&decode(data.strip_prefix(&[1, 1])?, true)?)
}

fn parse_marker_entries(data: &[u8]) -> Option<Vec<NmlCue>> {
    let mut data = data.strip_prefix(&[1, 1])?;
    let mut cues = Vec::new();
    while !data.is_empty() && data[0] != 0 {
        let (kind, entry) = split_c_string(data)?;
        let (entry, rest) = entry
            .get(4..)?
            .split_at_checked(u32_at(entry, 0)? as usize)?;
        if let Some(cue) = parse_cue(kind, entry, cues.len() as i64) {
            cues.push(cue);
        }
        data = rest;
    }
    Some(cues)
}

fn parse_cue(kind: &[u8], data: &[u8], displ_order: i64) -> Option<NmlCue> {
    // shortcut: NML cues have no color or lock fields, map COLOR, BPMLOCK and cue colors if the model gains them.
    let (cue_type, name_offset) = match kind {
        b"CUE" => (CueType::Cue, 12),
        b"LOOP" => (CueType::Loop, 20),
        _ => return None,
    };
    let (name, _) = split_c_string(data.get(name_offset..)?)?;
    let start = u32_at(data, 2)?;
    let (hotcue, length) = if cue_type == CueType::Loop {
        let length = u32_at(data, 6)?.checked_sub(start).filter(|&len| len > 0)?;
        // Serato saves loops in a separate bank from its hot cues.
        (-1, length)
    } else {
        (i64::from(data[1]), 0)
    };
    Some(NmlCue {
        name: String::from_utf8_lossy(name).into_owned(),
        displ_order,
        cue_type,
        start_ms: f64::from(start),
        len_ms: f64::from(length),
        repeats: -1,
        hotcue,
        grid_bpm: None,
    })
}

fn parse_grid(data: &[u8]) -> Option<NmlCue> {
    let data = data.strip_prefix(&[1, 0])?;
    let count = u32_at(data, 0)? as usize;
    let markers = data.get(4..data.len().checked_sub(1)?)?;
    if count == 0 || markers.len() != count.checked_mul(8)? {
        return None;
    }
    let seconds = f64::from(f32::from_bits(u32_at(markers, 0)?));
    // shortcut: playback uses one tempo, import the first segment until the deck supports tempo changes.
    let bpm = if count == 1 {
        f64::from(f32::from_bits(u32_at(markers, 4)?))
    } else {
        let next = f64::from(f32::from_bits(u32_at(markers, 8)?));
        if !next.is_finite() || next <= seconds {
            return None;
        }
        60.0 * f64::from(u32_at(markers, 4)?) / (next - seconds)
    };
    grid_cue(seconds, bpm)
}

fn ogg_grid(text: &str) -> Option<NmlCue> {
    // shortcut: only the one-marker Ogg text layout has public examples, extend when multi-marker samples are documented.
    let marker = text
        .trim()
        .strip_prefix("000000010000000000000001(")?
        .strip_suffix(')')?;
    let (seconds, bpm) = marker.split_once(',')?;
    grid_cue(seconds.parse().ok()?, bpm.parse().ok()?)
}

fn grid_cue(seconds: f64, bpm: f64) -> Option<NmlCue> {
    // Bound externally supplied tempo because waveform drawing iterates over the visible beats.
    let start_ms = seconds * 1000.0;
    if !start_ms.is_finite() || !(1.0..=1000.0).contains(&bpm) {
        return None;
    }
    Some(NmlCue {
        name: "Serato Grid".into(),
        displ_order: 0,
        cue_type: CueType::Grid,
        start_ms,
        len_ms: 0.0,
        repeats: -1,
        hotcue: -1,
        grid_bpm: Some(bpm),
    })
}

fn split_c_string(data: &[u8]) -> Option<(&[u8], &[u8])> {
    let (text, rest) = data.split_at(data.iter().position(|&byte| byte == 0)?);
    Some((text, &rest[1..]))
}

fn u32_at(data: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        data.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tsi::base64_encode;
    use lofty::id3::v2::{BinaryFrame, FrameId};
    use lofty::mp4::{Atom, DataType};
    use lofty::ogg::VorbisComments;
    use lofty::TextEncoding;

    const TERMINAL_GRID: &[u8] = b"\x01\x00\x00\x00\x00\x01\x3e\x80\x00\x00\x42\xf0\x00\x00\x37";

    fn marker_entries() -> Vec<u8> {
        let mut bytes = vec![1, 1];
        for (kind, entry) in [
            (b"COLOR\0".as_slice(), b"\0\xff\xff\xff".as_slice()),
            (
                b"CUE\0",
                b"\0\x02\0\0\x04\xd2\0\xcc\0\0\0\0Entr\xc3\xa9e\0".as_slice(),
            ),
            (b"BPMLOCK\0", b"\x01"),
            (
                b"LOOP\0",
                b"\0\x02\0\0\x07\xd0\0\0\x0b\xb8\xff\xff\xff\xff\0\x27\xaa\xe1\0\x01Build\0",
            ),
        ] {
            bytes.extend(kind);
            bytes.extend((entry.len() as u32).to_be_bytes());
            bytes.extend(entry);
        }
        bytes.push(0);
        bytes
    }

    fn envelope(name: &str, bytes: &[u8]) -> String {
        let mut wrapped = ENVELOPE.to_vec();
        wrapped.extend(name.as_bytes());
        wrapped.push(0);
        wrapped.extend(bytes);
        base64_encode(&wrapped).trim_end_matches('=').into()
    }

    #[test]
    fn reads_all_tag_carriers() {
        let mut entries = marker_entries();
        while entries.len() % 3 != 1 {
            entries.push(0);
        }
        let mut encoded = base64_encode(&entries);
        encoded.truncate(encoded.len() - 3); // The missing final 'A' sextet and the two '='.
        encoded.insert(72, '\n');
        let mut markers = vec![1, 1];
        markers.extend(encoded.bytes());
        markers.resize(470, 0);

        let mut id3 = Id3v2Tag::new();
        for (name, payload) in [(MARKERS, markers.as_slice()), (GRID, TERMINAL_GRID)] {
            let geob = GeneralEncapsulatedObject::new(
                TextEncoding::Latin1,
                Some("application/octet-stream".into()),
                None,
                Some(name.into()),
                payload.to_vec(),
            );
            id3.insert(Frame::Binary(BinaryFrame::new(
                FrameId::Valid("GEOB".into()),
                geob.as_bytes(),
            )));
        }

        let wrapped_markers = envelope(MARKERS, &markers);
        let mut wrapped_grid = envelope(GRID, TERMINAL_GRID);
        assert_eq!(wrapped_grid.len() % 4, 0);
        wrapped_grid.push('X'); // Serato's extra outer sextet.
        let mut flac = VorbisComments::new();
        flac.insert("SERATO_MARKERS_V2".into(), wrapped_markers.clone());
        flac.insert("SERATO_BEATGRID".into(), wrapped_grid.clone());

        let mut ogg = VorbisComments::new();
        ogg.insert(
            "serato_markers2".into(),
            base64_encode(&entries).trim_end_matches('=').into(),
        );
        ogg.insert(
            "serato_beatgrid".into(),
            "000000010000000000000001(0.250000,120.000000)".into(),
        );

        let mut mp4 = Ilst::new();
        for (name, value) in [
            ("markersv2", AtomData::UTF8(wrapped_markers)),
            (
                "beatgrid",
                AtomData::Unknown {
                    code: DataType::Reserved,
                    data: wrapped_grid.into_bytes(),
                },
            ),
        ] {
            mp4.insert(Atom::new(
                AtomIdent::Freeform {
                    mean: "com.serato.dj".into(),
                    name: name.into(),
                },
                value,
            ));
        }

        for tag in [id3.into(), flac.into(), ogg.into(), mp4.into()] {
            let cues = read_cues([tag]);
            assert_eq!(cues.len(), 3);
            let [hot, saved, grid] = &cues[..] else {
                unreachable!()
            };
            assert_eq!(
                (hot.cue_type, hot.hotcue, hot.start_ms, hot.name.as_str()),
                (CueType::Cue, 2, 1234.0, "Entrée")
            );
            assert_eq!(
                (
                    saved.cue_type,
                    saved.hotcue,
                    saved.start_ms,
                    saved.len_ms,
                    saved.name.as_str()
                ),
                (CueType::Loop, -1, 2000.0, 1000.0, "Build")
            );
            assert_eq!(
                (grid.cue_type, grid.start_ms, grid.grid_bpm),
                (CueType::Grid, 250.0, Some(120.0))
            );
        }
    }

    #[test]
    fn dynamic_grid_uses_the_first_interval() {
        let mut bytes = vec![1, 0, 0, 0, 0, 2];
        bytes.extend(0.25f32.to_be_bytes());
        bytes.extend(4u32.to_be_bytes());
        bytes.extend(2.25f32.to_be_bytes());
        bytes.extend(125.0f32.to_be_bytes());
        bytes.push(0x37);
        let grid = parse_grid(&bytes).unwrap();
        assert_eq!((grid.start_ms, grid.grid_bpm), (250.0, Some(120.0)));
    }

    #[test]
    fn rejects_truncated_and_invalid_data() {
        let mut entries = marker_entries();
        entries.truncate(entries.len() - 3);
        assert!(parse_marker_entries(&entries).is_none());
        assert!(parse_grid(&TERMINAL_GRID[..14]).is_none());
        let mut grid = TERMINAL_GRID.to_vec();
        grid[10..14].copy_from_slice(&f32::NAN.to_be_bytes());
        assert!(parse_grid(&grid).is_none());
    }
}
