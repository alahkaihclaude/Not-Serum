# Local patches

`nih_plug_egui` is copied from nih-plug at `de421011f41a6d10fc8c7a6084e4f4dee0143683` (ISC, see `LICENSE`).
Its `nih_plug` dependency was switched from a path to the same git rev. All edits are marked `MORPHWAVE PATCH`.

1. `src/editor.rs`: the editor cleared the pending resize before calling `request_resize()`. The wrapper reads the size back through `Editor::size()`, so hosts were asked to resize to the old size and the window never grew. Now the request stays set during the call. The baseview resize is also converted to physical pixels.
2. `src/lib.rs`: `EguiState::set_requested_size` is now `pub`, so the GUI can offer fixed scale presets.

Drop this copy when upstream fixes it.
