//! Hand-painted widgets. Every size is multiplied by the layout scale, so the whole GUI scales with the window.

use crate::dsp::wavetable::Wavetable;
use crate::voice::EnvPatch;
use nih_plug::prelude::{Param, ParamSetter};
use nih_plug_egui::egui::{self, pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui};
use std::f32::consts::PI;

pub mod palette {
    use super::Color32;
    pub const BG: Color32 = Color32::from_rgb(0x17, 0x14, 0x1c);
    pub const PANEL: Color32 = Color32::from_rgb(0x24, 0x1f, 0x2b);
    pub const PANEL_EDGE: Color32 = Color32::from_rgb(0x3a, 0x32, 0x44);
    pub const SCREEN: Color32 = Color32::from_rgb(0x10, 0x0e, 0x13);
    pub const ACCENT: Color32 = Color32::from_rgb(0xf2, 0xa5, 0x41);
    pub const TEXT: Color32 = Color32::from_rgb(0xed, 0xe6, 0xda);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8f, 0x86, 0x96);
    pub const TRACK: Color32 = Color32::from_rgb(0x3d, 0x35, 0x47);
    /// Modulation (rings, matrix): teal against the amber accent.
    pub const MOD: Color32 = Color32::from_rgb(0x5f, 0xd4, 0xc4);
}
use palette::*;

/// Maps the 1000x650 design grid onto the window.
#[derive(Clone, Copy)]
pub struct Layout {
    pub origin: Pos2,
    pub s: f32,
}

impl Layout {
    pub fn r(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(self.origin + vec2(x, y) * self.s, vec2(w, h) * self.s)
    }

    pub fn font(&self, size: f32) -> FontId {
        FontId::proportional(size * self.s)
    }

    pub fn mono(&self, size: f32) -> FontId {
        FontId::monospace(size * self.s)
    }
}

pub fn panel(ui: &Ui, l: &Layout, rect: Rect, title: &str) {
    let p = ui.painter();
    p.rect_filled(rect, 6.0 * l.s, PANEL);
    p.rect_stroke(rect, 6.0 * l.s, Stroke::new(1.0_f32, PANEL_EDGE), StrokeKind::Inside);
    p.text(rect.min + vec2(10.0, 8.0) * l.s, Align2::LEFT_TOP, title, l.font(13.0), ACCENT);
}

fn arc(center: Pos2, radius: f32, from: f32, to: f32) -> Vec<Pos2> {
    let n = ((to - from).abs() * 12.0).ceil().max(2.0) as usize;
    (0..=n)
        .map(|i| {
            let a = from + (to - from) * i as f32 / n as f32;
            center + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// Shared drag behaviour: drag (Shift for fine) changes the value, double-click resets it.
/// `vertical` picks the drag axis (up / right increases).
fn drag_param<P: Param>(ui: &mut Ui, l: &Layout, setter: &ParamSetter, param: &P, rect: Rect, vertical: bool) -> egui::Response {
    let id = ui.id().with(param.name());
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.double_clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, param.default_normalized_value());
        setter.end_set_parameter(param);
    } else if resp.drag_started() {
        setter.begin_set_parameter(param);
        ui.data_mut(|d| d.insert_temp(id, param.unmodulated_normalized_value()));
    }
    if resp.dragged() {
        let fine = ui.input(|i| i.modifiers.shift);
        let per_px = if fine { 0.0005 } else { 0.005 };
        let delta = if vertical { -resp.drag_delta().y } else { resp.drag_delta().x };
        // accumulate unquantized so stepped (int/enum) params still move with slow drags
        let v = (ui.data(|d| d.get_temp::<f32>(id)).unwrap_or(0.0) + delta / l.s * per_px).clamp(0.0, 1.0);
        ui.data_mut(|d| d.insert_temp(id, v));
        setter.set_parameter_normalized(param, v);
    }
    if resp.drag_stopped() {
        setter.end_set_parameter(param);
    }
    resp
}

/// Bipolar params (centered default, range below zero, e.g. pan or semitones) light up from the middle.
fn fill_origin<P: Param>(param: &P) -> f32 {
    let min = param.normalized_value_to_string(0.0, false);
    let below_zero = min.starts_with('-') || min.ends_with('L'); // "-12", or "100L" for pan
    if below_zero && (param.default_normalized_value() - 0.5).abs() < 1e-3 {
        0.5
    } else {
        0.0
    }
}

/// Rotary knob: drag vertically (Shift for fine), double-click to reset. Shows the value while hovered or dragged.
pub fn knob<P: Param>(ui: &mut Ui, l: &Layout, setter: &ParamSetter, param: &P, center: Pos2, label: &str) {
    knob_mod(ui, l, setter, param, center, label, 0.0);
}

/// Knob with a modulation ring: `modulation` is the summed mod-matrix amount aimed at this knob
/// (1.0 = the knob's full range), drawn as a teal arc outside the knob.
pub fn knob_mod<P: Param>(ui: &mut Ui, l: &Layout, setter: &ParamSetter, param: &P, center: Pos2, label: &str, modulation: f32) {
    let radius = 15.0 * l.s;
    let rect = Rect::from_center_size(center + vec2(0.0, 6.0 * l.s), vec2(48.0, 52.0) * l.s);
    let resp = drag_param(ui, l, setter, param, rect, true);

    let value = param.unmodulated_normalized_value();
    let (start, sweep) = (0.75 * PI, 1.5 * PI);
    let p = ui.painter();
    let stroke_w = 3.5 * l.s;
    p.add(Shape::line(arc(center, radius, start, start + sweep), Stroke::new(stroke_w, TRACK)));
    if modulation.abs() > 0.001 {
        let end = (value + modulation).clamp(0.0, 1.0);
        p.add(Shape::line(
            arc(center, radius + 4.5 * l.s, start + sweep * value.min(end), start + sweep * value.max(end)),
            Stroke::new(2.0 * l.s, MOD),
        ));
    }
    let from = fill_origin(param);
    if (value - from).abs() > 0.001 {
        p.add(Shape::line(
            arc(center, radius, start + sweep * from.min(value), start + sweep * from.max(value)),
            Stroke::new(stroke_w, ACCENT),
        ));
    }
    let a = start + sweep * value;
    p.line_segment(
        [center + vec2(a.cos(), a.sin()) * radius * 0.35, center + vec2(a.cos(), a.sin()) * radius * 0.85],
        Stroke::new(2.0 * l.s, TEXT),
    );

    let text_pos = center + vec2(0.0, radius + 4.0 * l.s);
    if resp.hovered() || resp.dragged() {
        p.text(text_pos, Align2::CENTER_TOP, param.normalized_value_to_string(value, true), l.mono(10.0), ACCENT);
    } else {
        p.text(text_pos, Align2::CENTER_TOP, label, l.font(10.5), TEXT_DIM);
    }
}

/// Horizontal bar slider (mod amounts): drag sideways, Shift for fine, double-click to reset.
pub fn slider<P: Param>(ui: &mut Ui, l: &Layout, setter: &ParamSetter, param: &P, rect: Rect) {
    let resp = drag_param(ui, l, setter, param, rect, false);
    let value = param.unmodulated_normalized_value();
    let from = fill_origin(param);
    let p = ui.painter();
    p.rect_filled(rect, 3.0 * l.s, SCREEN);
    let x = |v: f32| rect.left() + v * rect.width();
    let fill = Rect::from_x_y_ranges(x(from.min(value))..=x(from.max(value)), rect.y_range());
    p.rect_filled(fill, 3.0 * l.s, if value == from { TRACK } else { ACCENT.gamma_multiply(0.75) });
    let color = if resp.hovered() || resp.dragged() { TEXT } else { TEXT_DIM };
    p.text(rect.center(), Align2::CENTER_CENTER, param.normalized_value_to_string(value, true), l.mono(10.0), color);
}

/// Small on/off pill.
pub fn toggle<P: Param>(ui: &mut Ui, l: &Layout, setter: &ParamSetter, param: &P, rect: Rect, label: &str) {
    let resp = ui.interact(rect, ui.id().with(param.name()), Sense::click());
    let on = param.unmodulated_normalized_value() > 0.5;
    if resp.clicked() {
        setter.begin_set_parameter(param);
        setter.set_parameter_normalized(param, if on { 0.0 } else { 1.0 });
        setter.end_set_parameter(param);
    }
    let p = ui.painter();
    p.rect_filled(rect, rect.height() / 2.0, if on { ACCENT } else { TRACK });
    p.text(rect.center(), Align2::CENTER_CENTER, label, l.font(10.0), if on { BG } else { TEXT_DIM });
}

/// `◀ value ▶` selector for enum and small int params.
pub fn stepper<P: Param>(ui: &mut Ui, l: &Layout, setter: &ParamSetter, param: &P, rect: Rect) {
    let id = ui.id().with(param.name());
    let arrow_w = rect.height();
    let left = Rect::from_min_size(rect.min, vec2(arrow_w, rect.height()));
    let right = Rect::from_min_size(pos2(rect.max.x - arrow_w, rect.min.y), vec2(arrow_w, rect.height()));
    let value = param.unmodulated_normalized_value();
    for (r, next, glyph) in [(left, false, "◀"), (right, true, "▶")] {
        let resp = ui.interact(r, id.with(next), Sense::click());
        if resp.clicked() {
            let v = if next { param.next_normalized_step(value, false) } else { param.previous_normalized_step(value, false) };
            setter.begin_set_parameter(param);
            setter.set_parameter_normalized(param, v);
            setter.end_set_parameter(param);
        }
        let color = if resp.hovered() { ACCENT } else { TEXT_DIM };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, glyph, l.font(9.0), color);
    }
    // clicking the name opens a list of every option
    let name_rect = Rect::from_x_y_ranges(left.right()..=right.left(), rect.y_range());
    let name_resp = ui.interact(name_rect, id.with("name"), Sense::click());
    let popup = id.with("popup");
    let mut open = ui.data(|d| d.get_temp::<bool>(popup)).unwrap_or(false);
    if name_resp.clicked() {
        open = !open;
    }
    let p = ui.painter();
    p.rect_stroke(rect, 3.0 * l.s, Stroke::new(1.0_f32, if open || name_resp.hovered() { ACCENT } else { PANEL_EDGE }), StrokeKind::Inside);
    p.text(rect.center(), Align2::CENTER_CENTER, param.normalized_value_to_string(value, true), l.font(11.0), TEXT);

    if open {
        let n = param.step_count().unwrap_or(1) + 1;
        let row_h = 18.0 * l.s;
        let size = vec2(rect.width().max(130.0 * l.s), row_h * n as f32);
        let screen = ui.ctx().screen_rect();
        let mut pos = rect.left_bottom();
        pos.y = pos.y.min(screen.bottom() - size.y).max(screen.top());
        pos.x = pos.x.min(screen.right() - size.x);
        let area = egui::Area::new(popup).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
            let (list, _) = ui.allocate_exact_size(size, Sense::hover());
            ui.painter().rect_filled(list, 4.0 * l.s, PANEL);
            ui.painter().rect_stroke(list, 4.0 * l.s, Stroke::new(1.0_f32, ACCENT), StrokeKind::Inside);
            for i in 0..n {
                let v = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
                let row = Rect::from_min_size(list.min + vec2(0.0, row_h * i as f32), vec2(size.x, row_h));
                let resp = ui.interact(row, popup.with(i), Sense::click());
                let selected = (v - value).abs() < 1e-4;
                if resp.hovered() {
                    ui.painter().rect_filled(row.shrink(1.0), 3.0 * l.s, TRACK);
                }
                let color = if selected { ACCENT } else { TEXT };
                ui.painter().text(row.left_center() + vec2(8.0 * l.s, 0.0), Align2::LEFT_CENTER, param.normalized_value_to_string(v, true), l.font(10.5), color);
                if resp.clicked() {
                    setter.begin_set_parameter(param);
                    setter.set_parameter_normalized(param, v);
                    setter.end_set_parameter(param);
                    open = false;
                }
            }
        });
        if !name_resp.clicked() && area.response.clicked_elsewhere() {
            open = false;
        }
    }
    ui.data_mut(|d| d.insert_temp(popup, open));
}

/// Current frame up front, the other frames stacked behind it in a faint 3D-ish fan.
pub fn wavetable_view(ui: &Ui, l: &Layout, rect: Rect, table: &Wavetable, position: f32) {
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0 * l.s, SCREEN);
    let frames = table.frames();
    let frame_pos = position * (frames - 1) as f32;
    let depth = vec2(28.0, -22.0) * l.s; // offset from first to last frame
    let front = Rect::from_min_max(rect.min + vec2(10.0 * l.s, -depth.y + 8.0 * l.s), rect.max - vec2(depth.x + 10.0 * l.s, 8.0 * l.s));
    const POINTS: usize = 96;
    let trace = |frame: f32, offset: egui::Vec2| -> Vec<Pos2> {
        (0..=POINTS)
            .map(|i| {
                let x = i as f32 / POINTS as f32;
                let y = table.sample(frame, 0, x.min(0.9999));
                pos2(front.left() + x * front.width(), front.center().y - y * front.height() * 0.45) + offset
            })
            .collect()
    };
    let shown = frames.min(24);
    for k in (0..shown).rev() {
        let t = if shown == 1 { 0.0 } else { k as f32 / (shown - 1) as f32 };
        p.add(Shape::line(trace(t * (frames - 1) as f32, depth * t), Stroke::new(1.0_f32, TRACK)));
    }
    let t = position;
    p.add(Shape::line(trace(frame_pos, depth * t), Stroke::new(2.0 * l.s, ACCENT)));
    p.text(rect.right_bottom() - vec2(6.0, 4.0) * l.s, Align2::RIGHT_BOTTOM, format!("{:.0}%", position * 100.0), l.mono(10.0), TEXT_DIM);
}

/// ADSR outline. Segment widths use a square-root time scale so short and long times both stay readable.
pub fn envelope_view(ui: &Ui, l: &Layout, rect: Rect, e: &EnvPatch) {
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0 * l.s, SCREEN);
    let inner = rect.shrink(8.0 * l.s);
    let w = |t: f32| t.max(0.0).sqrt() + 0.05;
    let (a, d, hold, r) = (w(e.attack), w(e.decay), 0.6, w(e.release));
    let total = a + d + hold + r;
    let x = |u: f32| inner.left() + u / total * inner.width();
    let y = |v: f32| inner.bottom() - v * inner.height();
    let mut pts = vec![pos2(x(0.0), y(0.0)), pos2(x(a), y(1.0))];
    // decay and release are exponential, so draw them as curves
    for i in 1..=16 {
        let u = i as f32 / 16.0;
        pts.push(pos2(x(a + d * u), y(e.sustain + (1.0 - e.sustain) * (-6.9 * u).exp())));
    }
    pts.push(pos2(x(a + d + hold), y(e.sustain)));
    for i in 1..=16 {
        let u = i as f32 / 16.0;
        pts.push(pos2(x(a + d + hold + r * u), y(e.sustain * (-6.9 * u).exp())));
    }
    p.line_segment([pos2(inner.left(), y(0.0)), pos2(inner.right(), y(0.0))], Stroke::new(1.0_f32, TRACK));
    p.add(Shape::line(pts, Stroke::new(2.0 * l.s, ACCENT)));
}

/// Magnitude response of the SVF (analytic), on a 20 Hz - 20 kHz log axis, -36..+18 dB.
pub fn filter_view(ui: &Ui, l: &Layout, rect: Rect, on: bool, mode: usize, cutoff: f32, resonance: f32) {
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0 * l.s, SCREEN);
    let inner = rect.shrink(8.0 * l.s);
    let y_of_db = |db: f32| inner.top() + (18.0 - db.clamp(-40.0, 20.0)) / 54.0 * inner.height();
    p.line_segment([pos2(inner.left(), y_of_db(0.0)), pos2(inner.right(), y_of_db(0.0))], Stroke::new(1.0_f32, TRACK));
    // same k as dsp::filter::Coefs
    let k1 = 2.0 - 1.98 * resonance;
    let k2 = 2.0 - 1.98 * 0.3;
    let stage = |x: f32, k: f32| {
        let den = ((1.0 - x * x).powi(2) + (k * x).powi(2)).sqrt();
        match mode / 2 {
            0 => 1.0 / den,
            1 => x * x / den,
            _ => k * x / den,
        }
    };
    let pts: Vec<Pos2> = (0..=120)
        .map(|i| {
            let u = i as f32 / 120.0;
            let f = 20.0 * 1000f32.powf(u);
            let x = f / cutoff;
            let mut g = stage(x, k1);
            if mode % 2 == 1 {
                g *= stage(x, k2);
            }
            pos2(inner.left() + u * inner.width(), y_of_db(20.0 * g.max(1e-6).log10()))
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0 * l.s, if on { ACCENT } else { TRACK })));
}

/// One cycle of an LFO shape, starting at its phase offset. S&H shows a fixed example pattern.
pub fn lfo_view(ui: &Ui, l: &Layout, rect: Rect, shape: crate::dsp::lfo::LfoShape, phase: f32) {
    use crate::dsp::lfo::{LfoShape, LfoState};
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0 * l.s, SCREEN);
    let inner = rect.shrink(6.0 * l.s);
    p.line_segment([pos2(inner.left(), inner.center().y), pos2(inner.right(), inner.center().y)], Stroke::new(1.0_f32, TRACK));
    const STEPS: [f32; 8] = [0.6, -0.3, 0.9, -0.8, 0.1, 0.7, -0.6, -0.1];
    let pts: Vec<Pos2> = (0..=96)
        .map(|i| {
            let u = i as f32 / 96.0;
            let state = LfoState { phase: (u + phase).fract(), held: STEPS[((u * 8.0) as usize).min(7)] };
            let v = if shape == LfoShape::SampleHold { state.held } else { state.value(shape) };
            pos2(inner.left() + u * inner.width(), inner.center().y - v * inner.height() * 0.45)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0 * l.s, MOD)));
}

/// Distortion transfer curve (input -> output) at the current drive, input range -1..1.
pub fn dist_view(ui: &Ui, l: &Layout, rect: Rect, mode: crate::dsp::fx::DistMode, drive: f32, on: bool) {
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0 * l.s, SCREEN);
    let inner = rect.shrink(8.0 * l.s);
    let c = inner.center();
    p.line_segment([pos2(inner.left(), c.y), pos2(inner.right(), c.y)], Stroke::new(1.0_f32, TRACK));
    p.line_segment([pos2(c.x, inner.top()), pos2(c.x, inner.bottom())], Stroke::new(1.0_f32, TRACK));
    let g = crate::dsp::fx::drive_gain(drive);
    let pts: Vec<Pos2> = (0..=120)
        .map(|i| {
            let x = i as f32 / 60.0 - 1.0;
            let y = mode.shape(x * g).clamp(-1.2, 1.2);
            pos2(c.x + x * inner.width() / 2.0, c.y - y * inner.height() / 2.4)
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0 * l.s, if on { ACCENT } else { TRACK })));
}

/// EQ magnitude response, 20 Hz - 20 kHz log axis, +-18 dB.
pub fn eq_view(ui: &Ui, l: &Layout, rect: Rect, low: f32, mid: f32, mid_hz: f32, high: f32, on: bool) {
    const SR: f32 = 48000.0;
    let p = ui.painter().with_clip_rect(rect);
    p.rect_filled(rect, 4.0 * l.s, SCREEN);
    let inner = rect.shrink(8.0 * l.s);
    let y_of = |db: f32| inner.center().y - db.clamp(-18.0, 18.0) / 18.0 * inner.height() / 2.0;
    p.line_segment([pos2(inner.left(), y_of(0.0)), pos2(inner.right(), y_of(0.0))], Stroke::new(1.0_f32, TRACK));
    let bands = crate::dsp::fx::bands(low, mid, mid_hz, high, SR);
    let pts: Vec<Pos2> = (0..=160)
        .map(|i| {
            let u = i as f32 / 160.0;
            let f = 20.0 * 1000f32.powf(u);
            let g: f32 = bands.iter().map(|b| b.magnitude(f, SR)).product();
            pos2(inner.left() + u * inner.width(), y_of(20.0 * g.max(1e-6).log10()))
        })
        .collect();
    p.add(Shape::line(pts, Stroke::new(2.0 * l.s, if on { ACCENT } else { TRACK })));
}
