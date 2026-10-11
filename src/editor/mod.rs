//! egui editor. Laid out on a fixed 1000x650 grid; dragging the corner scales it 75-200%.
//! All parameter changes go through `ParamSetter`, so host automation and undo see them.

mod browser;
mod widgets;

use crate::dsp::wavetable::Wavetable;
use crate::dsp::lfo::LfoShape;
use crate::dsp::modmatrix::{Dest, NUM_DESTS};
use crate::params::{EnvParams, LfoParams, MorphwaveParams, OscParams, Table};
use crate::wavetable_import::TableSlot;
use crate::voice::EnvPatch;
use nih_plug::prelude::{Editor, Enum, ParamSetter};
use nih_plug_egui::egui::{vec2, Align2, Color32, Sense, Stroke, StrokeKind, Ui, Vec2};
use nih_plug_egui::resizable_window::ResizableWindow;
use nih_plug_egui::{create_egui_editor, EguiState};
use std::sync::Arc;
use widgets::{
    dist_view, envelope_view, eq_view, filter_view, knob, knob_mod, lfo_view, palette::*, panel, slider, stepper, toggle,
    wavetable_view, Layout,
};

const W: f32 = 1000.0;
const H: f32 = 650.0;

pub fn default_state() -> Arc<EguiState> {
    EguiState::from_size(W as u32, H as u32)
}

pub fn create(
    params: Arc<MorphwaveParams>,
    tables: Arc<Vec<Wavetable>>,
    slots: Arc<[TableSlot; 2]>,
) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    create_egui_editor(params.editor_state.clone(), EditorUi::default(), |_, _| {}, move |ctx, setter, st| {
        ResizableWindow::new("morphwave").min_size(Vec2::new(W * 0.75, H * 0.75)).show(ctx, &state, |ui| {
            let avail = ui.max_rect();
            ui.painter().rect_filled(avail, 0.0, BG);
            let s = (avail.width() / W).min(avail.height() / H).clamp(0.75, 2.0);
            let l = Layout { origin: avail.center() - vec2(W, H) * s / 2.0, s };
            header(ui, &l, &mut st.page);
            browser::bar(ui, &l, setter, &params, &slots, &mut st.browser, 400.0, 12.0);
            scale_button(ui, &l, &state);
            match st.page {
                Page::Synth => draw(ui, &l, setter, &params, &tables, &slots),
                Page::Mod => draw_mod(ui, &l, setter, &params),
                Page::Fx => draw_fx(ui, &l, setter, &params),
            }
        });
    })
}

/// GUI-only state, kept while the window is open.
#[derive(Default)]
pub struct EditorUi {
    page: Page,
    browser: browser::Browser,
}

/// Window size: each click steps 75 -> 100 -> 125 -> 150 -> 200%. The corner drag covers everything in between.
fn scale_button(ui: &mut Ui, l: &Layout, state: &EguiState) {
    const STEPS: [u32; 5] = [75, 100, 125, 150, 200];
    let now = (l.s * 100.0).round() as u32;
    let rect = l.r(822.0, 12.0, 56.0, 22.0);
    let resp = ui.interact(rect, ui.id().with("scale"), Sense::click());
    if resp.clicked() {
        let next = STEPS.iter().copied().find(|&s| s > now + 2).unwrap_or(STEPS[0]);
        state.set_requested_size((W as u32 * next / 100, H as u32 * next / 100));
    }
    let p = ui.painter();
    p.rect_stroke(rect, 4.0 * l.s, Stroke::new(1.0_f32, if resp.hovered() { ACCENT } else { PANEL_EDGE }), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, format!("{now}%"), l.mono(10.0), if resp.hovered() { TEXT } else { TEXT_DIM });
}

#[derive(Clone, Copy, PartialEq, Default)]
pub enum Page {
    #[default]
    Synth,
    Mod,
    Fx,
}

fn header(ui: &mut Ui, l: &Layout, page: &mut Page) {
    let painter = ui.painter();
    painter.text(l.r(16.0, 22.0, 0.0, 0.0).min, Align2::LEFT_CENTER, "MORPHWAVE", l.font(20.0), ACCENT);
    painter.text(
        l.r(984.0, 24.0, 0.0, 0.0).min,
        Align2::RIGHT_CENTER,
        concat!("v", env!("CARGO_PKG_VERSION")),
        l.mono(10.0),
        TEXT_DIM,
    );
    for (i, (tab, label)) in [(Page::Synth, "SYNTH"), (Page::Mod, "MOD"), (Page::Fx, "FX")].into_iter().enumerate() {
        let rect = l.r(160.0 + i as f32 * 76.0, 12.0, 70.0, 22.0);
        let resp = ui.interact(rect, ui.id().with(("page", i)), Sense::click());
        if resp.clicked() {
            *page = tab;
        }
        let active = *page == tab;
        let p = ui.painter();
        p.rect_filled(rect, 11.0 * l.s, if active { ACCENT } else { PANEL });
        let color = if active { BG } else if resp.hovered() { TEXT } else { TEXT_DIM };
        p.text(rect.center(), Align2::CENTER_CENTER, label, l.font(11.0), color);
    }
}

/// Summed matrix amount per destination, for the knob rings (1.0 = a knob's full range).
fn mod_sums(p: &MorphwaveParams) -> [f32; NUM_DESTS] {
    let mut m = [0.0; NUM_DESTS];
    for s in &p.mods {
        if s.source.value().to_index() != 0 {
            m[s.dest.value().to_index()] += s.amount.value();
        }
    }
    m[Dest::None as usize] = 0.0;
    m
}

fn draw(
    ui: &mut Ui,
    l: &Layout,
    setter: &ParamSetter,
    p: &Arc<MorphwaveParams>,
    tables: &[Wavetable],
    slots: &Arc<[TableSlot; 2]>,
) {
    let m = mod_sums(p);
    let md = |d: Dest| m[d as usize];
    osc(ui, l, setter, p, 0, tables, slots, &m);
    osc(ui, l, setter, p, 1, tables, slots, &m);

    // Sub
    let (x, y) = (810.0, 50.0);
    panel(ui, l, l.r(x, y, 180.0, 150.0), "SUB");
    toggle(ui, l, setter, &p.sub_to_filter, l.r(x + 125.0, y + 7.0, 45.0, 18.0), "FILTER");
    stepper(ui, l, setter, &p.sub_shape, l.r(x + 10.0, y + 34.0, 160.0, 20.0));
    knob(ui, l, setter, &p.sub_octave, l.r(x + 55.0, y + 100.0, 0.0, 0.0).min, "Octave");
    knob_mod(ui, l, setter, &p.sub_level, l.r(x + 125.0, y + 100.0, 0.0, 0.0).min, "Level", md(Dest::SubLevel));

    // Noise
    let (x, y) = (810.0, 210.0);
    panel(ui, l, l.r(x, y, 180.0, 150.0), "NOISE");
    toggle(ui, l, setter, &p.noise_to_filter, l.r(x + 125.0, y + 7.0, 45.0, 18.0), "FILTER");
    knob_mod(ui, l, setter, &p.noise_level, l.r(x + 90.0, y + 85.0, 0.0, 0.0).min, "Level", md(Dest::NoiseLevel));

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
    let at = |i: usize| l.r(x + 45.0 + i as f32 * 80.0, y + 205.0, 0.0, 0.0).min;
    knob_mod(ui, l, setter, &p.cutoff, at(0), "Cutoff", md(Dest::Cutoff));
    knob_mod(ui, l, setter, &p.resonance, at(1), "Res", md(Dest::Resonance));
    knob_mod(ui, l, setter, &p.drive, at(2), "Drive", md(Dest::Drive));
    knob(ui, l, setter, &p.keytrack, at(3), "Key");

    env(ui, l, setter, &p.env1, "ENV 1 · AMP", 350.0, 370.0, 240.0);
    env(ui, l, setter, &p.env2, "ENV 2", 600.0, 370.0, 240.0);

    // Voice + master
    let (x, y) = (850.0, 370.0);
    panel(ui, l, l.r(x, y, 140.0, 270.0), "VOICE");
    stepper(ui, l, setter, &p.voice_mode, l.r(x + 10.0, y + 34.0, 120.0, 20.0));
    knob(ui, l, setter, &p.glide, l.r(x + 38.0, y + 100.0, 0.0, 0.0).min, "Glide");
    knob(ui, l, setter, &p.bend_range, l.r(x + 102.0, y + 100.0, 0.0, 0.0).min, "Bend");
    ui.painter().text(l.r(x + 70.0, y + 160.0, 0.0, 0.0).min, Align2::CENTER_CENTER, "MASTER", l.font(11.0), ACCENT);
    knob_mod(ui, l, setter, &p.master, l.r(x + 70.0, y + 205.0, 0.0, 0.0).min, "Volume", md(Dest::Volume));
}

fn osc_params(p: &MorphwaveParams, i: usize) -> &OscParams {
    if i == 0 {
        &p.osc_a
    } else {
        &p.osc_b
    }
}

/// Loads a WAV on a background thread (file dialog if `path` is None) so the GUI never blocks.
fn spawn_load(p: &Arc<MorphwaveParams>, slots: &Arc<[TableSlot; 2]>, i: usize, path: Option<std::path::PathBuf>) {
    let (p, slots) = (p.clone(), slots.clone());
    std::thread::spawn(move || {
        let path = path.or_else(|| rfd::FileDialog::new().add_filter("WAV wavetable", &["wav", "WAV"]).pick_file());
        if let Some(path) = path {
            slots[i].load_file(&path, &osc_params(&p, i).custom);
        }
    });
}

fn osc(
    ui: &mut Ui,
    l: &Layout,
    setter: &ParamSetter,
    p: &Arc<MorphwaveParams>,
    i: usize,
    tables: &[Wavetable],
    slots: &Arc<[TableSlot; 2]>,
    m: &[f32; NUM_DESTS],
) {
    let (o, slot) = (osc_params(p, i), &slots[i]);
    let (x, y) = (10.0 + 400.0 * i as f32, 50.0);
    let panel_rect = l.r(x, y, 390.0, 310.0);
    panel(ui, l, panel_rect, ["OSC A", "OSC B"][i]);
    stepper(ui, l, setter, &o.table, l.r(x + 70.0, y + 6.0, 180.0, 20.0));
    toggle(ui, l, setter, &o.to_filter, l.r(x + 320.0, y + 7.0, 60.0, 18.0), "FILTER");

    // LOAD button + drag-and-drop of a WAV onto the panel
    let load = l.r(x + 258.0, y + 7.0, 54.0, 18.0);
    let resp = ui.interact(load, ui.id().with(("load", i)), Sense::click());
    if resp.clicked() {
        spawn_load(p, slots, i, None);
    }
    ui.painter().rect_stroke(load, 9.0 * l.s, Stroke::new(1.0_f32, if resp.hovered() { ACCENT } else { PANEL_EDGE }), StrokeKind::Inside);
    ui.painter().text(load.center(), Align2::CENTER_CENTER, "LOAD", l.font(10.0), if resp.hovered() { ACCENT } else { TEXT_DIM });
    let (pointer, hovering, dropped) =
        ui.ctx().input(|inp| (inp.pointer.hover_pos(), !inp.raw.hovered_files.is_empty(), inp.raw.dropped_files.clone()));
    let over = pointer.is_some_and(|pos| panel_rect.contains(pos));
    if over {
        if let Some(path) = dropped.into_iter().find_map(|f| f.path) {
            spawn_load(p, slots, i, Some(path));
        }
    }

    // after a successful load, point the Table parameter at it (through the host, so it's undoable)
    if slot.select_custom.swap(false, std::sync::atomic::Ordering::AcqRel) {
        setter.begin_set_parameter(&o.table);
        setter.set_parameter(&o.table, Table::Custom);
        setter.end_set_parameter(&o.table);
    }

    let view = l.r(x + 10.0, y + 32.0, 370.0, 130.0);
    let current = slot.current.lock().unwrap().as_ref().map(|(name, t, _)| (name.clone(), t.clone()));
    let custom_selected = o.table.value() == Table::Custom;
    match (&current, custom_selected) {
        (Some((name, table)), true) => {
            wavetable_view(ui, l, view, table, o.position.value());
            ui.painter().text(view.min + vec2(6.0, 4.0) * l.s, Align2::LEFT_TOP, name, l.font(10.0), TEXT_DIM);
        }
        _ => {
            let table = &tables[o.table.value().to_index().min(tables.len() - 1)];
            wavetable_view(ui, l, view, table, o.position.value());
            if custom_selected {
                ui.painter().text(view.center(), Align2::CENTER_CENTER, "No custom table: click LOAD or drop a WAV here", l.font(11.0), TEXT);
            }
        }
    }
    if let Some(err) = slot.error.lock().unwrap().as_ref() {
        ui.painter().text(view.left_bottom() + vec2(6.0, -4.0) * l.s, Align2::LEFT_BOTTOM, format!("Load failed: {err}"), l.font(10.0), Color32::from_rgb(0xff, 0x6b, 0x5b));
    }
    if hovering && over {
        ui.painter().rect_stroke(panel_rect, 6.0 * l.s, Stroke::new(2.0 * l.s, ACCENT), StrokeKind::Inside);
    }

    let (row1, row2) = (y + 195.0, y + 268.0);
    let at = |i: usize, row: f32| l.r(x + 38.0 + i as f32 * 63.0, row, 0.0, 0.0).min;
    let om = &m[Dest::osc_base(i)..][..7]; // position, level, pan, pitch, detune, blend, width
    knob_mod(ui, l, setter, &o.position, at(0, row1), "Position", om[0]);
    knob_mod(ui, l, setter, &o.level, at(1, row1), "Level", om[1]);
    knob_mod(ui, l, setter, &o.pan, at(2, row1), "Pan", om[2] / 2.0);
    knob(ui, l, setter, &o.octave, at(3, row1), "Octave");
    knob_mod(ui, l, setter, &o.semi, at(4, row1), "Semi", om[3] + m[Dest::Pitch as usize]);
    knob(ui, l, setter, &o.fine, at(5, row1), "Fine");
    knob(ui, l, setter, &o.unison, at(0, row2), "Unison");
    knob_mod(ui, l, setter, &o.detune, at(1, row2), "Detune", om[4]);
    knob_mod(ui, l, setter, &o.blend, at(2, row2), "Blend", om[5]);
    knob_mod(ui, l, setter, &o.width, at(3, row2), "Width", om[6]);
    knob(ui, l, setter, &o.phase, at(4, row2), "Phase");
    knob(ui, l, setter, &o.random_phase, at(5, row2), "Random");
}

fn env(ui: &mut Ui, l: &Layout, setter: &ParamSetter, e: &EnvParams, title: &str, x: f32, y: f32, w: f32) {
    panel(ui, l, l.r(x, y, w, 270.0), title);
    let shape = EnvPatch { attack: e.attack.value(), decay: e.decay.value(), sustain: e.sustain.value(), release: e.release.value() };
    envelope_view(ui, l, l.r(x + 10.0, y + 32.0, w - 20.0, 130.0), &shape);
    let at = |i: usize| l.r(x + w * (2 * i + 1) as f32 / 8.0, y + 205.0, 0.0, 0.0).min;
    knob(ui, l, setter, &e.attack, at(0), "Attack");
    knob(ui, l, setter, &e.decay, at(1), "Decay");
    knob(ui, l, setter, &e.sustain, at(2), "Sustain");
    knob(ui, l, setter, &e.release, at(3), "Release");
}

fn lfo_params(p: &MorphwaveParams, k: usize) -> &LfoParams {
    [&p.lfo1, &p.lfo2, &p.lfo3, &p.lfo4][k]
}

fn draw_mod(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &MorphwaveParams) {
    let m = mod_sums(p);
    for k in 0..4 {
        let (x, y) = (10.0 + (k % 2) as f32 * 240.0, 50.0 + (k / 2) as f32 * 160.0);
        lfo_panel(ui, l, setter, lfo_params(p, k), k, x, y, m[Dest::Lfo1Rate as usize + k]);
    }
    env(ui, l, setter, &p.env3, "ENV 3", 10.0, 370.0, 230.0);

    let (x, y) = (250.0, 370.0);
    panel(ui, l, l.r(x, y, 230.0, 270.0), "MACROS");
    for (k, mac) in [&p.macro1, &p.macro2, &p.macro3, &p.macro4].into_iter().enumerate() {
        let c = l.r(x + 60.0 + (k % 2) as f32 * 110.0, y + 90.0 + (k / 2) as f32 * 105.0, 0.0, 0.0).min;
        knob(ui, l, setter, mac, c, ["Macro 1", "Macro 2", "Macro 3", "Macro 4"][k]);
    }

    matrix(ui, l, setter, p);
}

#[allow(clippy::too_many_arguments)]
fn lfo_panel(ui: &mut Ui, l: &Layout, setter: &ParamSetter, lfo: &LfoParams, k: usize, x: f32, y: f32, rate_mod: f32) {
    panel(ui, l, l.r(x, y, 230.0, 150.0), ["LFO 1", "LFO 2", "LFO 3", "LFO 4"][k]);
    stepper(ui, l, setter, &lfo.shape, l.r(x + 60.0, y + 6.0, 160.0, 18.0));
    let shape = LfoShape::ALL[lfo.shape.value().to_index()];
    lfo_view(ui, l, l.r(x + 10.0, y + 30.0, 210.0, 52.0), shape, lfo.phase.value());
    let at = |cx: f32| l.r(x + cx, y + 110.0, 0.0, 0.0).min;
    if lfo.sync.value() {
        knob(ui, l, setter, &lfo.division, at(35.0), "Division");
    } else {
        knob_mod(ui, l, setter, &lfo.rate, at(35.0), "Rate", rate_mod / 2.0);
    }
    knob(ui, l, setter, &lfo.phase, at(95.0), "Phase");
    toggle(ui, l, setter, &lfo.sync, l.r(x + 140.0, y + 94.0, 80.0, 18.0), "SYNC");
    toggle(ui, l, setter, &lfo.retrigger, l.r(x + 140.0, y + 118.0, 80.0, 18.0), "RETRIG");
}

fn matrix(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &MorphwaveParams) {
    let (x, y) = (490.0, 50.0);
    panel(ui, l, l.r(x, y, 500.0, 590.0), "MOD MATRIX");
    for (cx, label) in [(30.0, "SOURCE"), (150.0, "DESTINATION"), (300.0, "AMOUNT"), (430.0, "CURVE")] {
        ui.painter().text(l.r(x + cx, y + 32.0, 0.0, 0.0).min, Align2::LEFT_CENTER, label, l.font(9.5), TEXT_DIM);
    }
    for (k, s) in p.mods.iter().enumerate() {
        let ry = y + 44.0 + k as f32 * 34.0;
        let active = s.source.value().to_index() != 0 && s.dest.value().to_index() != 0;
        ui.painter().text(
            l.r(x + 16.0, ry + 11.0, 0.0, 0.0).min,
            Align2::CENTER_CENTER,
            format!("{}", k + 1),
            l.mono(10.0),
            if active { MOD } else { TEXT_DIM },
        );
        stepper(ui, l, setter, &s.source, l.r(x + 30.0, ry, 114.0, 22.0));
        stepper(ui, l, setter, &s.dest, l.r(x + 150.0, ry, 144.0, 22.0));
        slider(ui, l, setter, &s.amount, l.r(x + 300.0, ry, 124.0, 22.0));
        slider(ui, l, setter, &s.curve, l.r(x + 430.0, ry, 60.0, 22.0));
    }
}

fn draw_fx(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &MorphwaveParams) {
    let m = mod_sums(p);
    let md = |d: Dest| m[d as usize];
    let fx = &p.fx;
    let at = |x: f32, y: f32| l.r(x, y, 0.0, 0.0).min;

    // Distortion
    let (x, y) = (10.0, 50.0);
    panel(ui, l, l.r(x, y, 320.0, 290.0), "DISTORTION");
    toggle(ui, l, setter, &fx.dist_on, l.r(x + 120.0, y + 7.0, 40.0, 18.0), "ON");
    stepper(ui, l, setter, &fx.dist_mode, l.r(x + 170.0, y + 6.0, 140.0, 20.0));
    let mode = crate::dsp::fx::DistMode::ALL[fx.dist_mode.value().to_index()];
    dist_view(ui, l, l.r(x + 10.0, y + 32.0, 300.0, 140.0), mode, fx.dist_drive.value(), fx.dist_on.value());
    knob_mod(ui, l, setter, &fx.dist_drive, at(x + 100.0, y + 225.0), "Drive", md(Dest::DistDrive));
    knob_mod(ui, l, setter, &fx.dist_mix, at(x + 220.0, y + 225.0), "Mix", md(Dest::DistMix));

    // Chorus
    let (x, y) = (340.0, 50.0);
    panel(ui, l, l.r(x, y, 230.0, 290.0), "CHORUS");
    toggle(ui, l, setter, &fx.chorus_on, l.r(x + 180.0, y + 7.0, 40.0, 18.0), "ON");
    knob(ui, l, setter, &fx.chorus_rate, at(x + 60.0, y + 100.0), "Rate");
    knob(ui, l, setter, &fx.chorus_depth, at(x + 170.0, y + 100.0), "Depth");
    knob_mod(ui, l, setter, &fx.chorus_mix, at(x + 115.0, y + 210.0), "Mix", md(Dest::ChorusMix));

    // Delay
    let (x, y) = (580.0, 50.0);
    panel(ui, l, l.r(x, y, 410.0, 290.0), "DELAY");
    toggle(ui, l, setter, &fx.delay_on, l.r(x + 360.0, y + 7.0, 40.0, 18.0), "ON");
    toggle(ui, l, setter, &fx.delay_sync, l.r(x + 70.0, y + 7.0, 60.0, 18.0), "SYNC");
    toggle(ui, l, setter, &fx.delay_pingpong, l.r(x + 140.0, y + 7.0, 90.0, 18.0), "PING-PONG");
    if fx.delay_sync.value() {
        knob(ui, l, setter, &fx.delay_division, at(x + 70.0, y + 100.0), "Time");
    } else {
        knob(ui, l, setter, &fx.delay_time, at(x + 70.0, y + 100.0), "Time");
    }
    knob_mod(ui, l, setter, &fx.delay_feedback, at(x + 205.0, y + 100.0), "Feedback", md(Dest::DelayFeedback));
    knob(ui, l, setter, &fx.delay_tone, at(x + 340.0, y + 100.0), "Tone");
    knob_mod(ui, l, setter, &fx.delay_mix, at(x + 205.0, y + 210.0), "Mix", md(Dest::DelayMix));

    // Reverb
    let (x, y) = (10.0, 350.0);
    panel(ui, l, l.r(x, y, 400.0, 290.0), "REVERB");
    toggle(ui, l, setter, &fx.reverb_on, l.r(x + 350.0, y + 7.0, 40.0, 18.0), "ON");
    knob_mod(ui, l, setter, &fx.reverb_size, at(x + 70.0, y + 100.0), "Size", md(Dest::ReverbSize));
    knob(ui, l, setter, &fx.reverb_damping, at(x + 200.0, y + 100.0), "Damping");
    knob(ui, l, setter, &fx.reverb_width, at(x + 330.0, y + 100.0), "Width");
    knob_mod(ui, l, setter, &fx.reverb_mix, at(x + 200.0, y + 210.0), "Mix", md(Dest::ReverbMix));

    // EQ
    let (x, y) = (420.0, 350.0);
    panel(ui, l, l.r(x, y, 570.0, 290.0), "EQ");
    toggle(ui, l, setter, &fx.eq_on, l.r(x + 520.0, y + 7.0, 40.0, 18.0), "ON");
    eq_view(
        ui,
        l,
        l.r(x + 10.0, y + 32.0, 550.0, 140.0),
        fx.eq_low.value(),
        fx.eq_mid.value(),
        fx.eq_mid_freq.value(),
        fx.eq_high.value(),
        fx.eq_on.value(),
    );
    let col = |i: usize| x + 570.0 * (2 * i + 1) as f32 / 8.0;
    knob_mod(ui, l, setter, &fx.eq_low, at(col(0), y + 225.0), "Low", md(Dest::EqLow));
    knob_mod(ui, l, setter, &fx.eq_mid, at(col(1), y + 225.0), "Mid", md(Dest::EqMid));
    knob(ui, l, setter, &fx.eq_mid_freq, at(col(2), y + 225.0), "Mid Freq");
    knob_mod(ui, l, setter, &fx.eq_high, at(col(3), y + 225.0), "High", md(Dest::EqHigh));
}
