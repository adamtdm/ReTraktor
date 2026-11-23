//! Remix decks: a deck that plays short samples from a grid of cells instead of a track.
//!
//! Each deck has 4 columns of 16 cells (four pages of four rows). A column sounds one cell at a time: launching
//! another cell in it replaces the one playing, on the next beat or at once. The samples decode on one worker
//! thread ([`RemixLoader`]) into memory counted against one budget ([`SampleBudget`]); the audio callback plays
//! them through [`RemixEngine`] into the deck's own mixer channel, so its gain, EQ, filter, cue, fader and the
//! recorder treat it like the track it replaces.
//!
//! A cell can also capture a playing Track deck's active loop ([`CaptureSource`]): the same worker copies those
//! frames of the track's decoded audio and saves them as a WAV in [`captures_dir`], which the cell then plays and a
//! saved set names like any other sample file.

use crate::audio::{AudioFile, Retired};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc};

pub const COLUMNS: usize = 4;
/// Cells per column: four pages of [`PAGE_ROWS`].
pub const ROWS: usize = 16;
pub const PAGE_ROWS: usize = 4;
/// The longest sample a cell takes; longer files belong on a Track deck.
pub const MAX_SAMPLE_SEC: f64 = 60.0;
/// Memory for every cell's audio, on all decks, plus the one being decoded.
/// shortcut: a fixed 1 GiB (about 45 minutes of 48 kHz stereo), make it a preference if banks need more.
pub const BANK_BUDGET_BYTES: usize = 1 << 30;
/// How long a stopped or replaced cell fades out, so the cut never clicks.
const FADE_SEC: f64 = 0.003;

/// Bytes of sample audio in memory or being decoded, against a limit. Shared by the loader and every sample,
/// which gives its bytes back when its last reference is dropped.
pub struct SampleBudget {
    used: AtomicUsize,
    limit: usize,
}

impl SampleBudget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self { used: AtomicUsize::new(0), limit })
    }

    /// Takes `bytes` if they fit under the limit.
    fn try_take(&self, bytes: usize) -> bool {
        self.used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| used.checked_add(bytes).filter(|&n| n <= self.limit))
            .is_ok()
    }

    fn give_back(&self, bytes: usize) {
        self.used.fetch_sub(bytes, Ordering::AcqRel);
    }

    pub fn used(&self) -> usize {
        self.used.load(Ordering::Acquire)
    }
}

/// One decoded sample, as a cell holds it and the callback plays it. The callback never drops one: it hands
/// them back to be freed on the UI thread.
pub struct RemixSample {
    /// Unique for every decode, so the UI can tell a refilled cell from the audio still playing.
    pub id: u64,
    pub name: String,
    /// The file it was decoded from (empty for one made in memory), as a saved set names it.
    pub source: PathBuf,
    /// The library BPM it was decoded with, before fitting; a saved set decodes it with the same again.
    pub library_bpm: f64,
    /// Interleaved stereo at `sample_rate`.
    pub pcm: Vec<f32>,
    pub sample_rate: u32,
    /// The tempo a loop follows the master at: the library's BPM, fitted to the sample's length when that is a
    /// whole number of beats. `None`: unknown, so it plays at its own speed.
    pub bpm: Option<f64>,
    /// Its length is a whole number of beats at that BPM: a loop by default, else a one-shot.
    pub fits_beats: bool,
    budget: Arc<SampleBudget>,
    bytes: usize,
}

impl Drop for RemixSample {
    fn drop(&mut self) {
        self.budget.give_back(self.bytes);
    }
}

static NEXT_SAMPLE_ID: AtomicU64 = AtomicU64::new(1);

impl RemixSample {
    pub fn frames(&self) -> usize {
        self.pcm.len() / 2
    }

    pub fn seconds(&self) -> f64 {
        self.frames() as f64 / self.sample_rate.max(1) as f64
    }

    /// A sample made in memory, counted against `budget` like a decoded one (tests and fixtures).
    pub fn from_pcm(name: &str, pcm: Vec<f32>, sample_rate: u32, library_bpm: f64, budget: &Arc<SampleBudget>) -> Result<Self, String> {
        let bytes = pcm.len() * 4;
        if !budget.try_take(bytes) {
            return Err(bank_full());
        }
        Ok(Self::new(name, PathBuf::new(), pcm, sample_rate, library_bpm, Arc::clone(budget), bytes))
    }

    fn new(name: &str, source: PathBuf, pcm: Vec<f32>, sample_rate: u32, library_bpm: f64, budget: Arc<SampleBudget>, bytes: usize) -> Self {
        let seconds = (pcm.len() / 2) as f64 / sample_rate.max(1) as f64;
        let (bpm, fits_beats) = fitted_bpm(library_bpm, seconds);
        let id = NEXT_SAMPLE_ID.fetch_add(1, Ordering::Relaxed);
        Self { id, name: name.to_string(), source, library_bpm, pcm, sample_rate, bpm, fits_beats, budget, bytes }
    }

    /// Decodes `path` for a cell. Refuses a file longer than [`MAX_SAMPLE_SEC`] as soon as its decoded audio
    /// passes that (a header's length is only an estimate, so it decides nothing), and stops as soon as the bank's
    /// memory would run out or `cancelled` says the cell no longer wants it, so a whole track is never decoded just
    /// to be thrown away. With `whole`, a file is also refused unless it decodes without a damaged packet to exactly
    /// the length its header states (an imported set's FLAC or WAV, whose header is exact): a file cut short isn't
    /// taken as a shorter sample.
    pub fn decode(path: &Path, name: &str, library_bpm: f64, budget: &Arc<SampleBudget>, cancelled: &dyn Fn() -> bool, whole: bool) -> Result<Self, String> {
        let mut file = AudioFile::open(path)?;
        let max_samples = (MAX_SAMPLE_SEC * file.sample_rate as f64) as usize * 2;
        let too_long = || format!("'{name}' is longer than {MAX_SAMPLE_SEC:.0} s: remix cells take samples (load whole tracks on a Track deck)");
        let mut pcm: Vec<f32> = Vec::new();
        let mut taken = 0usize;
        let mut packet = Vec::new();
        let outcome = loop {
            if cancelled() {
                break Err("Cancelled".to_string());
            }
            packet.clear();
            if file.next_stereo(&mut packet).is_none() {
                break Ok(());
            }
            let need = pcm.len() + packet.len();
            if need > max_samples {
                break Err(too_long());
            }
            if need * 4 > taken {
                // Grow by doubling, never past the longest sample, and only with the budget's leave.
                let grow_to = need.max(pcm.capacity() * 2).min(max_samples);
                if !budget.try_take(grow_to * 4 - taken) {
                    break Err(bank_full());
                }
                taken = grow_to * 4;
                pcm.reserve_exact(grow_to - pcm.len());
            }
            pcm.extend_from_slice(&packet);
        };
        let frames = (pcm.len() / 2) as u64;
        let outcome = outcome.and_then(|()| match file.header_frames {
            _ if pcm.is_empty() => Err(format!("No audio decoded from '{name}'")),
            stated if whole && (file.damaged || stated != Some(frames)) => {
                let stated = stated.map_or("an unstated number of".to_string(), |n| n.to_string());
                Err(format!("'{name}' is damaged or cut short: {frames} frames decoded of {stated} in its header"))
            }
            _ => Ok(()),
        });
        if let Err(e) = outcome {
            budget.give_back(taken);
            return Err(e);
        }
        pcm.shrink_to_fit();
        let bytes = pcm.len() * 4;
        budget.give_back(taken - bytes);
        Ok(Self::new(name, path.to_path_buf(), pcm, file.sample_rate, library_bpm, Arc::clone(budget), bytes))
    }

    /// Copies `source`'s loop out of the track's audio and saves it as a new WAV in `dir` ([`write_capture`]), which
    /// becomes the sample's file. Refused (nothing taken or left behind) when the loop can't be a cell
    /// ([`CaptureSource::frames`]), the bank is full, the file can't be written or `cancelled` says the cell no
    /// longer wants it.
    pub fn capture(source: &CaptureSource, dir: &Path, budget: &Arc<SampleBudget>, cancelled: &dyn Fn() -> bool) -> Result<Self, String> {
        let (start, end) = source.frames()?;
        let bytes = (end - start) * 2 * 4;
        if !budget.try_take(bytes) {
            return Err(bank_full());
        }
        let title = source.title.trim();
        let title = if title.is_empty() { "Deck capture" } else { title };
        let at = start as f64 / source.sample_rate as f64;
        let (minutes, seconds) = ((at / 60.0).floor(), at % 60.0);
        let name = format!("{title} loop {minutes}:{seconds:06.3}");
        // The sample owns the bytes from here: dropped on any failure below, it gives them back.
        let mut sample = Self::new(&name, PathBuf::new(), source.pcm[2 * start..2 * end].to_vec(), source.sample_rate, source.bpm, Arc::clone(budget), bytes);
        if cancelled() {
            return Err("Cancelled".to_string());
        }
        let stem = format!("{} loop {minutes:.0}m{seconds:06.3}s {}", file_stem_text(title), crate::setlist::format_recording_timestamp(std::time::SystemTime::now()));
        let path = write_capture(dir, &stem, &sample.pcm, sample.sample_rate)?;
        if cancelled() {
            let _ = std::fs::remove_file(&path); // no cell ever held it
            return Err("Cancelled".to_string());
        }
        sample.source = path;
        Ok(sample)
    }
}

/// A playing Track deck's active loop, read in one go on the UI thread: the track's decoded audio (shared, not
/// copied, and never changed by the capture) and the loop as the deck plays it.
pub struct CaptureSource {
    /// The track's interleaved stereo, at its own rate, before any gain or effect.
    pub pcm: Arc<Vec<f32>>,
    pub sample_rate: u32,
    pub loop_start_sec: f64,
    pub loop_len_sec: f64,
    /// The track's BPM, which the loop's length is a whole number of beats of.
    pub bpm: f64,
    pub title: String,
}

impl CaptureSource {
    /// The loop's frames in the track, `start..end`, each end rounded to the nearest whole frame. Refused when that
    /// is no frame at all, isn't inside the decoded audio, or is longer than a cell takes ([`MAX_SAMPLE_SEC`]).
    pub fn frames(&self) -> Result<(usize, usize), String> {
        let rate = self.sample_rate as f64;
        let start = (self.loop_start_sec * rate).round();
        let end = ((self.loop_start_sec + self.loop_len_sec) * rate).round();
        if self.sample_rate == 0 || !start.is_finite() || !end.is_finite() || start < 0.0 {
            return Err("the loop isn't a valid place in the track".into());
        }
        let (start, end) = (start as usize, end as usize);
        if end <= start {
            return Err("the loop is shorter than one frame".into());
        }
        if end > self.pcm.len() / 2 {
            return Err("the loop runs past the end of the track's audio: move it inside the track".into());
        }
        if end - start > (MAX_SAMPLE_SEC * rate) as usize {
            return Err(format!("the loop is longer than {MAX_SAMPLE_SEC:.0} s, the most a cell takes: shorten it"));
        }
        Ok((start, end))
    }
}

/// Where captured loops are saved: `~/Music/ReTraktor/Captures`, beside the master recordings. Refused for a home
/// folder that isn't an absolute UTF-8 path, which a saved set couldn't name.
pub fn captures_dir() -> Result<PathBuf, String> {
    captures_dir_in(std::env::var_os("HOME"))
}

fn captures_dir_in(home: Option<std::ffi::OsString>) -> Result<PathBuf, String> {
    let home = home.map(PathBuf::from).filter(|home| home.is_absolute() && home.to_str().is_some());
    let home = home.ok_or("there is no home folder a set can name (HOME isn't an absolute UTF-8 path), so captured and imported samples have nowhere to be saved")?;
    Ok(home.join("Music").join("ReTraktor").join("Captures"))
}

/// Where imported Traktor Remix Sets keep their samples: `~/Music/ReTraktor/Remix Sets`, a new folder per import
/// ([`import_trak`]). Refused as [`captures_dir`] is.
pub fn imports_dir() -> Result<PathBuf, String> {
    captures_dir().map(|dir| dir.with_file_name("Remix Sets"))
}

/// The folder of samples one import copied out of a Remix Set, owned by that import's result: dropped without
/// [`Imported::keep`] (the import failed, was cancelled or superseded, or its deck couldn't take it), the folder is
/// removed with everything in it. Only that import ever wrote there.
pub struct Imported {
    dir: PathBuf,
    /// What of the set ReTraktor doesn't carry over (settings, shortened names), for the status line (empty: nothing).
    pub notes: String,
    kept: bool,
}

impl Imported {
    /// The deck took the samples: they stay for good, for a saved set to name. Returns their folder.
    pub fn keep(mut self) -> PathBuf {
        self.kept = true;
        std::mem::take(&mut self.dir)
    }
}

impl Drop for Imported {
    fn drop(&mut self) {
        if !self.kept {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }
}

/// The most bytes of samples one import copies out of a set: no bank holds more audio than this.
/// shortcut: caps the files' bytes, not their decoded audio, raise it if multichannel sets need more.
const MAX_IMPORT_BYTES: u64 = BANK_BUDGET_BYTES as u64;

/// Imports the Traktor Remix Set (`.trak`) at `path`, which is only read, into a new bank, all or nothing: each
/// filled cell in its place with its name, as a loop or a one-shot, Q as the set has it and page 1 showing. Each
/// sample file the cells play is copied out once, under its exact name in the set, into a new folder in `root` that
/// only this import writes to (cells sharing a file share its copy), then decoded whole ([`RemixSample::decode`]).
/// A synced cell keeps its BPM; an unsynced one plays at its own speed. On any failure, or once `cancelled`, the
/// folder is removed again; on success the returned [`Imported`] owns it. Errors read on their own.
pub fn import_trak(path: &Path, root: &Path, budget: &Arc<SampleBudget>, cancelled: &dyn Fn() -> bool) -> Result<(RemixBank, Imported), Vec<String>> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let file_name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
    let mut file = std::fs::File::open(path).map_err(|e| vec![format!("'{file_name}' can't be read ({e})")])?;
    let set = retraktor_formats::trak::read_remix_set(&mut file).map_err(|e| vec![format!("'{file_name}' {e}")])?;
    let mut files: Vec<&retraktor_formats::trak::TrakMember> = Vec::new();
    for cell in &set.cells {
        if !files.iter().any(|m| m.name == cell.file) {
            files.extend(set.members.iter().find(|m| m.name == cell.file));
        }
    }
    if files.iter().map(|m| m.size).sum::<u64>() > MAX_IMPORT_BYTES {
        return Err(vec![format!("'{file_name}' holds more sample data than a deck's whole sample memory ({} MiB)", MAX_IMPORT_BYTES >> 20)]);
    }

    let refused = |e: std::io::Error| vec![format!("can't make a folder for the set's samples in {} ({e})", root.display())];
    std::fs::create_dir_all(root).map_err(refused)?;
    let stem = file_stem_text(&Path::new(&file_name).file_stem().map_or_else(String::new, |s| s.to_string_lossy().into_owned()));
    let base = format!("{stem} {}", crate::setlist::format_recording_timestamp(std::time::SystemTime::now()));
    let mut imported = None;
    for n in 1..=1000 {
        let dir = root.join(if n == 1 { base.clone() } else { format!("{base}_{n}") });
        match std::fs::create_dir(&dir) {
            Ok(()) => {
                imported = Some(Imported { dir, notes: String::new(), kept: false });
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(refused(e)),
        }
    }
    let mut imported = imported.ok_or_else(|| vec![format!("can't make a folder for the set's samples in {}: every name for '{base}' is taken", root.display())])?;

    for member in files {
        if cancelled() {
            return Err(vec!["Cancelled".into()]);
        }
        // A new file under a name read_remix_set checked is plain: never one that was there, never outside the folder.
        let to = imported.dir.join(&member.name);
        let copied = (|| -> std::io::Result<()> {
            let mut out = std::fs::File::create_new(&to)?;
            file.seek(SeekFrom::Start(member.offset))?;
            let mut source = (&mut file).take(member.size);
            let mut head = vec![0u8; member.size.min(WAV_HEAD_BYTES as u64) as usize];
            source.read_exact(&mut head)?;
            let head_len = head.len() as u64;
            out.write_all(&standard_wav_head(&head, member.size).unwrap_or(head))?;
            if head_len + std::io::copy(&mut source, &mut out)? != member.size {
                return Err(std::io::ErrorKind::UnexpectedEof.into());
            }
            out.sync_all()
        })();
        copied.map_err(|e| vec![format!("'{}' couldn't be copied out of '{file_name}' to {} ({e})", member.name, imported.dir.display())])?;
    }
    if let Ok(dir) = std::fs::File::open(&imported.dir) {
        let _ = dir.sync_all(); // the new names last through a power cut
    }

    let mut bank = RemixBank { quantize: set.quantize, ..Default::default() };
    for cell in &set.cells {
        let bpm = if cell.synced { cell.bpm } else { 0.0 };
        let sample = RemixSample::decode(&imported.dir.join(&cell.file), &cell.name, bpm, budget, cancelled, true).map_err(|e| {
            vec![format!("cell {}.{} (page {}) '{}' from '{}': {e}", cell.column + 1, cell.row + 1, cell.row / PAGE_ROWS + 1, cell.name, cell.file)]
        })?;
        bank.cells[cell.column][cell.row] = Some(RemixCell { sample: Arc::new(sample), looped: cell.looped });
    }
    let mut unapplied: Vec<&str> = set.cells.iter().flat_map(|c| c.unapplied.iter().copied()).collect();
    unapplied.sort_unstable();
    unapplied.dedup();
    let mut notes = Vec::new();
    let differing = set.cells.iter().filter(|c| !c.unapplied.is_empty()).count();
    if differing > 0 {
        notes.push(format!("Traktor settings not applied on {differing} cells: {}", unapplied.join(", ")));
    }
    let renamed = set.cells.iter().filter(|c| c.name_shortened).count();
    if renamed > 0 {
        let most = retraktor_formats::trak::MAX_CELL_NAME_CHARS;
        notes.push(format!("{renamed} cell names cut to {most} characters or cleaned of control characters, so pads and saved sets can show them"));
    }
    imported.notes = notes.join(". ");
    Ok((bank, imported))
}

/// Bytes of a WAV's start that [`standard_wav_head`] looks at.
const WAV_HEAD_BYTES: usize = 40;

/// The start of a WAV copy that the decoder reads, for the one non-standard header seen in NI's own sets (19 WAVs
/// of *Drums and Beatbox*): a plain PCM (format 1) `fmt ` chunk of 20 bytes whose last four are zero, as the first
/// chunk of a RIFF whose size is exactly the file's. The decoder takes a PCM `fmt ` of 16, 18 or 40 bytes only, so
/// the copy starts with the same 16 format bytes in a 16-byte chunk and a RIFF size 4 smaller; every byte after the
/// chunk (the audio and any other chunk) is copied unchanged. `None` (copy it as it is) for anything else.
fn standard_wav_head(head: &[u8], file_size: u64) -> Option<Vec<u8>> {
    let u32_at = |at: usize| u32::from_le_bytes(head[at..at + 4].try_into().unwrap());
    let odd = head.len() == WAV_HEAD_BYTES
        && &head[..4] == b"RIFF"
        && u64::from(u32_at(4)) + 8 == file_size
        && &head[8..16] == b"WAVEfmt "
        && u32_at(16) == 20
        && head[20..22] == [1, 0]
        && head[36..40] == [0; 4];
    odd.then(|| {
        let mut standard = head[..36].to_vec();
        standard[4..8].copy_from_slice(&(u32_at(4) - 4).to_le_bytes());
        standard[16..20].copy_from_slice(&16u32.to_le_bytes());
        standard
    })
}

/// The most bytes of a track title in a capture's file name: with the loop, time, a `_n` and the temporary file's
/// suffix it stays well inside the 255 bytes a file name may have on common drives.
const TITLE_NAME_BYTES: usize = 120;

/// `title` as part of a file name: no path separators, characters other drives refuse or control characters, at
/// most [`TITLE_NAME_BYTES`] (cut between characters), and never hidden.
fn file_stem_text(title: &str) -> String {
    let mut text = String::new();
    for c in title.chars().map(|c| if c.is_control() || "/\\:*?\"<>|".contains(c) { '_' } else { c }) {
        if text.len() + c.len_utf8() > TITLE_NAME_BYTES {
            break;
        }
        text.push(c);
    }
    let text = text.trim().trim_start_matches('.');
    if text.is_empty() { "Capture".to_string() } else { text.to_string() }
}

/// Writes `pcm` (interleaved stereo at `rate`) as a 32-bit float WAV, so the decoded audio is kept exactly, named
/// `{stem}.wav` in `dir` (created if missing), or `{stem}_2.wav` and on when that is taken. It is written whole into
/// a hidden file beside it and flushed to disk before it takes its name, and only a name no file has, so another
/// capture is never replaced and no half-written file carries a capture's name. Returns the WAV's path.
/// shortcut: a process killed mid-write leaves its hidden .tmp file in the folder, sweep stale ones at startup if they pile up.
pub fn write_capture(dir: &Path, stem: &str, pcm: &[f32], rate: u32) -> Result<PathBuf, String> {
    use std::io::Write;
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let refused = |e: std::io::Error| format!("can't save the capture in {}: {e}", dir.display());
    std::fs::create_dir_all(dir).map_err(refused)?;
    let temp = dir.join(format!(".{stem}.{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
    let written = std::fs::File::create_new(&temp).map_err(refused).and_then(|file| {
        let mut out = std::io::BufWriter::new(file);
        out.write_all(&float_wav_header(pcm.len() / 2, rate))
            .and_then(|_| pcm.iter().try_for_each(|s| out.write_all(&s.to_le_bytes())))
            .and_then(|_| out.into_inner().map_err(std::io::IntoInnerError::into_error))
            .and_then(|file| file.sync_all())
            .map_err(|e| {
                let _ = std::fs::remove_file(&temp);
                refused(e)
            })
    });
    written?;
    // A drive without a no-replace rename may still link a second name to the file, which also never replaces one.
    let publish = |path: &Path| match rename_no_replace(&temp, path) {
        Err(e) if matches!(e.kind(), std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported) => {
            std::fs::hard_link(&temp, path).map(|()| {
                let _ = std::fs::remove_file(&temp);
            })
        }
        renamed => renamed,
    };
    for n in 1..=1000 {
        let path = dir.join(if n == 1 { format!("{stem}.wav") } else { format!("{stem}_{n}.wav") });
        match publish(&path) {
            Ok(()) => {
                if let Ok(dir) = std::fs::File::open(dir) {
                    let _ = dir.sync_all(); // the new name lasts through a power cut
                }
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                return Err(match e.kind() {
                    std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported | std::io::ErrorKind::PermissionDenied => {
                        format!("the drive holding {} can't add a file without risking replacing one (no safe rename or link: {e}), so the capture wasn't saved", dir.display())
                    }
                    _ => refused(e),
                });
            }
        }
    }
    let _ = std::fs::remove_file(&temp);
    Err(format!("can't save the capture in {}: every name for '{stem}' is taken", dir.display()))
}

/// The 58-byte header of a stereo 32-bit IEEE float WAV of `frames` frames (`fmt ` with its empty extension, then
/// `fact`, as the format asks of non-PCM data). A cell's 60 s at 768 kHz is far below the 4 GiB a RIFF size holds.
fn float_wav_header(frames: usize, rate: u32) -> Vec<u8> {
    let data = (frames * 8) as u32;
    let mut h = Vec::with_capacity(58);
    h.extend_from_slice(b"RIFF");
    h.extend_from_slice(&(50 + data).to_le_bytes());
    h.extend_from_slice(b"WAVEfmt ");
    h.extend_from_slice(&18u32.to_le_bytes());
    for field in [3u16, 2] {
        h.extend_from_slice(&field.to_le_bytes()); // IEEE float, two channels
    }
    h.extend_from_slice(&rate.to_le_bytes());
    h.extend_from_slice(&(rate * 8).to_le_bytes());
    for field in [8u16, 32, 0] {
        h.extend_from_slice(&field.to_le_bytes()); // bytes per frame, bits per sample, no extension
    }
    h.extend_from_slice(b"fact");
    h.extend_from_slice(&4u32.to_le_bytes());
    h.extend_from_slice(&(frames as u32).to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&data.to_le_bytes());
    h
}

/// Tests only: `pcm` (interleaved stereo at `rate`) as the bytes of a float WAV file.
#[cfg(test)]
pub(crate) fn test_wav(pcm: &[f32], rate: u32) -> Vec<u8> {
    let mut wav = float_wav_header(pcm.len() / 2, rate);
    wav.extend(pcm.iter().flat_map(|s| s.to_le_bytes()));
    wav
}

/// Tests only: synthetic Traktor Remix Sets, shared with `retraktor-formats`' tests (no NI audio needed).
#[cfg(test)]
#[path = "../../retraktor-formats/tests/support/trak_fixture.rs"]
pub(crate) mod trak_fixture;

fn bank_full() -> String {
    format!("Remix sample memory ({} MiB) is full: clear some cells first", BANK_BUDGET_BYTES >> 20)
}

/// The library's BPM, fitted to a sample `seconds` long when that is within 0.05 beat of a whole number of beats
/// (so a trimmed loop repeats on the beat instead of drifting), and whether it was.
fn fitted_bpm(library_bpm: f64, seconds: f64) -> (Option<f64>, bool) {
    let Some(library_bpm) = crate::audio::valid_bpm(library_bpm).filter(|_| seconds > 0.0) else {
        return (None, false);
    };
    let beats = seconds * library_bpm / 60.0;
    let whole = beats.round();
    if whole >= 1.0 && (beats - whole).abs() <= 0.05 {
        (Some(whole * 60.0 / seconds), true)
    } else {
        (Some(library_bpm), false)
    }
}

/// What a cell holds: its sample and whether it loops or plays once.
#[derive(Clone)]
pub struct RemixCell {
    pub sample: Arc<RemixSample>,
    pub looped: bool,
}

/// One deck's cells, the page on screen and whether launches wait for the next beat.
pub struct RemixBank {
    /// `cells[column][row]`.
    pub cells: [[Option<RemixCell>; ROWS]; COLUMNS],
    pub page: usize,
    /// Launch and stop on the next beat (on) or at once (off).
    pub quantize: bool,
    /// The load each cell waits for (its generation), 0 for none.
    loading: [[u64; ROWS]; COLUMNS],
}

impl Default for RemixBank {
    fn default() -> Self {
        Self { cells: Default::default(), page: 0, quantize: true, loading: [[0; ROWS]; COLUMNS] }
    }
}

impl RemixBank {
    pub fn is_loading(&self, column: usize, row: usize) -> bool {
        self.loading[column][row] != 0
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().flatten().all(Option::is_none)
    }

    pub fn any_loading(&self) -> bool {
        self.loading.iter().flatten().any(|&g| g != 0)
    }

    /// What a saved set of this bank depends on: Q and each filled cell (column, row, sample id, loop). Equal values
    /// mean no cell or setting changed in between (the page is left out: flipping it edits nothing).
    pub fn contents(&self) -> BankContents {
        let cells = (0..COLUMNS)
            .flat_map(|c| (0..ROWS).map(move |r| (c, r)))
            .filter_map(|(c, r)| self.cells[c][r].as_ref().map(|cell| (c, r, cell.sample.id, cell.looped)))
            .collect();
        (self.quantize, cells)
    }
}

/// See [`RemixBank::contents`].
pub type BankContents = (bool, Vec<(usize, usize, u64, bool)>);

struct LoadJob {
    deck: usize,
    column: usize,
    row: usize,
    generation: u64,
    source: JobSource,
}

/// What a cell load makes its sample from.
enum JobSource {
    /// Decodes a file (name and library BPM as the cell keeps them).
    File { path: PathBuf, name: String, bpm: f64 },
    /// Captures a deck's loop into a new WAV in `dir`.
    Capture { source: CaptureSource, dir: PathBuf },
}

/// The sample loader's work, in order.
enum Work {
    Load(LoadJob),
    /// Deletes a capture's WAV that no cell took (its load was superseded once finished).
    Remove(PathBuf),
}

/// A finished cell load: where it goes, which load it was, and the sample or why not.
pub struct LoadedCell {
    pub deck: usize,
    pub column: usize,
    pub row: usize,
    generation: u64,
    /// A capture: its WAV is new and only this load has it, and the cell takes it as a loop.
    captured: bool,
    pub result: Result<Arc<RemixSample>, String>,
}

/// Decodes cell samples (and captures loops) one at a time on its own worker thread, started on the first load.
pub struct RemixLoader {
    pub budget: Arc<SampleBudget>,
    /// Where captures are saved ([`captures_dir`]), or why there is nowhere.
    pub captures: Result<PathBuf, String>,
    jobs: Option<mpsc::Sender<Work>>,
    /// The worker thread, joined by [`RemixLoader::shut_down`].
    worker: Option<std::thread::JoinHandle<()>>,
    results: (mpsc::Sender<LoadedCell>, mpsc::Receiver<LoadedCell>),
    next_generation: u64,
    /// The load each cell (by [`cell_index`]) wants now, 0 for none, so the worker skips or stops outdated work.
    wanted: Arc<Vec<AtomicU64>>,
}

impl Default for RemixLoader {
    fn default() -> Self {
        let wanted = Arc::new((0..4 * COLUMNS * ROWS).map(|_| AtomicU64::new(0)).collect());
        let captures = captures_dir();
        Self { budget: SampleBudget::new(BANK_BUDGET_BYTES), captures, jobs: None, worker: None, results: mpsc::channel(), next_generation: 0, wanted }
    }
}

impl Drop for RemixLoader {
    fn drop(&mut self) {
        self.shut_down();
    }
}

fn cell_index(deck: usize, column: usize, row: usize) -> usize {
    (deck * COLUMNS + column) * ROWS + row
}

impl RemixLoader {
    /// Queues `path` for `bank`'s cell; a newer load of the same cell, or [`RemixLoader::cancel`], supersedes it.
    /// The cell keeps what it has until a load succeeds.
    pub fn request(&mut self, bank: &mut RemixBank, deck: usize, column: usize, row: usize, path: PathBuf, name: &str, bpm: f64) {
        self.queue(bank, deck, column, row, JobSource::File { path, name: name.to_string(), bpm });
    }

    /// Queues a capture of `source`'s loop for `bank`'s cell, saved in `dir`; superseded and cancelled like a load.
    pub fn capture(&mut self, bank: &mut RemixBank, deck: usize, column: usize, row: usize, source: CaptureSource, dir: PathBuf) {
        self.queue(bank, deck, column, row, JobSource::Capture { source, dir });
    }

    fn queue(&mut self, bank: &mut RemixBank, deck: usize, column: usize, row: usize, source: JobSource) {
        self.next_generation += 1;
        let generation = self.next_generation;
        bank.loading[column][row] = generation;
        self.wanted[cell_index(deck, column, row)].store(generation, Ordering::Release);
        let captured = matches!(source, JobSource::Capture { .. });
        let job = LoadJob { deck, column, row, generation, source };
        let sender = self.jobs.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<Work>();
            let (results, budget, wanted) = (self.results.0.clone(), Arc::clone(&self.budget), Arc::clone(&self.wanted));
            self.worker = Some(std::thread::spawn(move || {
                for work in rx {
                    let job = match work {
                        Work::Load(job) => job,
                        Work::Remove(path) => {
                            let _ = std::fs::remove_file(path);
                            continue;
                        }
                    };
                    let outdated = || wanted[cell_index(job.deck, job.column, job.row)].load(Ordering::Acquire) != job.generation;
                    if outdated() {
                        continue; // superseded or cancelled while queued
                    }
                    let (captured, result) = match &job.source {
                        JobSource::File { path, name, bpm } => (false, RemixSample::decode(path, name, *bpm, &budget, &outdated, false)),
                        JobSource::Capture { source, dir } => (true, RemixSample::capture(source, dir, &budget, &outdated)),
                    };
                    let done = LoadedCell { deck: job.deck, column: job.column, row: job.row, generation: job.generation, captured, result: result.map(Arc::new) };
                    if results.send(done).is_err() {
                        break;
                    }
                }
            }));
            tx
        });
        if let Err(mpsc::SendError(_)) = sender.send(Work::Load(job)) {
            // The worker is gone (it panicked): say so on the cell instead of waiting forever.
            self.jobs = None;
            let _ = self.results.0.send(LoadedCell { deck, column, row, generation, captured, result: Err("The sample loader stopped".into()) });
        }
    }

    /// Ends the worker for good as the app closes (also on drop). Every load is cancelled, so it skips what is
    /// queued and stops what it is doing, and runs the deletes already asked for; a capture it finishes anyway
    /// deletes its own WAV. Once it has stopped, the WAVs of finished captures no cell took are deleted. A WAV a cell
    /// holds is never touched. Waits at most for the one file being written.
    /// shortcut: a killed process skips this, leaving a hidden .tmp or an unclaimed WAV in the captures folder.
    pub fn shut_down(&mut self) {
        for wanted in self.wanted.iter() {
            wanted.store(0, Ordering::Release);
        }
        self.jobs = None; // the worker's queue ends after what is already in it
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
        while let Ok(done) = self.results.1.try_recv() {
            if let (true, Ok(sample)) = (done.captured, &done.result) {
                let _ = std::fs::remove_file(&sample.source);
            }
        }
    }

    /// Drops whatever load `bank`'s cell waits for (a clear, or a newer drop that can't load); what the cell already
    /// holds stays. The worker skips or stops that load.
    pub fn cancel(&mut self, bank: &mut RemixBank, deck: usize, column: usize, row: usize) {
        bank.loading[column][row] = 0;
        self.wanted[cell_index(deck, column, row)].store(0, Ordering::Release);
    }

    /// Finished loads still wanted by their cell; a superseded one is dropped here (a capture's new WAV deleted on
    /// the worker). A success fills the cell (a capture as a loop), a failure leaves it as it was. Returns each
    /// outcome (deck, column, row, name or error) for the status line.
    pub fn poll(&mut self, banks: &mut [RemixBank; 4]) -> Vec<(usize, usize, usize, Result<String, String>)> {
        let mut outcomes = Vec::new();
        while let Ok(done) = self.results.1.try_recv() {
            let bank = &mut banks[done.deck];
            if bank.loading[done.column][done.row] != done.generation {
                if let (true, Ok(sample)) = (done.captured, &done.result) {
                    let path = sample.source.clone();
                    if !self.jobs.as_ref().is_some_and(|jobs| jobs.send(Work::Remove(path.clone())).is_ok()) {
                        let _ = std::fs::remove_file(path); // the worker is gone: one small delete here
                    }
                }
                continue;
            }
            bank.loading[done.column][done.row] = 0;
            let outcome = match done.result {
                Ok(sample) => {
                    let name = sample.name.clone();
                    bank.cells[done.column][done.row] = Some(RemixCell { looped: done.captured || sample.fits_beats, sample });
                    Ok(name)
                }
                Err(e) => Err(e),
            };
            outcomes.push((done.deck, done.column, done.row, outcome));
        }
        outcomes
    }

    /// Whether any cell still waits for a load.
    pub fn busy(banks: &[RemixBank; 4]) -> bool {
        banks.iter().any(RemixBank::any_loading)
    }
}

// ---------------------------------------------------------------- saved sets

/// A remix set file: `My set.remixset`.
pub const SET_EXTENSION: &str = "remixset";
const SET_FORMAT: &str = "retraktor-remix-set";
/// The newest set format this build reads and the one it writes.
const SET_VERSION: u64 = 1;
/// 64 cells with the longest paths fit in well under this; anything bigger isn't a set.
const MAX_SET_BYTES: u64 = 1 << 20;

/// One filled cell of a saved set: where it sits and how to decode it again.
#[derive(Clone, Debug, PartialEq)]
pub struct SetCell {
    pub column: usize,
    pub row: usize,
    /// The sample's absolute file path. Text, so the JSON holds it exactly.
    pub path: String,
    pub name: String,
    /// The library BPM it was decoded with (0: none).
    pub bpm: f64,
    pub looped: bool,
}

impl SetCell {
    /// `cell 2.14 (page 4) 'Vocal'`, as the deck labels its cells.
    pub fn label(&self) -> String {
        format!("cell {}.{} (page {}) '{}'", self.column + 1, self.row + 1, self.row / PAGE_ROWS + 1, self.name)
    }
}

/// A Remix deck's cells and settings as a `.remixset` file keeps them: no audio, ids, transport or mixer state.
///
/// The file is JSON: `format`, `version`, the grid's `columns` and `rows`, `quantize`, `page` (1–4) and `cells`,
/// each with its 1-based `column` and `row`, `path`, `name`, `bpm` and `looped`. Empty cells are left out.
#[derive(Clone, Debug, PartialEq)]
pub struct RemixSet {
    pub quantize: bool,
    pub page: usize,
    pub cells: Vec<SetCell>,
}

impl RemixSet {
    /// The set to save from `bank`; refused while a cell loads, or for a cell with no file it could be found by.
    pub fn from_bank(bank: &RemixBank) -> Result<Self, String> {
        let mut cells = Vec::new();
        for column in 0..COLUMNS {
            for row in 0..ROWS {
                let at = format!("cell {}.{}", column + 1, row + 1);
                if bank.is_loading(column, row) {
                    return Err(format!("{at} is still loading: wait for it, or cancel it"));
                }
                let Some(cell) = &bank.cells[column][row] else { continue };
                let name = &cell.sample.name;
                if !cell.sample.source.is_absolute() {
                    return Err(format!("{at} '{name}' has no file to save it by"));
                }
                // shortcut: refuses a path that isn't UTF-8 rather than store it mangled, store its bytes if real libraries have them.
                let Some(path) = cell.sample.source.to_str() else {
                    return Err(format!("{at} '{name}': its file path ({}) isn't UTF-8 text, so a set can't keep it exactly", cell.sample.source.display()));
                };
                let bpm = crate::audio::valid_bpm(cell.sample.library_bpm).unwrap_or(0.0);
                cells.push(SetCell { column, row, path: path.to_string(), name: name.clone(), bpm, looped: cell.looped });
            }
        }
        Ok(Self { quantize: bank.quantize, page: bank.page, cells })
    }

    pub fn to_json(&self) -> String {
        let cells: Vec<serde_json::Value> = self
            .cells
            .iter()
            .map(|c| serde_json::json!({ "column": c.column + 1, "row": c.row + 1, "path": c.path, "name": c.name, "bpm": c.bpm, "looped": c.looped }))
            .collect();
        let doc = serde_json::json!({
            "format": SET_FORMAT, "version": SET_VERSION, "columns": COLUMNS, "rows": ROWS,
            "quantize": self.quantize, "page": self.page + 1, "cells": cells,
        });
        serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
    }

    /// Reads a set's JSON, checking everything a bank relies on; errors read after "the set ...".
    pub fn parse(text: &str) -> Result<Self, String> {
        use serde_json::Value;
        let doc: Value = serde_json::from_str(text).map_err(|e| format!("isn't readable JSON ({e})"))?;
        let doc = doc.as_object().ok_or("isn't a remix set")?;
        if doc.get("format").and_then(Value::as_str) != Some(SET_FORMAT) {
            return Err(format!("isn't a ReTraktor remix set (no \"format\": \"{SET_FORMAT}\")"));
        }
        let version = doc.get("version").and_then(Value::as_u64).filter(|&v| v >= 1).ok_or("has no valid format version")?;
        if version > SET_VERSION {
            return Err(format!("was saved by a newer ReTraktor (set format {version}; this one reads format {SET_VERSION}): update ReTraktor to open it"));
        }
        let number = |key: &str| doc.get(key).and_then(Value::as_u64);
        if number("columns") != Some(COLUMNS as u64) || number("rows") != Some(ROWS as u64) {
            return Err(format!("has a grid other than {COLUMNS} columns of {ROWS} cells"));
        }
        let quantize = doc.get("quantize").and_then(Value::as_bool).ok_or("has no quantize setting")?;
        let pages = (ROWS / PAGE_ROWS) as u64;
        let page = number("page").filter(|p| (1..=pages).contains(p)).ok_or(format!("has no page 1–{pages}"))? as usize - 1;
        let entries = doc.get("cells").and_then(Value::as_array).ok_or("has no cell list")?;
        if entries.len() > COLUMNS * ROWS {
            return Err(format!("lists {} cells; a deck has {}", entries.len(), COLUMNS * ROWS));
        }
        let mut taken = [[false; ROWS]; COLUMNS];
        let mut cells = Vec::with_capacity(entries.len());
        for (i, entry) in entries.iter().enumerate() {
            let entry = entry.as_object().ok_or(format!("has a broken cell entry ({})", i + 1))?;
            let place = |key: &str, most: usize| entry.get(key).and_then(Value::as_u64).filter(|n| (1..=most as u64).contains(n)).map(|n| n as usize - 1);
            let (Some(column), Some(row)) = (place("column", COLUMNS), place("row", ROWS)) else {
                return Err(format!("has a cell entry ({}) outside columns 1–{COLUMNS} and rows 1–{ROWS}", i + 1));
            };
            let at = format!("cell {}.{}", column + 1, row + 1);
            if std::mem::replace(&mut taken[column][row], true) {
                return Err(format!("lists {at} twice"));
            }
            let path = entry.get("path").and_then(Value::as_str).filter(|p| Path::new(p).is_absolute());
            let name = entry.get("name").and_then(Value::as_str);
            let bpm = entry.get("bpm").and_then(Value::as_f64).filter(|b| b.is_finite() && *b >= 0.0);
            let looped = entry.get("looped").and_then(Value::as_bool);
            let (Some(path), Some(name), Some(bpm), Some(looped)) = (path, name, bpm, looped) else {
                return Err(format!("has {at} without an absolute path, a name, a BPM and loop setting"));
            };
            cells.push(SetCell { column, row, path: path.to_string(), name: name.to_string(), bpm, looped });
        }
        Ok(Self { quantize, page, cells })
    }
}

/// Reads and checks the set at `path`, refusing anything too big to be one.
pub fn read_set(path: &Path) -> Result<RemixSet, String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| format!("can't be read ({e})"))?;
    let mut text = String::new();
    file.take(MAX_SET_BYTES + 1).read_to_string(&mut text).map_err(|e| format!("can't be read ({e})"))?;
    if text.len() as u64 > MAX_SET_BYTES {
        return Err(format!("is bigger than any remix set ({} KiB at most)", MAX_SET_BYTES >> 10));
    }
    RemixSet::parse(&text)
}

/// Writes `set` to `path` all at once: into a new file beside it, flushed to disk, then put in place. With `replace`
/// (the chooser confirmed the name) it is renamed over whatever is there; without, it is published only if nothing
/// has that name yet. A failure at any step, or a set too big to open again, leaves whatever was at `path` as it was.
pub fn write_set(path: &Path, set: &RemixSet, replace: bool) -> Result<(), String> {
    use std::io::Write;
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    let text = set.to_json();
    if text.len() as u64 > MAX_SET_BYTES {
        return Err(format!("the set would be {} KiB, more than ReTraktor opens ({} KiB): shorten the cell names", text.len() >> 10, MAX_SET_BYTES >> 10));
    }
    let file_name = path.file_name().ok_or("that isn't a file name")?;
    let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".{}-{}.tmp", std::process::id(), NEXT_TEMP.fetch_add(1, Ordering::Relaxed)));
    let temp = dir.join(temp_name);
    let mut file = std::fs::File::create_new(&temp).map_err(|e| e.to_string())?;
    let written = file
        .write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .and_then(|_| if replace { std::fs::rename(&temp, path) } else { rename_no_replace(&temp, path) });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&temp);
        return Err(match e.kind() {
            std::io::ErrorKind::AlreadyExists => {
                format!("'{}' already exists and the chooser never asked to replace it: choose that file by its full name to replace it, or pick another name", path.display())
            }
            // The drive can't add a file only-if-new: refused rather than risk a file that appeared meanwhile.
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::Unsupported => {
                format!("this drive can't safely add '{}' under a name typed without .{SET_EXTENSION}: save again and type the full name ending .{SET_EXTENSION}", path.display())
            }
            _ => e.to_string(),
        });
    }
    // The rename lasts through a power cut once the folder is flushed too.
    if let Ok(dir) = std::fs::File::open(dir) {
        let _ = dir.sync_all();
    }
    Ok(())
}

/// Renames the finished `from` to `to` in one step, only if nothing has that name (Linux `renameat2` with
/// `RENAME_NOREPLACE`): an existing file is never replaced, and readers never see a partial one.
#[cfg(target_os = "linux")]
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    extern "C" {
        // In the C library std already links (glibc 2.28+, musl 1.2.2+).
        fn renameat2(olddirfd: i32, oldpath: *const std::ffi::c_char, newdirfd: i32, newpath: *const std::ffi::c_char, flags: u32) -> i32;
    }
    const AT_FDCWD: i32 = -100;
    const RENAME_NOREPLACE: u32 = 1;
    let (from, to) = (std::ffi::CString::new(from.as_os_str().as_bytes())?, std::ffi::CString::new(to.as_os_str().as_bytes())?);
    // SAFETY: both are NUL-terminated paths that outlive the call, which only reads them.
    match unsafe { renameat2(AT_FDCWD, from.as_ptr(), AT_FDCWD, to.as_ptr(), RENAME_NOREPLACE) } {
        0 => Ok(()),
        _ => Err(std::io::Error::last_os_error()),
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_no_replace(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::ErrorKind::Unsupported.into())
}

/// Decodes every cell of `set` into a new bank, all or nothing: any file missing or unreadable (each one named) or
/// a cell that fails to decode (that one) refuses the whole set, freeing what it had decoded. Off the UI thread.
pub fn stage(set: &RemixSet, budget: &Arc<SampleBudget>, cancelled: &dyn Fn() -> bool) -> Result<RemixBank, Vec<String>> {
    // Every missing file first, so one look tells the DJ all there is to fix.
    let missing: Vec<String> = set
        .cells
        .iter()
        .filter_map(|c| match std::fs::metadata(&c.path) {
            Ok(m) if m.is_file() => None,
            Ok(_) => Some(format!("{}: {} isn't a file", c.label(), c.path)),
            Err(e) => Some(format!("{}: {} ({e})", c.label(), c.path)),
        })
        .collect();
    if !missing.is_empty() {
        let mut lines = missing;
        lines.push("Put each file back at its path (or fix its \"path\" in the .remixset file, it is plain text), then open the set again".into());
        return Err(lines);
    }
    let mut bank = RemixBank { quantize: set.quantize, page: set.page, ..Default::default() };
    for c in &set.cells {
        let sample = RemixSample::decode(Path::new(&c.path), &c.name, c.bpm, budget, cancelled, false).map_err(|e| vec![format!("{}: {} ({e})", c.label(), c.path)])?;
        bank.cells[c.column][c.row] = Some(RemixCell { sample: Arc::new(sample), looped: c.looped });
    }
    Ok(bank)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SetAction {
    Save,
    Open,
}

/// A deck's save or open, from its SET click until the file is written or the bank replaced.
pub struct SetJob {
    pub ticket: u64,
    pub action: SetAction,
    /// The bank when asked: an edit since makes the job stale.
    pub contents: BankContents,
    /// Save: the set as it was when asked.
    pub snapshot: Option<RemixSet>,
    /// Whether the deck was a Remix deck, and its track load generation, when asked: an open lands only on the deck
    /// as it was, never over a track loaded or a mode switched since.
    pub deck: (bool, u64),
    /// Its desktop chooser is still open.
    pub choosing: bool,
}

/// A step of a set job finished off the UI thread.
pub enum SetDone {
    /// The desktop chooser closed: the file picked, or none when cancelled.
    Chosen(Option<PathBuf>),
    Saved(PathBuf, Result<(), String>),
    /// The set at that path decoded into a bank (with the samples an imported Traktor set copied out, removed unless
    /// kept), or every reason it can't be.
    Staged(PathBuf, Result<(RemixBank, Option<Imported>), Vec<String>>),
}

/// Why a deck's last open or save failed, kept on the deck until dismissed or a later one succeeds.
pub struct SetReport {
    pub title: String,
    pub lines: Vec<String>,
    pub expanded: bool,
}

/// Every deck's set job: a desktop chooser that doesn't block the UI, then reading and decoding (or writing) on
/// a worker. A newer request on a deck supersedes its job; a superseded staging stops early.
pub struct RemixSets {
    pub jobs: [Option<SetJob>; 4],
    pub reports: [Option<SetReport>; 4],
    /// Each deck's current ticket (0: none), which a staging worker checks to stop once superseded.
    tickets: Arc<[AtomicU64; 4]>,
    next_ticket: u64,
    done: (mpsc::Sender<(usize, u64, SetDone)>, mpsc::Receiver<(usize, u64, SetDone)>),
    /// The one thread that writes sets, in the order they were accepted, so a later save is never overtaken.
    writer: Option<mpsc::Sender<(usize, u64, PathBuf, RemixSet, bool)>>,
    /// Saves accepted whose outcome the UI hasn't taken yet.
    writes_pending: usize,
    /// Where an imported Traktor set's samples go ([`imports_dir`]), or why there is nowhere.
    pub imports: Result<PathBuf, String>,
    /// Staging workers that may still run, joined by [`RemixSets::shut_down`].
    stagers: Vec<std::thread::JoinHandle<()>>,
}

impl Drop for RemixSets {
    fn drop(&mut self) {
        self.shut_down();
    }
}

impl Default for RemixSets {
    fn default() -> Self {
        let (jobs, reports, tickets) = Default::default();
        Self { jobs, reports, tickets, next_ticket: 0, done: mpsc::channel(), writer: None, writes_pending: 0, imports: imports_dir(), stagers: Vec::new() }
    }
}

/// A Traktor Remix Set file: `Electric Vice.trak`.
pub const TRAK_EXTENSION: &str = "trak";

impl RemixSets {
    /// Starts deck `d`'s job, superseding any it had, and opens the chooser for it. `deck`: see [`SetJob::deck`].
    pub fn start(&mut self, d: usize, action: SetAction, contents: BankContents, snapshot: Option<RemixSet>, deck: (bool, u64)) -> u64 {
        self.next_ticket += 1;
        let ticket = self.next_ticket;
        self.tickets[d].store(ticket, Ordering::Release);
        self.jobs[d] = Some(SetJob { ticket, action, contents, snapshot, deck, choosing: true });
        if cfg!(test) {
            return ticket; // tests answer for the chooser
        }
        let tx = self.done.0.clone();
        let letter = (b'A' + d as u8) as char;
        std::thread::spawn(move || {
            let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
            let dialog = rfd::FileDialog::new().set_directory(&home);
            // The desktop's save chooser asks before replacing an existing file. Only ReTraktor's own sets are saved.
            let picked = match action {
                SetAction::Save => dialog
                    .add_filter("ReTraktor remix set", &[SET_EXTENSION])
                    .set_title(format!("Save deck {letter}'s remix set"))
                    .set_file_name(format!("Deck {letter} remix set.{SET_EXTENSION}"))
                    .save_file(),
                SetAction::Open => dialog
                    .add_filter("Remix sets (ReTraktor .remixset, Traktor .trak)", &[SET_EXTENSION, TRAK_EXTENSION])
                    .set_title(if deck.0 {
                        format!("Open a remix set onto deck {letter} (replaces its cells)")
                    } else {
                        format!("Open a remix set onto deck {letter} (it becomes a Remix deck; a stopped track there is unloaded)")
                    })
                    .pick_file(),
            };
            let _ = tx.send((d, ticket, SetDone::Chosen(picked)));
        });
        ticket
    }

    /// Deck `d`'s job, if `ticket` is still it.
    pub fn job(&self, d: usize, ticket: u64) -> Option<&SetJob> {
        self.jobs[d].as_ref().filter(|j| j.ticket == ticket)
    }

    pub fn finish(&mut self, d: usize) {
        self.jobs[d] = None;
        self.tickets[d].store(0, Ordering::Release);
    }

    /// Drops deck `d`'s open, if it has one (its mode changed); a save keeps going.
    pub fn cancel_open(&mut self, d: usize) {
        if self.jobs[d].as_ref().is_some_and(|j| j.action == SetAction::Open) {
            self.finish(d);
        }
    }

    pub fn busy(&self) -> bool {
        self.jobs.iter().any(Option::is_some) || self.writing()
    }

    /// Whether an accepted save is still to be written (or its outcome still to be shown).
    pub fn writing(&self) -> bool {
        self.writes_pending > 0
    }

    /// Reads and decodes the set at `path` (a Traktor `.trak` is imported, [`import_trak`]) for deck `d`'s job
    /// `ticket` on a worker, stopping once superseded.
    pub fn stage(&mut self, d: usize, ticket: u64, path: PathBuf, budget: &Arc<SampleBudget>) {
        let (tx, tickets, budget, imports) = (self.done.0.clone(), Arc::clone(&self.tickets), Arc::clone(budget), self.imports.clone());
        self.stagers.retain(|worker| !worker.is_finished());
        self.stagers.push(std::thread::spawn(move || {
            let cancelled = || tickets[d].load(Ordering::Acquire) != ticket;
            let result = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case(TRAK_EXTENSION)) {
                imports.map_err(|e| vec![e]).and_then(|root| import_trak(&path, &root, &budget, &cancelled)).map(|(bank, imported)| (bank, Some(imported)))
            } else {
                read_set(&path).map_err(|e| vec![format!("The set {e}")]).and_then(|set| stage(&set, &budget, &cancelled)).map(|bank| (bank, None))
            };
            // A superseded result (and any samples it copied out) is freed here, off the UI thread.
            if !cancelled() {
                let _ = tx.send((d, ticket, SetDone::Staged(path, result)));
            }
        }));
    }

    /// Ends every staging worker as the app closes (also on drop): each one, superseded ones too, is cancelled and
    /// waited for, so an import stops between files or packets and removes the folder it was filling. Results not yet
    /// taken are dropped here, removing any samples no deck took. File choosers aren't waited for (they hold no
    /// files), and neither is the set writer, whose saves the close already waits on.
    /// shortcut: a killed process skips this, leaving a partly filled folder in Remix Sets, sweep them at startup if that matters.
    pub fn shut_down(&mut self) {
        for ticket in self.tickets.iter() {
            ticket.store(0, Ordering::Release);
        }
        for worker in self.stagers.drain(..) {
            let _ = worker.join();
        }
        drop(self.poll());
    }

    /// Queues `set` to be written to `path` (see [`write_set`] for `replace`) for deck `d`'s job `ticket`, after
    /// every save accepted before it.
    pub fn save(&mut self, d: usize, ticket: u64, path: PathBuf, set: RemixSet, replace: bool) {
        self.writes_pending += 1;
        let writer = self.writer.get_or_insert_with(|| {
            let (tx, rx) = mpsc::channel::<(usize, u64, PathBuf, RemixSet, bool)>();
            let done = self.done.0.clone();
            std::thread::spawn(move || {
                for (d, ticket, path, set, replace) in rx {
                    let result = write_set(&path, &set, replace);
                    let _ = done.send((d, ticket, SetDone::Saved(path, result)));
                }
            });
            tx
        });
        if let Err(mpsc::SendError((d, ticket, path, ..))) = writer.send((d, ticket, path, set, replace)) {
            // The writer is gone (it panicked): say so instead of waiting forever.
            self.writer = None;
            let _ = self.done.0.send((d, ticket, SetDone::Saved(path, Err("the set writer stopped".into()))));
        }
    }

    /// As if a worker had sent `done`; a save outcome counts as one accepted earlier.
    #[cfg(test)]
    pub fn inject(&mut self, d: usize, ticket: u64, done: SetDone) {
        if matches!(done, SetDone::Saved(..)) {
            self.writes_pending += 1;
        }
        let _ = self.done.0.send((d, ticket, done));
    }

    pub fn poll(&mut self) -> Vec<(usize, u64, SetDone)> {
        let done: Vec<_> = self.done.1.try_iter().collect();
        self.writes_pending -= done.iter().filter(|(.., step)| matches!(step, SetDone::Saved(..))).count();
        done
    }
}

// ---------------------------------------------------------------- the audio callback's side

/// A cell to start, as a column's launch carries it to the callback.
#[derive(Clone)]
pub(crate) struct Launch {
    pub sample: Arc<RemixSample>,
    pub row: u8,
    pub looped: bool,
}

/// A launch (or, with none, a stop) for one column, at once or on the next beat.
#[derive(Clone)]
pub(crate) struct RemixRequest {
    pub launch: Option<Launch>,
    pub on_beat: bool,
}

/// What one remix column is doing, as of the callback's last handoff.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RemixColumn {
    /// The row playing and its sample's id (a cell refilled since still plays its old audio until relaunched).
    pub playing: Option<(u8, u64)>,
    /// Waiting for the next beat: a launch of that row, or a stop (`None`).
    pub queued: Option<Option<u8>>,
    /// How far through its sample the playing cell is, 0..1.
    pub progress: f32,
}

struct Voice {
    launch: Launch,
    /// Source frames from the sample's start.
    pos: f64,
}

/// One column in the callback: the cell sounding, the one fading out under it, and a launch or stop waiting.
#[derive(Default)]
pub(crate) struct RemixColumnPlayer {
    playing: Option<Voice>,
    /// The cell fading out under `playing`, playing on through the ramp.
    fading: Option<Voice>,
    /// Instead of `fading`: one frame held to fade out (an interrupted ramp, or a one-shot that ran out in one).
    held: Option<(f32, f32)>,
    /// Frames of the running ramp done (only meaningful while `fading` or `held` is some).
    fade_done: u32,
    /// The column's last output frame.
    last: (f32, f32),
    pending: Option<RemixRequest>,
}

/// A linear read of interleaved stereo at a fractional frame; a loop reads across its seam into its start.
fn read(pcm: &[f32], pos: f64, looped: bool) -> (f32, f32) {
    let n = pcm.len() / 2;
    let i = pos as usize;
    if i >= n {
        return (0.0, 0.0);
    }
    let frac = (pos - i as f64) as f32;
    let j = if i + 1 < n { i + 1 } else if looped { 0 } else { i };
    (pcm[2 * i] + (pcm[2 * j] - pcm[2 * i]) * frac, pcm[2 * i + 1] + (pcm[2 * j + 1] - pcm[2 * i + 1]) * frac)
}

impl Voice {
    /// Source frames per output frame: a loop with a known tempo follows `tempo`, anything else plays at its own speed.
    fn step(&self, tempo: f64, out_rate: u32) -> f64 {
        let natural = self.launch.sample.sample_rate as f64 / out_rate.max(1) as f64;
        match (self.launch.looped, self.launch.sample.bpm) {
            (true, Some(bpm)) => natural * crate::audio::sync_ratio(bpm, tempo),
            _ => natural,
        }
    }

    /// This frame of the voice, then on to the next. False once a one-shot has played its last frame.
    fn next(&mut self, tempo: f64, out_rate: u32) -> ((f32, f32), bool) {
        let frames = self.launch.sample.frames() as f64;
        let out = read(&self.launch.sample.pcm, self.pos, self.launch.looped);
        self.pos += self.step(tempo, out_rate);
        if self.pos >= frames {
            if !self.launch.looped || frames <= 0.0 {
                return (out, false);
            }
            self.pos = self.pos.rem_euclid(frames);
        }
        (out, true)
    }
}

impl RemixColumnPlayer {
    /// Takes the UI's newest request for this column at a handoff. Any older one still waiting is replaced.
    fn accept(&mut self, request: RemixRequest, retire: &mut Vec<Retired>) {
        if let Some(old) = self.pending.take().and_then(|r| r.launch) {
            retire.push(old.sample);
        }
        if request.launch.is_some() || self.playing.is_some() {
            self.pending = Some(request);
        }
    }

    /// One output frame. `beat` is `Some(frames past the beat line)` on the frame the beat clock crosses one.
    ///
    /// Every switch happens on the frame it is due. A cell starting in a silent column starts at full level.
    /// Replacing (or stopping) a sounding cell is a complementary ramp over [`FADE_SEC`] from that frame: the new
    /// cell rises as the outgoing sound falls, their gains always summing to one, so neither a level jump nor a
    /// doubled loop is heard. The outgoing sound is the old cell, playing on, or, when a switch interrupts a ramp
    /// still running, the column's last mixed frame held (as a beat jump's ease does); an outgoing one-shot that runs
    /// out during the ramp is held at its last frame the same way. So the incoming cell always gets its full ramp,
    /// and a column holds at most one fading voice and one held frame.
    fn frame(&mut self, beat: Option<f64>, tempo: f64, out_rate: u32, retire: &mut Vec<Retired>) -> (f32, f32) {
        let fade = (FADE_SEC * out_rate as f64).round().max(1.0) as u32;
        let due = self.pending.as_ref().and_then(|p| if p.on_beat { beat } else { Some(0.0) });
        if let Some((late, request)) = due.and_then(|late| Some((late, self.pending.take()?))) {
            let ramping = self.fading.is_some() || self.held.is_some();
            if ramping {
                // Interrupted: what was just heard, as one frame, falls away under the newest cell.
                self.held = Some(self.last);
                for voice in [self.fading.take(), self.playing.take()].into_iter().flatten() {
                    retire.push(voice.launch.sample);
                }
            } else {
                self.fading = self.playing.take();
            }
            self.fade_done = 0;
            self.playing = request.launch.map(|launch| {
                let mut voice = Voice { launch, pos: 0.0 };
                // Started late by a fraction of a frame: begin that far in, so the beat lands between frames exactly.
                voice.pos = late * voice.step(tempo, out_rate);
                voice
            });
        }
        let rising = (self.fading.is_some() || self.held.is_some()).then(|| {
            self.fade_done += 1;
            self.fade_done as f32 / fade as f32
        });
        let mut out = (0.0, 0.0);
        if let Some(voice) = self.playing.as_mut() {
            let (sound, more) = voice.next(tempo, out_rate);
            let gain = rising.unwrap_or(1.0);
            out = (sound.0 * gain, sound.1 * gain);
            if !more {
                retire.push(self.playing.take().unwrap().launch.sample);
            }
        }
        if let Some(rising) = rising {
            let falling = 1.0 - rising;
            let outgoing = match self.fading.as_mut() {
                Some(voice) => {
                    let (sound, more) = voice.next(tempo, out_rate);
                    if !more {
                        retire.push(self.fading.take().unwrap().launch.sample);
                        self.held = Some(sound); // ran out: its last frame finishes the ramp
                    }
                    sound
                }
                None => self.held.unwrap_or_default(),
            };
            out = (out.0 + outgoing.0 * falling, out.1 + outgoing.1 * falling);
            if self.fade_done >= fade {
                if let Some(voice) = self.fading.take() {
                    retire.push(voice.launch.sample);
                }
                self.held = None;
            }
        }
        self.last = out;
        out
    }

    fn sounding(&self) -> bool {
        self.playing.is_some() || self.fading.is_some() || self.held.is_some()
    }

    fn status(&self) -> RemixColumn {
        RemixColumn {
            playing: self.playing.as_ref().map(|v| (v.launch.row, v.launch.sample.id)),
            queued: self.pending.as_ref().map(|p| p.launch.as_ref().map(|l| l.row)),
            progress: self.playing.as_ref().map_or(0.0, |v| (v.pos / v.launch.sample.frames().max(1) as f64) as f32),
        }
    }

    fn into_samples(self) -> impl Iterator<Item = Retired> {
        let pending = self.pending.and_then(|p| p.launch).map(|l| l.sample);
        let playing = self.playing.map(|v| v.launch.sample);
        let fading = self.fading.map(|v| v.launch.sample);
        [pending, playing, fading].into_iter().flatten().map(|s| s as Retired)
    }
}

/// The reference id a sounding Pattern Player takes in [`RemixEngine::frame`], after the decks' 0–3.
pub(crate) const PATTERN_CLOCK: usize = 4;

/// The beat that launches wait for. While a track with a beatgrid plays as the reference, it is that track's
/// beat (or, with none, a sounding Pattern Player's); otherwise the callback keeps counting on from the last beat
/// at the master tempo.
#[derive(Default)]
pub(crate) struct BeatClock {
    /// Where the clock will be on the next frame, in beats.
    pub next: f64,
    last: f64,
}

/// More than this many frames' worth of beats in one frame is a jump (a seek, hot cue, loop wrap or a new
/// reference), not playback: even a fast scratch moves less.
const JUMP_FRAMES: f64 = 8.0;

impl BeatClock {
    /// A clock carrying on from `beat`, with no beat line crossed on its first frame.
    pub fn starting_at(beat: f64) -> Self {
        Self { next: beat, last: beat }
    }

    /// One frame: the reference's (id, beat, live tempo), if one plays, else the clock at `clock_bpm`. Returns the
    /// tempo to follow and, on the frame playback crosses a beat line going forward, how many frames past the line
    /// this frame is (0..1). A jump is no crossing, so a launch waits for the next line after it, unless the jump
    /// lands on a line (a hot cue on the beat), which counts.
    fn tick(&mut self, reference: Option<(usize, f64, f64)>, clock_bpm: f64, dt: f64) -> (f64, Option<f64>) {
        let (now, tempo) = match reference {
            Some((_, beat, tempo)) => (beat, tempo),
            None => (self.next, clock_bpm),
        };
        let step = dt * tempo.max(1.0) / 60.0; // beats in one frame at this tempo
        let moved = now - self.last;
        let past = now - now.floor();
        let crossed = if moved.abs() > JUMP_FRAMES * step {
            (past < step).then(|| past / step)
        } else {
            (moved > 0.0 && now.floor() > self.last.floor()).then(|| (past / moved).min(1.0))
        };
        self.last = now;
        self.next = now + step;
        (tempo, crossed)
    }
}

/// Bound on samples handed back between two handoffs: 16 columns hold at most a waiting, a playing and a fading
/// sample each, plus one more superseded at a handoff.
const RETIRING_CAPACITY: usize = 128;

/// Every remix column on every deck, as the audio callback runs them.
pub(crate) struct RemixEngine {
    pub columns: [[RemixColumnPlayer; COLUMNS]; 4],
    pub clock: BeatClock,
    /// Samples the callback is done with, until the next handoff passes them to the UI thread to free.
    retiring: Vec<Retired>,
}

impl Default for RemixEngine {
    fn default() -> Self {
        Self { columns: Default::default(), clock: BeatClock::default(), retiring: Vec::with_capacity(RETIRING_CAPACITY) }
    }
}

impl RemixEngine {
    /// Handoff in: passes finished samples on to `retired` as far as it has room, then takes the UI's requests,
    /// but only with nothing left over, so `retiring` never outgrows its capacity in the callback.
    pub fn take_requests(&mut self, requests: &mut [[Option<RemixRequest>; COLUMNS]; 4], retired: &mut Vec<Retired>) {
        self.pass_on(retired);
        if !self.retiring.is_empty() {
            return; // shortcut: requests wait for the UI to free retired audio, which it does every frame
        }
        for (deck, players) in requests.iter_mut().zip(self.columns.iter_mut()) {
            for (request, player) in deck.iter_mut().zip(players.iter_mut()) {
                if let Some(request) = request.take() {
                    player.accept(request, &mut self.retiring);
                }
            }
        }
        self.pass_on(retired);
    }

    fn pass_on(&mut self, retired: &mut Vec<Retired>) {
        while retired.len() < retired.capacity() {
            match self.retiring.pop() {
                Some(sample) => retired.push(sample),
                None => break,
            }
        }
    }

    /// One output frame for every deck's columns into `out`; `true` per deck that sounds. `reference` is the
    /// reference track's (deck, beat, tempo) at this frame.
    pub fn frame(&mut self, reference: Option<(usize, f64, f64)>, clock_bpm: f64, out_rate: u32, out: &mut [(f32, f32); 4]) -> [bool; 4] {
        let dt = 1.0 / out_rate.max(1) as f64;
        let (tempo, beat) = self.clock.tick(reference, clock_bpm, dt);
        let mut sounding = [false; 4];
        for (d, players) in self.columns.iter_mut().enumerate() {
            for player in players.iter_mut() {
                if player.pending.is_none() && !player.sounding() {
                    continue;
                }
                let (l, r) = player.frame(beat, tempo, out_rate, &mut self.retiring);
                out[d].0 += l;
                out[d].1 += r;
                sounding[d] |= player.sounding() || l != 0.0 || r != 0.0;
            }
        }
        debug_assert!(self.retiring.len() <= RETIRING_CAPACITY);
        sounding
    }

    pub fn publish(&self, status: &mut [[RemixColumn; COLUMNS]; 4]) {
        for (players, published) in self.columns.iter().zip(status.iter_mut()) {
            for (player, column) in players.iter().zip(published.iter_mut()) {
                *column = player.status();
            }
        }
    }

    /// Off the real-time path (a closing stream): every sample this engine holds, for the UI thread to free.
    pub fn release_all(&mut self, retired: &mut Vec<Retired>) {
        retired.append(&mut self.retiring);
        for players in std::mem::take(&mut self.columns) {
            for player in players {
                retired.extend(player.into_samples());
            }
        }
    }

    /// Off the real-time path: what is left to retire (a closing stream hands its columns on, not these).
    pub fn release_retiring(&mut self, retired: &mut Vec<Retired>) {
        retired.append(&mut self.retiring);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn budget() -> Arc<SampleBudget> {
        SampleBudget::new(BANK_BUDGET_BYTES)
    }

    /// A constant-level sample: easy to see where it starts and which column it is.
    fn level(name: &str, value: f32, frames: usize, rate: u32, bpm: f64, b: &Arc<SampleBudget>) -> Arc<RemixSample> {
        Arc::new(RemixSample::from_pcm(name, vec![value; frames * 2], rate, bpm, b).unwrap())
    }

    #[test]
    fn library_bpm_fits_whole_beat_loops_only() {
        // 2 s at 120 BPM is 4 beats: a loop, kept at 120.
        assert_eq!(fitted_bpm(120.0, 2.0), (Some(120.0), true));
        // A loop trimmed a hair short still fits, at the tempo that makes it repeat on the beat.
        let (bpm, fits) = fitted_bpm(125.0, 7.679);
        assert!(fits && (bpm.unwrap() - 16.0 * 60.0 / 7.679).abs() < 1e-9);
        // A 2.37 s vocal at 120 BPM is 4.74 beats: a one-shot that keeps the tag.
        assert_eq!(fitted_bpm(120.0, 2.37), (Some(120.0), false));
        assert_eq!(fitted_bpm(0.0, 2.0), (None, false));
        // A corrupt or absurd tag is no tempo: the sample plays at its own speed.
        for bad in [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -120.0, 1e300, 5e-324, 4000.0] {
            assert_eq!(fitted_bpm(bad, 2.0), (None, false), "{bad}");
        }
    }

    /// `sync_ratio` runs in the callback: whatever the tempos, it ends at once (no loop on 0 or infinity, and a
    /// fixed amount of folding for extreme exponents) with a speed within its octave band, and ordinary half- and
    /// double-time pairings still play at their own speed.
    #[test]
    fn corrupt_and_extreme_tempos_stay_bounded() {
        use crate::audio::sync_ratio;
        let odd = [f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 0.0, -1.0, 5e-324, 1e-300, 1e300, f64::MAX, 20.0, 400.0];
        for &bpm in &odd {
            for &target in &odd {
                let r = sync_ratio(bpm, target);
                assert!(r.is_finite() && (0.70..=1.42).contains(&r), "sync_ratio({bpm}, {target}) = {r}");
            }
        }
        assert_eq!(sync_ratio(70.0, 140.0), 1.0);
        assert_eq!(sync_ratio(140.0, 70.0), 1.0);
        assert!((sync_ratio(120.0, 126.0) - 1.05).abs() < 1e-12);
        assert!((sync_ratio(130.0, 120.0) - 120.0 / 130.0).abs() < 1e-12);
        let b = budget();
        let tagged = level("inf", 0.1, 1000, 1000, f64::INFINITY, &b);
        assert_eq!((tagged.bpm, tagged.fits_beats), (None, false));
    }

    /// A loop with a BPM follows the master tempo (folded like SYNC, so 70 under 140 plays as it is); a one-shot,
    /// or a loop with no BPM, plays at its own speed. Both convert their own rate to the card's.
    #[test]
    fn tagged_loops_follow_the_tempo_and_the_rest_keep_their_own() {
        let b = budget();
        let voice = |bpm: f64, looped: bool| Voice { launch: Launch { sample: level("s", 0.1, 44_100, 44_100, bpm, &b), row: 0, looped }, pos: 0.0 };
        let natural = 44_100.0 / 48_000.0;
        assert!((voice(120.0, true).step(126.0, 48_000) - natural * 1.05).abs() < 1e-12);
        assert!((voice(70.0, true).step(140.0, 48_000) - natural).abs() < 1e-12);
        assert_eq!(voice(120.0, false).step(126.0, 48_000), natural);
        assert_eq!(voice(0.0, true).step(126.0, 48_000), natural);
    }

    /// The budget is taken while decoding, and the length cap judged on the decoded audio: a header's length is an
    /// estimate, so a genuinely short sample under an overstated (even absurd) header loads, while a long file is
    /// stopped as soon as it decodes past the cap. A full bank or a cancelled load stops a decode early; every
    /// refusal gives its bytes back, and so does a sample when dropped.
    #[test]
    fn decode_refuses_long_files_and_a_full_bank_early() {
        use crate::audio::{register_test_stream, TestStream};
        let rate = 1000u32; // 60 s is 60 000 frames
        let packets = |n: usize, frames: usize| (0..n).map(|i| ((i * frames) as u64, vec![0.25f32; frames * 2])).collect::<Vec<_>>();
        let dir = std::env::temp_dir();
        let short = dir.join("remix-short.test");
        register_test_stream(&short, TestStream { rate, header_frames: Some(4000), packets: packets(4, 1000), seekable: true });
        let b = budget();
        let s = RemixSample::decode(&short, "short", 120.0, &b, &|| false, false).unwrap();
        assert_eq!((s.frames(), s.sample_rate, s.bpm, s.fits_beats), (4000, rate, Some(120.0), true), "4 s at 120 BPM = 8 beats");
        assert_eq!(b.used(), 4000 * 2 * 4);
        drop(s);
        assert_eq!(b.used(), 0, "a dropped sample gives its memory back");

        for (name, header) in [("remix-overstated-header.test", 15 * 60 * 1000), ("remix-absurd-header.test", u64::MAX)] {
            let path = dir.join(name);
            register_test_stream(&path, TestStream { rate, header_frames: Some(header), packets: packets(4, 1000), seekable: true });
            let s = RemixSample::decode(&path, "short", 0.0, &b, &|| false, false).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(s.frames(), 4000, "{name}");
        }
        assert_eq!(b.used(), 0);

        // A 15-minute stream, with or without a header: stopped at the cap, never past it.
        for (name, header) in [("remix-long-stream.test", None), ("remix-long-stated.test", Some(900_000))] {
            let path = dir.join(name);
            register_test_stream(&path, TestStream { rate, header_frames: header, packets: packets(900, 1000), seekable: true });
            let e = RemixSample::decode(&path, "mix", 0.0, &b, &|| false, false).err().unwrap();
            assert!(e.contains("longer than 60 s"), "{name}: {e}");
            assert_eq!(b.used(), 0);
        }

        // A load the cell no longer wants stops and gives everything back.
        let e = RemixSample::decode(&short, "short", 0.0, &b, &|| true, false).err().unwrap();
        assert_eq!((e.as_str(), b.used()), ("Cancelled", 0));

        // A bank with room for 3 s: a 4 s sample stops on the way and leaves nothing taken.
        let small = SampleBudget::new(3000 * 2 * 4);
        let e = RemixSample::decode(&short, "short", 0.0, &small, &|| false, false).err().unwrap();
        assert!(e.contains("full"), "{e}");
        assert_eq!(small.used(), 0);
        let held = RemixSample::from_pcm("held", vec![0.0; 2000 * 2], rate, 0.0, &small).unwrap();
        assert!(RemixSample::from_pcm("more", vec![0.0; 2000 * 2], rate, 0.0, &small).is_err());
        drop(held);
        assert_eq!(small.used(), 0);
    }

    /// Loads finish on the worker; only the newest load of a cell lands, a failure leaves the cell as it was,
    /// and a superseded load's sample is freed.
    #[test]
    fn stale_and_failed_cell_loads_leave_the_right_sample() {
        use crate::audio::{register_test_stream, TestStream};
        let dir = std::env::temp_dir();
        let (a, b) = (dir.join("remix-cell-a.test"), dir.join("remix-cell-b.test"));
        register_test_stream(&a, TestStream { rate: 1000, header_frames: None, packets: vec![(0, vec![0.1; 2000])], seekable: true });
        register_test_stream(&b, TestStream { rate: 1000, header_frames: None, packets: vec![(0, vec![0.2; 4000])], seekable: true });
        let mut loader = RemixLoader::default();
        let mut banks: [RemixBank; 4] = Default::default();
        let wait = |loader: &mut RemixLoader, banks: &mut [RemixBank; 4]| {
            let mut all = Vec::new();
            for _ in 0..500 {
                all.extend(loader.poll(banks));
                if !RemixLoader::busy(banks) {
                    return all;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            panic!("cell loads never finished");
        };

        // A, then B over it before A finishes: B lands, A is dropped.
        loader.request(&mut banks[1], 1, 2, 5, a.clone(), "A", 0.0);
        loader.request(&mut banks[1], 1, 2, 5, b.clone(), "B", 0.0);
        let outcomes = wait(&mut loader, &mut banks);
        assert_eq!(outcomes.len(), 1, "only the newest load reports");
        assert_eq!(banks[1].cells[2][5].as_ref().unwrap().sample.name, "B");
        let kept = Arc::clone(&banks[1].cells[2][5].as_ref().unwrap().sample);

        // A failed replacement keeps B.
        loader.request(&mut banks[1], 1, 2, 5, dir.join("remix-missing-file.wav"), "gone", 0.0);
        let outcomes = wait(&mut loader, &mut banks);
        assert!(matches!(&outcomes[0], (1, 2, 5, Err(_))));
        assert!(Arc::ptr_eq(&banks[1].cells[2][5].as_ref().unwrap().sample, &kept));
        drop(kept);
        assert_eq!(loader.budget.used(), 2000 * 2 * 4, "only B's audio is held");
        assert!(banks.iter().enumerate().all(|(d, bank)| d == 1 || bank.is_empty()), "other decks untouched");

        // A cancelled load never lands: over B (B stays), and on an empty cell (it stays empty).
        loader.request(&mut banks[1], 1, 2, 5, a.clone(), "A", 0.0);
        loader.request(&mut banks[0], 0, 0, 0, a.clone(), "A", 0.0);
        loader.cancel(&mut banks[1], 1, 2, 5);
        loader.cancel(&mut banks[0], 0, 0, 0);
        assert!(!RemixLoader::busy(&banks));
        // Whether the worker skipped them or finished one first, nothing lands and nothing stays held.
        for _ in 0..200 {
            assert!(loader.poll(&mut banks).is_empty());
            if loader.budget.used() == 2000 * 2 * 4 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(banks[1].cells[2][5].as_ref().unwrap().sample.name, "B");
        assert!(banks[0].is_empty());
        assert_eq!(loader.budget.used(), 2000 * 2 * 4, "the cancelled loads hold nothing");
    }

    fn set_cell(column: usize, row: usize, name: &str) -> SetCell {
        SetCell { column, row, path: format!("/music/samples/{name}.wav"), name: name.into(), bpm: 124.0, looped: column % 2 == 0 }
    }

    /// A sparse set over all four pages, its last cell included, reads back from its JSON exactly. Positions and the
    /// page are 1-based as the deck labels them, and a cell holds only its file, name, BPM and loop setting.
    #[test]
    fn a_saved_set_reads_back_exactly() {
        let set = RemixSet { quantize: false, page: 3, cells: vec![set_cell(0, 0, "Kick"), set_cell(1, 6, "Vocal: ünï \"cœur\""), set_cell(3, 15, "Last")] };
        let json = set.to_json();
        assert_eq!(RemixSet::parse(&json), Ok(set));
        let doc: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!((doc["page"].as_u64(), doc["cells"][2]["column"].as_u64(), doc["cells"][2]["row"].as_u64()), (Some(4), Some(4), Some(16)));
        let mut keys: Vec<&str> = doc["cells"][0].as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort();
        assert_eq!(keys, ["bpm", "column", "looped", "name", "path", "row"]);
    }

    /// Anything but a whole, current set is refused, saying what is wrong with it.
    #[test]
    fn broken_or_newer_sets_are_refused_with_the_reason() {
        use serde_json::Value;
        let good = RemixSet { quantize: true, page: 0, cells: vec![set_cell(0, 0, "Kick")] }.to_json();
        let edit = |change: &dyn Fn(&mut Value)| {
            let mut doc: Value = serde_json::from_str(&good).unwrap();
            change(&mut doc);
            doc.to_string()
        };
        let cases = [
            ("{\"format\": ".to_string(), "JSON"),
            ("[]".into(), "isn't a remix set"),
            (edit(&|v| v["format"] = "retraktor-collection".into()), "isn't a ReTraktor remix set"),
            (edit(&|v| v["version"] = 2.into()), "newer ReTraktor"),
            (edit(&|v| v["version"] = 0.into()), "format version"),
            (edit(&|v| v["columns"] = 8.into()), "grid"),
            (edit(&|v| v["rows"] = 4.into()), "grid"),
            (edit(&|v| v["page"] = 5.into()), "page"),
            (edit(&|v| v["quantize"] = "yes".into()), "quantize"),
            (edit(&|v| v["cells"][0]["column"] = 5.into()), "outside"),
            (edit(&|v| v["cells"][0]["row"] = 0.into()), "outside"),
            (edit(&|v| v["cells"][0]["row"] = (-3).into()), "outside"),
            (edit(&|v| v["cells"][0]["row"] = 2.5.into()), "outside"),
            (edit(&|v| v["cells"][0]["path"] = "samples/kick.wav".into()), "absolute path"),
            (edit(&|v| v["cells"][0]["bpm"] = "fast".into()), "BPM"),
            (edit(&|v| v["cells"][0]["bpm"] = (-1.0).into()), "BPM"),
            (edit(&|v| v["cells"][0].as_object_mut().unwrap().clear()), "outside"),
            (edit(&|v| { let c = v["cells"][0].clone(); v["cells"].as_array_mut().unwrap().push(c) }), "cell 1.1 twice"),
            (edit(&|v| v["cells"] = vec![Value::Null; 65].into()), "lists 65 cells"),
        ];
        for (text, want) in cases {
            let e = RemixSet::parse(&text).unwrap_err();
            assert!(e.contains(want), "wanted '{want}', got '{e}' for {text}");
        }

        let dir = std::env::temp_dir().join(format!("retraktor-set-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let big = dir.join("big.remixset");
        std::fs::write(&big, vec![b' '; MAX_SET_BYTES as usize + 1]).unwrap();
        assert!(read_set(&big).unwrap_err().contains("bigger"));
        let binary = dir.join("binary.remixset");
        std::fs::write(&binary, [0xff, 0xfe, 0x00, 0x7b]).unwrap();
        assert!(read_set(&binary).unwrap_err().contains("can't be read"));
        assert!(read_set(&dir.join("gone.remixset")).unwrap_err().contains("can't be read"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Saving over a set swaps the whole new file in, or (a folder it can't write to, a folder in its place, a set too
    /// big to open again, a name the chooser never confirmed) leaves the old one exactly as it was, with no temporary
    /// file left behind. Without a confirmed name a set is only ever published under a free one.
    #[test]
    fn saving_over_a_set_replaces_it_whole_or_not_at_all() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("retraktor-set-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Friday.remixset");
        let first = RemixSet { quantize: true, page: 0, cells: vec![set_cell(0, 0, "Kick")] };
        let second = RemixSet { quantize: false, page: 2, cells: vec![set_cell(3, 15, "Last")] };
        write_set(&path, &first, false).unwrap();
        write_set(&path, &second, true).unwrap();
        assert_eq!(read_set(&path), Ok(second.clone()));
        let files = || std::fs::read_dir(&dir).unwrap().count();

        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
        let root = std::fs::File::create(dir.join("probe")).is_ok(); // permissions don't bind root
        let refused = write_set(&path, &first, true);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        if !root {
            assert!(refused.is_err());
            assert_eq!(read_set(&path), Ok(second.clone()), "a failed save touched the old set");
            assert_eq!(files(), 1, "a temporary file was left behind");
        }
        let _ = std::fs::remove_file(dir.join("probe"));

        let folder = dir.join("folder.remixset");
        std::fs::create_dir(&folder).unwrap();
        assert!(write_set(&folder, &first, true).is_err());
        assert_eq!((files(), read_set(&path)), (2, Ok(second.clone())), "a temporary file was left behind");

        let e = write_set(&path, &first, false).unwrap_err();
        assert!(e.contains("already exists"), "{e}");
        let huge = RemixSet { quantize: true, page: 0, cells: vec![SetCell { name: "x".repeat(MAX_SET_BYTES as usize), ..set_cell(0, 0, "Kick") }] };
        let e = write_set(&path, &huge, true).unwrap_err();
        assert!(e.contains("more than ReTraktor opens"), "{e}");
        assert_eq!((files(), read_set(&path)), (2, Ok(second)), "a refused save touched the old set");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Runs `frames` frames of `engine` against a reference track playing at `bpm` from beat `start`, at `rate`.
    fn run(engine: &mut RemixEngine, frames: usize, rate: u32, bpm: f64, start: f64, from: usize) -> Vec<[(f32, f32); 4]> {
        (from..from + frames)
            .map(|f| {
                let beat = start + f as f64 * bpm / 60.0 / rate as f64;
                let mut out = [(0.0, 0.0); 4];
                engine.frame(Some((0, beat, bpm)), bpm, rate, &mut out);
                out
            })
            .collect()
    }

    /// On-beat launches start on the beat line to within a frame, replacing the column's cell there with a 3 ms
    /// fade while another column plays on untouched; an immediate launch starts on the next frame; stops fade.
    #[test]
    fn columns_launch_on_the_beat_and_replace_only_themselves() {
        for rate in [44_100u32, 48_000] {
            let b = budget();
            let mut engine = RemixEngine::default();
            let mut retired = Vec::with_capacity(256);
            let loop_a = level("loop a", 0.25, rate as usize * 2, rate, 120.0, &b); // 4 beats at 120
            let loop_b = level("loop b", 0.5, rate as usize * 2, rate, 120.0, &b);
            let vocal = level("vocal", 0.125, rate as usize * 10, rate, 0.0, &b);
            let mut requests: [[Option<RemixRequest>; COLUMNS]; 4] = Default::default();
            let launch = |s: &Arc<RemixSample>, row: u8, looped: bool, on_beat: bool| Some(RemixRequest { launch: Some(Launch { sample: Arc::clone(s), row, looped }), on_beat });
            requests[1][0] = launch(&loop_a, 0, true, true);
            requests[1][3] = launch(&vocal, 4, false, false);
            engine.take_requests(&mut requests, &mut retired);
            // From beat 2.3: the next beat line (3) is 0.7 beat = 0.35 s away.
            let beat_frames = rate as f64 / 2.0;
            let out = run(&mut engine, rate as usize, rate, 120.0, 2.3, 0);
            let first = |col_level: f32, out: &[[(f32, f32); 4]]| out.iter().position(|o| o[1].0 >= col_level - 1e-6).unwrap();
            assert!(out[0][1].0 == 0.125, "{rate}: the immediate vocal starts on the first frame");
            let onset = first(0.125 + 0.25, &out);
            let ideal = 0.7 * beat_frames;
            assert!((onset as f64 - ideal).abs() <= 1.0, "{rate}: loop at frame {onset}, beat at {ideal}");

            // One second on (beat 4.3), replace column 0 on the next beat line, 5.
            let mut status = [[RemixColumn::default(); COLUMNS]; 4];
            requests[1][0] = launch(&loop_b, 1, true, true);
            engine.take_requests(&mut requests, &mut retired);
            engine.publish(&mut status);
            assert_eq!(status[1][0].queued, Some(Some(1)));
            assert_eq!(status[1][0].playing.map(|p| p.0), Some(0));
            let at = rate as usize;
            let out = run(&mut engine, rate as usize, rate, 120.0, 2.3, at);
            let swap = out.iter().position(|o| o[1].0 > 0.125 + 0.25 + 1e-4).unwrap();
            let ideal = (5.0 - 2.3) * beat_frames - at as f64;
            assert!((swap as f64 - ideal).abs() <= 1.0, "{rate}: swapped at {swap}, beat at {ideal}");
            // Over the 3 ms crossfade, B rises as A falls: the level climbs steadily from A's to B's, never past it.
            let fade = (0.003 * rate as f64).round() as usize;
            let mut before = 0.125 + 0.25;
            for o in &out[swap..swap + fade] {
                let now = o[1].0;
                assert!(now >= before - 1e-6 && now - before <= 0.25 / fade as f32 + 1e-5 && now <= 0.125 + 0.5 + 1e-6, "{rate}: {before} then {now}");
                before = now;
            }
            assert!((out[swap + fade - 1][1].0 - (0.125 + 0.5)).abs() < 1e-6);
            assert!((out[swap + fade + 1][1].0 - (0.125 + 0.5)).abs() < 1e-6);
            assert!(out.iter().all(|o| o[0] == (0.0, 0.0) && o[2] == (0.0, 0.0) && o[3] == (0.0, 0.0)), "other decks silent");
            engine.publish(&mut status);
            assert_eq!(status[1][0].playing, Some((1, loop_b.id)));
            assert_eq!(status[1][3].playing, Some((4, vocal.id)), "the vocal column plays on");

            // Stop column 0 at once: it fades out over 3 ms and its samples come back for the UI to free.
            requests[1][0] = Some(RemixRequest { launch: None, on_beat: false });
            engine.take_requests(&mut requests, &mut retired);
            let out = run(&mut engine, fade + 2, rate, 120.0, 2.3, 2 * rate as usize);
            assert!((out[fade + 1][1].0 - 0.125).abs() < 1e-6, "{rate}: only the vocal after the fade");
            engine.take_requests(&mut requests, &mut retired);
            let back: Vec<_> = retired.drain(..).collect();
            for s in [&loop_a, &loop_b] {
                assert!(back.iter().any(|r| Arc::ptr_eq(&(Arc::clone(s) as Retired), r)), "{rate}: {} not retired", s.name);
            }
        }
    }

    /// Replacing a sounding cell crossfades. Samples whose first frame is at full level (0.25, then 0.5): the
    /// first launch, into silence, is at full level on its first frame; the replacement never jumps, never
    /// passes the louder level and moves by at most one crossfade step a frame. A loop relaunched over itself
    /// never doubles. Switches faster than a crossfade happen at once, each ramping from what was last heard, with
    /// the same bounds.
    #[test]
    fn replacements_crossfade_without_jumps_or_doubling() {
        let rate = 48_000u32;
        let step = 0.25 / 144.0 + 1e-6; // the level change of one crossfade frame from 0.25 to 0.5
        let b = budget();
        let quiet = level("quiet", 0.25, rate as usize * 2, rate, 120.0, &b);
        let loud = level("loud", 0.5, rate as usize * 2, rate, 120.0, &b);
        let mut engine = RemixEngine::default();
        let mut retired = Vec::with_capacity(256);
        let mut heard: Vec<f32> = Vec::new();
        let play = |engine: &mut RemixEngine, retired: &mut Vec<Retired>, sample: &Arc<RemixSample>, frames: usize, heard: &mut Vec<f32>| {
            let mut requests: [[Option<RemixRequest>; COLUMNS]; 4] = Default::default();
            requests[0][0] = Some(RemixRequest { launch: Some(Launch { sample: Arc::clone(sample), row: 0, looped: true }), on_beat: false });
            engine.take_requests(&mut requests, retired);
            let start = heard.len();
            heard.extend(run(engine, frames, rate, 120.0, 0.3, start).iter().map(|o| o[0].0));
            retired.clear();
            start
        };
        play(&mut engine, &mut retired, &quiet, 200, &mut heard);
        assert_eq!(heard[0], 0.25, "a cell into silence starts at full level");
        let swap = play(&mut engine, &mut retired, &loud, 300, &mut heard);
        assert_eq!(heard[swap + 143], 0.5, "the crossfade ends on the new level");
        let relaunch = play(&mut engine, &mut retired, &loud, 300, &mut heard);
        assert!(heard[relaunch..].iter().all(|&x| (x - 0.5).abs() < 1e-6), "a loop over itself doubled");
        // Three switches within one crossfade.
        play(&mut engine, &mut retired, &quiet, 70, &mut heard);
        play(&mut engine, &mut retired, &loud, 30, &mut heard);
        play(&mut engine, &mut retired, &quiet, 600, &mut heard);
        assert_eq!(*heard.last().unwrap(), 0.25, "the last switch wins");
        for (i, pair) in heard.windows(2).enumerate() {
            assert!((pair[1] - pair[0]).abs() <= step, "a jump of {} at frame {}", pair[1] - pair[0], i + 1);
            assert!((0.25 - 1e-6..=0.5 + 1e-6).contains(&pair[1]), "{} at frame {}", pair[1], i + 1);
        }
    }

    /// A cell launched at once 50 frames before a beat, then a quantized swap asked for during its crossfade: the
    /// swap's row takes over on the beat's very frame, its sample starting there (not after the ramp), and the
    /// level still moves smoothly (one ramp step a frame at most) to the new cell's.
    #[test]
    fn a_quantized_swap_during_a_crossfade_lands_on_its_beat() {
        let rate = 48_000u32;
        let b = budget();
        let samples = [0.5f32, 0.25, 0.375].map(|v| level("s", v, rate as usize * 2, rate, 120.0, &b));
        let mut engine = RemixEngine::default();
        let mut retired = Vec::with_capacity(256);
        let beat_step = 2.0 / rate as f64; // beats per frame at 120 BPM
        let start = 2.0 - 999.5 * beat_step; // beat line 2 half a frame before frame 1000
        let mut heard = Vec::new();
        let frame_at =|engine: &mut RemixEngine, f: usize, heard: &mut Vec<f32>| {
            let mut out = [(0.0, 0.0); 4];
            engine.frame(Some((0, start + f as f64 * beat_step, 120.0)), 120.0, rate, &mut out);
            heard.push(out[0].0);
        };
        let ask = |engine: &mut RemixEngine, retired: &mut Vec<Retired>, row: u8, on_beat: bool| {
            let mut requests: [[Option<RemixRequest>; COLUMNS]; 4] = Default::default();
            requests[0][0] = Some(RemixRequest { launch: Some(Launch { sample: Arc::clone(&samples[row as usize]), row, looped: true }), on_beat });
            engine.take_requests(&mut requests, retired);
        };
        ask(&mut engine, &mut retired, 0, false);
        (0..900).for_each(|f| frame_at(&mut engine, f, &mut heard));
        ask(&mut engine, &mut retired, 1, false); // at once, 100 frames before the beat
        (900..960).for_each(|f| frame_at(&mut engine, f, &mut heard));
        ask(&mut engine, &mut retired, 2, true); // on the beat, mid-crossfade
        let mut status = [[RemixColumn::default(); COLUMNS]; 4];
        for f in 960..1400 {
            frame_at(&mut engine, f, &mut heard);
            engine.publish(&mut status);
            let row = status[0][0].playing.map(|p| p.0);
            if f < 1000 {
                assert_eq!(row, Some(1), "frame {f}: swapped before its beat");
            } else if f == 1000 {
                assert_eq!(row, Some(2), "the swap missed its beat's frame");
                let frames_in = status[0][0].progress as f64 * rate as f64 * 2.0;
                assert!((0.99..=2.01).contains(&frames_in), "the new cell started {frames_in} frames in, not at its beat");
            }
        }
        assert_eq!(*heard.last().unwrap(), 0.375);
        let biggest = heard[1..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest <= 0.25 / 144.0 + 1e-6, "a step of {biggest}");
    }

    /// The old cell is a one-shot that runs out 30 frames into the crossfade: its last frame is held and fades
    /// for the rest of the ramp, so the new cell keeps rising for the whole 3 ms instead of jumping to full level.
    #[test]
    fn an_outgoing_one_shot_running_out_keeps_the_ramp() {
        let rate = 48_000u32;
        let b = budget();
        let shot = level("shot", 0.5, 100, rate, 0.0, &b);
        let pad = level("pad", 0.25, rate as usize, rate, 120.0, &b);
        let mut engine = RemixEngine::default();
        let mut retired = Vec::with_capacity(256);
        let mut heard = Vec::new();
        for (sample, looped, frames) in [(&shot, false, 70), (&pad, true, 300)] {
            let mut requests: [[Option<RemixRequest>; COLUMNS]; 4] = Default::default();
            requests[0][0] = Some(RemixRequest { launch: Some(Launch { sample: Arc::clone(sample), row: 0, looped }), on_beat: false });
            engine.take_requests(&mut requests, &mut retired);
            let from = heard.len();
            heard.extend(run(&mut engine, frames, rate, 120.0, 0.3, from).iter().map(|o| o[0].0));
        }
        let biggest = heard.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(biggest <= 0.25 / 144.0 + 1e-6, "a step of {biggest} when the one-shot ran out");
        assert!(heard[70 + 100] > 0.25 + 0.05, "the new cell reached full level early: {}", heard[70 + 100]);
        assert_eq!(heard[70 + 143], 0.25);
    }

    /// The callback's own clock carries on from the reference's last beat at the master tempo when the reference
    /// stops, so a launch then still lands one beat on; picking up a new reference doesn't fire a launch early.
    #[test]
    fn clock_carries_on_without_a_reference() {
        let mut clock = BeatClock::default();
        let rate = 48_000.0;
        let dt = 1.0 / rate;
        clock.tick(Some((0, 10.5, 120.0)), 120.0, dt);
        let mut crossed_at = None;
        for f in 1..48_000 {
            if clock.tick(None, 120.0, dt).1.is_some() {
                crossed_at = Some(f);
                break;
            }
        }
        let f = crossed_at.expect("no beat");
        assert!((12_000..=12_001).contains(&f), "half a beat at 120 BPM after the reference stopped, not {f} frames");
        assert!(clock.tick(Some((2, 40.75, 120.0)), 120.0, dt).1.is_none(), "a new reference is no beat");
        assert!(clock.tick(Some((2, 40.75 + 0.5 / 24_000.0, 120.0)), 120.0, dt).1.is_none());
    }

    /// A jump of the reference (a hot cue to beat 15.5, a seek back) is no beat: a launch waits for the next line
    /// the playback crosses, 16. A jump that lands on a line (a hot cue on the beat) is one. A hand easing the
    /// platter back over a line never fires; playing forward over it again does.
    #[test]
    fn jumps_wait_for_the_next_real_beat() {
        let dt = 1.0 / 48_000.0;
        let step = 2.0 / 48_000.0; // beats per frame at 120 BPM
        let mut clock = BeatClock::starting_at(10.2);
        assert!(clock.tick(Some((0, 10.2 + step, 120.0)), 120.0, dt).1.is_none());
        assert!(clock.tick(Some((0, 15.5, 120.0)), 120.0, dt).1.is_none(), "a hot cue into beat 15 fired");
        let fired = (1..48_000).find(|&f| clock.tick(Some((0, 15.5 + f as f64 * step, 120.0)), 120.0, dt).1.is_some());
        assert!(fired.is_some_and(|f| (12_000..=12_001).contains(&f)), "fired after {fired:?} frames, not at beat 16");
        assert!(clock.tick(Some((0, 3.25, 120.0)), 120.0, dt).1.is_none(), "a seek back fired");
        assert_eq!(clock.tick(Some((0, 20.0, 120.0)), 120.0, dt).1, Some(0.0), "a hot cue on beat 20 is that beat");
        assert!(clock.tick(Some((0, 20.0 - step, 120.0)), 120.0, dt).1.is_none(), "easing back over the line fired");
        assert!(clock.tick(Some((0, 20.0 + step / 2.0, 120.0)), 120.0, dt).1.is_some(), "playing over it again didn't");
    }

    /// A track `total` frames long that is 0.1 / -0.1 before frame `start`, a ramp different on each side from there
    /// to `end`, and 0.9 / -0.9 after it: any frame from outside the loop shows.
    fn looped_track(start: usize, end: usize, total: usize) -> Vec<f32> {
        (0..total)
            .flat_map(|f| match f {
                f if f < start => [0.1, -0.1],
                f if f < end => [0.2 + (f - start) as f32 * 1e-6, -0.3 - (f - start) as f32 * 1e-6],
                _ => [0.9, -0.9],
            })
            .collect()
    }

    fn capture_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("retraktor-captures-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn eventually(what: &str, mut done: impl FnMut() -> bool) {
        for _ in 0..1000 {
            if done() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("{what} never happened");
    }

    /// A 4-beat loop at 120 BPM in a 44.1 kHz track, its loop-in a fraction of a frame past 3 s: the capture is that
    /// loop's frames exactly (rounded to whole frames, stereo and rate as they are, nothing from before or after),
    /// a 120 BPM loop, saved as a float WAV that decodes back bit for bit, and the track's audio is unchanged. Played
    /// at 120 BPM it repeats every 4 beats with every frame exact, across the seam too.
    #[test]
    fn a_capture_is_the_loops_exact_audio_and_repeats_on_the_beat() {
        let rate = 44_100u32;
        let (start, end) = (132_300, 220_500);
        let pcm = Arc::new(looped_track(start, end, 10 * rate as usize));
        let dir = capture_dir("exact");
        let b = budget();
        let src = CaptureSource { pcm: Arc::clone(&pcm), sample_rate: rate, loop_start_sec: 3.000_004, loop_len_sec: 2.0, bpm: 120.0, title: "Groove/Edit".into() };
        assert_eq!(src.frames(), Ok((start, end)));
        let s = RemixSample::capture(&src, &dir, &b, &|| false).unwrap();
        assert_eq!((s.frames(), s.sample_rate, s.bpm, s.fits_beats), (end - start, rate, Some(120.0), true));
        assert!(s.pcm == pcm[2 * start..2 * end], "not exactly the loop's frames");
        assert_eq!((s.pcm[0], s.pcm[1]), (0.2, -0.3), "it doesn't start on the loop-in frame");
        assert!(s.pcm.iter().all(|&x| x.abs() != 0.1 && x.abs() != 0.9), "audio from outside the loop");
        assert_eq!((s.name.as_str(), b.used()), ("Groove/Edit loop 0:03.000", (end - start) * 8));
        assert!(*pcm == looped_track(start, end, 10 * rate as usize), "the track's audio changed");

        let files: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
        assert_eq!(files, [s.source.clone()], "one WAV and nothing else");
        let file_name = s.source.file_name().unwrap().to_str().unwrap();
        assert!(file_name.starts_with("Groove_Edit loop 0m03.000s ") && file_name.ends_with(".wav"), "{file_name}");
        let mut wav = AudioFile::open(&s.source).unwrap();
        assert_eq!(wav.sample_rate, rate);
        let decoded = wav.decode_all(&|| false).unwrap();
        assert!(decoded.len() == s.pcm.len() && decoded.iter().zip(&s.pcm).all(|(a, b)| a.to_bits() == b.to_bits()), "the WAV isn't the loop bit for bit");

        let sample = Arc::new(s);
        let n = sample.frames();
        assert_eq!(n as f64, 4.0 * 60.0 / 120.0 * rate as f64, "the loop isn't 4 beats");
        let mut engine = RemixEngine::default();
        let mut retired = Vec::with_capacity(256);
        let mut requests: [[Option<RemixRequest>; COLUMNS]; 4] = Default::default();
        requests[1][0] = Some(RemixRequest { launch: Some(Launch { sample: Arc::clone(&sample), row: 0, looped: true }), on_beat: false });
        engine.take_requests(&mut requests, &mut retired);
        for (i, o) in run(&mut engine, 2 * n + 100, rate, 120.0, 0.0, 0).iter().enumerate() {
            let f = i % n;
            assert_eq!(o[1], (sample.pcm[2 * f], sample.pcm[2 * f + 1]), "output frame {i}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A loop that can't be a cell (no frame, not a place in the track, past its end, over 60 s), a full bank, a
    /// cancelled capture and a folder it can't write in are refused, saying why, with nothing taken or left on disk.
    /// A capture never replaces another file, even one with the same name.
    #[test]
    fn captures_that_cant_be_cells_or_files_are_refused_and_leave_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let rate = 1000u32;
        let pcm = Arc::new(vec![0.5f32; 100 * rate as usize * 2]);
        let src = |start: f64, len: f64| CaptureSource { pcm: Arc::clone(&pcm), sample_rate: rate, loop_start_sec: start, loop_len_sec: len, bpm: 120.0, title: "t".into() };
        let refusals = [
            (1.0, 0.0, "shorter than one frame"),
            (1.0, 0.0004, "shorter than one frame"),
            (f64::NAN, 2.0, "valid place"),
            (-1.0, 2.0, "valid place"),
            (1.0, f64::INFINITY, "valid place"),
            (99.0, 2.0, "past the end"),
            (10.0, 60.5, "longer than 60 s"),
        ];
        for (start, len, want) in refusals {
            let e = src(start, len).frames().unwrap_err();
            assert!(e.contains(want), "{start} + {len}: {e}");
        }
        assert_eq!(src(10.0, 60.0).frames(), Ok((10_000, 70_000)), "60 s is a cell");
        assert_eq!(src(98.0, 2.0).frames(), Ok((98_000, 100_000)), "up to the last frame is in the track");

        let dir = capture_dir("refused");
        let listing = |dir: &Path| std::fs::read_dir(dir).unwrap().count();
        let small = SampleBudget::new(1000 * 8);
        assert!(RemixSample::capture(&src(1.0, 2.0), &dir, &small, &|| false).err().unwrap().contains("full"));
        let b = budget();
        assert_eq!(RemixSample::capture(&src(1.0, 2.0), &dir, &b, &|| true).err().as_deref(), Some("Cancelled"));
        assert!(RemixSample::capture(&src(99.0, 2.0), &dir, &b, &|| false).err().unwrap().contains("past the end"));
        assert_eq!((small.used(), b.used(), listing(&dir)), (0, 0, 0));

        let blocked = dir.join("a file, not a folder");
        std::fs::write(&blocked, b"x").unwrap();
        let e = RemixSample::capture(&src(1.0, 2.0), &blocked, &b, &|| false).err().unwrap();
        assert!(e.contains("can't save the capture in") && e.contains("a file, not a folder"), "{e}");
        let locked = dir.join("locked");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let root = std::fs::File::create(locked.join("probe")).is_ok(); // permissions don't bind root
        let refused = RemixSample::capture(&src(1.0, 2.0), &locked, &b, &|| false);
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).unwrap();
        match refused {
            Err(e) => assert!(e.contains("locked") && listing(&locked) == 0, "{e}, or a partial file was left behind"),
            Ok(_) => assert!(root, "wrote into a folder it can't write in"),
        }
        assert_eq!(b.used(), 0);

        let first = write_capture(&dir, "Same loop", &[0.25; 8], rate).unwrap();
        let second = write_capture(&dir, "Same loop", &[0.75; 8], rate).unwrap();
        let named = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!((named(&first), named(&second)), ("Same loop.wav".to_string(), "Same loop_2.wav".to_string()));
        assert_eq!(AudioFile::open(&first).unwrap().decode_all(&|| false).unwrap(), vec![0.25; 8], "the first capture was replaced");

        // A long title in three-byte characters is cut between characters, so its capture still gets a file name.
        let long = CaptureSource { title: "音".repeat(100), ..src(1.0, 2.0) };
        assert_eq!(file_stem_text(&long.title), "音".repeat(TITLE_NAME_BYTES / 3));
        let s = RemixSample::capture(&long, &dir, &b, &|| false).unwrap_or_else(|e| panic!("{e}"));
        assert!(s.source.file_name().unwrap().len() < 200 && s.name.starts_with(&long.title));
        drop(s);
        assert_eq!(b.used(), 0);

        // Only a home folder a set can name (absolute UTF-8) takes captures.
        use std::os::unix::ffi::OsStringExt;
        let not_text = std::ffi::OsString::from_vec(b"/home/dj\xff".to_vec());
        assert!(captures_dir_in(Some(not_text)).unwrap_err().contains("UTF-8"));
        assert!(captures_dir_in(None).is_err() && captures_dir_in(Some("music".into())).is_err());
        assert_eq!(captures_dir_in(Some("/home/dj".into())), Ok(PathBuf::from("/home/dj/Music/ReTraktor/Captures")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The loader puts a capture in its cell as a loop, even one with no BPM. A capture cancelled before it lands
    /// leaves no file, also when it had already finished: the cell stays empty, its WAV is deleted and its memory
    /// freed, while the WAV a cell holds is kept.
    #[test]
    fn captures_land_as_loops_and_superseded_ones_leave_no_file() {
        let rate = 1000u32;
        let pcm = Arc::new(vec![0.5f32; 20 * rate as usize * 2]);
        let src = |title: &str| CaptureSource { pcm: Arc::clone(&pcm), sample_rate: rate, loop_start_sec: 2.0, loop_len_sec: 3.3, bpm: 0.0, title: title.into() };
        let dir = capture_dir("superseded");
        let names = || std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let mut loader = RemixLoader::default();
        let mut banks: [RemixBank; 4] = Default::default();

        loader.capture(&mut banks[2], 2, 1, 3, src("Break"), dir.clone());
        let mut outcomes = Vec::new();
        eventually("the capture", || {
            outcomes.extend(loader.poll(&mut banks));
            !RemixLoader::busy(&banks)
        });
        assert!(matches!(&outcomes[..], [(2, 1, 3, Ok(name))] if name == "Break loop 0:02.000"), "{:?}", outcomes.iter().map(|o| &o.3).collect::<Vec<_>>());
        let kept = banks[2].cells[1][3].as_ref().unwrap().sample.source.clone();
        assert!(banks[2].cells[1][3].as_ref().is_some_and(|c| c.looped && !c.sample.fits_beats && c.sample.bpm.is_none()));

        // Cancelled at once: the worker skips it, or stops before or after writing, and deletes what it wrote.
        loader.capture(&mut banks[2], 2, 2, 0, src("Gone"), dir.clone());
        loader.cancel(&mut banks[2], 2, 2, 0);
        // A capture that finished, WAV and all, for a load no cell wants any more: poll drops it and has the worker
        // delete its WAV, after the cancelled one (the worker goes in order), so then both are gone for sure.
        let stale = write_capture(&dir, "Stale", &[0.5; 8], rate).unwrap();
        let mut sample = RemixSample::from_pcm("Stale", vec![0.5; 8], rate, 0.0, &loader.budget).unwrap();
        sample.source = stale.clone();
        let done = LoadedCell { deck: 2, column: 3, row: 0, generation: u64::MAX, captured: true, result: Ok(Arc::new(sample)) };
        loader.results.0.send(done).unwrap();
        assert!(loader.poll(&mut banks).is_empty(), "a superseded capture reported");
        eventually("the stale capture's WAV deleted", || !stale.exists());
        assert_eq!(names(), [kept.file_name().unwrap().to_string_lossy()], "the kept capture went, or another stayed");
        assert!(banks[2].cells[2][0].is_none() && banks[2].cells[3][0].is_none());
        assert_eq!(loader.budget.used(), 3300 * 8, "only the kept capture's audio is held");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Closing the app (the loader dropped): a capture still queued or being made never leaves a file, a finished one
    /// no cell took yet is deleted, and the WAV a cell holds stays, with the cell.
    #[test]
    fn closing_keeps_installed_captures_and_deletes_unclaimed_ones() {
        let rate = 1000u32;
        let pcm = Arc::new(vec![0.5f32; 20 * rate as usize * 2]);
        let src = |title: &str| CaptureSource { pcm: Arc::clone(&pcm), sample_rate: rate, loop_start_sec: 2.0, loop_len_sec: 3.3, bpm: 120.0, title: title.into() };
        let dir = capture_dir("closing");
        let names = || std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect::<Vec<_>>();
        let mut loader = RemixLoader::default();
        let mut banks: [RemixBank; 4] = Default::default();
        loader.capture(&mut banks[0], 0, 0, 0, src("Kept"), dir.clone());
        eventually("the capture", || {
            loader.poll(&mut banks);
            !RemixLoader::busy(&banks)
        });
        let kept = banks[0].cells[0][0].as_ref().expect("nothing captured").sample.source.clone();

        loader.capture(&mut banks[1], 1, 0, 0, src("Pending"), dir.clone());
        let unclaimed = write_capture(&dir, "Unclaimed", &[0.5; 8], rate).unwrap();
        let mut sample = RemixSample::from_pcm("Unclaimed", vec![0.5; 8], rate, 0.0, &loader.budget).unwrap();
        sample.source = unclaimed;
        let done = LoadedCell { deck: 0, column: 2, row: 0, generation: loader.next_generation, captured: true, result: Ok(Arc::new(sample)) };
        loader.results.0.send(done).unwrap();
        drop(loader);
        assert_eq!(names(), [kept.file_name().unwrap().to_string_lossy()], "an unclaimed capture stayed, or the kept one went");
        assert!(banks[0].cells[0][0].as_ref().is_some_and(|c| c.sample.source == kept));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A set storing its files in another order than its cells: a synced loop (one beat of 0.5 s at 120 BPM), a
    /// one-shot, and an unsynced loop sharing the first loop's file. Imported, each cell sits in its place with its
    /// name, loop or one-shot and tempo (the unsynced loop at its own speed) and the exact audio, each file copied
    /// once into a new folder, the .trak untouched. A WAV cut short or a cancelled import is refused and leaves no
    /// folder or memory behind; a second import takes a folder of its own, removed unless kept.
    #[test]
    fn a_traktor_set_imports_whole_or_leaves_nothing() {
        use trak_fixture::{archive, grid, nisound, Cell};
        let dir = std::env::temp_dir().join(format!("retraktor-trak-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("Remix Sets");
        let rate = 8000;
        let loop_pcm: Vec<f32> = (0..4000).flat_map(|i| [i as f32 / 8000.0, -0.25]).collect();
        let shot_pcm = vec![0.5f32; 2 * 1000];
        let cells = [
            (0, 0, Cell { file: "loop.wav", name: "Trance Kick", play_type: 0, synced: true, bpm: 120.0 }),
            (1, 6, Cell { file: "shot.wav", name: "Hi Reverse", play_type: 1, synced: false, bpm: 0.0 }),
            (3, 15, Cell { file: "loop.wav", name: "Free loop", play_type: 0, synced: false, bpm: 98.0 }),
        ];
        let set = |shot: &[u8]| archive(&[("shot.wav", shot), ("loop.wav", &test_wav(&loop_pcm, rate)), ("NISOUND", &nisound(true, &grid(&cells), 3))]);
        let trak = dir.join("Electric Test.trak");
        let bytes = set(&test_wav(&shot_pcm, rate));
        std::fs::write(&trak, &bytes).unwrap();
        let folders = || std::fs::read_dir(&root).map_or(0, |d| d.count());
        let b = budget();

        let (bank, imported) = import_trak(&trak, &root, &b, &|| false).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(bank.quantize && bank.page == 0 && bank.contents().1.len() == 3);
        let cell = |c: usize, r: usize| bank.cells[c][r].as_ref().unwrap_or_else(|| panic!("cell {}.{} is empty", c + 1, r + 1));
        let (kick, shot, free) = (cell(0, 0), cell(1, 6), cell(3, 15));
        assert_eq!((kick.sample.name.as_str(), kick.looped, kick.sample.bpm, kick.sample.fits_beats), ("Trance Kick", true, Some(120.0), true));
        assert_eq!((shot.sample.name.as_str(), shot.looped, shot.sample.bpm), ("Hi Reverse", false, None));
        assert_eq!((free.sample.name.as_str(), free.looped, free.sample.bpm, free.sample.library_bpm), ("Free loop", true, None, 0.0));
        assert!(kick.sample.pcm == loop_pcm && free.sample.pcm == loop_pcm && shot.sample.pcm == shot_pcm, "not the set's exact audio");
        let folder = kick.sample.source.parent().unwrap().to_path_buf();
        assert_eq!((free.sample.source.as_path(), shot.sample.source.parent()), (kick.sample.source.as_path(), Some(folder.as_path())));
        let mut files: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
        files.sort();
        assert_eq!(files, ["loop.wav", "shot.wav"]);
        assert!(std::fs::read(folder.join("loop.wav")).unwrap() == test_wav(&loop_pcm, rate), "a standard WAV's copy isn't byte for byte");
        assert_eq!(imported.keep(), folder);
        let used = b.used();

        // Its header promises 1000 frames and only 750 are there: refused, not taken as a shorter sample.
        let mut cut = test_wav(&shot_pcm, rate);
        cut.truncate(cut.len() - 250 * 8);
        std::fs::write(&trak, set(&cut)).unwrap();
        let e = import_trak(&trak, &root, &b, &|| false).err().unwrap().join("\n");
        assert!(e.contains("cell 2.7 (page 2) 'Hi Reverse' from 'shot.wav'") && e.contains("cut short"), "{e}");
        std::fs::write(&trak, &bytes).unwrap();
        assert_eq!(import_trak(&trak, &root, &b, &|| true).err().unwrap(), ["Cancelled"]);
        assert_eq!((folders(), b.used()), (1, used), "a refused import left files or memory behind");

        let before = std::fs::read(folder.join("shot.wav")).unwrap();
        let (_, again) = import_trak(&trak, &root, &b, &|| false).unwrap();
        assert_eq!(folders(), 2);
        drop(again);
        assert_eq!(folders(), 1, "an import nobody kept left its folder");
        assert_eq!(std::fs::read(folder.join("shot.wav")).unwrap(), before);
        assert_eq!(std::fs::read(&trak).unwrap(), bytes, "the .trak changed");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A 16-bit PCM WAV whose `fmt ` chunk is 20 bytes, its last `extension` (zero in NI's own sets), then the audio
    /// and a `LIST` chunk.
    fn wav_with_20_byte_fmt(frames: &[[i16; 2]], rate: u32, extension: [u8; 4]) -> Vec<u8> {
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend(20u32.to_le_bytes());
        for field in [1u16, 2] {
            wav.extend(field.to_le_bytes());
        }
        wav.extend(rate.to_le_bytes());
        wav.extend((rate * 4).to_le_bytes());
        for field in [4u16, 16] {
            wav.extend(field.to_le_bytes());
        }
        wav.extend(extension);
        wav.extend(b"data");
        wav.extend((frames.len() as u32 * 4).to_le_bytes());
        wav.extend(frames.iter().flatten().flat_map(|s| s.to_le_bytes()));
        wav.extend(b"LIST\x04\0\0\0INFO");
        let riff = (wav.len() - 8) as u32;
        wav[4..8].copy_from_slice(&riff.to_le_bytes());
        wav
    }

    /// NI's *Drums and Beatbox* stores WAVs with a 20-byte PCM `fmt ` chunk the decoder doesn't read. Its copy gets the
    /// same format in a standard 16-byte chunk and a RIFF size 4 smaller, every byte after it (audio, LIST) unchanged;
    /// it decodes to the stored audio and reopens from a saved set. The same chunk with a nonzero extension isn't the
    /// observed case: copied as it is, and the set is refused.
    #[test]
    fn a_wav_with_a_20_byte_format_chunk_imports_and_reopens() {
        use trak_fixture::{archive, grid, nisound, Cell};
        let dir = std::env::temp_dir().join(format!("retraktor-trak-fmt20-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let frames: Vec<[i16; 2]> = (0..3000).map(|i| [i as i16 * 7, -(i as i16)]).collect();
        let odd = wav_with_20_byte_fmt(&frames, 44_100, [0; 4]);
        let cells = [(0, 14, Cell { file: "ClosedHH.wav", name: "ClosedHH", play_type: 1, synced: false, bpm: 0.0 })];
        let trak = dir.join("Beatbox.trak");
        let write = |wav: &[u8]| std::fs::write(&trak, archive(&[("ClosedHH.wav", wav), ("NISOUND", &nisound(false, &grid(&cells), 3))])).unwrap();
        write(&odd);
        let b = budget();
        let (bank, imported) = import_trak(&trak, &dir.join("Remix Sets"), &b, &|| false).unwrap_or_else(|e| panic!("{e:?}"));
        let cell = bank.cells[0][14].as_ref().unwrap();
        let mut standard = odd[..36].to_vec();
        standard[4..8].copy_from_slice(&(odd.len() as u32 - 12).to_le_bytes());
        standard[16..20].copy_from_slice(&16u32.to_le_bytes());
        standard.extend(&odd[40..]);
        assert!(std::fs::read(&cell.sample.source).unwrap() == standard, "the copy isn't the same WAV with a standard format chunk");
        let want: Vec<f32> = frames.iter().flatten().map(|&s| s as f32 / 32768.0).collect();
        assert!(cell.sample.pcm == want && cell.sample.sample_rate == 44_100, "not the stored audio");

        let saved = dir.join("Beatbox.remixset");
        write_set(&saved, &RemixSet::from_bank(&bank).unwrap(), false).unwrap();
        let reopened = stage(&read_set(&saved).unwrap(), &b, &|| false).unwrap_or_else(|e| panic!("{e:?}"));
        let again = reopened.cells[0][14].as_ref().unwrap();
        assert!(again.sample.pcm == want && again.sample.name == "ClosedHH" && !again.looped);
        drop(imported.keep());

        write(&wav_with_20_byte_fmt(&frames, 44_100, [0, 0, 1, 0]));
        let e = import_trak(&trak, &dir.join("Remix Sets"), &b, &|| false).err().unwrap().join("\n");
        assert!(e.contains("cell 1.15 (page 4) 'ClosedHH'"), "{e}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Closing while sets import (one superseded, one under way, or one finished and not yet taken) cancels and waits
    /// for every staging worker: no copied sample, folder or decoded audio is left behind.
    #[test]
    fn closing_mid_import_leaves_no_samples_behind() {
        use trak_fixture::{archive, grid, nisound, Cell};
        let dir = std::env::temp_dir().join(format!("retraktor-trak-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.join("Remix Sets");
        let names: Vec<String> = (0..40).map(|i| format!("s{i}.wav")).collect();
        let wav = test_wav(&vec![0.1f32; 2 * 16_000], 8000);
        let cells: Vec<_> = names.iter().enumerate().map(|(i, n)| (i % COLUMNS, i / COLUMNS, Cell { file: n, name: n, play_type: 1, synced: false, bpm: 0.0 })).collect();
        let set = nisound(false, &grid(&cells), 3);
        let mut records: Vec<(&str, &[u8])> = names.iter().map(|n| (n.as_str(), wav.as_slice())).collect();
        records.push(("NISOUND", &set));
        let trak = dir.join("Big.trak");
        std::fs::write(&trak, archive(&records)).unwrap();
        let folders = || std::fs::read_dir(&root).map_or(0, |d| d.count());

        let b = budget();
        let mut sets = RemixSets::default();
        sets.imports = Ok(root.clone());
        for _ in 0..2 {
            let ticket = sets.start(0, SetAction::Open, Default::default(), None, (true, 0));
            sets.stage(0, ticket, trak.clone(), &b);
        }
        for _ in 0..400 {
            if folders() > 0 {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        sets.shut_down();
        assert!(sets.stagers.is_empty());
        assert_eq!((folders(), b.used()), (0, 0), "closing left samples or audio behind");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Opt-in and local only (no NI audio is in the repository): imports every `.trak` in `RETRAKTOR_TRAK_DIR` through
    /// the real importer into `RETRAKTOR_TRAK_OUT`, keeping what it copies, and lists every cell in `cells.tsv` there
    /// for comparison with an independent reading of the same archives. Each bank is also saved as a .remixset there and
    /// reopened natively. Fails naming each set that was refused or didn't reopen the same.
    #[test]
    #[ignore = "needs genuine Traktor Remix Sets: set RETRAKTOR_TRAK_DIR and RETRAKTOR_TRAK_OUT"]
    fn genuine_traktor_sets_import_whole() {
        let (Some(sets), Some(out)) = (std::env::var_os("RETRAKTOR_TRAK_DIR"), std::env::var_os("RETRAKTOR_TRAK_OUT")) else {
            panic!("set RETRAKTOR_TRAK_DIR and RETRAKTOR_TRAK_OUT");
        };
        let out = PathBuf::from(out);
        let mut paths: Vec<PathBuf> = std::fs::read_dir(sets).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().is_some_and(|e| e == TRAK_EXTENSION)).collect();
        paths.sort();
        assert!(!paths.is_empty(), "no .trak files");
        let mut lines = vec!["archive\tcolumn\trow\tpage\tfile\tname\tlooped\tlibrary_bpm\tbpm\tframes\trate\tpath".to_string()];
        let mut refused = Vec::new();
        for path in &paths {
            let b = budget();
            let (bank, imported) = match import_trak(path, &out, &b, &|| false) {
                Ok(done) => done,
                Err(e) => {
                    refused.push(format!("{}: {e:?}", path.display()));
                    continue;
                }
            };
            imported.keep();
            let archive = path.file_name().unwrap().to_string_lossy();
            for column in 0..COLUMNS {
                for row in 0..ROWS {
                    let Some(cell) = &bank.cells[column][row] else { continue };
                    let s = &cell.sample;
                    let file = s.source.file_name().unwrap().to_string_lossy();
                    lines.push(format!(
                        "{archive}\t{}\t{}\t{}\t{file}\t{}\t{}\t{}\t{:?}\t{}\t{}\t{}",
                        column + 1,
                        row + 1,
                        row / PAGE_ROWS + 1,
                        s.name,
                        cell.looped,
                        s.library_bpm,
                        s.bpm,
                        s.frames(),
                        s.sample_rate,
                        s.source.display()
                    ));
                }
            }
            // Saved as a .remixset and opened again natively, every cell plays the same audio.
            let saved = out.join(format!("{archive}.remixset"));
            write_set(&saved, &RemixSet::from_bank(&bank).unwrap(), false).unwrap();
            match stage(&read_set(&saved).unwrap(), &b, &|| false) {
                Ok(reopened) => {
                    let same = (0..COLUMNS).flat_map(|c| (0..ROWS).map(move |r| (c, r))).all(|(c, r)| match (&bank.cells[c][r], &reopened.cells[c][r]) {
                        (Some(a), Some(b)) => a.sample.pcm == b.sample.pcm && a.sample.name == b.sample.name && a.looped == b.looped && a.sample.bpm == b.sample.bpm,
                        (None, None) => true,
                        _ => false,
                    });
                    if !same {
                        refused.push(format!("{archive}: the reopened .remixset differs"));
                    }
                }
                Err(e) => refused.push(format!("{archive}: the saved .remixset doesn't reopen: {e:?}")),
            }
        }
        std::fs::write(out.join("cells.tsv"), lines.join("\n") + "\n").unwrap();
        assert!(refused.is_empty(), "refused:\n{}", refused.join("\n"));
    }
}
