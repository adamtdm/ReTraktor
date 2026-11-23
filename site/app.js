(function () {
  const SHOTS = {
    mix: {
      src: "shots/mix.webp",
      key: "F1 · MIX",
      title: "F1 — Mix Workspace",
      desc: "Stacked Deck A & B waveforms, symmetric dot-matrix EQ/stem mixer, Beat Jump (4–32 beats), 8 hotcue pads, and live collection browser.",
      meta: "Shows Deck A & B bar numbers, 4-beat sweep counter, phase alignment meter, hotcue banks, and collection browser."
    },
    stems: {
      src: "shots/stems.webp",
      key: "STEMS · SPLIT",
      title: "4-Stem Isolation & Neural Mix™",
      desc: "Deck A in 4-lane split stem waveform mode (DRUMS, BASS, OTHER, VOCALS) with per-stem FX toggles, ACAPELLA/INST isolators, and dot-matrix stem icons.",
      meta: "Instant Linkwitz-Riley 4-band split on every track plus automatic loading of cached Meta Demucs FLAC stems."
    },
    fourdeck: {
      src: "shots/fourdeck.webp",
      key: "F2 · 4-DECK",
      title: "F2 — 4-Deck Workspace",
      desc: "Decks A, B, C, and D active together with a 4-channel dot-matrix mixer, individual channel fader brackets, and headphone CUE switches.",
      meta: "Full 4-deck routing with independent stem toggles, Beat Jump, key lock, and tempo master sync."
    },
    browser: {
      src: "shots/browser.webp",
      key: "F3 · BROWSER",
      title: "F3 — Library Browser & Headphone Preview",
      desc: "Mini deck strips over a 20,000+ track virtualized collection with whole-track preview waveforms, Traktor playlists, and 1-click headphone audition.",
      meta: "Click any row waveform to preview in headphones alone; 256 KB stripe cache speeds up repeat browsing."
    },
    timeline: {
      src: "shots/timeline.webp",
      key: "F4 · TIMELINE",
      title: "F4 — Transition Timeline Studio",
      desc: "8, 16, or 32-bar Deck A to Deck B transition arranger with Smooth Crossfade, Instant Bass-Swap, and Vocal Stem Bridge presets plus live auditioning.",
      meta: "Hit SYNC + PLAY BOTH to jump Deck A into the transition window and audition automated stem crossfading live."
    },
    set: {
      src: "shots/set.webp",
      key: "F5 · SET",
      title: "F5 — Set Builder (Build → Perform → Record)",
      desc: "Ordered setlist with dot-matrix Energy, BPM, and Open Key flow arc, key-clash warnings, greedy reorder, and timestamped WAV + .cue recording.",
      meta: "Tracks running time, harmonic compatibility, Next Up deck loading (Shift+N), and automatic cue sheets."
    },
    mx2: {
      src: "shots/mx2-mapper.webp",
      key: "PREFS · MX2 MAP",
      title: "Preferences — Interactive 2D MX2 Surface Mapper",
      desc: "Click any button, encoder, fader, pad, or jogwheel (or touch the physical MX2) to remap actions, adjust jog sensitivity, and pick hardware RGB LEDs.",
      meta: "Supports all 83 MX2 buttons, 105 top-panel LEDs, 12 bottom-shell ambient RGBs, plus generic MIDI Learn."
    },
    next: {
      src: "shots/next-up.webp",
      key: "PATTERN · NEXT UP",
      title: "Pattern Sequencer & Suggest Next Track",
      desc: "16-step drum pattern player (Kick, Clap, Hat, Perc) paired with the keyboard-driven Suggest Next Track harmonic/tempo picker.",
      meta: "Navigate Next Track picks with Up/Down and press Enter or 1–5 to load straight to the target deck."
    },
    themes: {
      src: "shots/themes.webp",
      key: "PREFS · LOOK",
      title: "Preferences — Themes, Waveforms & Overviews",
      desc: "Switch between Night, Field (light), and Charcoal themes, choose Traktor/Native/rekordbox track overviews, and pick Classic, Dots, Bars, or Line waveforms.",
      meta: "All UI tokens and animation switches respect contrast checks across dark and daylight booths."
    }
  };

  // 1. Hero Companion Screen Bay
  const bayImg = document.getElementById("bay-img");
  const bayKey = document.getElementById("bay-key");
  const bayDesc = document.getElementById("bay-desc");
  const bayTabs = Array.from(document.querySelectorAll(".bay-tabs [data-shot]"));
  let currentBayId = "mix";

  function selectBay(id) {
    const item = SHOTS[id];
    if (!item) return;
    currentBayId = id;
    bayImg.src = item.src;
    bayImg.alt = item.title + " — " + item.desc;
    bayKey.textContent = item.key;
    bayDesc.textContent = item.desc;
    bayTabs.forEach((btn) => {
      btn.setAttribute("aria-selected", String(btn.dataset.shot === id));
    });
  }

  bayTabs.forEach((btn) => {
    btn.addEventListener("click", () => selectBay(btn.dataset.shot));
  });

  // 2. Layouts Interactive List
  const layoutButtons = Array.from(document.querySelectorAll("[data-layout]"));
  const layoutImg = document.getElementById("layout-img");
  const layoutTitle = document.getElementById("layout-title");
  const layoutMeta = document.getElementById("layout-meta");
  let currentLayoutId = "mix";

  function selectLayout(id) {
    const item = SHOTS[id];
    if (!item) return;
    currentLayoutId = id;
    layoutImg.src = item.src;
    layoutImg.alt = item.title;
    layoutTitle.textContent = item.title;
    layoutMeta.textContent = item.meta;
    layoutButtons.forEach((btn) => {
      btn.setAttribute("aria-current", String(btn.dataset.layout === id));
    });
  }

  layoutButtons.forEach((btn) => {
    btn.addEventListener("click", () => selectLayout(btn.dataset.layout));
  });

  // 3. Lightbox Modal for 1440x900 Screenshots
  const modal = document.getElementById("shot-modal");
  const modalImg = document.getElementById("modal-img");
  const modalTitle = document.getElementById("modal-title");
  const modalClose = document.getElementById("modal-close");

  function openModal(src, alt, title) {
    modalImg.src = src;
    modalImg.alt = alt;
    modalTitle.textContent = title + " · 1440×900";
    modal.showModal();
  }

  document.getElementById("bay-expand").addEventListener("click", () => {
    const item = SHOTS[currentBayId];
    openModal(item.src, item.desc, item.title);
  });

  bayImg.addEventListener("click", () => {
    const item = SHOTS[currentBayId];
    openModal(item.src, item.desc, item.title);
  });

  document.getElementById("layout-zoom").addEventListener("click", () => {
    const item = SHOTS[currentLayoutId];
    openModal(item.src, item.meta, item.title);
  });

  layoutImg.addEventListener("click", () => {
    const item = SHOTS[currentLayoutId];
    openModal(item.src, item.meta, item.title);
  });

  document.querySelectorAll(".mapper-figure img, .shot-figure img").forEach((img) => {
    img.addEventListener("click", () => {
      openModal(img.getAttribute("src"), img.alt, img.alt.split(" showing ")[0] || "ReTraktor");
    });
  });

  modalClose.addEventListener("click", () => modal.close());
  modal.addEventListener("click", (e) => {
    if (e.target === modal) modal.close();
  });

  // 4. Interactive 4-Stem & Neural Mix Signal Lab
  const state = {
    neural: false,
    a: [true, true, true, true],
    b: [true, true, true, true],
    xfade: 50,
    eqModel: "Classic 24dB/oct",
    fx: "Filter"
  };

  const matrixGrid = document.getElementById("matrix-grid");
  const readout = document.getElementById("lab-readout");
  const modeLabel = document.getElementById("matrix-mode-label");
  const fxLabel = document.getElementById("matrix-fx-label");
  const xfadeSlider = document.getElementById("xfade-slider");

  function computeXfade(val) {
    if (val <= 5) return [1.0, 0.0];
    if (val >= 95) return [0.0, 1.0];
    const t = (val - 5) / 90;
    const a = Math.min(1, 2 * (1 - t));
    const b = Math.min(1, 2 * t);
    return [a, b];
  }

  function renderLab() {
    const [gainA, gainB] = computeXfade(state.xfade);
    const cols = state.neural
      ? [
          { name: "D+B A", active: state.a[0] || state.a[1], gain: gainA, cls: "lit-a" },
          { name: "INST A", active: state.a[2], gain: gainA, cls: "lit-a" },
          { name: "VOC A", active: state.a[3], gain: gainA, cls: "lit-a" },
          { name: "STEM B", active: state.b.some(Boolean), gain: gainB, cls: "lit-b" }
        ]
      : [
          { name: "DRM A", active: state.a[0], gain: gainA, cls: "lit-a" },
          { name: "BAS A", active: state.a[1], gain: gainA, cls: "lit-a" },
          { name: "OTH B", active: state.b[2], gain: gainB, cls: "lit-b" },
          { name: "VOC B", active: state.b[3], gain: gainB, cls: "lit-b" }
        ];

    matrixGrid.innerHTML = "";
    cols.forEach((col) => {
      const colEl = document.createElement("div");
      colEl.className = "matrix-col";
      const dotsEl = document.createElement("div");
      dotsEl.className = "matrix-dots";
      const litCount = col.active ? Math.round(col.gain * 6) : 0;
      for (let i = 0; i < 6; i++) {
        const d = document.createElement("span");
        d.className = "matrix-dot" + (i < litCount ? " " + col.cls : "");
        dotsEl.appendChild(d);
      }
      const lbl = document.createElement("span");
      lbl.className = "matrix-col-name";
      lbl.textContent = col.name;
      colEl.appendChild(dotsEl);
      colEl.appendChild(lbl);
      matrixGrid.appendChild(colEl);
    });

    modeLabel.textContent = state.neural ? "NEURAL MIX™ (EQ ⇄ STEMS)" : state.eqModel.toUpperCase();
    fxLabel.textContent = "FX: " + state.fx;
    readout.textContent =
      "A " + Math.round(gainA * 100) + "% · B " + Math.round(gainB * 100) + "% · " +
      (state.neural ? "Hardware EQ knobs mapped to Stems" : state.eqModel);
  }

  function syncDeckButtons(deck) {
    const arr = state[deck];
    document.querySelectorAll(`[data-deck="${deck}"][data-stem]`).forEach((btn) => {
      const idx = Number(btn.dataset.stem);
      btn.setAttribute("aria-pressed", String(arr[idx]));
    });
    const isAcapella = !arr[0] && !arr[1] && !arr[2] && arr[3];
    const isInst = arr[0] && arr[1] && arr[2] && !arr[3];
    document.querySelector(`[data-iso="${deck}:acapella"]`).setAttribute("aria-pressed", String(isAcapella));
    document.querySelector(`[data-iso="${deck}:inst"]`).setAttribute("aria-pressed", String(isInst));
  }

  document.querySelectorAll("[data-stem]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const deck = btn.dataset.deck;
      const idx = Number(btn.dataset.stem);
      state[deck][idx] = !state[deck][idx];
      syncDeckButtons(deck);
      renderLab();
    });
  });

  document.querySelectorAll("[data-iso]").forEach((btn) => {
    btn.addEventListener("click", () => {
      const [deck, action] = btn.dataset.iso.split(":");
      if (action === "acapella") state[deck] = [false, false, false, true];
      else if (action === "inst") state[deck] = [true, true, true, false];
      else state[deck] = [true, true, true, true];
      syncDeckButtons(deck);
      renderLab();
    });
  });

  document.getElementById("mode-classic").addEventListener("click", () => {
    state.neural = false;
    document.getElementById("mode-classic").setAttribute("aria-pressed", "true");
    document.getElementById("mode-neural").setAttribute("aria-pressed", "false");
    renderLab();
  });

  document.getElementById("mode-neural").addEventListener("click", () => {
    state.neural = true;
    document.getElementById("mode-classic").setAttribute("aria-pressed", "false");
    document.getElementById("mode-neural").setAttribute("aria-pressed", "true");
    renderLab();
  });

  document.getElementById("eq-model-select").addEventListener("change", (e) => {
    state.eqModel = e.target.value;
    renderLab();
  });

  document.getElementById("mixer-fx-select").addEventListener("change", (e) => {
    state.fx = e.target.value;
    renderLab();
  });

  xfadeSlider.addEventListener("input", (e) => {
    state.xfade = Number(e.target.value);
    renderLab();
  });

  renderLab();

  // 5. Hardware Matrix Filter
  const hwButtons = Array.from(document.querySelectorAll("[data-hw-filter]"));
  const hwRows = Array.from(document.querySelectorAll("#hw-tbody tr"));

  hwButtons.forEach((btn) => {
    btn.addEventListener("click", () => {
      const f = btn.dataset.hwFilter;
      hwButtons.forEach((b) => b.setAttribute("aria-selected", String(b === btn)));
      hwRows.forEach((row) => {
        row.hidden = f !== "all" && !row.dataset.tags.includes(f);
      });
    });
  });

  // 6. CLI Tabs
  const CLI_TABS = {
    doctor: {
      title: "System & Hardware Self-Test",
      desc: "Verifies PipeWire/ALSA audio subsystems, audio and plugdev group memberships, rootless udev device permissions, and connected controller state.",
      cmd: "retraktor doctor",
      out: `$ retraktor doctor
[audio]   PipeWire 0.3.53+ detected · RealtimeKit (rtkit) active
[groups]  user belongs to: audio, plugdev
[udev]    /etc/udev/rules.d/99-retraktor-ni.rules installed (VID 0x17CC)
[hidraw]  Traktor MX2 (17CC:2420) accessible (read/write)
[firmware] NHL2 HID mode ready (Left SHIFT + Right SHIFT toggles MIDI)`
    },
    analyze: {
      title: "Headless Track Analysis",
      desc: "Detects BPM, beatgrid offset, Open Key musical key, 1–10 energy level, and up to 8 auto cue points at phrase boundaries from the command line.",
      cmd: "retraktor-cli analyze ~/Music/*.flac",
      out: `$ retraktor-cli analyze ~/Music/01_solaris.flac
[analyze] 01_solaris.flac
          BPM: 124.00 · Key: 8m · Energy: 7/10
          Phrases: Intro (0:00), Build (0:30), Drop (1:00), Breakdown (2:00)
          Auto Cues: 6 phrase markers written`
    },
    nml: {
      title: "Traktor NML Collection Inspector",
      desc: "Reads Traktor Pro v19/v20 collection.nml files cleanly, reporting entries, BPMs, cue counts, and POSIX file locations without altering your database.",
      cmd: "retraktor-cli inspect-nml ~/.local/share/retraktor/collection.nml",
      out: `$ retraktor-cli inspect-nml ~/.local/share/retraktor/collection.nml
NML Version: 20
Program:     Traktor
Entries:     6
  [1] Articuno - Solaris (Extended Mix) (124.00 BPM, 3 cues) -> /music/01_solaris.flac
  [2] Helios - Midnight Sun (125.00 BPM, 2 cues) -> /music/02_midnight_sun.flac`
    },
    rb: {
      title: "One-Step rekordbox XML Export",
      desc: "Exports tracks, beatgrids, hotcues, loops, and playlists into a standard rekordbox XML file ready for CDJ USB preparation.",
      cmd: "retraktor-cli export-rekordbox collection.nml ~/Music/ReTraktor-rekordbox.xml",
      out: `$ retraktor-cli export-rekordbox collection.nml ~/Music/ReTraktor-rekordbox.xml
Exported 6 tracks to rekordbox XML at ~/Music/ReTraktor-rekordbox.xml`
    },
    sim: {
      title: "Terminal Player & 4-Deck Benchmark",
      desc: "Run an interactive terminal DJ player when headless or benchmark the lock-free 4-deck + 4-stem DSP engine.",
      cmd: "retraktor-cli simulate-mix",
      out: `$ retraktor-cli simulate-mix
[engine] 4 decks + 4-stem split @ 48000 Hz / 256 frames
[bench]  ~103x real-time speed (< 1% CPU usage)`
    }
  };

  const cliButtons = Array.from(document.querySelectorAll("[data-cli]"));
  const cliOut = document.getElementById("cli-output");
  const cliTitle = document.getElementById("cli-title");
  const cliDesc = document.getElementById("cli-desc");
  const cliCopy = document.getElementById("cli-copy");

  cliButtons.forEach((btn) => {
    btn.addEventListener("click", () => {
      const data = CLI_TABS[btn.dataset.cli];
      if (!data) return;
      cliButtons.forEach((b) => b.setAttribute("aria-selected", String(b === btn)));
      cliOut.textContent = data.out;
      cliTitle.textContent = data.title;
      cliDesc.textContent = data.desc;
      cliCopy.dataset.copy = data.cmd;
    });
  });

  // 7. Copy Buttons
  document.querySelectorAll("[data-copy]").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const text = btn.dataset.copy;
      const orig = btn.textContent;
      try {
        await navigator.clipboard.writeText(text);
        btn.textContent = "Copied";
        setTimeout(() => {
          btn.textContent = orig;
        }, 1600);
      } catch {
        btn.textContent = "Select & Copy";
      }
    });
  });
})();
