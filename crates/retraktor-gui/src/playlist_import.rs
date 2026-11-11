//! Playlist import and library track matching for Spotify, TIDAL, M3U and plain text track lists.

use crate::library_ui::UiTrackEntry;

/// A track wanted from an imported playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wanted {
    pub artist: String,
    pub title: String,
}

/// Parses an exported playlist or track list from various formats:
/// - Exportify-style Spotify CSV (`Track Name`, `Artist Name(s)`)
/// - TuneMyMusic / generic CSV (`Track name` / `Artist name`, or `Title` / `Artist`)
/// - .m3u / .m3u8 `#EXTINF:123,Artist - Title` lines
/// - Plain text lines `Artist - Title`, `Artist – Title`, `1. Artist - Title`
/// Skips blanks and comments.
pub fn parse_track_list(text: &str) -> Vec<Wanted> {
    if let Some(tracks) = try_parse_csv(text) {
        if !tracks.is_empty() {
            return tracks;
        }
    }

    let has_extinf = text.lines().any(|l| l.trim().to_ascii_uppercase().starts_with("#EXTINF:"));
    let mut wanted = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if has_extinf {
            if trimmed.to_ascii_uppercase().starts_with("#EXTINF:") {
                // Format: #EXTINF:123,Artist - Title or #EXTINF:-1,Artist - Title
                if let Some((_, rest)) = trimmed.split_once(',') {
                    if let Some(w) = parse_plain_line(rest.trim()) {
                        wanted.push(w);
                    }
                }
            }
            // Skip non-EXTINF lines (file paths, directives, comments) in M3U
            continue;
        }

        // Plain text lines
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }

        if let Some(w) = parse_plain_line(trimmed) {
            wanted.push(w);
        }
    }

    wanted
}

/// Parses a single line in plain text format: `[1.] Artist - Title`.
fn parse_plain_line(line: &str) -> Option<Wanted> {
    let mut s = line.trim();

    // Strip leading track numbering: "1. ", "01. ", "1) ", "1: ", "1 - "
    if let Some(rest) = strip_leading_track_number(s) {
        s = rest;
    }

    // Split on delimiters between Artist and Title: " - ", " – ", " — ", or en/em dashes
    let (artist, title) = split_artist_title(s)?;
    let artist = artist.trim();
    let title = title.trim();

    if artist.is_empty() || title.is_empty() {
        return None;
    }

    Some(Wanted {
        artist: artist.to_string(),
        title: title.to_string(),
    })
}

fn strip_leading_track_number(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 {
        return None;
    }
    let rest = &s[i..];
    if let Some(stripped) = rest.strip_prefix(". ") {
        Some(stripped.trim_start())
    } else if let Some(stripped) = rest.strip_prefix(") ") {
        Some(stripped.trim_start())
    } else if let Some(stripped) = rest.strip_prefix(": ") {
        Some(stripped.trim_start())
    } else if let Some(stripped) = rest.strip_prefix(" - ") {
        // Only strip if there's another separator later (to not eat "1 - Title" as number)
        if stripped.contains(" - ") || stripped.contains(" – ") || stripped.contains(" — ") {
            Some(stripped.trim_start())
        } else {
            None
        }
    } else {
        None
    }
}

fn split_artist_title(s: &str) -> Option<(&str, &str)> {
    // Check spaced delimiters first: " - ", " – ", " — "
    for delim in &[" - ", " – ", " — "] {
        if let Some((a, b)) = s.split_once(delim) {
            return Some((a, b));
        }
    }
    // Check unspaced unicode dashes
    for delim in &['–', '—'] {
        if let Some((a, b)) = s.split_once(*delim) {
            return Some((a, b));
        }
    }
    None
}

/// Attempts to parse CSV if a recognised header row is present.
fn try_parse_csv(text: &str) -> Option<Vec<Wanted>> {
    let mut lines = text.lines();
    let mut header_row = None;
    let mut data_lines = Vec::new();

    for line in lines.by_ref() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if header_row.is_none() {
            let row = parse_csv_record(trimmed);
            if find_csv_columns(&row).is_some() {
                header_row = Some(row);
            } else {
                return None;
            }
        } else {
            data_lines.push(trimmed);
        }
    }

    let header = header_row?;
    let (title_col, artist_col) = find_csv_columns(&header)?;

    let mut wanted = Vec::new();
    for line in data_lines {
        let row = parse_csv_record(line);
        if title_col < row.len() && artist_col < row.len() {
            let title = row[title_col].trim();
            let artist = row[artist_col].trim();
            if !title.is_empty() && !artist.is_empty() {
                wanted.push(Wanted {
                    artist: artist.to_string(),
                    title: title.to_string(),
                });
            }
        }
    }

    Some(wanted)
}

fn find_csv_columns(row: &[String]) -> Option<(usize, usize)> {
    let mut title_idx = None;
    let mut artist_idx = None;

    for (i, col) in row.iter().enumerate() {
        let lower = col.trim().to_ascii_lowercase();
        let stripped = lower.replace(['\"', '\'', '_', '-'], " ");
        let name = stripped.split_whitespace().collect::<Vec<_>>().join(" ");

        if name == "track name" || name == "title" || name == "track" || name == "song" || name == "song title" {
            if title_idx.is_none() {
                title_idx = Some(i);
            }
        } else if name == "artist name(s)"
            || name == "artist names"
            || name == "artist name"
            || name == "artist"
            || name == "artists"
            || name == "track artist"
        {
            if artist_idx.is_none() {
                artist_idx = Some(i);
            }
        }
    }

    match (title_idx, artist_idx) {
        (Some(t), Some(a)) if t != a => Some((t, a)),
        _ => None,
    }
}

/// Parses an RFC 4180 CSV line handling quoted fields and doubled quotes `""`.
fn parse_csv_record(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    current.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                current.push(c);
            }
        } else if c == '"' {
            in_quotes = true;
        } else if c == ',' {
            fields.push(current.trim().to_string());
            current.clear();
        } else {
            current.push(c);
        }
    }
    fields.push(current.trim().to_string());
    fields
}

/// Normalisation and matching logic.
///
/// Matches wanted tracks against the library:
/// 1. Normalises both sides (lowercase, accents stripped, drop bracketed parts like "(Original Mix)",
///    "- Radio Edit", "feat. X", punctuation and extra spaces; compares first listed artist).
/// 2. Exact normalised match first, then title match with artist containment.
/// 3. Prefers non-missing tracks, then higher bitrate when several match.
/// 4. Remix suffix must NOT match the original when the wanted title names a different remix;
///    "feat." differences still match.
pub fn match_tracks(wanted: &[Wanted], library: &[UiTrackEntry]) -> (Vec<usize>, Vec<Wanted>) {
    let mut matched_ids = Vec::with_capacity(wanted.len());
    let mut missing = Vec::new();

    // Pre-calculate normalized information for library tracks
    let lib_norm: Vec<NormTrack> = library
        .iter()
        .map(|t| NormTrack {
            id: t.id,
            artist: normalize_artist(&t.artist),
            title: normalize_title(&t.title),
            missing: t.missing,
            bitrate_kbps: t.bitrate_kbps,
        })
        .collect();

    for w in wanted {
        let w_artist = normalize_artist(&w.artist);
        let w_title = normalize_title(&w.title);

        let mut best: Option<(usize, bool, u32, usize)> = None; // (match_level, non_missing, bitrate, id)

        for l in &lib_norm {
            if !title_compatible(&w_title, &l.title) {
                continue;
            }

            let match_level = artist_match_level(&w_artist, &l.artist);
            if match_level == 0 {
                continue;
            }

            // match_level 2 = exact, 1 = containment
            let non_missing = !l.missing;
            let bitrate = l.bitrate_kbps;
            let id = l.id;

            let candidate = (match_level, non_missing, bitrate, id);
            let is_better = match &best {
                None => true,
                Some(current) => {
                    // Compare: higher match_level, higher non_missing, higher bitrate
                    candidate > *current
                }
            };

            if is_better {
                best = Some(candidate);
            }
        }

        if let Some((_, _, _, id)) = best {
            matched_ids.push(id);
        } else {
            missing.push(w.clone());
        }
    }

    (matched_ids, missing)
}

#[derive(Clone, Debug)]
struct NormArtist {
    first: String,
    full: String,
}

#[derive(Clone, Debug)]
struct NormTitle {
    base: String,
    remix: Option<String>,
}

#[derive(Clone, Debug)]
struct NormTrack {
    id: usize,
    artist: NormArtist,
    title: NormTitle,
    missing: bool,
    bitrate_kbps: u32,
}

fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => 'a',
            'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
            'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => 'e',
            'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ĭ' | 'į' => 'i',
            'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' | 'Ĭ' | 'Į' => 'i',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => 'o',
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ŏ' | 'Ő' => 'o',
            'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
            'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => 'u',
            'ý' | 'ÿ' | 'ŷ' => 'y',
            'Ý' | 'Ÿ' | 'Ŷ' => 'y',
            'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
            'Ç' | 'Ć' | 'Ĉ' | 'Ċ' | 'Č' => 'c',
            'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
            'Ñ' | 'Ń' | 'Ņ' | 'Ň' => 'n',
            'š' | 'ś' | 'ŝ' | 'ş' => 's',
            'Š' | 'Ś' | 'Ŝ' | 'Ş' => 's',
            'ž' | 'ź' | 'ż' => 'z',
            'Ž' | 'Ź' | 'Ż' => 'z',
            'ß' => 's',
            'æ' => 'a',
            'Æ' => 'a',
            'œ' => 'o',
            'Œ' => 'o',
            _ => c,
        })
        .collect()
}

fn clean_punctuation(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if c == '\'' || c == '’' {
            // Drop apostrophes so "don't" -> "dont"
            continue;
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_artist(raw: &str) -> NormArtist {
    let unaccented = strip_accents(raw).to_ascii_lowercase();

    // Extract first artist before separators: " feat. ", " ft. ", " & ", ", ", " / ", " vs. ", etc.
    let mut first_raw = unaccented.as_str();
    for sep in &[
        " feat. ", " feat ", " ft. ", " ft ", " featuring ",
        " (feat. ", " (ft. ", " (featuring ",
        " & ", " and ", ",", " / ", " \\ ", " x ", " vs. ", " vs ", " pres. ", " presents ",
    ] {
        if let Some((f, _)) = first_raw.split_once(sep) {
            first_raw = f;
        }
    }

    let first = clean_punctuation(first_raw);
    let full = clean_punctuation(&unaccented);

    NormArtist { first, full }
}

fn normalize_title(raw: &str) -> NormTitle {
    let unaccented = strip_accents(raw).to_ascii_lowercase();

    // Extract bracketed parts: (...) or [...]
    let mut base_str = String::new();
    let mut remix = None;

    let mut i = 0;
    let chars: Vec<char> = unaccented.chars().collect();
    while i < chars.len() {
        let c = chars[i];
        if c == '(' || c == '[' {
            let close = if c == '(' { ')' } else { ']' };
            let mut inner = String::new();
            i += 1;
            while i < chars.len() && chars[i] != close {
                inner.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip closing bracket
            }
            let inner_trim = inner.trim();
            if is_dropped_bracket(inner_trim) {
                // Drop: original mix, radio edit, feat. X, etc.
            } else if is_remix_bracket(inner_trim) {
                remix = Some(clean_punctuation(inner_trim));
            } else {
                // Other bracketed content, check if it contains remix/mix
                if inner_trim.contains("remix") || inner_trim.contains("mix") || inner_trim.contains("dub") {
                    remix = Some(clean_punctuation(inner_trim));
                }
            }
        } else {
            base_str.push(c);
            i += 1;
        }
    }

    // Also check for trailing " - Radio Edit", " - Original Mix", or " - X Remix"
    if let Some((main, suffix)) = base_str.rsplit_once(" - ") {
        let s = suffix.trim().to_string();
        let m = main.to_string();
        if is_dropped_bracket(&s) {
            base_str = m;
        } else if is_remix_bracket(&s) {
            base_str = m;
            if remix.is_none() {
                remix = Some(clean_punctuation(&s));
            }
        }
    }

    // Strip trailing feat. from title: "Song feat. Artist"
    for feat_pat in &[" feat. ", " feat ", " ft. ", " ft ", " featuring "] {
        if let Some((main, _)) = base_str.split_once(feat_pat) {
            let m = main.to_string();
            base_str = m;
        }
    }

    let base = clean_punctuation(&base_str);
    NormTitle { base, remix }
}

fn is_dropped_bracket(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.starts_with("feat.")
        || lower.starts_with("feat ")
        || lower.starts_with("ft.")
        || lower.starts_with("ft ")
        || lower.starts_with("featuring ")
        || lower == "original mix"
        || lower == "original"
        || lower == "radio edit"
        || lower == "album version"
        || lower == "extended mix"
        || lower == "extended"
        || lower == "club mix"
        || lower == "clean"
        || lower == "explicit"
}

fn is_remix_bracket(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    (lower.contains("remix") || lower.contains("dub") || lower.contains("vip") || lower.contains("bootleg") || lower.contains("rework") || lower.contains("flip"))
        && !lower.contains("original")
}

fn title_compatible(w: &NormTitle, l: &NormTitle) -> bool {
    if w.base != l.base {
        return false;
    }

    match (&w.remix, &l.remix) {
        // Both original / standard mix -> matches
        (None, None) => true,
        // Wanted names a specific remix, library has no remix (original) -> NO match
        (Some(_), None) => false,
        // Wanted is original, library has a specific remix -> NO match
        (None, Some(_)) => false,
        // Both name remixes -> must match
        (Some(wr), Some(lr)) => wr == lr,
    }
}

/// Returns: 2 = exact match, 1 = containment match, 0 = no match
fn artist_match_level(w: &NormArtist, l: &NormArtist) -> usize {
    // Exact first-artist or full-artist match
    if !w.first.is_empty() && w.first == l.first {
        return 2;
    }
    if !w.full.is_empty() && w.full == l.full {
        return 2;
    }
    // Also ignore leading "the " for exact match: "the chemical brothers" == "chemical brothers"
    let w_strip_the = w.first.strip_prefix("the ").unwrap_or(&w.first);
    let l_strip_the = l.first.strip_prefix("the ").unwrap_or(&l.first);
    if !w_strip_the.is_empty() && w_strip_the == l_strip_the {
        return 2;
    }

    // Containment match
    if !w.first.is_empty() && (l.full.contains(&w.first) || w.full.contains(&l.first)) {
        return 1;
    }
    if !w.full.is_empty() && (l.full.contains(&w.full) || w.full.contains(&l.full)) {
        return 1;
    }

    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_exportify_csv() {
        let csv = "\
\"Track URI\",\"Track Name\",\"Artist Name(s)\",\"Album Name\",\"Album Artist Name(s)\"
\"spotify:track:0VjIjW4GlUZAMYd2vXMi3b\",\"Blinding Lights\",\"The Weeknd\",\"After Hours\",\"The Weeknd\"
\"spotify:track:2takcwOaAZWiRQiPHTrPUB\",\"Don't Start Now\",\"Dua Lipa\",\"Future Nostalgia\",\"Dua Lipa\"
\"spotify:track:123\",\"Song, With \"\"Doubled\"\" Quotes\",\"Artist, The\",\"Album\",\"Artist\"
";
        let parsed = parse_track_list(csv);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].title, "Blinding Lights");
        assert_eq!(parsed[0].artist, "The Weeknd");
        assert_eq!(parsed[1].title, "Don't Start Now");
        assert_eq!(parsed[1].artist, "Dua Lipa");
        assert_eq!(parsed[2].title, "Song, With \"Doubled\" Quotes");
        assert_eq!(parsed[2].artist, "Artist, The");
    }

    #[test]
    fn parse_tunemymusic_and_generic_csv() {
        let csv = "\
Track name,Artist name,Album
One More Time,Daft Punk,Discovery
Around the World,Daft Punk,Homework
";
        let parsed = parse_track_list(csv);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "One More Time");
        assert_eq!(parsed[0].artist, "Daft Punk");

        let csv2 = "\
Title,Artist,BPM
Glue,Bicep,130
Archangel,Burial,135
";
        let parsed2 = parse_track_list(csv2);
        assert_eq!(parsed2.len(), 2);
        assert_eq!(parsed2[0].title, "Glue");
        assert_eq!(parsed2[0].artist, "Bicep");
    }

    #[test]
    fn parse_m3u_extinf() {
        let m3u = "\
#EXTM3U
#EXTINF:234,Daft Punk - One More Time
/music/daft_punk/one_more_time.mp3
#EXTINF:-1,Bicep – Glue
https://stream.example.com/glue.flac
# Comment line
";
        let parsed = parse_track_list(m3u);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].artist, "Daft Punk");
        assert_eq!(parsed[0].title, "One More Time");
        assert_eq!(parsed[1].artist, "Bicep");
        assert_eq!(parsed[1].title, "Glue");
    }

    #[test]
    fn parse_plain_text_numbered_and_dashes() {
        let text = "\
1. Daft Punk - One More Time
02. Bicep – Glue
3) Burial — Archangel
Four Tet - Baby

# A comment
// Another comment
";
        let parsed = parse_track_list(text);
        assert_eq!(parsed.len(), 4);
        assert_eq!(parsed[0].artist, "Daft Punk");
        assert_eq!(parsed[0].title, "One More Time");
        assert_eq!(parsed[1].artist, "Bicep");
        assert_eq!(parsed[1].title, "Glue");
        assert_eq!(parsed[2].artist, "Burial");
        assert_eq!(parsed[2].title, "Archangel");
        assert_eq!(parsed[3].artist, "Four Tet");
        assert_eq!(parsed[3].title, "Baby");
    }

    #[test]
    fn test_accents_and_case_normalization() {
        let wanted = vec![Wanted {
            artist: "Beyoncé".to_string(),
            title: "CUFF IT".to_string(),
        }];
        let library = vec![UiTrackEntry {
            id: 42,
            artist: "Beyonce".to_string(),
            title: "Cuff It".to_string(),
            bitrate_kbps: 320,
            missing: false,
            ..Default::default()
        }];

        let (matched, missing) = match_tracks(&wanted, &library);
        assert_eq!(matched, vec![42]);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_remix_suffix_must_not_match_original_or_different_remix() {
        let wanted_remix = vec![Wanted {
            artist: "Deadmau5".to_string(),
            title: "Strobe (Feed Me Remix)".to_string(),
        }];

        let lib_original = vec![UiTrackEntry {
            id: 1,
            artist: "Deadmau5".to_string(),
            title: "Strobe (Original Mix)".to_string(),
            bitrate_kbps: 320,
            missing: false,
            ..Default::default()
        }];
        let (matched, missing) = match_tracks(&wanted_remix, &lib_original);
        assert!(matched.is_empty(), "Remix must not match Original Mix");
        assert_eq!(missing.len(), 1);

        let lib_other_remix = vec![UiTrackEntry {
            id: 2,
            artist: "Deadmau5".to_string(),
            title: "Strobe (Pluto Remix)".to_string(),
            bitrate_kbps: 320,
            missing: false,
            ..Default::default()
        }];
        let (matched, missing) = match_tracks(&wanted_remix, &lib_other_remix);
        assert!(matched.is_empty(), "Feed Me Remix must not match Pluto Remix");
        assert_eq!(missing.len(), 1);

        let lib_matching_remix = vec![UiTrackEntry {
            id: 3,
            artist: "Deadmau5".to_string(),
            title: "Strobe - Feed Me Remix".to_string(),
            bitrate_kbps: 320,
            missing: false,
            ..Default::default()
        }];
        let (matched, missing) = match_tracks(&wanted_remix, &lib_matching_remix);
        assert_eq!(matched, vec![3]);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_feat_differences_should_match() {
        let wanted = vec![Wanted {
            artist: "Daft Punk feat. Pharrell Williams".to_string(),
            title: "Get Lucky".to_string(),
        }];
        let library = vec![UiTrackEntry {
            id: 10,
            artist: "Daft Punk".to_string(),
            title: "Get Lucky (feat. Pharrell Williams)".to_string(),
            bitrate_kbps: 320,
            missing: false,
            ..Default::default()
        }];

        let (matched, missing) = match_tracks(&wanted, &library);
        assert_eq!(matched, vec![10]);
        assert!(missing.is_empty());
    }

    #[test]
    fn test_preference_non_missing_and_higher_bitrate() {
        let wanted = vec![Wanted {
            artist: "Bicep".to_string(),
            title: "Glue".to_string(),
        }];
        let library = vec![
            UiTrackEntry {
                id: 1,
                artist: "Bicep".to_string(),
                title: "Glue".to_string(),
                bitrate_kbps: 128,
                missing: false,
                ..Default::default()
            },
            UiTrackEntry {
                id: 2,
                artist: "Bicep".to_string(),
                title: "Glue".to_string(),
                bitrate_kbps: 1411,
                missing: true, // Missing file with higher bitrate
                ..Default::default()
            },
            UiTrackEntry {
                id: 3,
                artist: "Bicep".to_string(),
                title: "Glue".to_string(),
                bitrate_kbps: 320,
                missing: false, // Non-missing with higher bitrate than id 1
                ..Default::default()
            },
        ];

        let (matched, _) = match_tracks(&wanted, &library);
        assert_eq!(matched, vec![3], "Should prefer non-missing track with highest bitrate");
    }
}
