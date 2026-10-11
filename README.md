# Morphwave

A wavetable synthesizer plugin written in Rust with [nih-plug](https://github.com/robbert-vdh/nih-plug), for FL Studio on Windows x64.

Status: **Phase 2, custom GUI.**

## Using the GUI

- **Knobs:** drag up or down to change the value. Hold **Shift** for fine adjustment. **Double-click** resets to the default. The value shows while you hover or drag.
- **◀ ▶ selectors** step through wavetables, filter types, sub shapes and voice modes. **FILTER / ON** pills are switches.
- **Size:** click **75% to 200%** at the top right, or drag the bottom-right corner.
- Every control goes through the host's parameter system, so FL Studio automation, undo and "link to controller" all work.

## What's in it

- **Osc A / Osc B.** Wavetable oscillators with 4 built-in tables (Basic Shapes, which morphs sine → triangle → saw → square, plus PWM Sweep, Formant Sweep and Harmonic Build). Each has position, level, pan, octave/semi/fine, unison 1–16 with detune, blend and width, start phase and random phase. Osc B starts at level 0.
- **Sub** (sine/triangle/saw/square, octave) and **Noise**.
- **Filter.** LP/HP/BP at 12 or 24 dB, with cutoff, resonance, drive and key tracking. "Env 2 > Cutoff" sets how far Env 2 moves the cutoff. Each source has a "To Filter" switch; when it's off, that source skips the filter.
- **Env 1** controls volume. **Env 2** controls the filter.
- **Voices.** 16-voice poly with click-free voice stealing, plus Mono and Legato modes. Glide and pitch-bend range.
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
| `hound` (offline renderer only, not shipped yet) | Apache-2.0 | Yes |

**Result: CLAP only for now.** nih-plug is built with `default-features = false`, which turns off the `vst3` feature, so no GPL code goes into the binary. FL Studio loads CLAP plugins.

**Open item before any sale: VST3.** Steinberg moved the VST3 SDK to the MIT license with VST 3.8 (October 2025), but nih-plug still uses the GPLv3 `vst3-sys` bindings. Options: wait for nih-plug to move to permissive bindings (for example the `vst3` crate from coupler-rs), or patch that in ourselves.

To audit the whole shipped dependency tree again, run `cargo metadata` and check each crate's `license` field. This was last done in Phase 2: every other crate is MIT, Apache-2.0, ISC, BSD or Zlib. `rfd` (Phase 3) has not been checked yet.

## Patched upstream code

`vendor/` holds local copies of nih-plug (core) and nih_plug_egui with small fixes. Each copy's `PATCHES.md` explains them.
- **State loading** ([robbert-vdh/nih-plug#275](https://github.com/robbert-vdh/nih-plug/issues/275), [#279](https://github.com/robbert-vdh/nih-plug/issues/279)): corrupt state no longer aborts the host, and the host is told when parameter values change.
- **GUI resize:** the window can now be resized from inside the plugin.

clap-validator now runs every test with nothing skipped. Drop the copies once upstream has the fixes.

## Development

```
cargo test
cargo xtask bundle morphwave --release
cargo run --release --example render -- supersaw out.wav   # presets: init supersaw pluck bass pad; or `bench`
```

CI (`.github/workflows/build.yml`) runs on `windows-latest`. It runs the tests, bundles the plugin, runs [clap-validator](https://github.com/free-audio/clap-validator), and uploads `target/bundled/*` as the `Morphwave-windows` artifact.
