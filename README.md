# Morphwave

A wavetable synthesizer plugin written in Rust with [nih-plug](https://github.com/robbert-vdh/nih-plug), for FL Studio on Windows x64.

Status: **Phase 1, core synth.** There's no custom GUI yet. FL Studio shows its generic knob panel instead (click the plugin's wrench icon, or open the plugin window).

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

**Result: CLAP only for now.** nih-plug is built with `default-features = false`, which turns off the `vst3` feature, so no GPL code goes into the binary. FL Studio loads CLAP plugins.

**Open item before any sale: VST3.** Steinberg moved the VST3 SDK to the MIT license with VST 3.8 (October 2025), but nih-plug still uses the GPLv3 `vst3-sys` bindings. Options: wait for nih-plug to move to permissive bindings (for example the `vst3` crate from coupler-rs), or patch that in ourselves.

These will be checked when they are added in later phases: egui and nih_plug_egui (MIT / Apache-2.0 expected), baseview (MIT / Apache-2.0 expected), `realfft` / `rustfft` (MIT / Apache-2.0 expected), `hound` (Apache-2.0 expected), and `rfd` (MIT expected).

## Known issues

- **nih-plug state bugs.** clap-validator's `state-reproducibility-*` and `state-invalid-random` tests fail in nih-plug itself ([#275](https://github.com/robbert-vdh/nih-plug/issues/275), [#279](https://github.com/robbert-vdh/nih-plug/issues/279)). After a host loads state, the host isn't told that parameter values changed, and a corrupt state blob can abort the plugin instead of being rejected. CI skips these tests for now. Fix or patch them before Phase 3, which stores wavetables in the plugin state.

## Development

```
cargo test
cargo xtask bundle morphwave --release
cargo run --release --example render -- supersaw out.wav   # presets: init supersaw pluck bass pad; or `bench`
```

CI (`.github/workflows/build.yml`) runs on `windows-latest`. It runs the tests, bundles the plugin, runs [clap-validator](https://github.com/free-audio/clap-validator), and uploads `target/bundled/*` as the `Morphwave-windows` artifact.
