# Local patches

egui-baseview, copied from BillyDM/egui-baseview at `ec70c3fe6b2f070dcacbc22924431edbe24bd1c0` (MIT, see `LICENSE`).
It is wired in through `[patch]` in the root `Cargo.toml`. Edits are marked `MORPHWAVE PATCH`.

1. `src/window.rs` `on_event`: baseview already reports file drag-and-drop on Windows, but egui-baseview dropped those events. They now become egui `hovered_files` / `dropped_files`, and the drop is accepted as a copy. Morphwave uses this for dropping WAV wavetables onto an oscillator.
