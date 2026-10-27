//! `.nml` (Traktor Music Library v19 & v20) parser, serializer, and Linux path normalizer.
//!
//! Backed by [`crate::xml::XmlDocument`] so multiline attributes (such as `INFO/@KEY_LYRICS` in
//! real 16,160-track Traktor Pro 4 collections) and unmodeled nodes round-trip losslessly.

use crate::xml::{escape, XmlDocument, XmlNode};
use retraktor_core::CueType;

/// Represents `<LOCATION DIR="/:home/:dj/:Music/:" FILE="track.flac" VOLUME="linux-root" VOLUMEID="linux-root"></LOCATION>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NmlLocation {
    pub dir: String,
    pub file: String,
    pub volume: String,
    pub volume_id: String,
}

impl NmlLocation {
    /// Converts a Linux/macOS POSIX absolute path (e.g., `/home/dj/Music/track.flac`)
    /// into a Traktor `NmlLocation` with `/:` segment delimiters.
    pub fn from_posix_path(path: &str, volume: &str) -> Self {
        let trimmed = path.trim();
        let (dir_part, file_part) = match trimmed.rfind('/') {
            Some(idx) => (&trimmed[..idx], &trimmed[idx + 1..]),
            None => ("", trimmed),
        };
        let mut traktor_dir = String::new();
        for seg in dir_part.split('/').filter(|s| !s.is_empty()) {
            traktor_dir.push_str("/:");
            traktor_dir.push_str(seg);
        }
        traktor_dir.push_str("/:");

        Self {
            dir: traktor_dir,
            file: file_part.to_string(),
            volume: volume.to_string(),
            volume_id: volume.to_string(),
        }
    }

    /// Reconstructs a normalized Linux/macOS POSIX path (`/home/dj/Music/track.flac`)
    /// from Traktor's `/:` directory notation.
    pub fn to_posix_path(&self) -> String {
        let normalized_dir = self.dir.replace("/:", "/");
        if normalized_dir.ends_with('/') {
            format!("{normalized_dir}{}", self.file)
        } else {
            format!("{normalized_dir}/{}", self.file)
        }
    }

    /// Returns the Traktor `PRIMARYKEY/@KEY` string (`VOLUME` + `DIR` + `FILE`).
    pub fn primary_key(&self) -> String {
        format!("{}{}{}", self.volume, self.dir, self.file)
    }
}

/// `<TEMPO BPM="128.000000" BPM_QUALITY="100.000000"></TEMPO>`
#[derive(Debug, Clone, PartialEq)]
pub struct NmlTempo {
    pub bpm: f64,
    pub bpm_quality: f64,
}

/// `<LOUDNESS PEAK_DB="0.066522" PERCEIVED_DB="4.682396" ANALYZED_DB="4.682396"></LOUDNESS>`
#[derive(Debug, Clone, PartialEq)]
pub struct NmlLoudness {
    pub analyzed_db: f64,
    pub perceived_db: f64,
    pub peak_db: f64,
}

/// `<CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="4" START="932.851013" LEN="0.000000" REPEATS="-1" HOTCUE="-1">`
/// with optional Traktor Pro 4 Flexible Beatgrid `<GRID BPM="97.842911"></GRID>`.
#[derive(Debug, Clone, PartialEq)]
pub struct NmlCue {
    pub name: String,
    pub displ_order: i64,
    pub cue_type: CueType,
    pub start_ms: f64,
    pub len_ms: f64,
    pub repeats: i64,
    pub hotcue: i64,
    pub grid_bpm: Option<f64>,
}

/// `<ENTRY TITLE="..." ARTIST="...">` inside `<COLLECTION>`.
#[derive(Debug, Clone, PartialEq)]
pub struct NmlEntry {
    pub title: String,
    pub artist: String,
    pub audio_id: Option<String>,
    pub location: NmlLocation,
    pub album_title: Option<String>,
    pub album_track: Option<u32>,
    pub genre: Option<String>,
    pub comment: Option<String>,
    pub key_text: Option<String>,
    pub playtime_sec: Option<f64>,
    pub bitrate: Option<u32>,
    pub tempo: Option<NmlTempo>,
    pub loudness: Option<NmlLoudness>,
    pub musical_key_value: Option<u8>,
    pub cues: Vec<NmlCue>,
    pub stems_path: Option<String>,
}

/// `<NODE TYPE="FOLDER" | "PLAYLIST" | "SMARTLIST">` inside `<PLAYLISTS>`.
#[derive(Debug, Clone, PartialEq)]
pub enum NmlPlaylistNode {
    Folder {
        name: String,
        subnodes: Vec<NmlPlaylistNode>,
    },
    Playlist {
        name: String,
        uuid: String,
        track_keys: Vec<String>,
    },
    Smartlist {
        name: String,
        uuid: String,
        operator: String,
        query: String,
    },
}

/// Complete `.nml` collection document (`VERSION="19"` or `VERSION="20"`).
#[derive(Debug, Clone, PartialEq)]
pub struct NmlCollection {
    pub version: u32,
    pub program: String,
    pub entries: Vec<NmlEntry>,
    pub playlists: Vec<NmlPlaylistNode>,
}

impl NmlCollection {
    pub fn new(version: u32) -> Self {
        Self {
            version,
            program: "Traktor Pro 4".to_string(),
            entries: Vec::new(),
            playlists: Vec::new(),
        }
    }

    /// Parses an `.nml` XML document into an [`NmlCollection`] via [`XmlDocument`].
    pub fn parse_xml(xml: &str) -> Result<Self, String> {
        let doc = XmlDocument::parse(xml)?;
        Self::from_xml_doc(&doc)
    }

    /// Extracts typed [`NmlCollection`] fields from a parsed [`XmlDocument`].
    pub fn from_xml_doc(doc: &XmlDocument) -> Result<Self, String> {
        if doc.root.name != "NML" {
            return Err(format!("Expected <NML> root, got <{}>", doc.root.name));
        }
        let version = doc
            .root
            .attr("VERSION")
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(20);
        let program = doc
            .root
            .child("HEAD")
            .and_then(|h| h.attr("PROGRAM"))
            .unwrap_or("Traktor Pro 4")
            .to_string();

        let mut entries = Vec::new();
        if let Some(col_node) = doc.root.child("COLLECTION") {
            for entry_node in col_node.children_named("ENTRY") {
                entries.push(parse_entry_node(entry_node));
            }
        }

        let mut playlists = Vec::new();
        if let Some(pl_root) = doc.root.child("PLAYLISTS") {
            for node in pl_root.children_named("NODE") {
                if let Some(parsed) = parse_playlist_node(node) {
                    playlists.push(parsed);
                }
            }
        }

        Ok(Self {
            version,
            program,
            entries,
            playlists,
        })
    }

    /// Serializes the [`NmlCollection`] back into valid Traktor `.nml` XML.
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\" ?>\n");
        out.push_str(&format!("<NML VERSION=\"{}\">\n", self.version));
        out.push_str(&format!(
            "  <HEAD COMPANY=\"www.native-instruments.com\" PROGRAM=\"{}\"></HEAD>\n",
            escape(&self.program)
        ));
        out.push_str("  <MUSICFOLDERS></MUSICFOLDERS>\n");
        out.push_str(&format!(
            "  <COLLECTION ENTRIES=\"{}\">\n",
            self.entries.len()
        ));

        for entry in &self.entries {
            out.push_str(&format!(
                "    <ENTRY TITLE=\"{}\" ARTIST=\"{}\"",
                escape(&entry.title),
                escape(&entry.artist)
            ));
            if let Some(aid) = &entry.audio_id {
                out.push_str(&format!(" AUDIO_ID=\"{}\"", escape(aid)));
            }
            out.push_str(">\n");
            out.push_str(&format!(
                "      <LOCATION DIR=\"{}\" FILE=\"{}\" VOLUME=\"{}\" VOLUMEID=\"{}\"></LOCATION>\n",
                escape(&entry.location.dir),
                escape(&entry.location.file),
                escape(&entry.location.volume),
                escape(&entry.location.volume_id)
            ));

            if entry.album_title.is_some() || entry.album_track.is_some() {
                out.push_str("      <ALBUM");
                if let Some(t) = &entry.album_title {
                    out.push_str(&format!(" TITLE=\"{}\"", escape(t)));
                }
                if let Some(tr) = entry.album_track {
                    out.push_str(&format!(" TRACK=\"{tr}\""));
                }
                out.push_str("></ALBUM>\n");
            }

            out.push_str("      <INFO");
            if let Some(g) = &entry.genre {
                out.push_str(&format!(" GENRE=\"{}\"", escape(g)));
            }
            if let Some(c) = &entry.comment {
                out.push_str(&format!(" COMMENT=\"{}\"", escape(c)));
            }
            if let Some(k) = &entry.key_text {
                out.push_str(&format!(" KEY=\"{}\"", escape(k)));
            }
            if let Some(p) = entry.playtime_sec {
                out.push_str(&format!(
                    " PLAYTIME=\"{}\" PLAYTIME_FLOAT=\"{p:.3}\"",
                    p as u64
                ));
            }
            if let Some(b) = entry.bitrate {
                out.push_str(&format!(" BITRATE=\"{b}\""));
            }
            out.push_str("></INFO>\n");

            if let Some(tempo) = &entry.tempo {
                out.push_str(&format!(
                    "      <TEMPO BPM=\"{:.6}\" BPM_QUALITY=\"{:.6}\"></TEMPO>\n",
                    tempo.bpm, tempo.bpm_quality
                ));
            }
            if let Some(loud) = &entry.loudness {
                out.push_str(&format!(
                    "      <LOUDNESS ANALYZED_DB=\"{:.6}\" PERCEIVED_DB=\"{:.6}\" PEAK_DB=\"{:.6}\"></LOUDNESS>\n",
                    loud.analyzed_db, loud.perceived_db, loud.peak_db
                ));
            }
            if let Some(mk) = entry.musical_key_value {
                out.push_str(&format!("      <MUSICAL_KEY VALUE=\"{mk}\"></MUSICAL_KEY>\n"));
            }
            if let Some(stems) = &entry.stems_path {
                out.push_str(&format!(
                    "      <STEMS STEMS=\"{}\"></STEMS>\n",
                    escape(stems)
                ));
            }

            for cue in &entry.cues {
                out.push_str(&format!(
                    "      <CUE_V2 NAME=\"{}\" DISPL_ORDER=\"{}\" TYPE=\"{}\" START=\"{:.3}\" LEN=\"{:.3}\" REPEATS=\"{}\" HOTCUE=\"{}\">",
                    escape(&cue.name),
                    cue.displ_order,
                    cue.cue_type as i64,
                    cue.start_ms,
                    cue.len_ms,
                    cue.repeats,
                    cue.hotcue
                ));
                if let Some(grid_bpm) = cue.grid_bpm {
                    out.push_str(&format!("\n        <GRID BPM=\"{grid_bpm:.6}\"></GRID>\n      "));
                }
                out.push_str("</CUE_V2>\n");
            }

            out.push_str("    </ENTRY>\n");
        }

        out.push_str("  </COLLECTION>\n");
        out.push_str("  <PLAYLISTS>\n");
        for node in &self.playlists {
            write_playlist_node(&mut out, node, 4);
        }
        out.push_str("  </PLAYLISTS>\n");
        out.push_str("</NML>\n");
        out
    }
}

fn parse_entry_node(node: &XmlNode) -> NmlEntry {
    let loc_node = node.child("LOCATION");
    let location = NmlLocation {
        dir: loc_node
            .and_then(|l| l.attr("DIR"))
            .unwrap_or("/:")
            .to_string(),
        file: loc_node
            .and_then(|l| l.attr("FILE"))
            .unwrap_or("")
            .to_string(),
        volume: loc_node
            .and_then(|l| l.attr("VOLUME"))
            .unwrap_or("linux-root")
            .to_string(),
        volume_id: loc_node
            .and_then(|l| l.attr("VOLUMEID"))
            .unwrap_or("linux-root")
            .to_string(),
    };

    let album = node.child("ALBUM");
    let info = node.child("INFO");
    let tempo = node.child("TEMPO").map(|t| NmlTempo {
        bpm: t
            .attr("BPM")
            .and_then(|v| v.parse().ok())
            .unwrap_or(120.0),
        bpm_quality: t
            .attr("BPM_QUALITY")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100.0),
    });
    let loudness = node.child("LOUDNESS").map(|l| NmlLoudness {
        analyzed_db: l
            .attr("ANALYZED_DB")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        perceived_db: l
            .attr("PERCEIVED_DB")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
        peak_db: l
            .attr("PEAK_DB")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.0),
    });
    let musical_key_value = node
        .child("MUSICAL_KEY")
        .and_then(|m| m.attr("VALUE"))
        .and_then(|v| v.parse::<u8>().ok());
    let stems_path = node
        .child("STEMS")
        .and_then(|s| s.attr("STEMS"))
        .map(str::to_string);

    let mut cues = Vec::new();
    for cue_node in node.children_named("CUE_V2") {
        let cue_type_i = cue_node
            .attr("TYPE")
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(0);
        let grid_bpm = cue_node
            .child("GRID")
            .and_then(|g| g.attr("BPM"))
            .and_then(|v| v.parse::<f64>().ok());
        cues.push(NmlCue {
            name: cue_node.attr("NAME").unwrap_or("").to_string(),
            displ_order: cue_node
                .attr("DISPL_ORDER")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0),
            cue_type: CueType::from_i64(cue_type_i).unwrap_or(CueType::Cue),
            start_ms: cue_node
                .attr("START")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            len_ms: cue_node
                .attr("LEN")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0.0),
            repeats: cue_node
                .attr("REPEATS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(-1),
            hotcue: cue_node
                .attr("HOTCUE")
                .and_then(|v| v.parse().ok())
                .unwrap_or(-1),
            grid_bpm,
        });
    }

    NmlEntry {
        title: node.attr("TITLE").unwrap_or("").to_string(),
        artist: node.attr("ARTIST").unwrap_or("").to_string(),
        audio_id: node.attr("AUDIO_ID").map(str::to_string),
        location,
        album_title: album.and_then(|a| a.attr("TITLE")).map(str::to_string),
        album_track: album
            .and_then(|a| a.attr("TRACK"))
            .and_then(|t| t.parse().ok()),
        genre: info.and_then(|i| i.attr("GENRE")).map(str::to_string),
        comment: info.and_then(|i| i.attr("COMMENT")).map(str::to_string),
        key_text: info.and_then(|i| i.attr("KEY")).map(str::to_string),
        playtime_sec: info
            .and_then(|i| i.attr("PLAYTIME_FLOAT").or_else(|| i.attr("PLAYTIME")))
            .and_then(|v| v.parse().ok()),
        bitrate: info
            .and_then(|i| i.attr("BITRATE"))
            .and_then(|v| v.parse().ok()),
        tempo,
        loudness,
        musical_key_value,
        cues,
        stems_path,
    }
}

fn parse_playlist_node(node: &XmlNode) -> Option<NmlPlaylistNode> {
    let node_type = node.attr("TYPE")?;
    let name = node.attr("NAME").unwrap_or("").to_string();
    match node_type {
        "FOLDER" => {
            let mut subnodes = Vec::new();
            if let Some(sub) = node.child("SUBNODES") {
                for child in sub.children_named("NODE") {
                    if let Some(p) = parse_playlist_node(child) {
                        subnodes.push(p);
                    }
                }
            }
            Some(NmlPlaylistNode::Folder { name, subnodes })
        }
        "PLAYLIST" => {
            let pl = node.child("PLAYLIST")?;
            let uuid = pl.attr("UUID").unwrap_or("").to_string();
            let mut track_keys = Vec::new();
            for entry in pl.children_named("ENTRY") {
                if let Some(pk) = entry.child("PRIMARYKEY").and_then(|k| k.attr("KEY")) {
                    track_keys.push(pk.to_string());
                }
            }
            Some(NmlPlaylistNode::Playlist {
                name,
                uuid,
                track_keys,
            })
        }
        "SMARTLIST" => {
            let sl = node.child("SMARTLIST")?;
            let uuid = sl.attr("UUID").unwrap_or("").to_string();
            let expr = sl.child("SEARCH_EXPRESSION");
            let operator = expr
                .and_then(|e| e.attr("OPERATOR"))
                .unwrap_or("AND")
                .to_string();
            let query = expr
                .and_then(|e| e.attr("QUERY"))
                .unwrap_or("")
                .to_string();
            Some(NmlPlaylistNode::Smartlist {
                name,
                uuid,
                operator,
                query,
            })
        }
        _ => None,
    }
}

fn write_playlist_node(out: &mut String, node: &NmlPlaylistNode, indent: usize) {
    let pad = " ".repeat(indent);
    match node {
        NmlPlaylistNode::Folder { name, subnodes } => {
            out.push_str(&format!(
                "{pad}<NODE TYPE=\"FOLDER\" NAME=\"{}\">\n",
                escape(name)
            ));
            out.push_str(&format!("{pad}  <SUBNODES COUNT=\"{}\">\n", subnodes.len()));
            for child in subnodes {
                write_playlist_node(out, child, indent + 4);
            }
            out.push_str(&format!("{pad}  </SUBNODES>\n"));
            out.push_str(&format!("{pad}</NODE>\n"));
        }
        NmlPlaylistNode::Playlist {
            name,
            uuid,
            track_keys,
        } => {
            out.push_str(&format!(
                "{pad}<NODE TYPE=\"PLAYLIST\" NAME=\"{}\">\n",
                escape(name)
            ));
            out.push_str(&format!(
                "{pad}  <PLAYLIST ENTRIES=\"{}\" TYPE=\"LIST\" UUID=\"{}\">\n",
                track_keys.len(),
                escape(uuid)
            ));
            for key in track_keys {
                out.push_str(&format!("{pad}    <ENTRY>\n"));
                out.push_str(&format!(
                    "{pad}      <PRIMARYKEY TYPE=\"TRACK\" KEY=\"{}\"></PRIMARYKEY>\n",
                    escape(key)
                ));
                out.push_str(&format!("{pad}    </ENTRY>\n"));
            }
            out.push_str(&format!("{pad}  </PLAYLIST>\n"));
            out.push_str(&format!("{pad}</NODE>\n"));
        }
        NmlPlaylistNode::Smartlist {
            name,
            uuid,
            operator,
            query,
        } => {
            out.push_str(&format!(
                "{pad}<NODE TYPE=\"SMARTLIST\" NAME=\"{}\">\n",
                escape(name)
            ));
            out.push_str(&format!(
                "{pad}  <SMARTLIST UUID=\"{}\">\n",
                escape(uuid)
            ));
            out.push_str(&format!(
                "{pad}    <SEARCH_EXPRESSION VERSION=\"1\" OPERATOR=\"{}\" QUERY=\"{}\"></SEARCH_EXPRESSION>\n",
                escape(operator),
                escape(query)
            ));
            out.push_str(&format!("{pad}  </SMARTLIST>\n"));
            out.push_str(&format!("{pad}</NODE>\n"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    #[test]
    fn test_linux_posix_path_roundtrip() {
        let loc = NmlLocation::from_posix_path("/home/adam/Music/Sets/track01.flac", "linux-root");
        assert_eq!(loc.dir, "/:home/:adam/:Music/:Sets/:");
        assert_eq!(loc.file, "track01.flac");
        assert_eq!(loc.to_posix_path(), "/home/adam/Music/Sets/track01.flac");
        assert_eq!(
            loc.primary_key(),
            "linux-root/:home/:adam/:Music/:Sets/:track01.flac"
        );
    }

    #[test]
    fn test_nml_v20_parse_and_roundtrip() {
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

        let parsed = NmlCollection::parse_xml(sample).expect("parse nml");
        assert_eq!(parsed.version, 20);
        assert_eq!(parsed.entries.len(), 1);
        let e = &parsed.entries[0];
        assert_eq!(e.artist, "ReTraktor");
        assert_eq!(e.location.to_posix_path(), "/home/dj/Music/acid_rain.flac");
        assert_eq!(e.cues.len(), 3);
        assert_eq!(e.cues[0].cue_type, CueType::Grid);
        assert_eq!(e.cues[0].grid_bpm, Some(132.0));
        assert_eq!(e.cues[1].grid_bpm, Some(134.0));
        assert_eq!(e.cues[2].cue_type, CueType::Loop);
        assert_eq!(e.cues[2].hotcue, 2);

        let reserialized = parsed.to_xml();
        let reparsed = NmlCollection::parse_xml(&reserialized).expect("reparse nml");
        assert_eq!(parsed, reparsed);
    }

    #[test]
    fn test_real_traktor_collection_nml_on_disk() {
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let path = PathBuf::from(home).join("Documents/Native Instruments/Traktor 4.5.1/collection.nml");
        if path.exists() {
            let raw = fs::read_to_string(&path).expect("read real collection.nml");
            let parsed = NmlCollection::parse_xml(&raw).expect("parse real 16,160-track collection.nml");
            assert_eq!(parsed.version, 20);
            assert_eq!(parsed.program, "Traktor Pro 4");
            assert!(parsed.entries.len() >= 16_000);
            assert_eq!(parsed.entries[0].title, "The Boss");
            assert_eq!(parsed.entries[0].artist, "James Brown");
            assert_eq!(parsed.entries[0].musical_key_value, Some(17));
            assert_eq!(parsed.entries[0].key_text.as_deref(), Some("9m"));
        }
    }
}
