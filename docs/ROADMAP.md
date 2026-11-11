# ReTraktor roadmap

Written 2026-11-10 from the whole design conversation, the current build, and what the DJ
industry is shipping in 2026. Read `docs/design/COMPANION.md` first: the app is a companion
display to the hardware, in a dot-matrix house style, with the library kept plain and readable.

## Where we are

Mix, 4-Deck, Browser and Timeline views; dot-matrix mixer with EQ and stem displays, cheers,
beat strip and phase meter; four waveform styles; sync that follows tempo and phase; Demucs
stems from a button or key; pattern player; MX2 mapper; theme editor; headless screenshot and
recording scripts.

## 1. Fix first (reported, or known and unverified)

| Item | State |
|---|---|
| Stem icons never bounced; icon should rest at the bottom when muted, one row from the top when on | fixed in build 11 |
| Crossfade cheers do not play when a deck is in stems view | fixed in build 11 |
| Recording always overwrites `ReTraktor_Master_Mix.wav` | fixed in build 12 (timestamped + .cue) |
| Timeline header: title sits lower than the STEMS button | fixed in build 13 |
| Decks C and D have a fader but no EQ or stem display | fixed in build 13 |
| Grid nudge, tempo x½ / x2, sync by ear, MX2 end to end | needs the user's ears and hardware |

## 2. Finish the design language

- Mixer centre as one continuous dot canvas (EQ, stems, meters and cheers share one grid).
- Preferences and the MX2 mapper restyled in the dot language.
- Richer OP-1 style choreography: load, eject, loop in/out, sync lock, key match, recording.
- A UI and UX review with screenshots and a score after every stage; nothing ships below 8.

## 3. Three modes: build, perform, record

The 2026 pattern (DJ.Studio, rekordbox, djay) is a pipeline: analyse, sequence, perform, export.

- **Build**: a set list with running time and an energy / key / tempo arc drawn in dots;
  drag to reorder; warnings on key clashes and tempo jumps; one key reorders for best flow.
- **Perform**: the set list becomes "next up" on the deck display; one key loads the next track.
- **Record**: timestamped recordings (never overwrite), a tracklist and `.cue` file written
  beside the audio from what was actually played.

## 4. AI assistant, with your own key

Mixed In Key and DJ.Studio charge for playlist building. ReTraktor does it with the user's own
API key (Gemini, OpenAI, Anthropic, or any OpenAI-compatible endpoint such as a local Ollama).

- **Build a playlist from a sentence** ("45 minutes of warm-up house, 118 to 124, ending bright").
  Only track metadata is sent (title, artist, BPM, key, genre, energy, length); never audio.
- **Suggest next track**: works with no key at all (key, tempo, energy and history scoring);
  with a key, the model re-ranks and explains its top picks in one line each.
- **Going further**: describe a vibe and get a full set with transition notes; ask why two
  tracks clash; auto-tag genre and mood for untagged tracks.
- Keys are stored in `~/.config/retraktor/ai.txt` with mode 600 and never logged.

## 5. Streaming services

Spotify (since Sept 2025), Apple Music and TIDAL play inside rekordbox, Serato and djay only
through private partner agreements. There is no public API that lets an open-source app stream
their audio, and ripping it would break their terms. So:

- **Now**: import a playlist from Spotify, TIDAL or anything else as a track list (CSV or text),
  match it against the local library, and show what is missing as a shopping list.
- **Next**: fetch public Spotify playlists by link using the user's own Spotify developer
  credentials (metadata only).
- **Later**: Beatport / Beatsource and TIDAL are the services with developer programmes worth
  applying to if the project gets users.

## 6. Controllers

- **Any class-compliant MIDI controller**: MIDI input and a learn-based mapper.
- **Pioneer DDJ-FLX4** (and the FLX10 after it): preset built from Pioneer's published MIDI
  message list. Untested without the hardware.
- **Traktor X1 MK3 and Z1 MK2**: these speak NI's private HID protocol, which nobody has
  published. A generic "learn" mode will let them be mapped control by control; a drawn
  replica and LED / screen feedback need a device on the desk to capture reports from.
- Mixxx's mappings are GPL and are not copied.

## 7. What the industry says to build next

From the 2026 Digital DJ Tips census (22,000 DJs) and current releases:

- Real-time stems are now used by most DJs (73% of stem users do it live). We have offline
  Demucs; real-time separation is the biggest gap.
- AI-assisted preparation: auto cue points, phrase detection, energy scoring.
- Library portability: rekordbox / Serato / Engine import and export, USB export for CDJs.
- Streaming (see section 5).

## Order of work

1. Fixes (section 1), publish.
2. In parallel, by Gemini agents: AI assistant, set modes, MIDI and controller learn,
   playlist import.
3. Merge, review, score, publish.
4. Dot canvas mixer, Preferences / mapper restyle, choreography. (fixed in build 13)
5. Phrase detection and auto cues (fixed in build 13); real-time stems research.
