//! egui editor. Laid out on a fixed 1000x650 grid; dragging the corner scales it 75-200%.
//! All parameter changes go through `ParamSetter`, so host automation and undo see them.

mod widgets;

use crate::dsp::wavetable::Wavetable;
use crate::params::{EnvParams, MorphwaveParams, OscParams};
use crate::voice::EnvPatch;
use nih_plug::prelude::{Editor, Enum, ParamSetter};
use nih_plug_egui::egui::{vec2, Align2, Sense, Ui, Vec2};
use nih_plug_egui::resizable_window::ResizableWindow;
use nih_plug_egui::{create_egui_editor, EguiState};
use std::sync::Arc;
use widgets::{envelope_view, filter_view, knob, palette::*, panel, stepper, toggle, wavetable_view, Layout};

const W: f32 = 1000.0;
const H: f32 = 650.0;

pub fn default_state() -> Arc<EguiState> {
    EguiState::from_size(W as u32, H as u32)
}

pub fn create(params: Arc<MorphwaveParams>, tables: Arc<Vec<Wavetable>>) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    create_egui_editor(params.editor_state.clone(), (), |_, _| {}, move |ctx, setter, _| {
        ResizableWindow::new("morphwave").min_size(Vec2::new(W * 0.75, H * 0.75)).show(ctx, &state, |ui| {
            let avail = ui.max_rect();
            ui.painter().rect_filled(avail, 0.0, BG);
            let s = (avail.width() / W).min(avail.height() / H).clamp(0.75, 2.0);
            let l = Layout { origin: avail.center() - vec2(W, H) * s / 2.0, s };
            draw(ui, &l, setter, &params, &tables);
            scale_presets(ui, &l, &state);
        });
    })
}

/// Header buttons that resize the window to a fixed scale (the corner drag covers everything in between).
fn scale_presets(ui: &mut Ui, l: &Layout, state: &EguiState) {
    for (i, pct) in [75u32, 100, 125, 150, 200].into_iter().enumerate() {
        let rect = l.r(700.0 + i as f32 * 46.0, 14.0, 42.0, 18.0);
        let resp = ui.interact(rect, ui.id().with(("scale", pct)), Sense::click());
        if resp.clicked() {
            state.set_requested_size((W as u32 * pct / 100, H as u32 * pct / 100));
        }
        let current = (l.s * 100.0 - pct as f32).abs() < 3.0;
        let color = if current { ACCENT } else if resp.hovered() { TEXT } else { TEXT_DIM };
        ui.painter().text(rect.center(), Align2::CENTER_CENTER, format!("{pct}%"), l.mono(10.0), color);
    }
}

fn draw(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &MorphwaveParams, tables: &[Wavetable]) {
    let painter = ui.painter();
    painter.text(l.r(16.0, 22.0, 0.0, 0.0).min, Align2::LEFT_CENTER, "MORPHWAVE", l.font(20.0), ACCENT);
    painter.text(l.r(150.0, 24.0, 0.0, 0.0).min, Align2::LEFT_CENTER, "wavetable synthesizer", l.font(11.0), TEXT_DIM);
    painter.text(
        l.r(984.0, 24.0, 0.0, 0.0).min,
        Align2::RIGHT_CENTER,
        concat!("v", env!("CARGO_PKG_VERSION")),
        l.mono(10.0),
        TEXT_DIM,
    );

    osc(ui, l, setter, &p.osc_a, "OSC A", 10.0, tables);
    osc(ui, l, setter, &p.osc_b, "OSC B", 410.0, tables);

    // Sub
    let (x, y) = (810.0, 50.0);
    panel(ui, l, l.r(x, y, 180.0, 150.0), "SUB");
    toggle(ui, l, setter, &p.sub_to_filter, l.r(x + 125.0, y + 7.0, 45.0, 18.0), "FILTER");
    stepper(ui, l, setter, &p.sub_shape, l.r(x + 10.0, y + 34.0, 160.0, 20.0));
    knob(ui, l, setter, &p.sub_octave, l.r(x + 55.0, y + 100.0, 0.0, 0.0).min, "Octave");
    knob(ui, l, setter, &p.sub_level, l.r(x + 125.0, y + 100.0, 0.0, 0.0).min, "Level");

    // Noise
    let (x, y) = (810.0, 210.0);
    panel(ui, l, l.r(x, y, 180.0, 150.0), "NOISE");
    toggle(ui, l, setter, &p.noise_to_filter, l.r(x + 125.0, y + 7.0, 45.0, 18.0), "FILTER");
    knob(ui, l, setter, &p.noise_level, l.r(x + 90.0, y + 85.0, 0.0, 0.0).min, "Level");

    // Filter
    let (x, y) = (10.0, 370.0);
    panel(ui, l, l.r(x, y, 330.0, 270.0), "FILTER");
    toggle(ui, l, setter, &p.filter_on, l.r(x + 70.0, y + 7.0, 40.0, 18.0), "ON");
    stepper(ui, l, setter, &p.filter_type, l.r(x + 200.0, y + 6.0, 120.0, 20.0));
    filter_view(
        ui,
        l,
        l.r(x + 10.0, y + 32.0, 310.0, 130.0),
        p.filter_on.value(),
        p.filter_type.value().to_index(),
        p.cutoff.value(),
        p.resonance.value(),
    );
    let at = |i: usize| l.r(x + 37.0 + i as f32 * 64.0, y + 205.0, 0.0, 0.0).min;
    knob(ui, l, setter, &p.cutoff, at(0), "Cutoff");
    knob(ui, l, setter, &p.resonance, at(1), "Res");
    knob(ui, l, setter, &p.drive, at(2), "Drive");
    knob(ui, l, setter, &p.keytrack, at(3), "Key");
    knob(ui, l, setter, &p.env2_amount, at(4), "Env 2");

    env(ui, l, setter, &p.env1, "ENV 1 · AMP", 350.0);
    env(ui, l, setter, &p.env2, "ENV 2 · FILTER", 600.0);

    // Voice + master
    let (x, y) = (850.0, 370.0);
    panel(ui, l, l.r(x, y, 140.0, 270.0), "VOICE");
    stepper(ui, l, setter, &p.voice_mode, l.r(x + 10.0, y + 34.0, 120.0, 20.0));
    knob(ui, l, setter, &p.glide, l.r(x + 38.0, y + 100.0, 0.0, 0.0).min, "Glide");
    knob(ui, l, setter, &p.bend_range, l.r(x + 102.0, y + 100.0, 0.0, 0.0).min, "Bend");
    ui.painter().text(l.r(x + 70.0, y + 160.0, 0.0, 0.0).min, Align2::CENTER_CENTER, "MASTER", l.font(11.0), ACCENT);
    knob(ui, l, setter, &p.master, l.r(x + 70.0, y + 205.0, 0.0, 0.0).min, "Volume");
}

fn osc(ui: &mut Ui, l: &Layout, setter: &ParamSetter, o: &OscParams, title: &str, x: f32, tables: &[Wavetable]) {
    let y = 50.0;
    panel(ui, l, l.r(x, y, 390.0, 310.0), title);
    stepper(ui, l, setter, &o.table, l.r(x + 70.0, y + 6.0, 180.0, 20.0));
    toggle(ui, l, setter, &o.to_filter, l.r(x + 320.0, y + 7.0, 60.0, 18.0), "FILTER");
    let table = &tables[o.table.value().to_index().min(tables.len() - 1)];
    wavetable_view(ui, l, l.r(x + 10.0, y + 32.0, 370.0, 130.0), table, o.position.value());

    let (row1, row2) = (y + 195.0, y + 268.0);
    let at = |i: usize, row: f32| l.r(x + 38.0 + i as f32 * 63.0, row, 0.0, 0.0).min;
    knob(ui, l, setter, &o.position, at(0, row1), "Position");
    knob(ui, l, setter, &o.level, at(1, row1), "Level");
    knob(ui, l, setter, &o.pan, at(2, row1), "Pan");
    knob(ui, l, setter, &o.octave, at(3, row1), "Octave");
    knob(ui, l, setter, &o.semi, at(4, row1), "Semi");
    knob(ui, l, setter, &o.fine, at(5, row1), "Fine");
    knob(ui, l, setter, &o.unison, at(0, row2), "Unison");
    knob(ui, l, setter, &o.detune, at(1, row2), "Detune");
    knob(ui, l, setter, &o.blend, at(2, row2), "Blend");
    knob(ui, l, setter, &o.width, at(3, row2), "Width");
    knob(ui, l, setter, &o.phase, at(4, row2), "Phase");
    knob(ui, l, setter, &o.random_phase, at(5, row2), "Random");
}

fn env(ui: &mut Ui, l: &Layout, setter: &ParamSetter, e: &EnvParams, title: &str, x: f32) {
    let y = 370.0;
    panel(ui, l, l.r(x, y, 240.0, 270.0), title);
    let shape = EnvPatch { attack: e.attack.value(), decay: e.decay.value(), sustain: e.sustain.value(), release: e.release.value() };
    envelope_view(ui, l, l.r(x + 10.0, y + 32.0, 220.0, 130.0), &shape);
    let at = |i: usize| l.r(x + 37.0 + i as f32 * 55.0, y + 205.0, 0.0, 0.0).min;
    knob(ui, l, setter, &e.attack, at(0), "Attack");
    knob(ui, l, setter, &e.decay, at(1), "Decay");
    knob(ui, l, setter, &e.sustain, at(2), "Sustain");
    knob(ui, l, setter, &e.release, at(3), "Release");
}
