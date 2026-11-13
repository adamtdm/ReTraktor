# Real-time stems: research notes (2026-11-13)

## Question

The roadmap calls real-time stem separation the biggest gap against Serato, rekordbox and
Traktor. Can ReTraktor separate stems live, while a track plays, without preparing them first?

## What ReTraktor does today

- **Instant DSP isolation**: crossover filters split every track into bands the moment it loads.
  Zero latency, but bleed between "stems" is high.
- **Demucs (`htdemucs`)**: Python + PyTorch in `~/.local/share/retraktor/venv`, run in the
  background under `nice -n 10`, cached as FLAC per track. Clean stems, but only after it finishes.

## Measured on the dev machine (Intel i3-5157U, 2 cores / 4 threads, 7.7 GB RAM)

30 s of 44.1 kHz stereo, CPU-only PyTorch:

| Model / settings | Wall time | vs real time |
|---|---|---|
| `htdemucs` (what ReTraktor used) | 103 s | 3.4× slower |
| `htdemucs -j 2` (now the default) | 83 s | 2.8× slower |
| `htdemucs -j 4` | 71 s | 2.4× slower, more RAM |
| `htdemucs --segment 4` (smaller windows) | 149 s | 5× slower |
| `mdx` | 239 s | 8× slower |

So on this laptop a 6-minute track takes about 16 minutes to separate. Live neural separation
needs a model at least ~1.5× faster than real time to keep ahead of the playhead with headroom
for the audio engine. Nothing tested here comes close.

## How the commercial apps do it

Serato, rekordbox and VirtualDJ run compact proprietary models locally, with quality/speed modes,
and recommend pre-analysing tracks on slower laptops. Reports of stutters on low-spec machines
are common. None publish their latency or architecture.

## Open-source options

1. **StemgenRT** (C++/JUCE + ONNX Runtime): a small 4-stem model built for low latency
   (the README says 256 samples total at a 128-sample buffer). This is the only open
   real-time candidate. Quality is below htdemucs. It could be called from Rust through the
   `ort` crate. Not tested here.
2. **htdemucs as ONNX** (`stem-splitter-core` crate, or community exports on Hugging Face):
   the same model without Python/PyTorch. It would shrink the install and might run somewhat
   faster, but it is still a fixed ~7.8 s window, so not live. Not tested here.
3. **Research models** (Hybrid Spectrogram-TasNet, band-split Mamba): papers only, no
   ready-to-ship weights checked.

## Recommendation

1. **On this laptop**: keep instant DSP isolation for live use and prepare stems in advance.
   The cheap win is a "prepare stems for this playlist" action that queues a set overnight.
   Separation now uses `-j 2`.
2. **Try next, on a faster machine**: test StemgenRT's model through `ort`, inside the existing
   per-deck stem path. If it runs faster than real time with acceptable quality, use it as the
   live fallback while Demucs prepares the clean version.
3. **Later**: swap the Python/PyTorch install for an ONNX htdemucs run in Rust (one less
   dependency to install), once option 2 shows `ort` is reliable here.

## Sources

- StemgenRT: https://github.com/sweetspotsoundsystem/stemgen-rt
- Towards Practical Real-Time Low-Latency Music Source Separation: https://arxiv.org/pdf/2511.13146
- Hybrid Spectrogram-TasNet: https://arxiv.org/pdf/2402.17701
- stem-splitter-core (Rust, ONNX htdemucs): https://docs.rs/crate/stem-splitter-core/1.2.0
- htdemucs ONNX export: https://huggingface.co/adowu/htdemucs-onnx
- Mixxx GSoC 2025, Demucs to ONNX: https://mixxx.org/news/2025-10-27-gsoc2025-demucs-to-onnx-dhunstack/
- Serato real-time stems: https://www.digitaldjtips.com/real-time-stems-separation-comes-to-serato-dj-software/
