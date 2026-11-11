//! AI assistant module for ReTraktor.
//!
//! Provides playlist generation from natural-language descriptions, next-track suggestions
//! and re-ranking using the user's own API key (Gemini, OpenAI, Anthropic, or OpenAI-compatible).
//! Only metadata (title, artist, BPM, key, genre, energy, duration) is sent — never file paths or audio.

use crate::library_ui::{match_score, UiTrackEntry};
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AiProvider {
    #[default]
    Gemini,
    OpenAi,
    Anthropic,
    OpenAiCompatible,
}

impl AiProvider {
    pub const ALL: [Self; 4] = [Self::Gemini, Self::OpenAi, Self::Anthropic, Self::OpenAiCompatible];

    pub fn label(self) -> &'static str {
        match self {
            Self::Gemini => "Gemini",
            Self::OpenAi => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::OpenAiCompatible => "OpenAI-compatible",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::Gemini => "gemini-flash-latest",
            Self::OpenAi => "gpt-4o-mini",
            Self::Anthropic => "claude-haiku-5-5",
            Self::OpenAiCompatible => "llama3",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "http://localhost:11434/v1",
            _ => "",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::OpenAi => "openai",
            Self::Anthropic => "anthropic",
            Self::OpenAiCompatible => "openai-compatible",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "openai" => Self::OpenAi,
            "anthropic" => Self::Anthropic,
            "openai-compatible" | "openaicompatible" | "ollama" => Self::OpenAiCompatible,
            _ => Self::Gemini,
        }
    }
}

/// Persistent AI settings saved in `~/.config/retraktor/ai.txt`.
#[derive(Clone, Debug, PartialEq)]
pub struct AiSettings {
    pub provider: AiProvider,
    pub api_key: String,
    pub model: String,
    pub base_url: String,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            provider: AiProvider::Gemini,
            api_key: String::new(),
            model: AiProvider::Gemini.default_model().to_string(),
            base_url: String::new(),
        }
    }
}

fn ai_config_path() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".config/retraktor/ai.txt")
}

impl AiSettings {
    pub fn is_configured(&self) -> bool {
        match self.provider {
            AiProvider::OpenAiCompatible => !self.base_url.trim().is_empty(),
            _ => !self.api_key.trim().is_empty(),
        }
    }

    pub fn from_text(text: &str) -> Self {
        let setting = |key: &str| {
            text.lines()
                .find_map(|l| l.trim().strip_prefix(key).map(|v| v.trim().to_string()))
        };
        let provider = setting("provider=").map(|s| AiProvider::from_str(&s)).unwrap_or_default();
        let api_key = setting("api_key=").unwrap_or_default();
        let model = setting("model=")
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| provider.default_model().to_string());
        let base_url = setting("base_url=").unwrap_or_default();
        Self {
            provider,
            api_key,
            model,
            base_url,
        }
    }

    pub fn to_text(&self) -> String {
        format!(
            "provider={}\napi_key={}\nmodel={}\nbase_url={}\n",
            self.provider.as_str(),
            self.api_key,
            self.model,
            self.base_url,
        )
    }

    pub fn load() -> Self {
        let path = ai_config_path();
        let text = std::fs::read_to_string(path).unwrap_or_default();
        Self::from_text(&text)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = ai_config_path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        #[cfg(unix)]
        {
            use std::fs::OpenOptions;
            use std::os::unix::fs::OpenOptionsExt;
            use std::os::unix::fs::PermissionsExt;
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)?;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            file.write_all(self.to_text().as_bytes())?;
            file.flush()?;
            Ok(())
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, self.to_text())
        }
    }
}

/// Metadata-only summary of a track sent to the LLM.
#[derive(Clone, Debug, PartialEq)]
pub struct TrackBrief {
    pub id: usize,
    pub title: String,
    pub artist: String,
    pub bpm: f64,
    pub key: String,
    pub genre: String,
    pub energy: u8,
    pub duration_sec: f64,
}

impl TrackBrief {
    pub fn from_ui(t: &UiTrackEntry) -> Self {
        Self {
            id: t.id,
            title: t.title.clone(),
            artist: t.artist.clone(),
            bpm: t.bpm,
            key: t.musical_key.clone(),
            genre: t.genre.clone(),
            energy: t.energy,
            duration_sec: t.duration_sec,
        }
    }
}

/// Formats the prompt sent to the model for playlist building.
pub fn build_playlist_prompt(request: &str, tracks: &[TrackBrief], max_tracks: usize) -> String {
    let mut out = format!(
        "You are an expert DJ sequencing a playlist from a collection.\n\
         The user request is: \"{}\"\n\
         Select up to {} tracks that best fit this vibe and sequence them for seamless flow (harmonically compatible, smooth tempo and energy build).\n\n\
         Candidate tracks:\n",
        request.trim(),
        max_tracks
    );
    for t in tracks {
        let dur = if t.duration_sec > 0.0 {
            format!("{}:{:02}", (t.duration_sec / 60.0).floor() as u32, (t.duration_sec % 60.0).floor() as u32)
        } else {
            "-".to_string()
        };
        let bpm = if t.bpm > 0.0 { format!("{:.1}", t.bpm) } else { "-".to_string() };
        let energy = if t.energy > 0 { format!("{}", t.energy) } else { "-".to_string() };
        let key = if !t.key.is_empty() { &t.key } else { "-" };
        let genre = if !t.genre.is_empty() { &t.genre } else { "-" };
        out.push_str(&format!(
            "ID: {} | \"{}\" - {} | BPM: {} | Key: {} | E: {} | Genre: {} | Len: {}\n",
            t.id, t.title, t.artist, bpm, key, energy, genre, dur
        ));
    }
    out.push_str(
        "\nReturn ONLY a JSON array of track IDs in recommended sequence order, e.g. [14, 2, 88]. Do not include explanation outside the JSON array.\n"
    );
    out
}

/// Parses a list of track IDs from model output, handling code fences, surrounding prose, duplicates, and unknown IDs.
pub fn parse_id_list(reply: &str, valid_ids: &[usize]) -> Vec<usize> {
    let valid_set: HashSet<usize> = valid_ids.iter().copied().collect();
    let text = reply.trim();

    // Find the innermost or primary bracketed JSON array `[...]`
    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if start < end {
            let slice = &text[start..=end];
            if let Ok(nums) = serde_json::from_str::<Vec<serde_json::Value>>(slice) {
                let mut seen = HashSet::new();
                let mut out = Vec::new();
                for v in nums {
                    if let Some(id) = v.as_u64().map(|n| n as usize) {
                        if valid_set.contains(&id) && seen.insert(id) {
                            out.push(id);
                        }
                    }
                }
                if !out.is_empty() {
                    return out;
                }
            }
        }
    }

    // Fallback: extract integers from any brackets or text
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut cur = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else {
            if !cur.is_empty() {
                if let Ok(id) = cur.parse::<usize>() {
                    if valid_set.contains(&id) && seen.insert(id) {
                        out.push(id);
                    }
                }
                cur.clear();
            }
        }
    }
    if !cur.is_empty() {
        if let Ok(id) = cur.parse::<usize>() {
            if valid_set.contains(&id) && seen.insert(id) {
                out.push(id);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Per-provider request builders and extractors
// ---------------------------------------------------------------------------

pub fn build_gemini_request(prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "contents": [{
            "parts": [{ "text": prompt }]
        }]
    })
}

pub fn extract_gemini_reply(json: &serde_json::Value) -> Result<String, String> {
    if let Some(err) = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
        return Err(format!("Gemini API error: {err}"));
    }
    json.get("candidates")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|cand| cand.get("content"))
        .and_then(|cont| cont.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|parts| parts.first())
        .and_then(|part| part.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Gemini returned unexpected response format".to_string())
}

pub fn build_openai_request(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": "You are a professional DJ music curator. Output clean JSON when requested." },
            { "role": "user", "content": prompt }
        ]
    })
}

pub fn extract_openai_reply(json: &serde_json::Value) -> Result<String, String> {
    if let Some(err) = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
        return Err(format!("OpenAI error: {err}"));
    }
    json.get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|choice| choice.get("message"))
        .and_then(|msg| msg.get("content"))
        .and_then(|cnt| cnt.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "OpenAI returned unexpected response format".to_string())
}

pub fn build_anthropic_request(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "max_tokens": 2048,
        "messages": [
            { "role": "user", "content": prompt }
        ]
    })
}

pub fn extract_anthropic_reply(json: &serde_json::Value) -> Result<String, String> {
    if let Some(err) = json.get("error").and_then(|e| e.get("message")).and_then(|m| m.as_str()) {
        return Err(format!("Anthropic error: {err}"));
    }
    if let Some(content) = json.get("content").and_then(|c| c.as_array()) {
        for item in content {
            if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                if let Some(txt) = item.get("text").and_then(|t| t.as_str()) {
                    return Ok(txt.to_string());
                }
            }
        }
    }
    Err("Anthropic returned unexpected response format".to_string())
}

/// Executes an HTTP request to the selected provider with a 60-second timeout.
/// Pure synchronous call intended for a background thread.
pub fn call_ai_api(settings: &AiSettings, prompt: &str) -> Result<String, String> {
    let agent = ureq::builder()
        .timeout(Duration::from_secs(60))
        .build();

    match settings.provider {
        AiProvider::Gemini => {
            if settings.api_key.trim().is_empty() {
                return Err("Gemini API key is required".to_string());
            }
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent",
                settings.model.trim()
            );
            let body = build_gemini_request(prompt);
            // Key goes in a header, not the URL, so it can never appear in an error message.
            let resp = agent.post(&url)
                .set("x-goog-api-key", settings.api_key.trim())
                .set("Content-Type", "application/json")
                .send_json(body)
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            let val: serde_json::Value = resp.into_json().map_err(|e| format!("Failed to parse JSON reply: {e}"))?;
            extract_gemini_reply(&val)
        }
        AiProvider::OpenAi => {
            if settings.api_key.trim().is_empty() {
                return Err("OpenAI API key is required".to_string());
            }
            let url = "https://api.openai.com/v1/chat/completions";
            let body = build_openai_request(&settings.model, prompt);
            let resp = agent.post(url)
                .set("Authorization", &format!("Bearer {}", settings.api_key.trim()))
                .set("Content-Type", "application/json")
                .send_json(body)
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            let val: serde_json::Value = resp.into_json().map_err(|e| format!("Failed to parse JSON reply: {e}"))?;
            extract_openai_reply(&val)
        }
        AiProvider::Anthropic => {
            if settings.api_key.trim().is_empty() {
                return Err("Anthropic API key is required".to_string());
            }
            let url = "https://api.anthropic.com/v1/messages";
            let body = build_anthropic_request(&settings.model, prompt);
            let resp = agent.post(url)
                .set("x-api-key", settings.api_key.trim())
                .set("anthropic-version", "2023-06-01")
                .set("Content-Type", "application/json")
                .send_json(body)
                .map_err(|e| format!("HTTP request failed: {e}"))?;
            let val: serde_json::Value = resp.into_json().map_err(|e| format!("Failed to parse JSON reply: {e}"))?;
            extract_anthropic_reply(&val)
        }
        AiProvider::OpenAiCompatible => {
            let base = if settings.base_url.trim().is_empty() {
                "http://localhost:11434/v1"
            } else {
                settings.base_url.trim().trim_end_matches('/')
            };
            let url = format!("{base}/chat/completions");
            let body = build_openai_request(&settings.model, prompt);
            let mut req = agent.post(&url).set("Content-Type", "application/json");
            if !settings.api_key.trim().is_empty() {
                req = req.set("Authorization", &format!("Bearer {}", settings.api_key.trim()));
            }
            let resp = req.send_json(body).map_err(|e| format!("HTTP request failed: {e}"))?;
            let val: serde_json::Value = resp.into_json().map_err(|e| format!("Failed to parse JSON reply: {e}"))?;
            extract_openai_reply(&val)
        }
    }
}

// ---------------------------------------------------------------------------
// Pre-filtering for large collections (> 1500 tracks)
// ---------------------------------------------------------------------------

/// If the collection exceeds `limit` (e.g. 1500 tracks), prioritizes analysed tracks and
/// crude keyword/BPM matches from `request`. Returns `(filtered_briefs, total_tracks)`.
pub fn prefilter_tracks(tracks: &[UiTrackEntry], request: &str, limit: usize) -> (Vec<TrackBrief>, usize) {
    let total = tracks.len();
    if total <= limit {
        let briefs: Vec<_> = tracks
            .iter()
            .filter(|t| !t.missing && t.file_path.is_some())
            .map(TrackBrief::from_ui)
            .collect();
        return (briefs, total);
    }

    // Extract potential BPM ranges (e.g. "124", "120-130", "120 to 128")
    let words: Vec<String> = request
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .filter(|w| !w.is_empty())
        .collect();

    let mut target_bpms = Vec::new();
    for w in &words {
        if let Ok(n) = w.parse::<f64>() {
            if (60.0..=220.0).contains(&n) {
                target_bpms.push(n);
            }
        }
    }

    let mut scored: Vec<(f32, TrackBrief)> = tracks
        .iter()
        .filter(|t| !t.missing && t.file_path.is_some())
        .map(|t| {
            let mut s = 0.0f32;
            // Analysed tracks get baseline priority
            if t.bpm > 0.0 {
                s += 10.0;
            }
            if !t.musical_key.is_empty() {
                s += 10.0;
            }
            if t.energy > 0 {
                s += 5.0;
            }

            // BPM closeness if request mentions tempos
            if t.bpm > 0.0 {
                for &tb in &target_bpms {
                    let diff = (t.bpm - tb).abs();
                    if diff < 6.0 {
                        s += 25.0 - (diff as f32 * 2.0);
                    }
                }
            }

            // Keyword match in genre, artist, title
            let genre_lower = t.genre.to_lowercase();
            let artist_lower = t.artist.to_lowercase();
            let title_lower = t.title.to_lowercase();
            for w in &words {
                if w.len() >= 3 {
                    if genre_lower.contains(w) {
                        s += 20.0;
                    }
                    if artist_lower.contains(w) || title_lower.contains(w) {
                        s += 15.0;
                    }
                }
            }

            (s, TrackBrief::from_ui(t))
        })
        .collect();

    // Sort by score descending
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let briefs = scored.into_iter().take(limit).map(|(_, b)| b).collect();
    (briefs, total)
}

// ---------------------------------------------------------------------------
// Suggest Next Track: Pure scoring and explanations
// ---------------------------------------------------------------------------

/// Pure scoring function for next-track recommendations.
/// Takes compatibility from `match_score`, rules out played tracks from `history`,
/// and favors similar or slightly higher energy.
pub fn score_next_candidate(candidate: &UiTrackEntry, live_ref: &UiTrackEntry, history: &[usize]) -> f32 {
    if candidate.missing || candidate.file_path.is_none() {
        return 0.0;
    }
    if candidate.id == live_ref.id || history.contains(&candidate.id) {
        return 0.0;
    }
    let base = match_score(live_ref, candidate);
    if base <= 0.0 {
        return 0.0;
    }

    // Energy progression: similar (0) or slightly higher (+1) is ideal for maintaining or building energy.
    let energy_mult = if live_ref.energy > 0 && candidate.energy > 0 {
        let diff = candidate.energy as i32 - live_ref.energy as i32;
        match diff {
            1 => 1.25,  // slightly higher: building energy
            0 => 1.15,  // same energy: seamless hold
            2 => 1.05,  // 2 steps higher: punchy transition
            -1 => 0.90, // slight drop: acceptable breather
            _ if diff < -1 => 0.70, // big energy loss
            _ => 0.85,  // very large jump (+3 or more)
        }
    } else {
        1.0
    };

    base * energy_mult
}

/// Generates a concise one-line reason (key match, tempo delta, energy) for a candidate.
pub fn explain_candidate(candidate: &UiTrackEntry, live_ref: &UiTrackEntry) -> String {
    let mut parts = Vec::new();

    // Key match description
    let (ref_k, cand_k) = (live_ref.musical_key.as_str(), candidate.musical_key.as_str());
    if !cand_k.is_empty() {
        if !ref_k.is_empty() && ref_k == cand_k {
            parts.push(format!("{cand_k} exact"));
        } else if !ref_k.is_empty() && retraktor_library::KeyWheel::are_harmonically_compatible(ref_k, cand_k) {
            parts.push(format!("{cand_k} harmonic"));
        } else {
            parts.push(cand_k.to_string());
        }
    }

    // Tempo delta
    if candidate.bpm > 0.0 && live_ref.bpm > 0.0 {
        // Half and double tempo mix as the same tempo, so say that instead of a huge delta.
        let (mut cand, mut note) = (candidate.bpm, "");
        if cand < live_ref.bpm * 0.7 {
            (cand, note) = (cand * 2.0, "half tempo ");
        } else if cand > live_ref.bpm * 1.4 {
            (cand, note) = (cand / 2.0, "double tempo ");
        }
        let diff = cand - live_ref.bpm;
        if !note.is_empty() {
            parts.push(format!("{note}{diff:+.1} BPM"));
        } else if diff.abs() < 0.05 {
            parts.push(format!("{:.1} BPM", candidate.bpm));
        } else {
            parts.push(format!("{:+.1} BPM", diff));
        }
    } else if candidate.bpm > 0.0 {
        parts.push(format!("{:.1} BPM", candidate.bpm));
    }

    // Energy
    if candidate.energy > 0 {
        if live_ref.energy > 0 {
            let diff = candidate.energy as i32 - live_ref.energy as i32;
            let arrow = if diff > 0 { format!("+{diff}") } else if diff < 0 { format!("{diff}") } else { "±0".to_string() };
            parts.push(format!("E{} ({arrow})", candidate.energy));
        } else {
            parts.push(format!("E{}", candidate.energy));
        }
    }

    if parts.is_empty() {
        "Match candidate".to_string()
    } else {
        parts.join(" · ")
    }
}

/// Formats the prompt sent to the model for AI next-track re-ranking.
pub fn build_rerank_prompt(live_ref: &TrackBrief, candidates: &[TrackBrief]) -> String {
    let mut out = format!(
        "You are an expert DJ assisting live in the mix.\n\
         Currently playing track:\n\
         \"{}\" by \"{}\" | BPM: {:.1} | Key: {} | Energy: {}\n\n\
         Top candidates for the next track:\n",
        live_ref.title, live_ref.artist, live_ref.bpm, live_ref.key, live_ref.energy
    );
    for t in candidates {
        out.push_str(&format!(
            "ID: {} | \"{}\" by \"{}\" | BPM: {:.1} | Key: {} | Energy: {} | Genre: {}\n",
            t.id, t.title, t.artist, t.bpm, t.key, t.energy, t.genre
        ));
    }
    out.push_str(
        "\nINSTRUCTIONS:\n\
         1. Re-rank the candidates from best to worst to play immediately following the currently playing track.\n\
         2. Provide a short one-line reason (under 12 words) for why each of your top picks works in the mix.\n\
         3. Return ONLY a JSON array of objects with \"id\" and \"reason\", e.g.:\n\
         [{\"id\": 12, \"reason\": \"Smooth harmonic transition with crisp hi-hat lift\"}, ...]\n"
    );
    out
}

/// Parses the AI re-ranking reply into an ordered list of `(id, reason)`.
pub fn parse_rerank_reply(reply: &str, valid_candidates: &[TrackBrief]) -> Vec<(usize, String)> {
    let valid_ids: HashSet<usize> = valid_candidates.iter().map(|t| t.id).collect();
    let text = reply.trim();

    if let (Some(start), Some(end)) = (text.find('['), text.rfind(']')) {
        if start < end {
            let slice = &text[start..=end];
            if let Ok(arr) = serde_json::from_str::<Vec<serde_json::Value>>(slice) {
                let mut out = Vec::new();
                let mut seen = HashSet::new();
                for item in arr {
                    if let Some(id) = item.get("id").and_then(|v| v.as_u64()).map(|n| n as usize) {
                        if valid_ids.contains(&id) && seen.insert(id) {
                            let reason = item
                                .get("reason")
                                .and_then(|r| r.as_str())
                                .unwrap_or("Model recommendation")
                                .to_string();
                            out.push((id, reason));
                        }
                    }
                }
                if !out.is_empty() {
                    return out;
                }
            }
        }
    }

    // Fallback: parse simple ID list and generate default reasons
    let ids = parse_id_list(reply, &valid_candidates.iter().map(|c| c.id).collect::<Vec<_>>());
    ids.into_iter().map(|id| (id, "AI top pick".to_string())).collect()
}

// ---------------------------------------------------------------------------
// Background Job State Holders
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum JobStatus<T> {
    Idle,
    Loading(String),
    Success(T),
    Error(String),
}

/// Async runner for AI test requests in Preferences.
pub struct AiTestRunner {
    rx: Receiver<Result<String, String>>,
}

impl AiTestRunner {
    pub fn start(settings: AiSettings) -> Self {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let res = call_ai_api(&settings, "Reply with the single word: OK");
            let _ = tx.send(res);
        });
        Self { rx }
    }

    pub fn poll(&self) -> Option<Result<String, String>> {
        self.rx.try_recv().ok()
    }
}

/// Async runner for AI playlist generation.
pub struct AiPlaylistRunner {
    rx: Receiver<Result<Vec<usize>, String>>,
}

impl AiPlaylistRunner {
    pub fn start(settings: AiSettings, prompt: String, valid_ids: Vec<usize>) -> Self {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let res = call_ai_api(&settings, &prompt).and_then(|reply| {
                let ids = parse_id_list(&reply, &valid_ids);
                if ids.is_empty() {
                    Err("Model returned no valid track IDs from your library".to_string())
                } else {
                    Ok(ids)
                }
            });
            let _ = tx.send(res);
        });
        Self { rx }
    }

    pub fn poll(&self) -> Option<Result<Vec<usize>, String>> {
        self.rx.try_recv().ok()
    }
}

/// Async runner for Ask AI next-track re-ranking.
pub struct AiRerankRunner {
    rx: Receiver<Result<Vec<(usize, String)>, String>>,
}

impl AiRerankRunner {
    pub fn start(settings: AiSettings, prompt: String, candidates: Vec<TrackBrief>) -> Self {
        let (tx, rx) = channel();
        thread::spawn(move || {
            let res = call_ai_api(&settings, &prompt).map(|reply| {
                parse_rerank_reply(&reply, &candidates)
            });
            let _ = tx.send(res);
        });
        Self { rx }
    }

    pub fn poll(&self) -> Option<Result<Vec<(usize, String)>, String>> {
        self.rx.try_recv().ok()
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ai_settings_from_to_text() {
        let text = "provider=anthropic\napi_key=sk-ant-test\nmodel=claude-3-5-sonnet\nbase_url=http://localhost:8000\n";
        let s = AiSettings::from_text(text);
        assert_eq!(s.provider, AiProvider::Anthropic);
        assert_eq!(s.api_key, "sk-ant-test");
        assert_eq!(s.model, "claude-3-5-sonnet");
        assert_eq!(s.base_url, "http://localhost:8000");

        let out = s.to_text();
        assert!(out.contains("provider=anthropic"));
        assert!(out.contains("api_key=sk-ant-test"));
        assert!(out.contains("model=claude-3-5-sonnet"));
        assert!(out.contains("base_url=http://localhost:8000"));
    }

    #[test]
    fn test_parse_id_list_tolerates_fences_and_prose() {
        let valid = vec![1, 2, 3, 4, 5, 10, 20];

        // Clean JSON array
        assert_eq!(parse_id_list("[1, 2, 3]", &valid), vec![1, 2, 3]);

        // Code fence with prose
        let fenced = "Here is your playlist:\n```json\n[2, 4, 10, 999]\n```\nEnjoy!";
        assert_eq!(parse_id_list(fenced, &valid), vec![2, 4, 10]);

        // Duplicates removed, preserving first occurrence
        assert_eq!(parse_id_list("[3, 2, 3, 2, 5]", &valid), vec![3, 2, 5]);

        // Fallback digit extraction without brackets
        let prose = "I suggest tracks 1 and then 20 followed by 5.";
        assert_eq!(parse_id_list(prose, &valid), vec![1, 20, 5]);
    }

    #[test]
    fn test_gemini_request_and_reply() {
        let req = build_gemini_request("test prompt");
        assert_eq!(req["contents"][0]["parts"][0]["text"], "test prompt");

        let reply_json = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{ "text": "[10, 20, 30]" }]
                }
            }]
        });
        assert_eq!(extract_gemini_reply(&reply_json).unwrap(), "[10, 20, 30]");
    }

    #[test]
    fn test_openai_request_and_reply() {
        let req = build_openai_request("gpt-4o-mini", "test prompt");
        assert_eq!(req["model"], "gpt-4o-mini");
        assert_eq!(req["messages"][1]["content"], "test prompt");

        let reply_json = serde_json::json!({
            "choices": [{
                "message": { "content": "[4, 5, 6]" }
            }]
        });
        assert_eq!(extract_openai_reply(&reply_json).unwrap(), "[4, 5, 6]");
    }

    #[test]
    fn test_anthropic_request_and_reply() {
        let req = build_anthropic_request("claude-3-5-haiku", "test prompt");
        assert_eq!(req["model"], "claude-3-5-haiku");
        assert_eq!(req["messages"][0]["content"], "test prompt");

        let reply_json = serde_json::json!({
            "content": [{
                "type": "text",
                "text": "[7, 8, 9]"
            }]
        });
        assert_eq!(extract_anthropic_reply(&reply_json).unwrap(), "[7, 8, 9]");
    }

    #[test]
    fn test_score_next_candidate_and_history() {
        let live = UiTrackEntry {
            id: 1,
            bpm: 124.0,
            musical_key: "8m".to_string(),
            energy: 5,
            file_path: Some(PathBuf::from("/music/a.mp3")),
            ..Default::default()
        };

        let candidate_perfect = UiTrackEntry {
            id: 2,
            bpm: 124.0,
            musical_key: "8m".to_string(),
            energy: 6, // +1 energy boost
            file_path: Some(PathBuf::from("/music/b.mp3")),
            ..Default::default()
        };

        let candidate_same_e = UiTrackEntry {
            id: 3,
            bpm: 124.0,
            musical_key: "8m".to_string(),
            energy: 5,
            file_path: Some(PathBuf::from("/music/c.mp3")),
            ..Default::default()
        };

        let history = vec![1];
        let score_boost = score_next_candidate(&candidate_perfect, &live, &history);
        let score_same = score_next_candidate(&candidate_same_e, &live, &history);

        assert!(score_boost > 0.0);
        assert!(score_boost > score_same);

        // Played in history must receive 0.0
        let history_with_2 = vec![1, 2];
        assert_eq!(score_next_candidate(&candidate_perfect, &live, &history_with_2), 0.0);
    }

    #[test]
    fn test_prefilter_tracks_large_collection() {
        let mut tracks = Vec::new();
        for i in 0..2000 {
            tracks.push(UiTrackEntry {
                id: i,
                title: format!("Track {i}"),
                genre: if i % 10 == 0 { "Techno".to_string() } else { "Pop".to_string() },
                bpm: if i % 10 == 0 { 128.0 } else { 90.0 },
                energy: (i % 10) as u8,
                file_path: Some(PathBuf::from(format!("/music/{i}.mp3"))),
                ..Default::default()
            });
        }

        let (filtered, total) = prefilter_tracks(&tracks, "techno 128 bpm", 1500);
        assert_eq!(total, 2000);
        assert_eq!(filtered.len(), 1500);
        // First track in filtered should be high-scoring techno track
        assert_eq!(filtered[0].genre, "Techno");
        assert_eq!(filtered[0].bpm, 128.0);
    }
}
