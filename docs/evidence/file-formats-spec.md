# ReTraktor Evidence Ledger: File Formats Specification (`.nml`, `.tsi`, `.trak`)

## 1. Provenance & Binary Evidence

Derived from static inspection of `Traktor Pro 4.app/Contents/MacOS/Traktor Pro 4` (`ev_fb958e256da8dbb4f77cb48853b8d825c706ee0e6c6c15656ae0de75af169bab`), `Contents/Info.plist` (`CFBundleDocumentTypes` registering `.nml`, `.tsi`, `.trak`, `.mp3`, `.wav`, `.aif`, `.flac`, `.ogg`, `.m4a`, `.mp4`, `.aac`), and embedded `.qtmimedatabase` (`ev_a9ec5856a95be76b9a45a68cf735caff40617150b9173991fb1fff36f2b8679a`).

---

## 2. Traktor Music Library (`.nml`, Versions 19 & 20)

`.nml` files (`collection.nml` and exported playlist `.nml` files) are UTF-8 XML documents rooted at `<NML VERSION="19">` (Traktor Pro 3) or `<NML VERSION="20">` (Traktor Pro 4, which adds Ozone Maximizer, Flexible Beatgrids `GRID` markers, and Pattern Player settings).

### 2.1 Document Structure
```xml
<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NML VERSION="20">
  <HEAD COMPANY="www.native-instruments.com" PROGRAM="Traktor"/>
  <MUSICFOLDERS/>
  <COLLECTION ENTRIES="1">
    <ENTRY MODIFIED_DATE="2026/10/24" MODIFIED_TIME="43200" AUDIO_ID="base64..." TITLE="Track Title" ARTIST="Artist Name">
      <LOCATION DIR="/:Users/:dj/:Music/:" FILE="track.flac" VOLUME="Macintosh HD" VOLUMEID="Macintosh HD"/>
      <ALBUM OF_TRACKS="10" TITLE="Album Name" TRACK="1"/>
      <MODIFICATION_INFO AUTHOR_TYPE="user"/>
      <INFO BITRATE="1411200" COVERARTID="001" FILESIZE="42100" GENRE="Techno" IMPORT_DATE="2026/10/24" KEY="1m" PLAYCOUNT="5" PLAYTIME="360" PLAYTIME_FLOAT="360.125" RANKING="255" RELEASE_DATE="2026/01/01"/>
      <TEMPO BPM="128.000000" BPM_QUALITY="100.000000"/>
      <LOUDNESS ANALYZED_DB="-2.100000" PERCEIVED_DB="-0.850000" PEAK_DB="0.120000"/>
      <MUSICAL_KEY VALUE="0"/>
      <CUE_V2 NAME="AutoGrid" DISPL_ORDER="0" TYPE="4" START="52.341" LEN="0.000" REPEATS="-1" HOTCUE="0">
        <GRID BPM="128.000000"/>
      </CUE_V2>
      <CUE_V2 NAME="Drop" DISPL_ORDER="0" TYPE="0" START="60052.341" LEN="0.000" REPEATS="-1" HOTCUE="1"/>
      <CUE_V2 NAME="Main Loop" DISPL_ORDER="0" TYPE="5" START="120052.341" LEN="7500.000" REPEATS="-1" HOTCUE="2"/>
      <STEMS STEMS="/:Users/:dj/:Music/:track.stem.m4a"/>
    </ENTRY>
  </COLLECTION>
  <SETS ENTRIES="0"/>
  <PLAYLISTS>
    <NODE TYPE="FOLDER" NAME="$ROOT">
      <SUBNODES COUNT="2">
        <NODE TYPE="PLAYLIST" NAME="Peak Time">
          <PLAYLIST ENTRIES="1" TYPE="LIST" UUID="11111111222233334444555566667777">
            <ENTRY>
              <PRIMARYKEY TYPE="TRACK" KEY="Macintosh HD/:Users/:dj/:Music/:track.flac"/>
            </ENTRY>
          </PLAYLIST>
        </NODE>
        <NODE TYPE="SMARTLIST" NAME="128 BPM Techno">
          <SMARTLIST UUID="888888889999aaaabbbbccccddddeeee">
            <SEARCH_EXPRESSION VERSION="1" OPERATOR="AND" QUERY="BPM &gt;= 126 AND GENRE CONTAINS Techno"/>
          </SMARTLIST>
        </NODE>
      </SUBNODES>
    </NODE>
  </PLAYLISTS>
  <INDEXING/>
</NML>
```

### 2.2 Path Normalization (`LOCATION` & `PRIMARYKEY`)
- Traktor encodes directory separators in `LOCATION/@DIR` and `PRIMARYKEY/@KEY` using `/:` for each path segment (`/:Users/:dj/:Music/:`).
- `PRIMARYKEY/@KEY` concatenates `VOLUME` + `DIR` + `FILE` (e.g., `Macintosh HD/:Users/:dj/:Music/:track.flac`).
- On Linux, ReTraktor maps POSIX absolute paths (`/home/dj/Music/track.flac`) seamlessly to and from Traktor's `/:` delimiter convention (`VOLUME="linux-root"`, `DIR="/:home/:dj/:Music/:"`, `FILE="track.flac"`), ensuring 100% round-trip compatibility between macOS/Windows Traktor collections and Linux filesystems.

### 2.3 Cue Marker Encoding (`CUE_V2`)
- `TYPE`:
  - `0` = Cue (`CueType::Cue`)
  - `1` = Fade-In (`CueType::FadeIn`)
  - `2` = Fade-Out (`CueType::FadeOut`)
  - `3` = Load (`CueType::Load`)
  - `4` = Grid / Beatgrid Marker (`CueType::Grid`) — in Traktor Pro 4 (`VERSION="20"`), multiple `TYPE="4"` markers with child `<GRID BPM="..."/>` elements define **Flexible Beatgrids** across tempo changes.
  - `5` = Loop (`CueType::Loop`)
- `START` and `LEN`: Expressed in **milliseconds** (`f64`).
- `HOTCUE`: `-1` for unmapped memory cues/grid markers, `0..7` for Hotcue pads `1..8`.

### 2.4 Musical Key Encoding (`MUSICAL_KEY/@VALUE` vs `INFO/@KEY`)
Verified across 16,160 tracks in `Traktor 4.5.1/collection.nml`, `MUSICAL_KEY VALUE="0..23"` maps chromatically to Open Key notation:
- `0..11` (Major `C`, `Db`, `D`, `Eb`, `E`, `F`, `Gb`, `G`, `Ab`, `A`, `Bb`, `B`): `["1d", "8d", "3d", "10d", "5d", "12d", "7d", "2d", "9d", "4d", "11d", "6d"]`
- `12..23` (Minor `Cm`, `C#m`, `Dm`, `Ebm`, `Em`, `Fm`, `F#m`, `Gm`, `G#m`, `Am`, `Bbm`, `Bm`): `["10m", "5m", "12m", "7m", "2m", "9m", "4m", "11m", "6m", "1m", "8m", "3m"]`

---

## 3. Traktor Settings & Controller Mapping Format (`.tsi`)

A `.tsi` file is an XML wrapper (`<NIXML><TraktorSettings>`) where controller mappings are stored inside `<Entry Name="DeviceIO.Config.Controller" Type="3" Value="BASE64..."></Entry>`.

### 3.1 Outer XML Envelope
```xml
<?xml version="1.0" encoding="UTF-8" standalone="no" ?>
<NIXML><TraktorSettings><Entry Name="DeviceIO.Config.Controller" Type="3" Value="RElPTS..."></Entry>
</TraktorSettings>
</NIXML>
```

### 3.2 Inner Binary FourCC TLV Framing (`Big-Endian`)
When the Base64 payload of `DeviceIO.Config.Controller` is decoded, it forms a nested tree of **Big-Endian FourCC TLV (Tag-Length-Value)** frames:

```
+-------------------+-------------------+-----------------------------------+
| FourCC (4 bytes)  | Length (u32 BE)   | Payload (`Length` bytes)          |
| ASCII e.g. "DIOM" | Big-Endian uint32 | Child frames or structured fields |
+-------------------+-------------------+-----------------------------------+
```

### 3.3 Key FourCC Chunk Hierarchy (Verified on `DDJ-FLX4.tsi`, 2,163,404-byte `DIOM`)
1. **`DIOM`** (*Device IO Mapping*): Pure container chunk (`0`-byte prefix) with 2 children:
   - **`DIOI`** (`4` bytes): `version: u32 BE = 1`.
   - **`DEVS`** (*Devices List*): Contains `count: u32 BE` followed by `count` `DEVI` chunks.
2. **`DEVI`** (*Device Entry*):
   - Device Target Name (`[char_len: u32 BE][utf16be_code_units]`, e.g., `"Traktor.Kontrol S4 MK3"` or `"Generic MIDI"`).
   - Child **`DDAT`** (*Device Data*, `0`-byte prefix) containing:
     - **`DDIF`** (`4` bytes: device flags `u32 BE`).
     - **`DDIV`** (UTF-16BE controller version string, e.g., `"4.5.1.0"`, + `u32 BE`).
     - **`DDIC`** (UTF-16BE device comment string, e.g., `"DDJ-FLX4 - Pads A (Play)"`).
     - **`DDPT`** (2x UTF-16BE strings: `in_port` and `out_port`).
     - **`DDDC`** (*Device Control Definitions*, `0`-byte prefix):
       - **`DDCI`** (Input controls: `count: u32 BE` + `DCDT` chunks) & **`DDCO`** (Output LEDs: `count: u32 BE` + `DCDT` chunks).
     - **`DDCB`** (*Device Control Bindings*, `0`-byte prefix):
       - **`CMAS`** (*Control Mapping Assignments*): Contains `count: u32 BE` followed by `CMAI` chunks.
       - **`CMAI`** (*Control Mapping Assignment Item*):
         - `midi_note_binding_id: u32 BE`
         - `interaction_type: u32 BE` (`0 = Input / Control`, `1 = Output / LED`)
         - `traktor_control_id: u32 BE` (Internal numeric ID for the Traktor command)
         - Child **`CMAD`** (*Control Mapping Assignment Data*, `156` bytes):
           - `[0..4]` Version (`4u32 BE`), `[4..8]` Device Type (`u32 BE`), `[8..12]` Interaction Mode (`u32 BE`), `[12..16]` Target Assignment (`0=DeviceTarget, 1=DeckA..4=DeckD`), Modifiers 1 & 2, LED/MIDI ranges, rotary sensitivity/acceleration.
       - **`DCBM`** (*Device Control Binding List*): `count: u32 BE` followed by child `DCBM` (`id: u32 BE` + UTF-16BE MIDI control name, e.g., `"Ch01.Note.C0"`).
     - **`DVST`** (`20` bytes: device state footer).

---

## 4. Binary Waveform `Stripes/` & `Transients/` Cache Files

Verified against `Traktor 4.5.1/Stripes/` and `Transients/`:

### 4.1 `Stripes/<subdir>/<28-char-base32-key>` (`262,169` bytes fixed size)
- **Header (`25` bytes, Little-Endian)**:
  - `offset 0x00` (`u8`): `0x06` (`header_tag`)
  - `offset 0x01..0x05` (`[u8; 4]`): `b"PRTS"` (`"STRP"` in Little-Endian)
  - `offset 0x05..0x09` (`u32 LE`): `2` (`version`)
  - `offset 0x09..0x0D` (`f32 LE`): `3.5` (`scale_factor`)
  - `offset 0x0D..0x11` (`u32 LE`): `1024` (`block_size`)
  - `offset 0x11..0x15` (`u32 LE`): `64` (`block_count`, yielding `1024 * 64 = 65,536` column slots)
  - `offset 0x15..0x19` (`u32 LE`): `populated_columns` (active columns written for this track)
- **Column Array (`262,144` bytes = `65,536 * 4` bytes)**:
  - Each column is `[low: u8, mid: u8, high: u8, total: u8]`.

### 4.2 `Transients/<subdir>/<28-char-base32-key>` (`5 + 16 * count` bytes)
- **Header (`5` bytes, Little-Endian)**:
  - `offset 0x00` (`u8`): `0x05` (`version`)
  - `offset 0x01..0x05` (`u32 LE`): `count` (number of transient records)
- **Records (`16 * count` bytes, Little-Endian)**:
  - `position_sec: f64 LE` (`8` bytes: onset timestamp in seconds)
  - `weight: f32 LE` (`4` bytes: transient onset weight)
  - `metric: f32 LE` (`4` bytes: local BPM candidate / spectral flux metric)

---

## 5. Stem Container (`.stem.m4a`) & `.trak` Pack Archive

### 4.1 `.stem.m4a` / `.stem.mp4` Format
A Traktor Stem file is an ISO Base Media File Format (`MP4` / `M4A`) container with:
1. **5 Audio Tracks (`trak` atoms)**:
   - Track 0: Master stereo mixdown (AAC or ALAC, backwards-compatible with standard media players)
   - Track 1: Stem 1 (e.g., `Drums`)
   - Track 2: Stem 2 (e.g., `Bass`)
   - Track 3: Stem 3 (e.g., `Other` / `Synths`)
   - Track 4: Stem 4 (e.g., `Vocals`)
2. **`udta` -> `stem` User Data Atom**:
   - FourCC: `stem` (`0x73, 0x74, 0x65, 0x6D`)
   - Contains a UTF-8 JSON payload specifying the mastering DSP parameters (compressor + limiter settings applied after summing the 4 stems so their sum matches the master mixdown) and the 4 stem names/colors:
   ```json
   {
     "version": 1,
     "mastering_dsp": {
       "compressor": {
         "enabled": true,
         "ratio": 2.5,
         "output_gain": 0.5,
         "threshold": -6.0,
         "attack": 0.003,
         "release": 0.08,
         "hp_cutoff": 120,
         "dry_wet": 50
       },
       "limiter": {
         "enabled": true,
         "release": 0.05,
         "threshold": -0.3,
         "ceiling": -0.1
       }
     },
     "stems": [
       { "name": "Drums",  "color": "#FF3B30" },
       { "name": "Bass",   "color": "#5856D6" },
       { "name": "Synths", "color": "#34C759" },
       { "name": "Vocals", "color": "#FF9500" }
     ]
   }
   ```

### 4.2 `.trak` Remix Set Archive
Observed in 9 genuine NI Remix Sets (the six of *Step Sequencer Essentials*, *Drums and Beatbox*, *Electric Vice*, *Remix Deck Tutorial*): 576 cell positions, 512 filled and 64 empty). It holds no `.nml` manifest. Read by `trak::read_remix_set`; all integers little-endian.

- **NI file container.** A 256-byte header starting `/\ NI FC MTD  /\`, then records back to back up to exactly the end of the file. Each record is a 1920-byte header then its payload: `u64 1`, `F0`×8, `u64 1`, `u64 size` (byte 24), `/\ NI FC TOC  /\` (byte 32), `u64 index` (byte 648, 1-based, in file order), the name as UTF-16LE up to a NUL in 600 bytes (byte 672), `u64 size` again (byte 1280), `F1`×8, `/\ NI FC TOC  /\` (byte 1312), zeros elsewhere.
- **Records.** Each sample is a whole FLAC or WAV file under its own name (44.1/48 kHz, mono or stereo, 16/24-bit). 19 WAVs of *Drums and Beatbox* have a 20-byte PCM (format 1) `fmt ` chunk whose last four bytes are zero; the others have 16. One record, `NISOUND` (250 000 bytes in every set seen), holds the cell list. Record order is not cell order, and one file can feed several cells (*Remix Deck Tutorial*: 16 files, 32 cells).
- **`NISOUND`: a tree of `hsin` items.** An item is `u64 size` (of the whole item), `u32 1`, `hsin`, `u64 1`, a 16-byte id, then a frame (`u64 frame size`, a four-character kind, `u32 id`, `u32 1`, payload), then `u32 1`, `u32 child count`, and each child as `u32 position`, kind, `u32 id` and its item. The root (`DSIN` 118) has one `kart` 1 (the set), which has four `kart` 2 (slots, the columns), each with sixteen `kart` 3 (cells, the rows; rows 1–4 are page 1 and so on). A `kart` frame ends in its XML: at frame byte 60 the XML's length twice, then the XML. The word at frame byte 44 is 2 in older sets and 3 in newer ones. Opaque filler follows the root item to the end of the record; it isn't always zeros (some sets repeat a nonzero pattern) and is ignored.
- **XML.** `<TraktorSet>` (`QuantState`, `QuantValue`, `BPM`, `Title`, cover art and tags), `<TraktorSlot>` (`FxEnable`, `Keylock`, `PunchEnable`), `<TraktorCell>` (`RelativeFilename`, `Name`, `Color`, `TriggerType`, `PlayType`, `Gain`, `Offset`, `Speed`, `Pitch`, `StartMarker`, `EndMarker`, `BPM`, `BeatGridOffset`, `Reverse`, `Synced`, `Active`). Text fields are Base64 of UTF-16LE. An empty cell has an empty `RelativeFilename`; a filled cell's equals a record name exactly (512/512 filled cells). `Name` often differs from the file's name. Of the 512 filled cells, 73 have `PlayType` 0 (loop) and 439 `PlayType` 1 (one-shot). The 64 empty cells also carry `PlayType` 0 with `BPM` 0, so they don't count as loops. Defaults seen: `Gain` 0.5, `Speed` 1, `TriggerType` 1, the others 0 or `false`.
- **`Pitch`.** A signed value, 0 untuned: n/12 is n semitones. Traktor Pro 4.4.1.22's own Pitch knob readout shows every whole semitone from −12 to +12 for n/12 written with six decimals (−1.000000 … 1.000000; −0.333333 as −4, as *Remix Deck Tutorial* column 3, row 5, Bubble Gate, superseding the F1 manual's −3), checked on an imported reference set (evidence `trak-export-2026-11-23`, `pitch-reference.tsv`). *Drop Squad* column 2, row 7 (Shaker, −0.0117647) reads −0 st; the readout is whole semitones, so its exact sounding offset is unmeasured. ReTraktor imports a value within 5.1e-7 of n/12 as n and −0.0117647 as 0; any other value is left at 0 and named with its raw value.

ReTraktor imports a set onto a deck A–D (Track or Remix) whole or not at all: each cell in its place with its `Name`, loop or one-shot, `QuantState` as `Q`, and a synced cell's `BPM` (an unsynced loop plays at its own speed). `Active`, colours, `QuantValue`, set `BPM`, slot `FxEnable` and `PunchEnable` (slot `Keylock` is imported) and non-default cell gain, reverse, speed, markers and trigger are not applied (pitch: see above); the status line names the cell settings it skipped. Each sample file is copied once into a new folder in `~/Music/ReTraktor/Remix Sets` and decoded whole (exactly the length its header states); the `.trak` is only read. The WAV reader (symphonia 0.5.5) takes only 16-, 18- or 40-byte PCM `fmt ` chunks, so a WAV with the 20-byte chunk above (zero extension, first chunk, RIFF size equal to the file's) is copied with the same 16 format bytes in a 16-byte chunk and a RIFF size 4 smaller; every byte after the chunk (audio and other chunks) is unchanged. Every other file is copied byte for byte. Cell names are cut to 128 characters, control characters made spaces, and the status line says so. All 9 sets import: 512 cells checked against an independent reading (place, name, loop or one-shot, BPM; SHA-256 of each unchanged copy, of the `data` chunk for the 19 rewritten WAVs), and each reopens the same from a saved `.remixset`.

**Writing** (`trak::container_header`, `record_header`, `write_nisound`; the GUI's Remix deck `SET` → Export): the container and record headers as above; samples first, in order of the first cell that plays each, then `NISOUND` (250 000 bytes, the tree followed by zeros). Every `hsin` item gets its own random version 4 UUID, as in NI's sets (70 of 70 distinct). The root `DSIN` 118 payload (130 bytes) is copied from *Remix Deck Tutorial* and *Drop Squad*, where it is identical (two *Step Sequencer Essentials* sets differ at payload bytes 76–79); `kart` items carry word 2, as *Remix Deck Tutorial* and *Drop Squad* do. `TraktorSet`: `QuantValue` 2, `QuantState` from Q, `Title` from the file name, `BPM` of the first synced loop (else 120), empty tags, zero dates, no cover art. Slots: `FxEnable` true, `Keylock` from the column, `PunchEnable` false; the four slot XMLs of an export with only column 2 locked match byte for byte those of a set Traktor saved itself in that state. Filled cells: `Color` 1, `TriggerType` 1, `PlayType` 0/1, `Gain` 0.5, `Speed` 1, `Pitch` n/12 to six decimals, `BPM` the sample's fitted tempo (0 if none), `Synced` for a loop with a tempo, `Active` false, other fields 0/`false`; empty cells exactly as NI writes them. Samples are 24-bit stereo PCM WAVs (16-byte `fmt `). In Traktor Pro 4.4.1.22 an exported set loads with Q on at 1 beat (`QuantValue` 2; 1 shows as 1/2 and 4 as 4), −4 st and 120 BPM, and its loop and one-shot record correctly. Loading any set, the exported one or Traktor's own with only slot 2 locked, turns all four slots' Key Lock on in Traktor; ReTraktor restores each slot's own value (evidence `trak-export-2026-11-23`, `native-keylock-comparison.txt`).
