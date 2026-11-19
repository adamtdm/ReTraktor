//! Master recorder for long sets.
//!
//! The audio callback copies the master L/R into preallocated chunks ([`RecorderTap`]); a writer
//! thread streams full chunks to a 16-bit stereo WAV and hands each chunk back for reuse. The two
//! sides share only lock-free bounded queues and atomics, never `audio.shared`, so the callback
//! never allocates, frees, blocks or touches the disk. A file that outgrows RIFF's 4 GiB becomes
//! RF64 (EBU Tech 3306): the header reserves a JUNK chunk that turns into `ds64`.

use concurrent_queue::ConcurrentQueue;
use std::fs::File;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// Stereo frames per chunk: 85 ms at 48 kHz.
const CHUNK_FRAMES: usize = 4096;
/// Chunks in the pool: the disk may stall about 11 s at 48 kHz (5.5 s at 96 kHz) before an overrun.
const POOL_CHUNKS: usize = 128;
const HEADER_LEN: u64 = 80;
const BYTES_PER_FRAME: u64 = 4;
/// Header written before the first audio block tells the real rate.
const PLACEHOLDER_RATE: u32 = 48_000;
const WRITER_POLL: Duration = Duration::from_millis(20);
/// How often the header is refreshed, so a crash still leaves a playable file.
const HEADER_REFRESH: Duration = Duration::from_secs(2);

/// Why a recording ended before the DJ pressed stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    /// The writer fell behind and the callback ran out of free chunks.
    Overrun = 1,
    /// The sound card was reopened at another sample rate; one WAV holds one rate.
    RateChanged = 2,
    /// Writing to the disk failed.
    WriteFailed = 3,
}

impl StopReason {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Self::Overrun),
            2 => Some(Self::RateChanged),
            3 => Some(Self::WriteFailed),
            _ => None,
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Self::Overrun => "the disk could not keep up",
            Self::RateChanged => "the sound card's sample rate changed",
            Self::WriteFailed => "writing to the disk failed",
        }
    }
}

struct Shared {
    /// Empty chunks. One producer (the writer, after the initial fill) and one consumer (the tap).
    free: ConcurrentQueue<Vec<i16>>,
    /// Chunks ever put in `free`, counted only after each push has returned. A bounded push moves
    /// the queue's tail before it fills the slot, and a pop that meets such a slot spins until it
    /// is filled; the tap pops only while this count is ahead of its own pops, so it never meets
    /// one. The full queue needs no such guard: it can hold the whole pool.
    free_published: AtomicU64,
    full: ConcurrentQueue<Vec<i16>>,
    /// Frames the callback accepted; the recording clock and CUE offsets.
    frames: AtomicU64,
    /// Rate of the first audio block; 0 until then.
    sample_rate: AtomicU32,
    stop_reason: AtomicU8,
    stop_requested: AtomicBool,
}

impl Shared {
    /// Allocates the whole pool up front; the callback starts with one chunk in hand.
    fn new() -> (Arc<Self>, Vec<i16>) {
        let shared = Arc::new(Self {
            free: ConcurrentQueue::bounded(POOL_CHUNKS),
            free_published: AtomicU64::new(0),
            full: ConcurrentQueue::bounded(POOL_CHUNKS),
            frames: AtomicU64::new(0),
            sample_rate: AtomicU32::new(0),
            stop_reason: AtomicU8::new(0),
            stop_requested: AtomicBool::new(false),
        });
        for _ in 1..POOL_CHUNKS {
            shared.recycle(Vec::with_capacity(CHUNK_FRAMES * 2));
        }
        (shared, Vec::with_capacity(CHUNK_FRAMES * 2))
    }

    /// Returns an empty chunk to the tap. Only the writer (and `new`, before the tap exists) calls it.
    fn recycle(&self, chunk: Vec<i16>) {
        if self.free.push(chunk).is_ok() {
            self.free_published.fetch_add(1, Ordering::Release);
        }
    }

    /// The first reason wins.
    fn halt(&self, reason: StopReason) {
        let _ = self.stop_reason.compare_exchange(0, reason as u8, Ordering::AcqRel, Ordering::Acquire);
    }

    fn stop_reason(&self) -> Option<StopReason> {
        StopReason::from_u8(self.stop_reason.load(Ordering::Acquire))
    }
}

/// The callback's end of a recording; lives in `AudioSharedState` while recording.
pub struct RecorderTap {
    shared: Arc<Shared>,
    current: Option<Vec<i16>>,
    /// Chunks this tap has popped from `free`; compared with `Shared::free_published`.
    free_taken: u64,
    rate: u32,
    frames: u64,
    stopped: bool,
}

impl RecorderTap {
    fn new(shared: Arc<Shared>, first: Vec<i16>) -> Self {
        Self { shared, current: Some(first), free_taken: 0, rate: 0, frames: 0, stopped: false }
    }

    /// Once per audio block, before its frames.
    pub fn begin_block(&mut self, sample_rate: u32) {
        if self.stopped {
            return;
        }
        if let Some(reason) = self.shared.stop_reason() {
            self.halt(reason);
        } else if self.rate == 0 {
            self.rate = sample_rate;
            self.shared.sample_rate.store(sample_rate, Ordering::Release);
        } else if sample_rate != self.rate {
            self.halt(StopReason::RateChanged);
        }
        // shortcut: reopening the card at the same rate keeps recording with a jump cut over the
        // reopen, stop on reopen too if a DJ needs gapless files across device changes.
    }

    /// One master frame, as the speakers get it before the cue and split routing.
    pub fn push(&mut self, left: f32, right: f32) {
        if self.stopped {
            return;
        }
        let Some(chunk) = self.current.as_mut() else { return };
        chunk.push((left.clamp(-1.0, 1.0) * 32767.0) as i16);
        chunk.push((right.clamp(-1.0, 1.0) * 32767.0) as i16);
        self.frames += 1;
        if chunk.len() >= CHUNK_FRAMES * 2 {
            let full = self.current.take().unwrap_or_default();
            self.hand_over(full);
            let next = if self.free_ready() { self.shared.free.pop().ok() } else { None };
            match next {
                Some(next) => {
                    self.free_taken += 1;
                    self.current = Some(next);
                }
                None => self.halt(StopReason::Overrun),
            }
        }
    }

    /// A published empty chunk is waiting, so a pop returns at once instead of spinning.
    fn free_ready(&self) -> bool {
        self.shared.free_published.load(Ordering::Acquire) > self.free_taken
    }

    /// Once per audio block, after its frames.
    pub fn end_block(&self) {
        self.shared.frames.store(self.frames, Ordering::Release);
    }

    /// Hands the last partial chunk to the writer. Not for the callback: called after the tap
    /// left `AudioSharedState`.
    fn flush(mut self) {
        if let Some(chunk) = self.current.take() {
            self.hand_over(chunk);
        }
        self.end_block();
    }

    fn halt(&mut self, reason: StopReason) {
        self.stopped = true;
        self.shared.halt(reason);
        if let Some(chunk) = self.current.take() {
            self.hand_over(chunk);
        }
    }

    fn hand_over(&mut self, chunk: Vec<i16>) {
        // The full queue holds every chunk of the pool, so this push cannot fail; were it to,
        // leaking beats freeing memory on the audio thread.
        if let Err(e) = self.shared.full.push(chunk) {
            std::mem::forget(e.into_inner());
        }
    }
}

/// What ended up in the file.
#[derive(Debug)]
pub struct Saved {
    pub frames: u64,
    pub sample_rate: u32,
    /// Set when the recording ended before the DJ stopped it.
    pub reason: Option<StopReason>,
    pub error: Option<String>,
}

impl Saved {
    pub fn seconds(&self) -> f64 {
        self.frames as f64 / self.sample_rate.max(1) as f64
    }
}

/// The audio is on disk but the header could not be finished; [`Recording::retry`] tries again.
pub struct Unsaved {
    file: File,
    frames: u64,
    sample_rate: u32,
    reason: Option<StopReason>,
    error: Option<String>,
}

/// The GUI's end of a recording: from REC until the WAV is finished.
pub struct Recording {
    shared: Arc<Shared>,
    writer: Option<JoinHandle<Result<Saved, (Unsaved, String)>>>,
    unsaved: Option<Unsaved>,
    pub wav: PathBuf,
    pub cue: PathBuf,
    pub txt: PathBuf,
}

impl Recording {
    /// Reserves fresh timestamped WAV/CUE/TXT names in `dir` and starts the writer. Install the
    /// tap with `state.recorder = Some(tap)`.
    pub fn start(dir: &Path, now: SystemTime) -> Result<(Self, RecorderTap), String> {
        std::fs::create_dir_all(dir).map_err(|e| format!("Could not create {}: {e}", dir.display()))?;
        let (mut file, wav, cue, txt) = crate::setlist::reserve_recording_paths(dir, now)
            .map_err(|e| format!("Could not create a recording in {}: {e}", dir.display()))?;
        let header = file.write_all(&wav_header(0, PLACEHOLDER_RATE));
        let started = header
            .map_err(|e| format!("Could not write {}: {e}", wav.display()))
            .and_then(|()| Self::spawn(file, wav.clone(), cue.clone(), txt.clone()));
        if started.is_err() {
            remove_files(&[&wav, &cue, &txt]);
        }
        started
    }

    /// Drops a recording that never took audio, files and all: its tap could not be installed.
    pub fn discard(mut self) {
        self.shared.stop_requested.store(true, Ordering::Release);
        if let Some(writer) = self.writer.take() {
            let _ = writer.join();
        }
        remove_files(&[&self.wav, &self.cue, &self.txt]);
    }

    fn spawn(file: File, wav: PathBuf, cue: PathBuf, txt: PathBuf) -> Result<(Self, RecorderTap), String> {
        let (shared, first) = Shared::new();
        let for_writer = Arc::clone(&shared);
        let writer = std::thread::Builder::new()
            .name("recorder".into())
            .spawn(move || write_loop(file, for_writer))
            .map_err(|e| format!("Could not start the recorder: {e}"))?;
        let tap = RecorderTap::new(Arc::clone(&shared), first);
        Ok((Self { shared, writer: Some(writer), unsaved: None, wav, cue, txt }, tap))
    }

    /// Recorded time so far, from the frames the callback accepted.
    pub fn seconds(&self) -> f64 {
        let rate = self.shared.sample_rate.load(Ordering::Acquire);
        if rate == 0 { 0.0 } else { self.shared.frames.load(Ordering::Acquire) as f64 / rate as f64 }
    }

    /// Still taking audio: neither stopped by the DJ nor ended by a [`StopReason`].
    pub fn is_active(&self) -> bool {
        !self.shared.stop_requested.load(Ordering::Acquire) && self.shared.stop_reason().is_none()
    }

    pub fn is_stopping(&self) -> bool {
        self.shared.stop_requested.load(Ordering::Acquire)
    }

    pub fn is_unsaved(&self) -> bool {
        self.unsaved.is_some()
    }

    /// Ends the recording. Pass the tap taken out of `AudioSharedState` so its last partial
    /// chunk is kept; `None` only when the tap could not be taken.
    pub fn stop(&self, tap: Option<RecorderTap>) {
        if let Some(tap) = tap {
            tap.flush();
        }
        self.shared.stop_requested.store(true, Ordering::Release);
    }

    /// The writer's result once it has drained and finished the file; `Err` leaves the
    /// recording unsaved for [`Self::retry`].
    pub fn poll(&mut self) -> Option<Result<Saved, String>> {
        if !self.writer.as_ref()?.is_finished() {
            return None;
        }
        let outcome = self.writer.take()?.join().unwrap_or_else(|_| {
            Ok(Saved {
                frames: 0,
                sample_rate: 0,
                reason: Some(StopReason::WriteFailed),
                error: Some("the recorder thread stopped unexpectedly".into()),
            })
        });
        Some(self.keep_unsaved(outcome))
    }

    /// Tries again to finish the header of an unsaved recording.
    pub fn retry(&mut self) -> Result<Saved, String> {
        let unsaved = self.unsaved.take().ok_or("No recording to save")?;
        self.keep_unsaved(unsaved.finish())
    }

    fn keep_unsaved(&mut self, outcome: Result<Saved, (Unsaved, String)>) -> Result<Saved, String> {
        outcome.map_err(|(unsaved, e)| {
            self.unsaved = Some(unsaved);
            e
        })
    }
}

impl Unsaved {
    fn finish(mut self) -> Result<Saved, (Unsaved, String)> {
        match finalize(&mut self.file, self.frames, self.sample_rate) {
            Ok(()) => Ok(Saved { frames: self.frames, sample_rate: self.sample_rate, reason: self.reason, error: self.error }),
            Err(e) => {
                let msg = e.to_string();
                Err((self, msg))
            }
        }
    }
}

/// Writer thread: streams full chunks to the file until the DJ stops and the queue is empty.
fn write_loop(mut file: File, shared: Arc<Shared>) -> Result<Saved, (Unsaved, String)> {
    let mut bytes = Vec::with_capacity(CHUNK_FRAMES * BYTES_PER_FRAME as usize);
    let mut frames = 0u64;
    let mut error: Option<String> = None;
    let mut last_header = Instant::now();
    loop {
        // Read the flag before draining: everything handed over before the stop gets written.
        let stop = shared.stop_requested.load(Ordering::Acquire);
        while let Ok(mut chunk) = shared.full.pop() {
            if error.is_none() {
                bytes.clear();
                for s in &chunk {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                match file.write_all(&bytes) {
                    Ok(()) => frames += (chunk.len() / 2) as u64,
                    Err(e) => {
                        error = Some(e.to_string());
                        shared.halt(StopReason::WriteFailed);
                        // Keep what made it to disk playable even if the app goes down now.
                        let _ = finalize(&mut file, frames, rate_or_placeholder(&shared));
                    }
                }
            }
            chunk.clear();
            shared.recycle(chunk);
        }
        if stop {
            break;
        }
        if error.is_none() && last_header.elapsed() >= HEADER_REFRESH {
            last_header = Instant::now();
            let refreshed = file
                .seek(SeekFrom::Start(0))
                .and_then(|_| file.write_all(&wav_header(frames, rate_or_placeholder(&shared))))
                .and_then(|_| file.seek(SeekFrom::End(0)).map(|_| ()));
            if let Err(e) = refreshed {
                error = Some(e.to_string());
                shared.halt(StopReason::WriteFailed);
            }
        }
        std::thread::sleep(WRITER_POLL);
    }
    let unsaved = Unsaved {
        file,
        frames,
        sample_rate: rate_or_placeholder(&shared),
        reason: shared.stop_reason(),
        error,
    };
    unsaved.finish()
}

fn remove_files(paths: &[&PathBuf]) {
    for path in paths {
        let _ = std::fs::remove_file(path);
    }
}

fn rate_or_placeholder(shared: &Shared) -> u32 {
    match shared.sample_rate.load(Ordering::Acquire) {
        0 => PLACEHOLDER_RATE,
        rate => rate,
    }
}

/// Cuts anything after the audio (a failed partial write), writes the final header and syncs.
fn finalize(file: &mut File, frames: u64, sample_rate: u32) -> std::io::Result<()> {
    file.set_len(HEADER_LEN + frames * BYTES_PER_FRAME)?;
    file.seek(SeekFrom::Start(0))?;
    file.write_all(&wav_header(frames, sample_rate))?;
    file.sync_all()
}

/// 80-byte header of a 16-bit stereo PCM WAV. Plain RIFF with a 28-byte JUNK chunk while the file
/// fits in 4 GiB; RF64 with that chunk turned into `ds64` once it does not.
pub fn wav_header(frames: u64, sample_rate: u32) -> [u8; HEADER_LEN as usize] {
    let data_bytes = frames * BYTES_PER_FRAME;
    let riff_bytes = HEADER_LEN - 8 + data_bytes;
    let rf64 = riff_bytes > u32::MAX as u64;
    let mut h = Vec::with_capacity(HEADER_LEN as usize);
    h.extend_from_slice(if rf64 { b"RF64" } else { b"RIFF" });
    h.extend_from_slice(&(if rf64 { u32::MAX } else { riff_bytes as u32 }).to_le_bytes());
    h.extend_from_slice(b"WAVE");
    h.extend_from_slice(if rf64 { b"ds64" } else { b"JUNK" });
    h.extend_from_slice(&28u32.to_le_bytes());
    for size in [riff_bytes, data_bytes, frames] {
        h.extend_from_slice(&(if rf64 { size } else { 0 }).to_le_bytes());
    }
    h.extend_from_slice(&0u32.to_le_bytes()); // ds64 table length
    h.extend_from_slice(b"fmt ");
    h.extend_from_slice(&16u32.to_le_bytes());
    h.extend_from_slice(&1u16.to_le_bytes()); // PCM
    h.extend_from_slice(&2u16.to_le_bytes()); // channels
    h.extend_from_slice(&sample_rate.to_le_bytes());
    h.extend_from_slice(&(sample_rate * BYTES_PER_FRAME as u32).to_le_bytes());
    h.extend_from_slice(&(BYTES_PER_FRAME as u16).to_le_bytes());
    h.extend_from_slice(&16u16.to_le_bytes());
    h.extend_from_slice(b"data");
    h.extend_from_slice(&(if rf64 { u32::MAX } else { data_bytes as u32 }).to_le_bytes());
    h.try_into().expect("header is 80 bytes")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u32_at(b: &[u8], i: usize) -> u32 {
        u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
    }
    fn u64_at(b: &[u8], i: usize) -> u64 {
        u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("retraktor-rec-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn wait(rec: &mut Recording) -> Result<Saved, String> {
        let t0 = Instant::now();
        loop {
            if let Some(outcome) = rec.poll() {
                return outcome;
            }
            assert!(t0.elapsed() < Duration::from_secs(60), "writer never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Distinct, reproducible value for frame `i`, channel `c`.
    fn pattern(i: u64, c: u64) -> f32 {
        (((i * 2 + c) * 7919 % 65_000) as f32 - 32_500.0) / 32_768.0
    }

    #[test]
    fn header_is_riff_below_4_gib_and_rf64_above() {
        let h = wav_header(1000, 96_000);
        assert_eq!(&h[0..4], b"RIFF");
        assert_eq!(u32_at(&h, 4), 72 + 4000);
        assert_eq!(&h[12..16], b"JUNK");
        assert_eq!(&h[48..52], b"fmt ");
        assert_eq!(u32_at(&h, 60), 96_000);
        assert_eq!(u32_at(&h, 64), 96_000 * 4);
        assert_eq!(&h[72..76], b"data");
        assert_eq!(u32_at(&h, 76), 4000);

        // Largest RIFF, then one frame more: 3 h 06 min at 96 kHz.
        let last_riff = (u32::MAX as u64 - 72) / 4;
        assert_eq!(&wav_header(last_riff, 96_000)[0..4], b"RIFF");
        let frames = last_riff + 1;
        let h = wav_header(frames, 96_000);
        assert_eq!(&h[0..4], b"RF64");
        assert_eq!(u32_at(&h, 4), u32::MAX);
        assert_eq!(&h[12..16], b"ds64");
        assert_eq!(u32_at(&h, 16), 28);
        assert_eq!(u64_at(&h, 20), 72 + frames * 4);
        assert_eq!(u64_at(&h, 28), frames * 4);
        assert_eq!(u64_at(&h, 36), frames);
        assert_eq!(u32_at(&h, 76), u32::MAX);
    }

    #[test]
    fn overrun_stops_with_every_accepted_frame_kept() {
        let (shared, first) = Shared::new();
        let mut tap = RecorderTap::new(Arc::clone(&shared), first);
        tap.begin_block(48_000);
        // Nobody drains: the pool fills, then the tap stops instead of dropping or allocating.
        for i in 0..(POOL_CHUNKS * CHUNK_FRAMES + 100) as u64 {
            tap.push(pattern(i, 0), pattern(i, 1));
        }
        tap.end_block();
        assert_eq!(shared.stop_reason(), Some(StopReason::Overrun));
        assert_eq!(shared.frames.load(Ordering::Acquire), (POOL_CHUNKS * CHUNK_FRAMES) as u64);
        assert_eq!(shared.full.len(), POOL_CHUNKS);
        assert!(shared.full.pop().unwrap().len() == CHUNK_FRAMES * 2);
    }

    #[test]
    fn callback_never_takes_a_chunk_the_writer_has_not_published() {
        let (shared, first) = Shared::new();
        let mut tap = RecorderTap::new(Arc::clone(&shared), first);
        tap.begin_block(48_000);
        let mut i = 0u64;
        for _ in 0..CHUNK_FRAMES {
            tap.push(pattern(i, 0), pattern(i, 1));
            i += 1;
        }
        // The writer is mid-recycle: its chunk is in the free queue but not yet counted, as when it
        // is preempted inside a push. The tap must stop at the pool's end instead of waiting on it.
        let mut recycled = shared.full.pop().unwrap();
        recycled.clear();
        shared.free.push(recycled).unwrap();
        while i < (POOL_CHUNKS * CHUNK_FRAMES + 100) as u64 {
            tap.push(pattern(i, 0), pattern(i, 1));
            i += 1;
        }
        tap.end_block();
        assert_eq!(shared.stop_reason(), Some(StopReason::Overrun));
        assert_eq!(shared.frames.load(Ordering::Acquire), (POOL_CHUNKS * CHUNK_FRAMES) as u64);
        assert_eq!(shared.free.len(), 1, "the unpublished chunk was left alone");
        assert_eq!(shared.full.len(), POOL_CHUNKS - 1);
        // Once counted, the same chunk is fair game.
        shared.free_published.fetch_add(1, Ordering::Release);
        assert!(tap.free_ready());
    }

    #[test]
    fn rate_change_stops_and_keeps_the_partial_chunk() {
        let (shared, first) = Shared::new();
        let mut tap = RecorderTap::new(Arc::clone(&shared), first);
        tap.begin_block(48_000);
        for i in 0..300 {
            tap.push(pattern(i, 0), pattern(i, 1));
        }
        tap.end_block();
        tap.begin_block(96_000);
        tap.push(0.5, 0.5);
        tap.end_block();
        assert_eq!(shared.stop_reason(), Some(StopReason::RateChanged));
        assert_eq!(shared.frames.load(Ordering::Acquire), 300);
        assert_eq!(shared.full.pop().unwrap().len(), 600);
        assert_eq!(shared.sample_rate.load(Ordering::Acquire), 48_000);
    }

    #[test]
    fn stop_writes_every_frame_and_reserves_fresh_names() {
        let dir = temp_dir("stop");
        let now = SystemTime::now();
        for rate in [44_100u32, 96_000] {
            let (mut rec, mut tap) = Recording::start(&dir, now).unwrap();
            // Blocks of 333 frames never line up with chunks, so the last chunk is partial.
            let total = 3 * CHUNK_FRAMES as u64 + 1234;
            let mut i = 0;
            while i < total {
                tap.begin_block(rate);
                for _ in 0..333.min(total - i) {
                    tap.push(pattern(i, 0), pattern(i, 1));
                    i += 1;
                }
                tap.end_block();
            }
            assert!(rec.is_active());
            assert!((rec.seconds() - total as f64 / rate as f64).abs() < 1e-9);
            rec.stop(Some(tap));
            let saved = wait(&mut rec).unwrap();
            assert_eq!((saved.frames, saved.sample_rate, saved.reason), (total, rate, None));

            let bytes = std::fs::read(&rec.wav).unwrap();
            assert_eq!(bytes[..80], wav_header(total, rate));
            assert_eq!(bytes.len() as u64, 80 + total * 4);
            for (j, frame) in bytes[80..].chunks(4).enumerate() {
                for c in 0..2 {
                    let s = i16::from_le_bytes([frame[c * 2], frame[c * 2 + 1]]);
                    assert_eq!(s, (pattern(j as u64, c as u64) * 32767.0) as i16, "frame {j} ch {c}");
                }
            }
            assert!(rec.cue.exists() && rec.txt.exists());
        }
        // Same minute twice: the second recording got its own names.
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 6);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn write_failure_stops_visibly_and_stays_retryable() {
        let full = std::fs::OpenOptions::new().write(true).open("/dev/full").unwrap();
        let p = PathBuf::from("/dev/full");
        let (mut rec, mut tap) = Recording::spawn(full, p.clone(), p.clone(), p).unwrap();
        tap.begin_block(48_000);
        for i in 0..CHUNK_FRAMES as u64 {
            tap.push(pattern(i, 0), pattern(i, 1));
        }
        tap.end_block();
        let t0 = Instant::now();
        while rec.is_active() {
            assert!(t0.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
        }
        tap.begin_block(48_000);
        tap.push(0.1, 0.1);
        tap.end_block();
        assert_eq!(rec.shared.frames.load(Ordering::Acquire), CHUNK_FRAMES as u64, "callback stopped taking audio");
        rec.stop(Some(tap));
        assert!(wait(&mut rec).is_err());
        assert!(rec.is_unsaved());
        assert!(rec.retry().is_err());
        assert!(rec.is_unsaved(), "still kept for another retry");
    }

    /// Records past the old in-memory limit (30 min at 48 kHz, 15 min at 96 kHz) as fast as the
    /// disk allows and checks the tail: a 61-minute set at 48 kHz (~703 MB), then 16 minutes at
    /// 96 kHz (~369 MB), one after the other, each removed after.
    /// `cargo test --release -p retraktor-gui long_set -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn long_set_past_the_old_capacity() {
        let dir = temp_dir("long");
        for (rate, minutes) in [(48_000u32, 61u64), (96_000, 16)] {
            let (mut rec, mut tap) = Recording::start(&dir, SystemTime::now()).unwrap();
            let total = rate as u64 * 60 * minutes + 777;
            let t0 = Instant::now();
            let mut i = 0;
            while i < total {
                // Pace like a real callback would be paced by the sound card: never outrun the pool.
                while !tap.free_ready() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                tap.begin_block(rate);
                for _ in 0..512.min(total - i) {
                    tap.push(pattern(i, 0), pattern(i, 1));
                    i += 1;
                }
                tap.end_block();
            }
            rec.stop(Some(tap));
            let saved = wait(&mut rec).unwrap();
            assert_eq!((saved.frames, saved.reason), (total, None));
            let mut f = File::open(&rec.wav).unwrap();
            let len = f.metadata().unwrap().len();
            assert_eq!(len, 80 + total * 4);
            let mut head = [0u8; 80];
            std::io::Read::read_exact(&mut f, &mut head).unwrap();
            assert_eq!(head, wav_header(total, rate));
            let tail_frames = 10_000u64;
            f.seek(SeekFrom::Start(80 + (total - tail_frames) * 4)).unwrap();
            let mut tail = vec![0u8; tail_frames as usize * 4];
            std::io::Read::read_exact(&mut f, &mut tail).unwrap();
            for (k, frame) in tail.chunks(4).enumerate() {
                let j = total - tail_frames + k as u64;
                for c in 0..2 {
                    let s = i16::from_le_bytes([frame[c * 2], frame[c * 2 + 1]]);
                    assert_eq!(s, (pattern(j, c as u64) * 32767.0) as i16, "frame {j} ch {c}");
                }
            }
            println!("{rate} Hz, {minutes} min: {len} bytes in {:?}, tail matches", t0.elapsed());
            std::fs::remove_file(&rec.wav).unwrap();
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
}
