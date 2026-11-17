//! Rekordbox XML (`DJ_PLAYLISTS` v1.0.0) exporter.
//!
//! Converts an [`NmlCollection`] into Pioneer DJ / AlphaTheta rekordbox XML format
//! so tracks, beatgrids, hotcues, loops, and playlists can be imported into rekordbox
//! and exported to Pioneer CDJ / XDJ USB drives.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::nml::{NmlCollection, NmlCue, NmlEntry, NmlPlaylistNode};
use retraktor_core::CueType;

/// 24 standard musical keys corresponding to Traktor `MUSICAL_KEY/@VALUE` (`0..=23`):
/// - `0..=11` Major (C..B)
/// - `12..=23` Minor (Cm..Bm)
const KEY_MAP: [&str; 24] = [
    // 0..11 Major
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    // 12..23 Minor
    "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm",
];

/// Open Key notation table mapped to 0..=23.
const OPEN_KEY_TABLE: [&str; 24] = [
    "1d", "8d", "3d", "10d", "5d", "12d", "7d", "2d", "9d", "4d", "11d", "6d",
    "10m", "5m", "12m", "7m", "2m", "9m", "4m", "11m", "6m", "1m", "8m", "3m",
];

/// Exports an [`NmlCollection`] into rekordbox `DJ_PLAYLISTS` XML format (version 1.0.0).
pub fn to_rekordbox_xml(col: &NmlCollection) -> String {
    let mut out = String::with_capacity(col.entries.len() * 512 + 1024);
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<DJ_PLAYLISTS Version=\"1.0.0\">\n");
    out.push_str("  <PRODUCT Name=\"ReTraktor\" Version=\"1.0.0\" Company=\"ReTraktor\"/>\n");

    // Build key lookup table: primary_key / POSIX path / file -> 1-based TrackID string
    let mut key_to_id: HashMap<String, String> = HashMap::with_capacity(col.entries.len() * 3);
    for (i, entry) in col.entries.iter().enumerate() {
        let track_id = (i + 1).to_string();
        key_to_id.entry(entry.location.primary_key()).or_insert_with(|| track_id.clone());
        key_to_id.entry(entry.location.to_posix_path()).or_insert_with(|| track_id.clone());
        key_to_id.entry(entry.location.file.clone()).or_insert_with(|| track_id.clone());
        let without_vol = format!("{}{}", entry.location.dir, entry.location.file);
        key_to_id.entry(without_vol).or_insert(track_id);
    }

    // <COLLECTION Entries="N">
    out.push_str(&format!("  <COLLECTION Entries=\"{}\">\n", col.entries.len()));
    for (i, entry) in col.entries.iter().enumerate() {
        let track_id = (i + 1).to_string();
        write_track(&mut out, entry, &track_id);
    }
    out.push_str("  </COLLECTION>\n");

    // <PLAYLISTS>
    out.push_str("  <PLAYLISTS>\n");
    write_playlists(&mut out, &col.playlists, &key_to_id);
    out.push_str("  </PLAYLISTS>\n");

    out.push_str("</DJ_PLAYLISTS>\n");
    out
}

fn write_track(out: &mut String, entry: &NmlEntry, track_id: &str) {
    let name = xml_escape(&entry.title);
    let artist = xml_escape(&entry.artist);
    let album = xml_escape(entry.album_title.as_deref().unwrap_or(""));
    let genre = xml_escape(entry.genre.as_deref().unwrap_or(""));
    let kind = file_kind(&entry.location.file);
    let total_time = entry
        .playtime_sec
        .map(|s| (s.round() as u64).to_string())
        .unwrap_or_else(|| "0".to_string());
    let average_bpm = entry
        .tempo
        .as_ref()
        .map(|t| format!("{:.2}", t.bpm))
        .unwrap_or_else(|| "0.00".to_string());
    let tonality = derive_tonality(entry);
    let comments = xml_escape(entry.comment.as_deref().unwrap_or(""));
    let location = xml_escape(&format_rekordbox_location(&entry.location));

    out.push_str(&format!(
        "    <TRACK TrackID=\"{track_id}\" Name=\"{name}\" Artist=\"{artist}\" Album=\"{album}\" \
         Genre=\"{genre}\" Kind=\"{kind}\" TotalTime=\"{total_time}\" AverageBpm=\"{average_bpm}\" \
         Tonality=\"{tonality}\" Comments=\"{comments}\" Location=\"{location}\""
    ));

    if let Some(b) = entry.bitrate {
        out.push_str(&format!(" BitRate=\"{b}\""));
    }
    if let Some(tr) = entry.album_track {
        out.push_str(&format!(" TrackNumber=\"{tr}\""));
    }

    let grid_cues: Vec<&NmlCue> = entry
        .cues
        .iter()
        .filter(|c| c.cue_type == CueType::Grid)
        .collect();

    let has_tempo = !grid_cues.is_empty() || entry.tempo.is_some();
    let has_cues = entry.cues.iter().any(|c| match c.cue_type {
        CueType::Grid => (0..=7).contains(&c.hotcue),
        _ => true,
    });

    if !has_tempo && !has_cues {
        out.push_str("/>\n");
        return;
    }

    out.push_str(">\n");

    // Write <TEMPO> markers
    if !grid_cues.is_empty() {
        for cue in &grid_cues {
            let bpm = cue
                .grid_bpm
                .or_else(|| entry.tempo.as_ref().map(|t| t.bpm))
                .unwrap_or(120.0);
            let inizio = cue.start_ms / 1000.0;
            out.push_str(&format!(
                "      <TEMPO Inizio=\"{inizio:.3}\" Bpm=\"{bpm:.2}\" Metro=\"4/4\" Battito=\"1\"/>\n"
            ));
        }
    } else if let Some(tempo) = &entry.tempo {
        out.push_str(&format!(
            "      <TEMPO Inizio=\"0.000\" Bpm=\"{:.2}\" Metro=\"4/4\" Battito=\"1\"/>\n",
            tempo.bpm
        ));
    }

    // Write <POSITION_MARK> markers
    for cue in &entry.cues {
        let name = xml_escape(&cue.name);
        let start = cue.start_ms / 1000.0;
        let num = if (0..=7).contains(&cue.hotcue) {
            cue.hotcue
        } else {
            -1
        };

        match cue.cue_type {
            CueType::Grid => {
                if (0..=7).contains(&cue.hotcue) {
                    out.push_str(&format!(
                        "      <POSITION_MARK Name=\"{name}\" Type=\"0\" Start=\"{start:.3}\" Num=\"{num}\"/>\n"
                    ));
                }
            }
            CueType::Loop => {
                let end = (cue.start_ms + cue.len_ms) / 1000.0;
                out.push_str(&format!(
                    "      <POSITION_MARK Name=\"{name}\" Type=\"4\" Start=\"{start:.3}\" End=\"{end:.3}\" Num=\"{num}\"/>\n"
                ));
            }
            other => {
                let type_id = match other {
                    CueType::FadeIn => 1,
                    CueType::FadeOut => 2,
                    CueType::Load => 3,
                    _ => 0,
                };
                out.push_str(&format!(
                    "      <POSITION_MARK Name=\"{name}\" Type=\"{type_id}\" Start=\"{start:.3}\" Num=\"{num}\"/>\n"
                ));
            }
        }
    }

    out.push_str("    </TRACK>\n");
}

fn write_playlists(
    out: &mut String,
    playlists: &[NmlPlaylistNode],
    key_to_id: &HashMap<String, String>,
) {
    // If top-level contains a single folder named $ROOT or ROOT, unwrap its subnodes as ROOT children
    let root_subnodes = if playlists.len() == 1 {
        match &playlists[0] {
            NmlPlaylistNode::Folder { name, subnodes } if name == "$ROOT" || name == "ROOT" => {
                subnodes.as_slice()
            }
            _ => playlists,
        }
    } else {
        playlists
    };

    let valid_count = root_subnodes
        .iter()
        .filter(|n| !matches!(n, NmlPlaylistNode::Smartlist { .. }))
        .count();

    if valid_count == 0 {
        out.push_str("    <NODE Type=\"0\" Name=\"ROOT\" Count=\"0\"/>\n");
        return;
    }

    out.push_str(&format!(
        "    <NODE Type=\"0\" Name=\"ROOT\" Count=\"{valid_count}\">\n"
    ));
    for node in root_subnodes {
        write_playlist_node(out, node, 6, key_to_id);
    }
    out.push_str("    </NODE>\n");
}

fn write_playlist_node(
    out: &mut String,
    node: &NmlPlaylistNode,
    indent: usize,
    key_to_id: &HashMap<String, String>,
) {
    let pad = " ".repeat(indent);
    match node {
        NmlPlaylistNode::Folder { name, subnodes } => {
            let valid_children: Vec<&NmlPlaylistNode> = subnodes
                .iter()
                .filter(|n| !matches!(n, NmlPlaylistNode::Smartlist { .. }))
                .collect();
            let safe_name = xml_escape(name);

            if valid_children.is_empty() {
                out.push_str(&format!("{pad}<NODE Type=\"0\" Name=\"{safe_name}\" Count=\"0\"/>\n"));
            } else {
                out.push_str(&format!(
                    "{pad}<NODE Type=\"0\" Name=\"{safe_name}\" Count=\"{}\">\n",
                    valid_children.len()
                ));
                for child in valid_children {
                    write_playlist_node(out, child, indent + 2, key_to_id);
                }
                out.push_str(&format!("{pad}</NODE>\n"));
            }
        }
        NmlPlaylistNode::Playlist {
            name,
            track_keys,
            ..
        } => {
            let mut matched_ids = Vec::new();
            for key in track_keys {
                if let Some(id) = key_to_id.get(key) {
                    matched_ids.push(id.as_str());
                }
            }
            let safe_name = xml_escape(name);
            if matched_ids.is_empty() {
                out.push_str(&format!(
                    "{pad}<NODE Type=\"1\" Name=\"{safe_name}\" Entries=\"0\" KeyType=\"0\"/>\n"
                ));
            } else {
                out.push_str(&format!(
                    "{pad}<NODE Type=\"1\" Name=\"{safe_name}\" Entries=\"{}\" KeyType=\"0\">\n",
                    matched_ids.len()
                ));
                for id in matched_ids {
                    out.push_str(&format!("{pad}  <TRACK Key=\"{id}\"/>\n"));
                }
                out.push_str(&format!("{pad}</NODE>\n"));
            }
        }
        NmlPlaylistNode::Smartlist { .. } => {
            // Rekordbox does not support Traktor Smartlist search expression queries.
        }
    }
}

/// Converts a Traktor `NmlLocation` into a `file://localhost/` URI
/// with percent-encoding of spaces and non-ASCII UTF-8 bytes.
fn format_rekordbox_location(loc: &crate::nml::NmlLocation) -> String {
    let posix = loc.to_posix_path();
    let raw_path = if loc.volume.len() == 2 && loc.volume.ends_with(':') {
        format!("{}/{}", loc.volume, posix.trim_start_matches('/'))
    } else {
        posix
    };

    let mut uri = String::with_capacity(16 + raw_path.len() * 3);
    uri.push_str("file://localhost");
    if !raw_path.starts_with('/') {
        uri.push('/');
    }

    for b in raw_path.bytes() {
        match b {
            b' ' => uri.push_str("%20"),
            b if b >= 0x80 => {
                let _ = write!(uri, "%{:02X}", b);
            }
            _ => uri.push(b as char),
        }
    }

    uri
}

/// Derives standard tonality ("Am", "C#m", "F", etc.) from `entry.musical_key_value`
/// or `entry.key_text`.
fn derive_tonality(entry: &NmlEntry) -> String {
    if let Some(v) = entry.musical_key_value {
        if let Some(&k) = KEY_MAP.get(v as usize) {
            return k.to_string();
        }
    }

    if let Some(ref text) = entry.key_text {
        let trimmed = text.trim();
        if let Some(pos) = OPEN_KEY_TABLE.iter().position(|&k| k == trimmed) {
            return KEY_MAP[pos].to_string();
        }
        return xml_escape(trimmed);
    }

    String::new()
}

/// Infers the audio file Kind string from the filename extension.
fn file_kind(filename: &str) -> &'static str {
    let ext = filename.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "mp3" => "MP3 File",
        "flac" => "FLAC File",
        "wav" => "WAV File",
        "aif" | "aiff" => "AIFF File",
        "m4a" | "alac" => "M4A File",
        "aac" => "AAC File",
        "ogg" => "OGG File",
        _ => "Audio File",
    }
}

/// Reads a rekordbox XML export (File > Export Collection in xml format) as a collection:
/// tracks, first beatgrid marker, hotcues, memory cues, loops and playlists.
/// Track locations keep rekordbox's paths; the caller relocates files from another computer.
pub fn from_rekordbox_xml(xml: &str) -> Result<NmlCollection, String> {
    use crate::nml::{NmlLocation, NmlTempo};
    let doc = crate::xml::XmlDocument::parse(xml)?;
    if doc.root.name != "DJ_PLAYLISTS" {
        return Err(format!("Not a rekordbox XML file (found <{}>)", doc.root.name));
    }
    let mut col = NmlCollection::new(20);
    col.program = "rekordbox".to_string();
    let mut key_by_id: HashMap<String, String> = HashMap::new();
    let mut key_by_location: HashMap<String, String> = HashMap::new();
    let num = |n: &crate::xml::XmlNode, k: &str| n.attr(k).and_then(|v| v.trim().parse::<f64>().ok());
    let text = |n: &crate::xml::XmlNode, k: &str| n.attr(k).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);

    for t in doc.root.child("COLLECTION").into_iter().flat_map(|c| c.children_named("TRACK")) {
        let Some(raw_location) = t.attr("Location") else { continue };
        let path = decode_rekordbox_location(raw_location);
        let location = NmlLocation::from_posix_path(&path, "linux-root");
        let key = location.primary_key();
        if let Some(id) = t.attr("TrackID") {
            key_by_id.insert(id.to_string(), key.clone());
        }
        key_by_location.insert(raw_location.to_string(), key);

        let tonality = text(t, "Tonality");
        let mut cues = Vec::new();
        // shortcut: only the first TEMPO marker is kept, so tempo-changing (dynamic) grids flatten to one bpm.
        if let Some(tempo) = t.children_named("TEMPO").next() {
            cues.push(NmlCue {
                name: "AutoGrid".into(),
                displ_order: 0,
                cue_type: CueType::Grid,
                start_ms: num(tempo, "Inizio").unwrap_or(0.0) * 1000.0,
                len_ms: 0.0,
                repeats: -1,
                hotcue: -1,
                grid_bpm: num(tempo, "Bpm"),
            });
        }
        for m in t.children_named("POSITION_MARK") {
            let start = num(m, "Start").unwrap_or(0.0);
            let cue_type = match num(m, "Type").unwrap_or(0.0) as i64 {
                1 => CueType::FadeIn,
                2 => CueType::FadeOut,
                3 => CueType::Load,
                4 => CueType::Loop,
                _ => CueType::Cue,
            };
            let len = match (cue_type, num(m, "End")) {
                (CueType::Loop, Some(end)) => (end - start).max(0.0),
                _ => 0.0,
            };
            cues.push(NmlCue {
                name: m.attr("Name").unwrap_or("").to_string(),
                displ_order: 0,
                cue_type,
                start_ms: start * 1000.0,
                len_ms: len * 1000.0,
                repeats: -1,
                hotcue: num(m, "Num").map_or(-1, |n| n as i64),
                grid_bpm: None,
            });
        }
        col.entries.push(NmlEntry {
            title: t.attr("Name").unwrap_or("").to_string(),
            artist: t.attr("Artist").unwrap_or("").to_string(),
            location,
            album_title: text(t, "Album"),
            album_track: num(t, "TrackNumber").map(|n| n as u32).filter(|n| *n > 0),
            genre: text(t, "Genre"),
            comment: text(t, "Comments"),
            musical_key_value: tonality.as_deref().and_then(tonality_to_value),
            key_text: tonality,
            playtime_sec: num(t, "TotalTime").filter(|s| *s > 0.0),
            bitrate: num(t, "BitRate").map(|b| b as u32).filter(|b| *b > 0),
            tempo: num(t, "AverageBpm").filter(|b| *b > 0.0).map(|bpm| NmlTempo { bpm, bpm_quality: 100.0 }),
            cues,
            ranking: num(t, "Rating").map(|r| r.clamp(0.0, 255.0) as u8).filter(|r| *r > 0),
            label: text(t, "Label"),
            remixer: text(t, "Remixer"),
            release_date: text(t, "Year").filter(|y| y != "0"),
            play_count: num(t, "PlayCount").map(|n| n as u32).filter(|n| *n > 0),
            ..Default::default()
        });
    }

    fn node(n: &crate::xml::XmlNode, by_id: &HashMap<String, String>, by_loc: &HashMap<String, String>) -> Option<NmlPlaylistNode> {
        let name = n.attr("Name").unwrap_or("").to_string();
        match n.attr("Type") {
            Some("0") => Some(NmlPlaylistNode::Folder {
                name: if name == "ROOT" { "$ROOT".into() } else { name },
                subnodes: n.children_named("NODE").filter_map(|c| node(c, by_id, by_loc)).collect(),
            }),
            Some("1") => {
                let by = if n.attr("KeyType") == Some("1") { by_loc } else { by_id };
                Some(NmlPlaylistNode::Playlist {
                    name,
                    uuid: String::new(),
                    track_keys: n.children_named("TRACK").filter_map(|t| by.get(t.attr("Key")?).cloned()).collect(),
                })
            }
            _ => None,
        }
    }
    if let Some(pl) = doc.root.child("PLAYLISTS") {
        col.playlists = pl.children_named("NODE").filter_map(|n| node(n, &key_by_id, &key_by_location)).collect();
    }
    Ok(col)
}

/// `file://localhost/Users/me/My%20Track.mp3` → `/Users/me/My Track.mp3`; Windows paths keep their drive (`C:/…`).
fn decode_rekordbox_location(uri: &str) -> String {
    let rest = uri.strip_prefix("file://localhost").or_else(|| uri.strip_prefix("file://")).unwrap_or(uri);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let path = String::from_utf8_lossy(&out).into_owned();
    match path.as_bytes() {
        [b'/', d, b':', ..] if d.is_ascii_alphabetic() => path[1..].to_string(),
        _ => path,
    }
}

/// rekordbox "Am" / "C#m" / "F" (any enharmonic spelling) → Traktor key value 0..=23.
fn tonality_to_value(t: &str) -> Option<u8> {
    let t = t.trim();
    let (root, minor) = match t.strip_suffix('m') {
        Some(r) => (r, true),
        None => (t, false),
    };
    let pc = match root {
        "C" | "B#" => 0,
        "C#" | "Db" => 1,
        "D" => 2,
        "D#" | "Eb" => 3,
        "E" | "Fb" => 4,
        "F" | "E#" => 5,
        "F#" | "Gb" => 6,
        "G" => 7,
        "G#" | "Ab" => 8,
        "A" => 9,
        "A#" | "Bb" => 10,
        "B" | "Cb" => 11,
        _ => return None,
    };
    Some(if minor { 12 + pc } else { pc })
}

/// XML-escapes `&`, `<`, `>`, `"`, and `'`.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nml::{NmlCollection, NmlCue, NmlEntry, NmlLocation, NmlPlaylistNode, NmlTempo};
    use retraktor_core::CueType;

    #[test]
    fn test_to_rekordbox_xml_two_tracks_and_playlist() {
        let mut col = NmlCollection::new(20);

        // Track 1: Non-ASCII + space in path, special XML chars in name/artist/comment, C#m key (13)
        col.entries.push(NmlEntry {
            title: "Santé & \"Bonheur\" <VIP>".to_string(),
            artist: "Stromae 'Remix'".to_string(),
            audio_id: None,
            location: NmlLocation::from_posix_path(
                "/home/dj/Músic Tracks/Santé & Bonheur.flac",
                "linux-root",
            ),
            album_title: Some("Multitude & More".to_string()),
            album_track: Some(1),
            genre: Some("Chanson & Electro".to_string()),
            comment: Some("Peak-time banger with >100dB energy".to_string()),
            key_text: None,
            playtime_sec: Some(245.8),
            bitrate: Some(1411200),
            tempo: Some(NmlTempo {
                bpm: 124.0,
                bpm_quality: 100.0,
            }),
            loudness: None,
            musical_key_value: Some(13), // C#m
            cues: vec![NmlCue {
                name: "BeatGrid".to_string(),
                displ_order: 0,
                cue_type: CueType::Grid,
                start_ms: 45.120,
                len_ms: 0.0,
                repeats: -1,
                hotcue: -1,
                grid_bpm: Some(124.0),
            }],
            stems_path: None,
            ..Default::default()
        });

        // Track 2: Hotcues and a Loop, Am key (21)
        col.entries.push(NmlEntry {
            title: "Loop Masters".to_string(),
            artist: "ReTraktor".to_string(),
            audio_id: None,
            location: NmlLocation::from_posix_path("/home/dj/Music/loop_masters.mp3", "linux-root"),
            album_title: Some("Berlin Sessions".to_string()),
            album_track: Some(2),
            genre: Some("Techno".to_string()),
            comment: None,
            key_text: None,
            playtime_sec: Some(360.0),
            bitrate: Some(320000),
            tempo: Some(NmlTempo {
                bpm: 130.0,
                bpm_quality: 100.0,
            }),
            loudness: None,
            musical_key_value: Some(21), // Am
            cues: vec![
                NmlCue {
                    name: "Intro Hotcue".to_string(),
                    displ_order: 0,
                    cue_type: CueType::Cue,
                    start_ms: 0.0,
                    len_ms: 0.0,
                    repeats: -1,
                    hotcue: 0,
                    grid_bpm: None,
                },
                NmlCue {
                    name: "Breakdown Cue".to_string(),
                    displ_order: 1,
                    cue_type: CueType::Cue,
                    start_ms: 32000.0,
                    len_ms: 0.0,
                    repeats: -1,
                    hotcue: -1, // memory cue
                    grid_bpm: None,
                },
                NmlCue {
                    name: "Build Loop".to_string(),
                    displ_order: 2,
                    cue_type: CueType::Loop,
                    start_ms: 60045.120,
                    len_ms: 7272.727,
                    repeats: -1,
                    hotcue: 2, // hot loop pad 3 (index 2)
                    grid_bpm: None,
                },
            ],
            stems_path: None,
            ..Default::default()
        });

        // Folder with one playlist referencing both tracks
        let track1_pk = col.entries[0].location.primary_key();
        let track2_pk = col.entries[1].location.primary_key();
        col.playlists.push(NmlPlaylistNode::Folder {
            name: "Club Sets".to_string(),
            subnodes: vec![NmlPlaylistNode::Playlist {
                name: "Main Room".to_string(),
                uuid: "12345678-abcd-ef01-2345-6789abcdef01".to_string(),
                track_keys: vec![track1_pk, track2_pk],
            }],
        });

        let xml = to_rekordbox_xml(&col);

        // 1. Root & Product
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<DJ_PLAYLISTS Version=\"1.0.0\">\n  <PRODUCT Name=\"ReTraktor\""));
        assert!(xml.contains("<COLLECTION Entries=\"2\">"));

        // 2. Escaping of attribute text (& < > " ')
        assert!(xml.contains("Name=\"Santé &amp; &quot;Bonheur&quot; &lt;VIP&gt;\""));
        assert!(xml.contains("Artist=\"Stromae &apos;Remix&apos;\""));
        assert!(xml.contains("Comments=\"Peak-time banger with &gt;100dB energy\""));

        // 3. URI encoding of space and non-ASCII (UTF-8 bytes)
        // Músic -> M%C3%BAsic, space -> %20, & -> &amp; in XML attribute
        assert!(xml.contains("Location=\"file://localhost/home/dj/M%C3%BAsic%20Tracks/Sant%C3%A9%20&amp;%20Bonheur.flac\""));

        // 4. Tonality (C#m for 13, Am for 21)
        assert!(xml.contains("Tonality=\"C#m\""));
        assert!(xml.contains("Tonality=\"Am\""));

        // 5. TotalTime (seconds rounded) and Kind
        assert!(xml.contains("Kind=\"FLAC File\" TotalTime=\"246\" AverageBpm=\"124.00\""));
        assert!(xml.contains("Kind=\"MP3 File\" TotalTime=\"360\" AverageBpm=\"130.00\""));

        // 6. TEMPO tag from grid cue
        assert!(xml.contains("<TEMPO Inizio=\"0.045\" Bpm=\"124.00\" Metro=\"4/4\" Battito=\"1\"/>"));

        // 7. POSITION_MARK tags
        // Hotcue 0 (start 0.000s)
        assert!(xml.contains("<POSITION_MARK Name=\"Intro Hotcue\" Type=\"0\" Start=\"0.000\" Num=\"0\"/>"));
        // Memory cue (start 32.000s, Num="-1")
        assert!(xml.contains("<POSITION_MARK Name=\"Breakdown Cue\" Type=\"0\" Start=\"32.000\" Num=\"-1\"/>"));
        // Loop Type="4" with Start="60.045" End="67.318" Num="2"
        assert!(xml.contains("<POSITION_MARK Name=\"Build Loop\" Type=\"4\" Start=\"60.045\" End=\"67.318\" Num=\"2\"/>"));

        // 8. Playlist & Folder structure under ROOT
        assert!(xml.contains("<PLAYLISTS>\n    <NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">\n      <NODE Type=\"0\" Name=\"Club Sets\" Count=\"1\">\n        <NODE Type=\"1\" Name=\"Main Room\" Entries=\"2\" KeyType=\"0\">\n          <TRACK Key=\"1\"/>\n          <TRACK Key=\"2\"/>\n        </NODE>\n      </NODE>\n    </NODE>\n  </PLAYLISTS>"));
    }

    #[test]
    fn rekordbox_xml_reads_back_what_we_export() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<DJ_PLAYLISTS Version="1.0.0">
  <COLLECTION Entries="2">
    <TRACK TrackID="7" Name="Deep &amp; Low" Artist="A" Album="LP" Genre="Techno" TotalTime="301" AverageBpm="126.00"
           Tonality="Abm" Rating="153" Location="file://localhost/Users/dj/Music/Deep%20Low%C3%A9.mp3">
      <TEMPO Inizio="0.120" Bpm="126.00" Metro="4/4" Battito="1"/>
      <TEMPO Inizio="60.120" Bpm="126.00" Metro="4/4" Battito="1"/>
      <POSITION_MARK Name="Drop" Type="0" Start="64.120" Num="2"/>
      <POSITION_MARK Name="" Type="0" Start="32.000" Num="-1"/>
      <POSITION_MARK Name="Roll" Type="4" Start="96.0" End="100.0" Num="3"/>
    </TRACK>
    <TRACK TrackID="8" Name="Win" Artist="B" Location="file://localhost/C:/Users/dj/Music/win.flac"/>
  </COLLECTION>
  <PLAYLISTS>
    <NODE Type="0" Name="ROOT" Count="1">
      <NODE Type="0" Name="Gigs" Count="1">
        <NODE Name="Friday" Type="1" KeyType="0" Entries="2"><TRACK Key="8"/><TRACK Key="7"/></NODE>
      </NODE>
    </NODE>
  </PLAYLISTS>
</DJ_PLAYLISTS>"#;
        let col = from_rekordbox_xml(xml).unwrap();
        assert_eq!(col.entries.len(), 2);
        let t = &col.entries[0];
        assert_eq!(t.title, "Deep & Low");
        assert_eq!(t.location.to_posix_path(), "/Users/dj/Music/Deep Lowé.mp3");
        assert_eq!(t.musical_key_value, Some(20)); // G#m
        assert_eq!(t.ranking, Some(153)); // 3 stars
        assert_eq!(t.tempo.as_ref().map(|t| t.bpm), Some(126.0));
        let grids: Vec<_> = t.cues.iter().filter(|c| c.cue_type == CueType::Grid).collect();
        assert_eq!(grids.len(), 1);
        assert!((grids[0].start_ms - 120.0).abs() < 1e-6);
        let hot: Vec<_> = t.cues.iter().filter(|c| c.hotcue >= 0).map(|c| (c.hotcue, c.cue_type)).collect();
        assert_eq!(hot, vec![(2, CueType::Cue), (3, CueType::Loop)]);
        assert!(t.cues.iter().any(|c| c.hotcue == -1 && c.cue_type == CueType::Cue && (c.start_ms - 32_000.0).abs() < 1e-6));
        assert_eq!(col.entries[1].location.to_posix_path(), "/C:/Users/dj/Music/win.flac");

        let NmlPlaylistNode::Folder { name, subnodes } = &col.playlists[0] else { panic!() };
        assert_eq!(name, "$ROOT");
        let NmlPlaylistNode::Folder { subnodes, .. } = &subnodes[0] else { panic!() };
        let NmlPlaylistNode::Playlist { name, track_keys, .. } = &subnodes[0] else { panic!() };
        assert_eq!(name, "Friday");
        assert_eq!(track_keys, &vec![col.entries[1].location.primary_key(), col.entries[0].location.primary_key()]);

        // And our own export reads back with the same tracks and cues.
        let again = from_rekordbox_xml(&to_rekordbox_xml(&col)).unwrap();
        assert_eq!(again.entries.len(), 2);
        assert_eq!(again.entries[0].cues.iter().filter(|c| c.hotcue >= 0).count(), 2);
        assert_eq!(again.entries[0].musical_key_value, Some(20));
    }

    #[test]
    fn test_nml_sample_roundtrip_to_rekordbox() {
        let sample = r#"<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NML VERSION="20">
  <HEAD COMPANY="www.native-instruments.com" PROGRAM="Traktor Pro 4"/>
  <MUSICFOLDERS/>
  <COLLECTION ENTRIES="1">
    <ENTRY TITLE=" Acid Rain " ARTIST="ReTraktor">
      <LOCATION DIR="/:home/:dj/:Music/:" FILE="acid_rain.flac" VOLUME="linux-root" VOLUMEID="linux-root"/>
      <ALBUM TITLE="Linux Warehouse" TRACK="2"/>
      <INFO GENRE="Techno" KEY="4m" PLAYTIME="360" PLAYTIME_FLOAT="360.500" BITRATE="1411200"/>
      <TEMPO BPM="132.000000" BPM_QUALITY="100.000000"/>
      <LOUDNESS ANALYZED_DB="-1.850000" PERCEIVED_DB="-0.900000" PEAK_DB="0.050000"/>
      <MUSICAL_KEY VALUE="18"/>
      <CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="4" START="45.120" LEN="0.000" REPEATS="-1" HOTCUE="0">
        <GRID BPM="132.000000"/>
      </CUE_V2>
      <CUE_V2 NAME="OutroGrid" DISPL_ORDER="0" TYPE="4" START="180045.120" LEN="0.000" REPEATS="-1" HOTCUE="-1">
        <GRID BPM="134.000000"/>
      </CUE_V2>
      <CUE_V2 NAME="Loop 1" DISPL_ORDER="0" TYPE="5" START="60045.120" LEN="7272.727" REPEATS="-1" HOTCUE="2"/>
    </ENTRY>
  </COLLECTION>
  <PLAYLISTS>
    <NODE TYPE="FOLDER" NAME="$ROOT">
      <SUBNODES COUNT="1">
        <NODE TYPE="PLAYLIST" NAME="Closing Set">
          <PLAYLIST ENTRIES="1" TYPE="LIST" UUID="abcd1234">
            <ENTRY>
              <PRIMARYKEY TYPE="TRACK" KEY="linux-root/:home/:dj/:Music/:acid_rain.flac"/>
            </ENTRY>
          </PLAYLIST>
        </NODE>
      </SUBNODES>
    </NODE>
  </PLAYLISTS>
</NML>"#;

        let parsed = NmlCollection::parse_xml(sample).expect("parse sample nml");
        let xml = to_rekordbox_xml(&parsed);

        assert!(xml.contains("<PRODUCT Name=\"ReTraktor\""));
        assert!(xml.contains("TrackID=\"1\" Name=\" Acid Rain \" Artist=\"ReTraktor\""));
        assert!(xml.contains("Tonality=\"F#m\"")); // 18 -> F#m
        assert!(xml.contains("Location=\"file://localhost/home/dj/Music/acid_rain.flac\""));
        assert!(xml.contains("<TEMPO Inizio=\"0.045\" Bpm=\"132.00\" Metro=\"4/4\" Battito=\"1\"/>"));
        assert!(xml.contains("<TEMPO Inizio=\"180.045\" Bpm=\"134.00\" Metro=\"4/4\" Battito=\"1\"/>"));
        assert!(xml.contains("<POSITION_MARK Name=\"AutoGrid\" Type=\"0\" Start=\"0.045\" Num=\"0\"/>"));
        assert!(xml.contains("<POSITION_MARK Name=\"Loop 1\" Type=\"4\" Start=\"60.045\" End=\"67.318\" Num=\"2\"/>"));
        assert!(xml.contains("<NODE Type=\"0\" Name=\"ROOT\" Count=\"1\">"));
        assert!(xml.contains("<NODE Type=\"1\" Name=\"Closing Set\" Entries=\"1\" KeyType=\"0\">"));
        assert!(xml.contains("<TRACK Key=\"1\"/>"));
    }
}
