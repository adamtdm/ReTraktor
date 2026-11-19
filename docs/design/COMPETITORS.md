# What it takes to compete (2026-11-14)

Compared against the current releases: Traktor Pro 4.5, rekordbox 7.2, Serato DJ Pro 4.0,
VirtualDJ 2026, Engine DJ, djay Pro 5.6 and Mixxx 2.6 (the open-source benchmark).

## Table stakes: every serious app has these

| Feature | Competitors | ReTraktor |
|---|---|---|
| Key lock (tempo change without pitch change) | all | Yes (Nov 2026): per-deck KEY, WSOLA time-stretch, stems included |
| Headphone cue on a second output | all | Yes (Nov 2026): outputs 3–4 or split mono; PHONES level and CUE/MIX blend |
| Real 3-band EQ / kills | all | Yes (Nov 2026): 24 dB/oct isolator (was a volume average) |
| Beat jump / quantised cues and loops | all | Beat jump supports 4, 8, 16 and 32 beats per deck. Cue and loop quantisation is still open. |
| Frequency-coloured waveforms with beat and bar markers | all | Single-colour waveform with beat ticks |
| Channel and master level meters | all | No visible per-channel meter in the 2-deck mixer |
| Pre-listen in the browser | rekordbox, Serato, Engine, djay | Missing |
| Stems | all (real time) | Offline Demucs plus instant DSP split; live neural stems not possible on the dev laptop ([REALTIME-STEMS.md](REALTIME-STEMS.md)) |
| Recording | all | Yes |
| MIDI/HID controller mapping | all | Yes: MX2 replica, learn mapper, FLX4 preset |
| Library import/export | rekordbox XML, Serato tags, Engine DB, USB for CDJs | Linked Traktor and rekordbox (XML) libraries side by side, re-read on start; rekordbox XML export. Missing: Serato, Engine, rekordbox master.db, USB |
| Sampler / remix decks | Traktor, Serato, rekordbox, VirtualDJ | Pattern player only |

## Where the field is heading in 2026

- **AI preparation**: rekordbox 7.2.14 added Smart Cue; rekordbox 7 has Collection and Streaming Radar
  for suggestions. VirtualDJ 2026 adds AI playlists, an LLM prompt folder and lyrics shown in the waveform.
  ReTraktor already has auto cues, phrase detection, next-track suggestions and an AI playlist assistant.
- **Streaming**: rekordbox edits Spotify playlists (7.2.14) and signs in to Spotify on CDJs (7.2.18);
  Serato 4.0.4 added Spotify playlisting; Apple Music reached Engine DJ hardware. ReTraktor has none.
- **4-stem real time everywhere**: rekordbox 7.2.8 moved to 4 stems; Traktor uses iZotope stems;
  Serato, VirtualDJ (Stems 2.0) and djay (Neural Mix) all separate live.
- **Hardware breadth**: point releases are mostly new controller support. Plug-and-play support for
  popular controllers matters more than features.

## Where ReTraktor can win

- **The only native Linux option besides Mixxx**, with a far more opinionated, modern UI.
- **Set building as a first-class mode** (Build / Perform / Record with an energy arc): only DJ.Studio does this well,
  and it isn't a live app.
- **Bring-your-own-key AI** with no subscription; rekordbox puts its AI behind a paid plan.

## Order to close the gaps

1. ~~Key lock, headphone cue, real EQ, device selection~~ (done Nov 2026; needs hardware testing).
2. ~~Linked libraries: Traktor + rekordbox XML~~ (done); next Engine DJ (SQLite) and Serato (crates + tags).
3. ~~Beat jump~~ (done Nov 2026) and quantise.
4. Channel and master meters; frequency-coloured waveform.
5. Browser pre-listen (headphone cue makes it cheap).
6. Serato / Engine export and USB export.
7. Real-time stems on capable machines (StemgenRT via `ort`).
8. Streaming, once credentials are available.

## Sources

- [rekordbox release notes](https://rekordbox.com/en/support/releasenote/), [v7.2.8 4-stem separation](https://www.digitaldjtips.com/rekordbox-v7-2-8-adds-4-stem-separation-new-cloud-setup-wizard/), [v7.2.14](https://rekordbox.com/en/2026/04/rekordbox-v7214-release-information/), [v7.2.18](https://rekordbox.com/en/2026/08/rekordbox-v7218-release-information/)
- [Traktor Pro 4 update status](https://community.native-instruments.com/discussion/43/), [Traktor Pro 4 review](https://www.digitaldjtips.com/reviews/traktor-pro-4-review/)
- [Serato DJ Pro 4.0 launch](https://djlifemag.com/2025/11/serato-dj-4-0-officially-launches-following-successful-public-beta/), [Serato 4.0.4 release notes](https://serato.com/dj/pro/downloads/4.0.4/releasenotes)
- [VirtualDJ 2026](https://www.gearnews.com/virtualdj-2026-dj/), [VirtualDJ stems](https://virtualdj.com/stems)
- [djay Pro release notes](https://www.algoriddim.com/djay-pro-mac/releasenotes), [djay free vs pro](https://help.algoriddim.com/topic/first-steps/free-vs-pro)
- [Mixxx 2.6 beta](https://mixxx.org/news/2025-05-11-mixxx-2_6_beta-released)
- [Best DJ software 2026](https://www.digitaldjtips.com/best-dj-software-2026/)
