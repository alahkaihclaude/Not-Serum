//! Preset browser in the header: prev/next, a clickable list grouped by category, and a save dialog.
//! Files are small, so reading and writing them on the GUI thread is fine.

use super::widgets::{palette::*, Layout};
use crate::params::MorphwaveParams;
use crate::presets::{self, Entry, PresetFile, CATEGORIES, WAVETABLE_FIELDS};
use crate::wavetable_import::{StoredTable, TableSlot};
use nih_plug::prelude::ParamSetter;
use nih_plug_egui::egui::{self, vec2, Align2, Rect, Sense, Stroke, StrokeKind, Ui};
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Default)]
pub struct Browser {
    scanned: bool,
    dir: Option<PathBuf>,
    entries: Vec<Entry>,
    list_open: bool,
    save_open: bool,
    save_name: String,
    save_category: usize,
    /// Message and the time (egui seconds) it disappears.
    status: Option<(String, f64)>,
}

impl Browser {
    /// First open: copy the factory bank out if needed, then list the folder.
    fn ensure_scanned(&mut self, now: f64) {
        if self.scanned {
            return;
        }
        self.scanned = true;
        self.dir = presets::presets_dir();
        match self.dir.clone() {
            Some(dir) => {
                if let Err(e) = presets::install_factory(&dir) {
                    self.say(format!("Couldn't write factory presets: {e}"), now);
                }
                self.entries = presets::scan(&dir);
            }
            None => self.say("Couldn't find your Documents folder".into(), now),
        }
    }

    fn say(&mut self, msg: String, now: f64) {
        self.status = Some((msg, now + 4.0));
    }
}

fn label(e: &Entry) -> String {
    format!("{} / {}", e.category, e.name)
}

/// Applies a preset through the host (so FL Studio sees every parameter change).
fn apply(setter: &ParamSetter, p: &Arc<MorphwaveParams>, slots: &[TableSlot; 2], file: PresetFile, name: String) {
    let mut state = file.state;
    presets::fill_defaults(&mut state, p.as_ref());
    for (slot, key) in slots.iter().zip(WAVETABLE_FIELDS) {
        let stored: Option<StoredTable> = state.fields.get(key).and_then(|j| serde_json::from_str(j).ok()).flatten();
        slot.preload(stored.as_ref());
    }
    setter.raw_context.set_state(state);
    *p.preset_name.lock().unwrap() = name;
}

fn load_entry(setter: &ParamSetter, p: &Arc<MorphwaveParams>, slots: &[TableSlot; 2], b: &mut Browser, e: &Entry, now: f64) {
    match std::fs::read(&e.path).map_err(|e| e.to_string()).and_then(|bytes| presets::parse(&bytes)) {
        Ok(file) => apply(setter, p, slots, file, label(e)),
        Err(err) => b.say(format!("Couldn't load {}: {err}", e.name), now),
    }
}

fn button(ui: &mut Ui, l: &Layout, rect: Rect, id: &str, text: &str, active: bool) -> bool {
    let resp = ui.interact(rect, ui.id().with(("browser", id)), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 4.0 * l.s, if active { ACCENT } else { PANEL });
    p.rect_stroke(rect, 4.0 * l.s, Stroke::new(1.0_f32, if resp.hovered() { ACCENT } else { PANEL_EDGE }), StrokeKind::Inside);
    let color = if active { BG } else if resp.hovered() { TEXT } else { TEXT_DIM };
    p.text(rect.center(), Align2::CENTER_CENTER, text, l.font(10.5), color);
    resp.clicked()
}

/// The browser bar, laid out at (x, y) on the design grid, 410 wide.
pub fn bar(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &Arc<MorphwaveParams>, slots: &[TableSlot; 2], b: &mut Browser, x: f32, y: f32) {
    let now = ui.input(|i| i.time);
    b.ensure_scanned(now);
    let current = p.preset_name.lock().unwrap().clone();
    let index = b.entries.iter().position(|e| label(e) == current);

    let step = |delta: isize| {
        let n = b.entries.len() as isize;
        (n > 0).then(|| match index {
            Some(i) => (i as isize + delta).rem_euclid(n) as usize,
            None => if delta > 0 { 0 } else { (n - 1) as usize },
        })
    };
    let (prev, next) = (step(-1), step(1));
    if button(ui, l, l.r(x, y, 22.0, 22.0), "prev", "◀", false) {
        if let Some(i) = prev {
            let e = b.entries[i].clone();
            load_entry(setter, p, slots, b, &e, now);
        }
    }
    let name_rect = l.r(x + 26.0, y, 296.0, 22.0);
    if button(ui, l, name_rect, "name", &current, false) {
        b.list_open = !b.list_open;
        b.save_open = false;
    }
    if button(ui, l, l.r(x + 326.0, y, 22.0, 22.0), "next", "▶", false) {
        if let Some(i) = next {
            let e = b.entries[i].clone();
            load_entry(setter, p, slots, b, &e, now);
        }
    }
    let save_rect = l.r(x + 354.0, y, 56.0, 22.0);
    if button(ui, l, save_rect, "save", "SAVE", b.save_open) {
        b.save_open = !b.save_open;
        b.list_open = false;
        if b.save_open {
            let mut parts = current.splitn(2, " / ");
            let (cat, name) = (parts.next().unwrap_or(""), parts.next());
            b.save_category = CATEGORIES.iter().position(|c| *c == cat).unwrap_or(0);
            b.save_name = name.map_or_else(|| "My Preset".into(), str::to_string);
        }
    }

    if b.list_open {
        list_popup(ui, l, setter, p, slots, b, name_rect, &current, now);
    }
    if b.save_open {
        save_popup(ui, l, setter, p, b, save_rect, now);
    }
    if let Some((msg, until)) = b.status.clone() {
        if now < until {
            ui.painter().text(l.r(x + 26.0, y + 30.0, 0.0, 0.0).min, Align2::LEFT_TOP, msg, l.font(10.0), ACCENT);
        } else {
            b.status = None;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn list_popup(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &Arc<MorphwaveParams>, slots: &[TableSlot; 2], b: &mut Browser, anchor: Rect, current: &str, now: f64) {
    let row_h = 20.0 * l.s;
    let width = anchor.width();
    let mut picked: Option<Option<Entry>> = None; // Some(None) = Init
    let area = egui::Area::new(ui.id().with("preset-list")).order(egui::Order::Foreground).fixed_pos(anchor.left_bottom() + vec2(0.0, 2.0)).show(ui.ctx(), |ui| {
        egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0_f32, ACCENT)).corner_radius(4.0 * l.s).show(ui, |ui| {
            egui::ScrollArea::vertical().max_height(440.0 * l.s).show(ui, |ui| {
                let row = |ui: &mut Ui, text: &str, selected: bool, header: bool| -> bool {
                    let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), if header { Sense::hover() } else { Sense::click() });
                    if resp.hovered() && !header {
                        ui.painter().rect_filled(rect.shrink(1.0), 3.0 * l.s, TRACK);
                    }
                    let (font, color, indent) =
                        if header { (l.font(9.5), TEXT_DIM, 6.0) } else { (l.font(11.0), if selected { ACCENT } else { TEXT }, 16.0) };
                    ui.painter().text(rect.left_center() + vec2(indent * l.s, 0.0), Align2::LEFT_CENTER, text, font, color);
                    resp.clicked()
                };
                if row(ui, "Init", current == "Init", false) {
                    picked = Some(None);
                }
                let mut last_cat = "";
                for e in &b.entries {
                    if e.category != last_cat {
                        last_cat = &e.category;
                        row(ui, &e.category.to_uppercase(), false, true);
                    }
                    if row(ui, &e.name, label(e) == current, false) {
                        picked = Some(Some(e.clone()));
                    }
                }
                if b.entries.is_empty() {
                    row(ui, "No presets found", false, true);
                }
            });
        });
    });
    match picked {
        Some(Some(e)) => {
            load_entry(setter, p, slots, b, &e, now);
            b.list_open = false;
        }
        Some(None) => {
            apply(setter, p, slots, presets::init_preset(), "Init".into());
            b.list_open = false;
        }
        None => {
            if area.response.clicked_elsewhere() && !ui.rect_contains_pointer(anchor) {
                b.list_open = false;
            }
        }
    }
}

fn save_popup(ui: &mut Ui, l: &Layout, setter: &ParamSetter, p: &Arc<MorphwaveParams>, b: &mut Browser, anchor: Rect, now: f64) {
    let size = vec2(300.0, 104.0) * l.s;
    let pos = anchor.right_bottom() + vec2(-size.x, 2.0);
    let mut action = None; // Some(true) = save, Some(false) = cancel
    egui::Area::new(ui.id().with("preset-save")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(rect, 4.0 * l.s, PANEL);
        ui.painter().rect_stroke(rect, 4.0 * l.s, Stroke::new(1.0_f32, ACCENT), StrokeKind::Inside);
        let at = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(rect.min + vec2(x, y) * l.s, vec2(w, h) * l.s);
        ui.painter().text(at(10.0, 16.0, 0.0, 0.0).min, Align2::LEFT_CENTER, "Name", l.font(10.0), TEXT_DIM);
        let edit = egui::TextEdit::singleline(&mut b.save_name).font(l.font(12.0)).text_color(TEXT).background_color(SCREEN);
        let resp = ui.put(at(60.0, 6.0, 230.0, 22.0), edit);
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            action = Some(true);
        }
        ui.painter().text(at(10.0, 46.0, 0.0, 0.0).min, Align2::LEFT_CENTER, "Category", l.font(10.0), TEXT_DIM);
        if button(ui, l, at(80.0, 36.0, 120.0, 22.0), "cat", CATEGORIES[b.save_category], false) {
            b.save_category = (b.save_category + 1) % CATEGORIES.len();
        }
        if button(ui, l, at(150.0, 72.0, 66.0, 22.0), "do-save", "Save", true) {
            action = Some(true);
        }
        if button(ui, l, at(224.0, 72.0, 66.0, 22.0), "cancel", "Cancel", false) {
            action = Some(false);
        }
    });
    match action {
        Some(true) => {
            let Some(dir) = b.dir.clone() else {
                b.say("Couldn't find your Documents folder".into(), now);
                return;
            };
            let category = CATEGORIES[b.save_category];
            match presets::save(&dir, &b.save_name, category, setter.raw_context.get_state()) {
                Ok(path) => {
                    let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                    *p.preset_name.lock().unwrap() = format!("{category} / {name}");
                    b.entries = presets::scan(&dir);
                    b.say(format!("Saved {name}"), now);
                    b.save_open = false;
                }
                Err(e) => b.say(format!("Couldn't save: {e}"), now),
            }
        }
        Some(false) => b.save_open = false,
        None => {}
    }
}
