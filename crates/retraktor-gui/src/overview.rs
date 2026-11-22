//! Whole-track overviews, the strip under a deck and the small waveform in a browser row. Each track has one
//! canonical stripe in Traktor's `PRTS` layout (version 2, 1024 x 64 slots spread evenly over the whole track),
//! made from a finished decode's three-band peaks, whatever overview look is chosen. Stripes are kept in a
//! ReTraktor-owned cache, so a row seen before needs no decode. What is drawn is a small reduction of the stripe.

use retraktor_formats::{StripeColumn, WaveformStripe};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Slots in a stripe (Traktor's 1024 x 64), spread evenly over the whole track whatever its length.
pub const STRIPE_SLOTS: usize = 1024 * 64;
/// A stripe file: a 25-byte header, then 4 bytes a slot.
const STRIPE_BYTES: usize = 25 + STRIPE_SLOTS * 4;
/// Columns kept in memory per track and drawn from: wider than a deck's overview usually is.
pub const OVERVIEW_COLUMNS: usize = 2048;
/// The header's last field counts the track's 1024-frame blocks at 44.1 kHz, as in Traktor's own stripes.
const BLOCK_SEC: f64 = 1024.0 / 44_100.0;

/// A track's overview as drawn: [`OVERVIEW_COLUMNS`] columns over the whole track (none for an empty track). Each
/// column's bands are the stripe slot at its middle, one real peak as Native has always sampled where a mark falls,
/// and its `total` the loudest slot under it. An all-zero column is not decoded yet and is not drawn.
pub type Overview = Arc<[StripeColumn]>;

/// Stripe slots under one overview column.
const SLOTS_PER_COLUMN: usize = STRIPE_SLOTS / OVERVIEW_COLUMNS;

/// A file as it was when read (modified time, byte size).
pub type FileIdentity = (std::time::SystemTime, u64);

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// The peaks of `n` (spread evenly over the track) under stripe slots `slots`: never none, so a short track's
/// peaks are stretched over many slots.
fn peak_range(n: usize, slots: std::ops::Range<usize>) -> std::ops::Range<usize> {
    let from = (slots.start * n / STRIPE_SLOTS).min(n - 1);
    let last_from = ((slots.end - 1) * n / STRIPE_SLOTS).min(n - 1);
    from..(slots.end * n / STRIPE_SLOTS).clamp(last_from + 1, n)
}

/// The loudest of each band among `peaks`, as bytes.
fn loudest(peaks: &[[f32; 3]]) -> [u8; 3] {
    let mut band = [0f32; 3];
    for p in peaks {
        for b in 0..3 {
            band[b] = band[b].max(p[b]);
        }
    }
    band.map(byte)
}

/// Slot `i` of the stripe of `peaks` (three-band peaks spread evenly over the track): the loudest of each band
/// among the peaks it covers, so a short hit or break survives however long the track. `total` is the loudest band.
fn slot(peaks: &[[f32; 3]], i: usize) -> StripeColumn {
    if peaks.is_empty() {
        return StripeColumn::default();
    }
    let [low, mid, high] = loudest(&peaks[peak_range(peaks.len(), i..i + 1)]);
    StripeColumn { low, mid, high, total: low.max(mid).max(high) }
}

/// An overview column: `middle`'s bands with the loudest `total` under it, blank where `middle` isn't decoded.
fn column(middle: StripeColumn, total: u8) -> StripeColumn {
    if middle == StripeColumn::default() {
        return middle;
    }
    StripeColumn { total, ..middle }
}

/// The canonical stripe of a finished decode: `peaks` over the whole track, `duration_sec` long.
pub fn stripe(peaks: &[[f32; 3]], duration_sec: f64) -> WaveformStripe {
    WaveformStripe {
        header_tag: 0x06,
        version: 2,
        scale_factor: 3.5,
        block_size: 1024,
        block_count: 64,
        populated_columns: (duration_sec / BLOCK_SEC).ceil().clamp(1.0, u32::MAX as f64) as u32,
        duration_sec: duration_sec as f32,
        columns: (0..STRIPE_SLOTS).map(|i| slot(peaks, i)).collect(),
        transients_sec: Vec::new(),
    }
}

/// The overview drawn from a stripe's slots (see [`Overview`]).
fn reduce(slots: &[StripeColumn]) -> Overview {
    slots.chunks_exact(SLOTS_PER_COLUMN).map(|run| column(run[SLOTS_PER_COLUMN / 2], run.iter().map(|s| s.total).max().unwrap_or(0))).collect()
}

/// The overview of `peaks` (zero where not decoded yet), exactly as drawn from its stripe once cached, made straight
/// in the overview's columns (no stripe), so a partly decoded row is cheap to redraw.
pub fn from_peaks(peaks: &[[f32; 3]]) -> Overview {
    if peaks.is_empty() {
        return Vec::new().into();
    }
    (0..OVERVIEW_COLUMNS)
        .map(|c| {
            let first = c * SLOTS_PER_COLUMN;
            let total = loudest(&peaks[peak_range(peaks.len(), first..first + SLOTS_PER_COLUMN)]).into_iter().max().unwrap_or(0);
            column(slot(peaks, first + SLOTS_PER_COLUMN / 2), total)
        })
        .collect()
}

/// The stripe in `bytes` and the track length its header states, if it is a whole version-2 stripe. Its
/// `duration_sec` is not used: that is a guess at another layout's timing.
fn parse(bytes: &[u8]) -> Option<(WaveformStripe, f64)> {
    if bytes.len() != STRIPE_BYTES {
        return None;
    }
    let s = WaveformStripe::from_bytes(bytes).ok()?;
    let whole = s.header_tag == 0x06
        && s.version == 2
        && s.scale_factor == 3.5
        && (s.block_size, s.block_count) == (1024, 64)
        && s.populated_columns > 0
        && s.columns.len() == STRIPE_SLOTS
        && s.columns.iter().any(|c| *c != StripeColumn::default());
    let duration_sec = s.populated_columns as f64 * BLOCK_SEC;
    whole.then_some((s, duration_sec))
}

/// The whole stripe kept in `file`, read no further than a stripe's size (anything longer is not one).
fn read(file: &Path) -> Option<(WaveformStripe, f64)> {
    use std::io::Read;
    let mut bytes = Vec::with_capacity(STRIPE_BYTES + 1);
    std::fs::File::open(file).ok()?.take(STRIPE_BYTES as u64 + 1).read_to_end(&mut bytes).ok()?;
    parse(&bytes)
}

/// Where finished tracks' stripes are kept: one file per version of an audio file (its path, modified time and
/// size), so a replaced or repaired file is never shown with its old waveform. `None` keeps nothing.
/// shortcut: entries for files since changed or removed are never deleted, add a size cap if the folder grows too big.
#[derive(Clone, Debug, Default)]
pub struct StripeCache(Option<PathBuf>);

impl StripeCache {
    /// `~/.cache/retraktor/stripes`. Tests keep nothing unless they choose a folder, so they never touch the user's.
    pub fn user() -> Self {
        if cfg!(test) {
            return Self(None);
        }
        Self(std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache/retraktor/stripes")))
    }

    pub fn at(dir: PathBuf) -> Self {
        Self(Some(dir))
    }

    /// The cache file for `path` as `identity` describes it.
    fn file(&self, path: &Path, (modified, size): FileIdentity) -> Option<PathBuf> {
        let modified = modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
        // FNV-1a: the same in every build (std's hasher may change), so an update still finds the cache.
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let (modified, size) = (modified.to_le_bytes(), size.to_le_bytes());
        for b in path.as_os_str().as_encoded_bytes().iter().chain(&modified).chain(&size) {
            hash = (hash ^ *b as u64).wrapping_mul(0x0100_0000_01b3);
        }
        Some(self.0.as_ref()?.join(format!("{hash:016x}")))
    }

    /// The overview and length kept for the file at `path` as it was (`identity`), if a whole stripe of it is
    /// kept. A missing, partial or damaged entry is simply not there.
    pub fn load(&self, path: &Path, identity: Option<FileIdentity>) -> Option<(Overview, f64)> {
        let (stripe, duration_sec) = read(&self.file(path, identity?)?)?;
        Some((reduce(&stripe.columns), duration_sec))
    }

    /// Keeps the stripe of a finished decode of `path` (its `peaks`, `duration_sec` long, of the file as
    /// `identity` said it was before decoding), unless a whole one is kept already or the file has changed since,
    /// and returns the overview now kept. The first whole stripe kept stays, so a row and a deck that decode the
    /// same track at once both end up drawing it. It is written under a name no other writer uses and then put in
    /// place, so nobody ever reads half a stripe. Failing (a read-only folder, a full disk) returns `None` and only
    /// means the track is decoded again next time.
    pub fn store(&self, path: &Path, identity: Option<FileIdentity>, peaks: &[[f32; 3]], duration_sec: f64) -> Option<Overview> {
        let identity = identity?;
        let file = self.file(path, identity)?;
        if peaks.is_empty() || !(duration_sec > 0.0) || crate::audio::file_identity(path) != Some(identity) {
            return None;
        }
        if let Some((kept, _)) = read(&file) {
            return Some(reduce(&kept.columns));
        }
        static WRITES: AtomicU64 = AtomicU64::new(0);
        let unique = format!("tmp-{}-{}", std::process::id(), WRITES.fetch_add(1, Ordering::Relaxed));
        let temp = file.with_extension(unique);
        let built = stripe(peaks, duration_sec);
        let mut kept = None;
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&temp, built.to_bytes()))
            // A link never replaces a stripe another writer just kept; a damaged entry (or a folder without links) is renamed over.
            .and_then(|_| {
                std::fs::hard_link(&temp, &file).or_else(|_| {
                    if let Some((existing, _)) = read(&file) {
                        kept = Some(existing);
                        Ok(())
                    } else {
                        std::fs::rename(&temp, &file)
                    }
                })
            });
        let _ = std::fs::remove_file(&temp);
        written.ok()?;
        Some(reduce(&kept.as_ref().unwrap_or(&built).columns))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("retraktor-overview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Steady music with one short hit `at` (a peak index), at 60 peaks a second.
    fn music_with_hit(peaks: usize, at: usize) -> Vec<[f32; 3]> {
        (0..peaks).map(|j| if j == at { [1.0, 1.0, 1.0] } else { [0.3, 0.25, 0.2] }).collect()
    }

    /// Every stripe has the exact shape of Traktor's version-2 stripes (262,169 bytes; tag 6, `PRTS`, version 2,
    /// 3.5, 1024 x 64, then the track's length in 44.1 kHz 1024-frame blocks), and its 65,536 slots follow the whole
    /// track, not just as many slots as that last field: a hit three peaks from the end of a 2 s, 3:51, 14:59 or
    /// (beyond the decode cap) 27:47 track lands in the last slots and the overview's last columns, at full height.
    #[test]
    fn stripes_have_traktors_shape_and_span_the_whole_track() {
        for seconds in [2.0, 230.7, 899.0, 1667.0] {
            let n = (seconds * 60.0) as usize;
            let hit = n - 3;
            let s = stripe(&music_with_hit(n, hit), seconds);
            let bytes = s.to_bytes();
            assert_eq!(bytes.len(), 262_169);
            let blocks = (seconds * 44_100.0 / 1024.0).ceil() as u32;
            let mut header = vec![0x06];
            header.extend_from_slice(b"PRTS");
            for field in [2u32.to_le_bytes(), 3.5f32.to_le_bytes(), 1024u32.to_le_bytes(), 64u32.to_le_bytes(), blocks.to_le_bytes()] {
                header.extend_from_slice(&field);
            }
            assert_eq!(&bytes[..25], &header[..], "{seconds} s header");
            let hit_slots: Vec<usize> = (0..STRIPE_SLOTS).filter(|&i| s.columns[i].total == 255).collect();
            let expected = hit * STRIPE_SLOTS / n;
            assert!(!hit_slots.is_empty() && hit_slots.iter().all(|&i| i.abs_diff(expected) <= STRIPE_SLOTS / n + 1), "{seconds} s: hit at {hit_slots:?}, not ~{expected}");
            assert!(s.columns.iter().all(|c| c.total > 0), "{seconds} s: a slot isn't filled");
            let (parsed, duration) = parse(&bytes).unwrap();
            assert!((duration - seconds).abs() <= BLOCK_SEC, "{seconds} s read back as {duration}");
            let overview = reduce(&parsed.columns);
            assert_eq!(overview.len(), OVERVIEW_COLUMNS);
            let per = SLOTS_PER_COLUMN;
            let hit_columns: Vec<usize> = (0..OVERVIEW_COLUMNS).filter(|&c| overview[c].total == 255).collect();
            assert_eq!(hit_columns, (hit_slots[0] / per..=hit_slots[hit_slots.len() - 1] / per).collect::<Vec<_>>(), "{seconds} s");
            assert!(hit_columns[0] >= OVERVIEW_COLUMNS * 97 / 100, "{seconds} s: the late hit is drawn at column {}", hit_columns[0]);
        }
    }

    /// A 3:51 house track at 125 BPM: a kick with a fast low decay every beat, an off-beat hat, a breakdown
    /// without kicks, then a tail not decoded yet. Native's feel: each drawn column is the track's own peak where
    /// it falls (a kick at its full height, not averaged down), so the beats keep their shape (kick columns tall,
    /// most others low) rather than one flat wall. Its total is the loudest peak under it. The breakdown stays
    /// quiet but visible, and the tail stays blank.
    #[test]
    fn overview_keeps_beats_breakdowns_and_blank_tail() {
        let beat = 60.0 * 60.0 / 125.0; // peaks per beat at 60 per second
        let (breakdown, undecoded) = (6000..8000, 12_000..13_860);
        let peaks: Vec<[f32; 3]> = (0..13_860)
            .map(|j| {
                let phase = (j as f32).rem_euclid(beat);
                if undecoded.contains(&j) {
                    [0.0; 3]
                } else if breakdown.contains(&j) {
                    [0.05, 0.4, 0.1]
                } else {
                    let low = 0.95 * (-phase / 4.0).exp() + 0.03;
                    [low, 0.3 + 0.1 * low, if (phase - beat / 2.0).abs() < 1.5 { 0.6 } else { 0.15 }]
                }
            })
            .collect();
        let overview = from_peaks(&peaks);
        assert_eq!(overview.len(), OVERVIEW_COLUMNS);
        let mut beat_lows = Vec::new();
        for (c, col) in overview.iter().enumerate() {
            let under = peak_range(peaks.len(), c * SLOTS_PER_COLUMN..(c + 1) * SLOTS_PER_COLUMN);
            let at = peak_range(peaks.len(), c * SLOTS_PER_COLUMN + SLOTS_PER_COLUMN / 2..c * SLOTS_PER_COLUMN + SLOTS_PER_COLUMN / 2 + 1).start;
            if undecoded.contains(&at) {
                assert_eq!(*col, StripeColumn::default(), "undecoded column {c} isn't blank");
                continue;
            }
            assert_eq!([col.low, col.mid, col.high], peaks[at].map(byte), "column {c} isn't the track's peak {at}");
            assert_eq!(col.total, loudest(&peaks[under.clone()]).into_iter().max().unwrap(), "column {c}'s total isn't its loudest");
            if breakdown.contains(&under.start) && breakdown.contains(&(under.end - 1)) {
                assert!(col.low < 26 && col.mid > 90, "the breakdown at column {c} is {col:?}");
            } else if !breakdown.contains(&under.start) && !breakdown.contains(&(under.end - 1)) {
                beat_lows.push(col.low);
            }
        }
        let between_kicks = beat_lows.iter().filter(|&&low| low < 77).count();
        assert!(beat_lows.iter().any(|&low| low > 235), "the kicks were averaged down: tallest low {:?}", beat_lows.iter().max());
        assert!(between_kicks > beat_lows.len() / 2, "beats lost their shape: {between_kicks} of {} lows between kicks", beat_lows.len());
        assert!(from_peaks(&[]).is_empty());
    }

    /// Cold, the cache has nothing; a finished decode is kept and found again (warm), for that version of the file
    /// only; a second decode of it keeps (and gets back) the first one's. A damaged, cut short, zeroed or oversized
    /// entry is a miss and the next finished decode replaces it. Without a
    /// folder, or with one that can't be made, nothing is kept and nothing fails.
    #[test]
    fn cache_keeps_whole_stripes_of_unchanged_files_only() {
        let dir = temp_dir("cache");
        let audio = dir.join("track.flac");
        std::fs::write(&audio, b"not really audio").unwrap();
        let identity = crate::audio::file_identity(&audio);
        let cache = StripeCache::at(dir.join("stripes"));
        let peaks = music_with_hit(13_842, 13_000);
        assert!(cache.load(&audio, identity).is_none(), "cold cache had it");
        cache.store(&audio, identity, &peaks, 230.7);
        let (overview, duration) = cache.load(&audio, identity).expect("warm cache missed");
        assert_eq!(overview, from_peaks(&peaks), "the kept overview isn't the decode's");
        assert!((duration - 230.7).abs() <= BLOCK_SEC);
        let entries = || std::fs::read_dir(dir.join("stripes")).unwrap().count();
        assert_eq!(entries(), 1, "a temporary file was left behind");
        let other = music_with_hit(13_842, 100);
        assert_eq!(cache.store(&audio, identity, &other, 230.7), Some(overview.clone()), "a second writer replaced the first's stripe");

        // Changed since: another size or time is another entry, and a decode of an older version isn't kept.
        let older = identity.map(|(t, size)| (t, size + 1));
        assert!(cache.load(&audio, older).is_none());
        cache.store(&audio, older, &peaks, 230.7);
        assert_eq!(entries(), 1, "a decode of a file since changed was kept");

        let file = cache.file(&audio, identity.unwrap()).unwrap();
        let whole = std::fs::read(&file).unwrap();
        let mut damaged = whole.clone();
        damaged[5] = 3; // version 3
        let oversized = [whole.clone(), vec![7; 1 << 20]].concat();
        for bad in [damaged, whole[..1000].to_vec(), { let mut z = whole.clone(); z[25..].fill(0); z }, oversized] {
            std::fs::write(&file, bad).unwrap();
            assert!(cache.load(&audio, identity).is_none(), "a bad entry was used");
            assert_eq!(cache.store(&audio, identity, &peaks, 230.7), Some(from_peaks(&peaks)));
            assert_eq!(std::fs::read(&file).unwrap(), whole, "a bad entry wasn't replaced");
        }

        let off = StripeCache::default();
        assert!(off.store(&audio, identity, &peaks, 230.7).is_none());
        assert!(off.load(&audio, identity).is_none());
        let unwritable = StripeCache::at(audio.join("stripes")); // under a file: can't be made
        assert!(unwritable.store(&audio, identity, &peaks, 230.7).is_none());
        assert!(unwritable.load(&audio, identity).is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    /// RACER (Extended Mix) as Traktor 4.4.1 wrote it, where present: the header's last field (9936) is its 3:50.7
    /// in 1024-frame blocks, and its slots carry the song across all 65,536, not up to slot 9936 then zeros.
    #[test]
    fn reads_traktors_own_stripe_where_present() {
        let Some(home) = std::env::var_os("HOME") else { return };
        let path = PathBuf::from(home).join("Documents/Native Instruments/Traktor 4.4.1/Stripes/096/AP53IVDDHIOXNC32OTQMBBIVZSXD");
        let Ok(bytes) = std::fs::read(path) else { return };
        let (stripe, duration) = parse(&bytes).expect("Traktor's stripe wasn't read");
        assert_eq!(stripe.populated_columns, 9936);
        assert!((duration - 230.7).abs() < 0.1, "{duration}");
        let drawn = stripe.columns.iter().rposition(|c| c.total > 0).unwrap();
        assert!(drawn > 65_000, "the last drawn slot is {drawn}");
        assert!(reduce(&stripe.columns).last().is_some_and(|c| c.total > 0));
    }
}
