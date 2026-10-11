# Morphwave

A wavetable synthesizer plugin written in Rust with [nih-plug](https://github.com/robbert-vdh/nih-plug), for FL Studio on Windows x64.

Status: **Phase 5, effects.**

## Using the GUI

- **Knobs:** drag up or down to change the value. Hold **Shift** for fine adjustment. **Double-click** resets to the default. The value shows while you hover or drag.
- **◀ ▶ selectors** step through wavetables, filter types, sub shapes and voice modes. **FILTER / ON** pills are switches.
- **Size:** click **75% to 200%** at the top right, or drag the bottom-right corner.

## Modulation (MOD page)

Click **MOD** at the top to switch pages.

- **LFO 1–4:** sine, triangle, saw, square and S&H shapes.
  - **SYNC** locks the rate to FL Studio's tempo, from 1/32 to 8 bars, including triplets (T) and dotted (D).
  - **RETRIG** restarts the LFO at its Phase on every note. Turn it off for one free-running LFO shared by all notes. With SYNC also on, it locks to the song position while FL Studio plays.
- **Env 3** and **Macro 1–4** are extra sources. Macros are host parameters, so you can link them to MIDI controllers in FL Studio.
- **Mod matrix:** 16 slots, each with a source, a destination, an amount (−100% to +100%) and a curve.
  - Sources: Env 1–3, LFO 1–4, Velocity, Mod Wheel, Aftertouch (channel or polyphonic), Note and Macro 1–4.
  - Destinations: oscillator position, level, pan, pitch, detune, blend and width (per osc), Pitch (all), sub and noise level, cutoff, resonance, drive, volume, LFO rates, and the effect controls (distortion drive/mix, chorus mix, delay mix/feedback, reverb mix/size, EQ gains).
  - At 100%: pitch moves ±24 semitones, cutoff ±8 octaves, LFO rate ±4 octaves, and everything else across its full range.
- Click a selector's name to pick from a list, or use ◀ ▶.
- Modulation runs per voice and never moves the knob itself. A **teal ring** around a knob on the SYNTH page shows how far it's being modulated.
- **Example, wobble bass:** set LFO 1 to SYNC at 1/8. In slot 2 pick LFO 1 → Cutoff at about 40%, and in slot 3 LFO 1 → A Position. Turn the filter on.
- **Example, 808 punch:** set Env 3 to Attack 0, Decay about 80 ms, Sustain 0. Add a slot with Env 3 → Pitch (all) at about 50% (+12 semitones).

**Older projects:** the Phase 1 "Env 2 > Cutoff" knob is gone. When an older project loads, its value moves into mod slot 1 automatically, so it sounds the same. Automation clips that targeted that old knob no longer do anything.

## Effects (FX page)

The effects run in this fixed order, after all voices are mixed: **Distortion → Chorus → Delay → Reverb → EQ → Master**.

- **Distortion:** Soft (tanh), Hard clip, Fold (wavefolder) and Asymmetric (tube-like). Drive goes up to +36 dB with rough level compensation. The display shows the curve.
- **Chorus:** two modulated delay lines, with left and right 90° apart. Rate, Depth and Mix.
- **Delay:** time in ms, or **SYNC** to tempo (1/32 to 8 bars). Also Feedback, **PING-PONG**, a Tone low-pass in the feedback loop, and Mix. Changing the time glides like tape instead of clicking.
- **Reverb:** a Freeverb-style room with Size, Damping, Width and Mix.
- **EQ:** low shelf at 200 Hz, a peak with adjustable frequency, and a high shelf at 5 kHz, each ±15 dB. The display shows the response.

Each effect has an **ON** switch. Switching fades over about 10 ms, so it doesn't click. While an effect is on, its delay and reverb tails keep ringing after you release the notes.

Mix, drive, feedback, size and the EQ gains are also **mod matrix destinations**. The effects process the mixed signal, not single notes, so their modulation follows the most recently played note.

## Custom wavetables

- Click **LOAD** on Osc A or Osc B and pick a `.wav`, or **drag a WAV file onto the oscillator panel**. The osc switches its Table to **Custom**.
- Any WAV works: mono or stereo (stereo is mixed to mono), 16/24/32-bit integer or 32-bit float.
- **Frame size** comes from the file's `clm ` chunk if it has one (the usual wavetable-WAV convention, e.g. `<!>2048`). Otherwise it's 2048. A file whose length isn't a whole number of frames is treated as one single-cycle wave. Up to 256 frames are used.
- The wavetable itself, not just its file path, is **saved in your FL Studio project**, so the project opens on another computer without the WAV. It's stored compressed at 16-bit, roughly 0.1 to 1 MB per osc.
- Selecting a built-in table keeps your custom table loaded. Step back to **Custom** to use it again.
- Every control goes through the host's parameter system, so FL Studio automation, undo and "link to controller" all work.

## What's in it

- **Osc A / Osc B.** Wavetable oscillators with 4 built-in tables (Basic Shapes, which morphs sine → triangle → saw → square, plus PWM Sweep, Formant Sweep and Harmonic Build). Each has position, level, pan, octave/semi/fine, unison 1–16 with detune, blend and width, start phase and random phase. Osc B starts at level 0.
- **Sub** (sine/triangle/saw/square, octave) and **Noise**.
- **Filter.** LP/HP/BP at 12 or 24 dB, with cutoff, resonance, drive and key tracking. Each source has a "To Filter" switch; when it's off, that source skips the filter.
- **Env 1** always controls volume. **Env 2** and **Env 3** are free modulation sources. Mod slot 1 comes pre-wired as Env 2 → Cutoff at 0%.
- **Voices.** 16-voice poly with click-free voice stealing, plus Mono and Legato modes. Glide and pitch-bend range.
- **Effects (FX page):** see below.
- **Master** volume, followed by an output soft-clip that never goes above 0 dBFS.

Every build also uploads `Morphwave-demo-renders`: WAV files of the demo patches (a chord, a filter sweep and a scale up to C8). You can listen to those without FL Studio.

## Install

1. Open the latest green run under **Actions → build** and download the `Morphwave-windows` artifact. Unzip it.
2. Close FL Studio.
3. Copy `Morphwave.clap` to `C:\Program Files\Common Files\CLAP`.
4. Start FL Studio and run **Options → Manage plugins → Find plugins**.
5. Add Morphwave to a channel and play a note.

There is no VST3 build yet. See Licensing below.

## Licensing (Phase 0 audit, 2026-10-10)

The plugin may be sold closed-source later, so every dependency that ships in the binary has to allow that.

| Component | License | OK for closed-source sale? |
|---|---|---|
| nih-plug (pinned at `de42101`) | ISC | Yes |
| nih-plug `vst3` feature → `vst3-sys` (robbert-vdh fork, branch `fix/drop-box-from-raw`) | **GPLv3** | **No** |
| CLAP bindings (`clap-sys`, used by nih-plug) | MIT / Apache-2.0 | Yes |
| nih_plug_xtask (build tool only, not shipped) | ISC | Yes |
| nih_plug_egui (copied into `vendor/` with a resize fix) | ISC | Yes |
| egui, egui-baseview, baseview | MIT / Apache-2.0 | Yes |
| egui's built-in fonts (`epaint_default_fonts`) | OFL-1.1, Ubuntu Font Licence 1.0 | Yes. Approved 2026-10-10. Font license texts must ship with the product (see `THIRD_PARTY.md`) |
| `clipboard-win`, `error-code` (egui clipboard) | BSL-1.0 | Yes. Approved 2026-10-10 |
| `realfft` / `rustfft` | MIT / Apache-2.0 | Yes |
| `hound` (WAV parsing) | Apache-2.0 | Yes |
| `rfd` (file dialog), `miniz_oxide`, `base64`, `crossbeam-queue`, `serde` | MIT / Apache-2.0 (miniz_oxide also Zlib) | Yes |
| egui-baseview (copied into `vendor/` with a drag-and-drop fix) | MIT | Yes |

**Result: CLAP only for now.** nih-plug is built with `default-features = false`, which turns off the `vst3` feature, so no GPL code goes into the binary. FL Studio loads CLAP plugins.

**Open item before any sale: VST3.** Steinberg moved the VST3 SDK to the MIT license with VST 3.8 (October 2025), but nih-plug still uses the GPLv3 `vst3-sys` bindings. Options: wait for nih-plug to move to permissive bindings (for example the `vst3` crate from coupler-rs), or patch that in ourselves.

To audit the whole shipped dependency tree again, run `cargo metadata` and check each crate's `license` field. This was last done in Phase 3 (144 shipped crates): every crate not listed above is MIT, Apache-2.0, ISC, BSD or Zlib.

## Patched upstream code

`vendor/` holds local copies of nih-plug (core), nih_plug_egui and egui-baseview with small fixes. Each copy's `PATCHES.md` explains them.
- **State loading** ([robbert-vdh/nih-plug#275](https://github.com/robbert-vdh/nih-plug/issues/275), [#279](https://github.com/robbert-vdh/nih-plug/issues/279)): corrupt state no longer aborts the host, and the host is told when parameter values change.
- **GUI resize:** the window can now be resized from inside the plugin.
- **Drag-and-drop** (egui-baseview): files dropped on the window now reach the GUI.

clap-validator now runs every test with nothing skipped. Drop the copies once upstream has the fixes.

## Development

```
cargo test
cargo xtask bundle morphwave --release
cargo run --release --example render -- supersaw out.wav   # presets: init supersaw pluck bass pad; or `bench`
```

CI (`.github/workflows/build.yml`) runs on `windows-latest`. It runs the tests, bundles the plugin, runs [clap-validator](https://github.com/free-audio/clap-validator), and uploads `target/bundled/*` as the `Morphwave-windows` artifact.
