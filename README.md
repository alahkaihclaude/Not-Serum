# Morphwave

A wavetable synthesizer plugin written in Rust with [nih-plug](https://github.com/robbert-vdh/nih-plug), for FL Studio on Windows x64.

Status: **Phase 0**. The plugin plays a single sine voice so you can check that the install works.

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

## Development

```
cargo test
cargo xtask bundle morphwave --release
```

CI (`.github/workflows/build.yml`) runs on `windows-latest`. It runs the tests, bundles the plugin, runs [clap-validator](https://github.com/free-audio/clap-validator), and uploads `target/bundled/*` as the `Morphwave-windows` artifact.
