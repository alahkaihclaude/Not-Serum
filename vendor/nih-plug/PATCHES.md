# Local patches

nih-plug core, copied from `de421011f41a6d10fc8c7a6084e4f4dee0143683` (ISC, see `LICENSE`).
Only `src/`, `nih_plug_derive/` and the manifest are kept. The manifest's `[workspace]` and `[profile]` sections are removed.
It is wired in through `[patch]` in the root `Cargo.toml`. All edits are marked `MORPHWAVE PATCH`.

1. `src/wrapper/clap/wrapper.rs` `ext_state_load`: reject state whose length prefix is over 256 MiB. Upstream allocated whatever the prefix said, so corrupt state aborted the host process (robbert-vdh/nih-plug#279).
2. `src/wrapper/clap/wrapper.rs` `set_state_inner`: also schedule `RescanParamValues` so the host re-reads parameter values after a state load (robbert-vdh/nih-plug#275).

Drop this copy when upstream fixes both.
