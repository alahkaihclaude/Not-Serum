//! Opens Morphwave as a standalone app (GUI check without a DAW): `cargo run --release --example standalone -- --backend dummy`
fn main() {
    nih_plug::nih_export_standalone::<morphwave::Morphwave>();
}
