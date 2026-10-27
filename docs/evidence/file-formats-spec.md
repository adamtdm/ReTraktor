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

Verified against `~/Documents/Native Instruments/Traktor 4.5.1/Stripes/` and `Transients/`:

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
A Native Instruments Stem file is an ISO Base Media File Format (`MP4` / `M4A`) container with:
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

### 4.2 `.trak` Remix Set / Stem Archive
A `.trak` file packages a Remix Set or Stem bundle as a container housing:
- An embedded `.nml` manifest describing the Remix Deck 64-cell matrix (`4 columns x 16 rows`), sample loop/one-shot modes, key/BPM metadata, and relative sample paths.
- The audio sample assets referenced by the manifest.
