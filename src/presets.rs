//! Preset files: `Documents\Morphwave\Presets\<Category>\<Name>.json`.
//!
//! A preset is the plugin's own saved state (parameters + embedded custom wavetables) wrapped with a
//! name, a category and a format number. Loading fills anything missing with defaults, so presets may
//! list only what they change; nih-plug's `filter_state` migrations run on them like on projects.

use nih_plug::prelude::{ParamPtr, Params, PluginState};
use nih_plug::wrapper::state::ParamValue;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Bump when the file layout changes, and teach `parse` to migrate the old layout.
pub const FORMAT: u32 = 1;
pub const CATEGORIES: [&str; 6] = ["Bass", "Lead", "Pad", "Pluck", "Keys", "FX"];
/// Fields that describe the plugin instance rather than the sound; never stored in presets.
const INSTANCE_FIELDS: [&str; 2] = ["editor-state", "preset-name"];
/// Persisted fields holding custom wavetables. A preset without one clears it.
pub const WAVETABLE_FIELDS: [&str; 2] = ["osc_a_wavetable", "osc_b_wavetable"];
/// Written once the factory bank has been copied out, so deleted factory presets stay deleted.
const FACTORY_MARKER: &str = ".factory-v1";

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PresetFile {
    pub morphwave_preset: u32,
    pub name: String,
    pub category: String,
    pub state: PluginState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub name: String,
    pub category: String,
    pub path: PathBuf,
}

/// Parses a preset file, migrating older formats. Never panics on bad input.
pub fn parse(bytes: &[u8]) -> Result<PresetFile, String> {
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|e| format!("not a preset file ({e})"))?;
    match value.get("morphwave_preset").and_then(|v| v.as_u64()) {
        Some(1) => serde_json::from_value(value).map_err(|e| format!("damaged preset ({e})")),
        // future: Some(1) files get upgraded here when FORMAT moves on
        Some(n) if n > FORMAT as u64 => Err("made by a newer version of Morphwave".into()),
        _ => Err("not a Morphwave preset".into()),
    }
}

pub fn to_json(file: &PresetFile) -> String {
    serde_json::to_string_pretty(file).expect("preset serializes")
}

/// A safe file name, or None if nothing usable is left.
pub fn sanitize(name: &str) -> Option<String> {
    let clean: String = name.chars().filter(|c| !"<>:\"/\\|?*".contains(*c) && !c.is_control()).collect();
    let clean = clean.trim().trim_end_matches('.').trim();
    (!clean.is_empty()).then(|| clean.chars().take(64).collect())
}

pub fn presets_dir() -> Option<PathBuf> {
    documents_dir().map(|d| d.join("Morphwave").join("Presets"))
}

#[cfg(windows)]
fn documents_dir() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};
    // the real Documents folder, even when redirected (e.g. to OneDrive)
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        let hr = SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut p);
        let dir = (hr >= 0 && !p.is_null()).then(|| {
            let len = (0..).take_while(|&i| *p.add(i) != 0).count();
            PathBuf::from(OsString::from_wide(std::slice::from_raw_parts(p, len)))
        });
        CoTaskMemFree(p as *const _);
        dir
    }
}

#[cfg(not(windows))]
fn documents_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Documents"))
}

/// All presets under `dir`, ordered by category (in `CATEGORIES` order) then name.
pub fn scan(dir: &Path) -> Vec<Entry> {
    let mut out = Vec::new();
    let Ok(cats) = std::fs::read_dir(dir) else { return out };
    for cat in cats.flatten().filter(|e| e.path().is_dir()) {
        let category = cat.file_name().to_string_lossy().into_owned();
        let Ok(files) = std::fs::read_dir(cat.path()) else { continue };
        for f in files.flatten() {
            let path = f.path();
            if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")) {
                let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                out.push(Entry { name, category: category.clone(), path });
            }
        }
    }
    let rank = |c: &str| CATEGORIES.iter().position(|k| *k == c).unwrap_or(CATEGORIES.len());
    out.sort_by(|a, b| (rank(&a.category), &a.category, a.name.to_lowercase()).cmp(&(rank(&b.category), &b.category, b.name.to_lowercase())));
    out
}

/// Writes `state` as a preset. Returns the file written.
pub fn save(dir: &Path, name: &str, category: &str, mut state: PluginState) -> Result<PathBuf, String> {
    let name = sanitize(name).ok_or("enter a name")?;
    let category = sanitize(category).unwrap_or_else(|| "FX".into());
    for f in INSTANCE_FIELDS {
        state.fields.remove(f);
    }
    let folder = dir.join(&category);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let path = folder.join(format!("{name}.json"));
    let file = PresetFile { morphwave_preset: FORMAT, name, category, state };
    std::fs::write(&path, to_json(&file)).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Adds every parameter missing from `state` at its default (and clears absent custom wavetables),
/// so a preset fully defines the sound even when it only lists a few changes.
pub fn fill_defaults(state: &mut PluginState, params: &dyn Params) {
    for (id, ptr, _) in params.param_map() {
        if state.params.contains_key(&id) {
            continue;
        }
        // SAFETY: the pointers come from `params`, which outlives this call
        let plain = unsafe { ptr.default_plain_value() };
        let value = match ptr {
            ParamPtr::FloatParam(_) => ParamValue::F32(plain),
            ParamPtr::IntParam(_) | ParamPtr::EnumParam(_) => ParamValue::I32(plain.round() as i32),
            ParamPtr::BoolParam(_) => ParamValue::Bool(plain > 0.5),
        };
        state.params.insert(id, value);
    }
    for f in WAVETABLE_FIELDS {
        state.fields.entry(f.to_string()).or_insert_with(|| "null".into());
    }
}

/// The "Init" sound: an empty preset, which `fill_defaults` turns into all defaults.
pub fn init_preset() -> PresetFile {
    PresetFile {
        morphwave_preset: FORMAT,
        name: "Init".into(),
        category: String::new(),
        state: PluginState { version: env!("CARGO_PKG_VERSION").into(), params: BTreeMap::new(), fields: BTreeMap::new() },
    }
}

/// Copies the factory bank into `dir` the first time; never overwrites files that exist.
pub fn install_factory(dir: &Path) -> std::io::Result<usize> {
    if dir.join(FACTORY_MARKER).exists() {
        return Ok(0);
    }
    let mut written = 0;
    for f in factory() {
        let folder = dir.join(&f.category);
        std::fs::create_dir_all(&folder)?;
        let path = folder.join(format!("{}.json", f.name));
        if !path.exists() {
            std::fs::write(path, to_json(&f))?;
            written += 1;
        }
    }
    std::fs::write(dir.join(FACTORY_MARKER), "Factory presets were installed. Delete this file to restore missing ones.\n")?;
    Ok(written)
}

// ---------------------------------------------------------------------------------------------
// Factory bank: original sounds, stored as changes from Init. IDs are checked by a test.

fn f(v: f32) -> ParamValue {
    ParamValue::F32(v)
}
fn i(v: i32) -> ParamValue {
    ParamValue::I32(v)
}
fn b(v: bool) -> ParamValue {
    ParamValue::Bool(v)
}
fn s(v: &str) -> ParamValue {
    ParamValue::String(v.into())
}

/// Mod slot `n` (1-based): source, destination, amount.
fn slot(n: usize, src: &str, dst: &str, amount: f32) -> [(String, ParamValue); 3] {
    [(format!("src_{n}"), s(src)), (format!("dst_{n}"), s(dst)), (format!("amt_{n}"), f(amount))]
}

fn preset(name: &str, category: &str, params: Vec<(&str, ParamValue)>, slots: &[[(String, ParamValue); 3]]) -> PresetFile {
    let mut map: BTreeMap<String, ParamValue> = params.into_iter().map(|(k, v)| (k.to_string(), v)).collect();
    for s in slots {
        map.extend(s.iter().cloned());
    }
    PresetFile {
        morphwave_preset: FORMAT,
        name: name.into(),
        category: category.into(),
        state: PluginState { version: env!("CARGO_PKG_VERSION").into(), params: map, fields: BTreeMap::new() },
    }
}

pub fn factory() -> Vec<PresetFile> {
    vec![
        // ---- Bass
        preset(
            "Rubber Sub",
            "Bass",
            vec![
                ("osc_a_pos", f(0.0)),
                ("osc_a_oct", i(-1)),
                ("osc_a_rand", f(0.0)),
                ("voice_mode", s("mono")),
                ("env1_d", f(1.4)),
                ("env1_s", f(0.0)),
                ("env1_r", f(0.15)),
                ("env3_a", f(0.0)),
                ("env3_d", f(0.08)),
                ("env3_s", f(0.0)),
                ("dist_on", b(true)),
                ("dist_drive", f(0.35)),
            ],
            &[slot(2, "env3", "pitch", 0.5)],
        ),
        preset(
            "Wobble Grit",
            "Bass",
            vec![
                ("voice_mode", s("mono")),
                ("osc_a_table", s("formant")),
                ("osc_a_pos", f(0.2)),
                ("osc_a_oct", i(-1)),
                ("osc_a_unison", i(3)),
                ("osc_a_detune", f(0.1)),
                ("sub_level", f(0.5)),
                ("sub_oct", i(-2)),
                ("filt_on", b(true)),
                ("filt_type", s("lp24")),
                ("filt_cutoff", f(250.0)),
                ("filt_res", f(0.45)),
                ("filt_drive", f(0.3)),
                ("lfo1_sync", b(true)),
                ("lfo1_div", s("1/8")),
                ("dist_on", b(true)),
                ("dist_mode", s("asym")),
                ("dist_drive", f(0.25)),
            ],
            &[slot(2, "lfo1", "cutoff", 0.4), slot(3, "lfo1", "a_pos", 0.4)],
        ),
        preset(
            "Reese Drift",
            "Bass",
            vec![
                ("voice_mode", s("legato")),
                ("glide", f(0.06)),
                ("osc_a_oct", i(-1)),
                ("osc_a_unison", i(4)),
                ("osc_a_detune", f(0.18)),
                ("osc_b_level", f(0.6)),
                ("osc_b_oct", i(-1)),
                ("osc_b_fine", f(14.0)),
                ("osc_b_unison", i(2)),
                ("filt_on", b(true)),
                ("filt_type", s("lp24")),
                ("filt_cutoff", f(700.0)),
                ("filt_res", f(0.2)),
                ("lfo1_rate", f(0.15)),
                ("lfo1_retrig", b(false)),
            ],
            &[slot(2, "lfo1", "b_detune", 0.15), slot(3, "modwheel", "cutoff", 0.5)],
        ),
        preset(
            "Pulse Pump",
            "Bass",
            vec![
                ("osc_a_table", s("pwm")),
                ("osc_a_pos", f(0.3)),
                ("osc_a_oct", i(-1)),
                ("sub_level", f(0.4)),
                ("filt_on", b(true)),
                ("filt_cutoff", f(300.0)),
                ("filt_res", f(0.3)),
                ("env2_d", f(0.3)),
                ("env2_s", f(0.2)),
                ("amt_1", f(0.45)),
                ("lfo2_rate", f(0.3)),
                ("lfo2_retrig", b(false)),
            ],
            &[slot(2, "lfo2", "a_pos", 0.25)],
        ),
        // ---- Lead
        preset(
            "Glass Lead",
            "Lead",
            vec![
                ("voice_mode", s("legato")),
                ("glide", f(0.05)),
                ("osc_a_table", s("harmonics")),
                ("osc_a_pos", f(0.7)),
                ("osc_a_unison", i(3)),
                ("osc_a_detune", f(0.08)),
                ("delay_on", b(true)),
                ("delay_mix", f(0.2)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.15)),
                ("lfo1_rate", f(5.5)),
            ],
            &[slot(2, "aftertouch", "pitch", 0.01), slot(3, "modwheel", "a_pos", 0.3)],
        ),
        preset(
            "Saw Anthem",
            "Lead",
            vec![
                ("osc_a_unison", i(7)),
                ("osc_a_detune", f(0.3)),
                ("osc_b_level", f(0.45)),
                ("osc_b_oct", i(-1)),
                ("osc_b_unison", i(5)),
                ("osc_b_detune", f(0.2)),
                ("env1_r", f(0.35)),
                ("delay_on", b(true)),
                ("delay_mix", f(0.18)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.2)),
            ],
            &[],
        ),
        preset(
            "Square Hop",
            "Lead",
            vec![
                ("osc_a_pos", f(1.0)),
                ("voice_mode", s("mono")),
                ("glide", f(0.03)),
                ("env1_r", f(0.08)),
                ("delay_on", b(true)),
                ("delay_div", s("1/16")),
                ("delay_mix", f(0.15)),
                ("delay_pingpong", b(true)),
            ],
            &[],
        ),
        preset(
            "Formant Talker",
            "Lead",
            vec![
                ("osc_a_table", s("formant")),
                ("osc_a_pos", f(0.3)),
                ("osc_a_unison", i(2)),
                ("osc_a_detune", f(0.06)),
                ("filt_on", b(true)),
                ("filt_type", s("bp12")),
                ("filt_cutoff", f(1200.0)),
                ("filt_res", f(0.3)),
                ("lfo1_shape", s("tri")),
                ("lfo1_rate", f(0.6)),
            ],
            &[slot(2, "lfo1", "a_pos", 0.45), slot(3, "lfo1", "cutoff", 0.15)],
        ),
        // ---- Pad
        preset(
            "Slow Bloom",
            "Pad",
            vec![
                ("osc_a_table", s("formant")),
                ("osc_a_pos", f(0.45)),
                ("osc_a_unison", i(4)),
                ("osc_a_detune", f(0.15)),
                ("osc_b_table", s("harmonics")),
                ("osc_b_level", f(0.4)),
                ("osc_b_pos", f(0.6)),
                ("osc_b_oct", i(1)),
                ("env1_a", f(1.2)),
                ("env1_s", f(0.85)),
                ("env1_r", f(2.5)),
                ("lfo1_rate", f(0.12)),
                ("chorus_on", b(true)),
                ("reverb_on", b(true)),
                ("reverb_size", f(0.85)),
                ("reverb_mix", f(0.4)),
            ],
            &[slot(2, "lfo1", "a_pos", 0.2)],
        ),
        preset(
            "Frozen Air",
            "Pad",
            vec![
                ("osc_a_table", s("harmonics")),
                ("osc_a_pos", f(0.9)),
                ("osc_a_unison", i(5)),
                ("osc_a_detune", f(0.12)),
                ("noise_level", f(0.05)),
                ("filt_on", b(true)),
                ("filt_type", s("hp12")),
                ("filt_cutoff", f(300.0)),
                ("env1_a", f(0.8)),
                ("env1_r", f(3.0)),
                ("reverb_on", b(true)),
                ("reverb_size", f(0.95)),
                ("reverb_mix", f(0.45)),
                ("eq_on", b(true)),
                ("eq_high", f(3.0)),
            ],
            &[],
        ),
        preset(
            "Velvet Strings",
            "Pad",
            vec![
                ("osc_a_unison", i(6)),
                ("osc_a_detune", f(0.2)),
                ("filt_on", b(true)),
                ("filt_cutoff", f(3000.0)),
                ("env1_a", f(0.4)),
                ("env1_r", f(1.2)),
                ("chorus_on", b(true)),
                ("chorus_mix", f(0.4)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.3)),
            ],
            &[slot(2, "velocity", "cutoff", 0.2)],
        ),
        preset(
            "Drift Choir",
            "Pad",
            vec![
                ("osc_a_table", s("formant")),
                ("osc_a_pos", f(0.55)),
                ("osc_a_unison", i(4)),
                ("osc_a_detune", f(0.1)),
                ("env1_a", f(0.6)),
                ("env1_r", f(1.8)),
                ("lfo1_shape", s("sh")),
                ("lfo1_rate", f(0.4)),
                ("lfo1_retrig", b(false)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.35)),
            ],
            &[slot(2, "lfo1", "a_pos", 0.12)],
        ),
        // ---- Pluck
        preset(
            "Bell Pluck",
            "Pluck",
            vec![
                ("osc_a_table", s("harmonics")),
                ("osc_a_pos", f(0.5)),
                ("env1_a", f(0.001)),
                ("env1_d", f(0.6)),
                ("env1_s", f(0.0)),
                ("env1_r", f(0.4)),
                ("filt_on", b(true)),
                ("filt_type", s("lp24")),
                ("filt_cutoff", f(400.0)),
                ("env2_d", f(0.35)),
                ("amt_1", f(0.5)),
                ("delay_on", b(true)),
                ("delay_mix", f(0.2)),
            ],
            &[],
        ),
        preset(
            "Glass Mallet",
            "Pluck",
            vec![
                ("osc_a_pos", f(0.33)),
                ("osc_b_level", f(0.3)),
                ("osc_b_pos", f(0.0)),
                ("osc_b_oct", i(2)),
                ("env1_a", f(0.001)),
                ("env1_d", f(0.35)),
                ("env1_s", f(0.0)),
                ("env1_r", f(0.3)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.25)),
            ],
            &[],
        ),
        preset(
            "Snap Pluck",
            "Pluck",
            vec![
                ("osc_a_pos", f(1.0)),
                ("filt_on", b(true)),
                ("filt_type", s("lp24")),
                ("filt_cutoff", f(300.0)),
                ("filt_res", f(0.4)),
                ("env1_a", f(0.001)),
                ("env1_d", f(0.6)),
                ("env1_s", f(0.0)),
                ("env1_r", f(0.3)),
                ("env2_a", f(0.001)),
                ("env2_d", f(0.25)),
                ("amt_1", f(0.6)),
            ],
            &[slot(2, "velocity", "cutoff", 0.15)],
        ),
        preset(
            "Harp Shimmer",
            "Pluck",
            vec![
                ("osc_a_table", s("pwm")),
                ("osc_a_pos", f(0.1)),
                ("env1_a", f(0.001)),
                ("env1_d", f(1.2)),
                ("env1_s", f(0.0)),
                ("env1_r", f(0.8)),
                ("chorus_on", b(true)),
                ("delay_on", b(true)),
                ("delay_pingpong", b(true)),
                ("delay_mix", f(0.25)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.3)),
            ],
            &[],
        ),
        // ---- Keys
        preset(
            "Soft Keys",
            "Keys",
            vec![
                ("osc_a_pos", f(0.2)),
                ("filt_on", b(true)),
                ("filt_cutoff", f(2000.0)),
                ("env1_a", f(0.005)),
                ("env1_d", f(1.5)),
                ("env1_s", f(0.3)),
                ("env1_r", f(0.4)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.15)),
            ],
            &[slot(2, "velocity", "cutoff", 0.3)],
        ),
        preset(
            "Organ Bars",
            "Keys",
            vec![
                ("osc_a_table", s("harmonics")),
                ("osc_a_pos", f(0.3)),
                ("osc_b_level", f(0.4)),
                ("osc_b_table", s("harmonics")),
                ("osc_b_pos", f(0.15)),
                ("osc_b_oct", i(1)),
                ("env1_r", f(0.05)),
                ("chorus_on", b(true)),
                ("chorus_rate", f(4.5)),
                ("chorus_mix", f(0.35)),
            ],
            &[],
        ),
        // ---- FX
        preset(
            "Noise Riser",
            "FX",
            vec![
                ("osc_a_level", f(0.3)),
                ("noise_level", f(0.6)),
                ("filt_on", b(true)),
                ("filt_type", s("bp12")),
                ("filt_cutoff", f(200.0)),
                ("filt_res", f(0.5)),
                ("env3_a", f(4.0)),
                ("env3_s", f(1.0)),
                ("reverb_on", b(true)),
                ("reverb_mix", f(0.4)),
            ],
            &[slot(2, "env3", "cutoff", 0.75), slot(3, "env3", "pitch", 0.5)],
        ),
        preset(
            "Laser Drop",
            "FX",
            vec![
                ("osc_a_pos", f(1.0)),
                ("env3_a", f(0.0)),
                ("env3_d", f(0.4)),
                ("env3_s", f(0.0)),
                ("delay_on", b(true)),
                ("delay_div", s("1/16")),
                ("delay_fb", f(0.5)),
                ("delay_mix", f(0.3)),
            ],
            &[slot(2, "env3", "pitch", 0.9)],
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::MorphwaveParams;

    #[test]
    fn factory_presets_use_real_ids_and_valid_values() {
        let params = MorphwaveParams::default();
        let map: BTreeMap<String, ParamPtr> = params.param_map().into_iter().map(|(id, p, _)| (id, p)).collect();
        let bank = factory();
        assert!(bank.len() >= 20);
        for p in &bank {
            assert!(CATEGORIES.contains(&p.category.as_str()), "{}", p.name);
            for (id, value) in &p.state.params {
                let ptr = map.get(id).unwrap_or_else(|| panic!("{}: unknown param id {id}", p.name));
                match (ptr, value) {
                    (ParamPtr::FloatParam(_), ParamValue::F32(v)) => {
                        // SAFETY: pointers into `params`, alive for the whole test
                        let n = unsafe { ptr.preview_normalized(*v) };
                        assert!((0.0..=1.0).contains(&n), "{}: {id} = {v} out of range", p.name);
                    }
                    (ParamPtr::IntParam(_), ParamValue::I32(_)) | (ParamPtr::BoolParam(_), ParamValue::Bool(_)) => {}
                    (ParamPtr::EnumParam(p_), ParamValue::String(variant)) => {
                        // SAFETY: as above; this only changes the test's own parameter copy
                        let known = unsafe { (**p_).set_from_id(variant) };
                        assert!(known, "{}: {id} has no variant {variant}", p.name);
                    }
                    _ => panic!("{}: {id} has the wrong value type", p.name),
                }
            }
        }
    }

    #[test]
    fn migration_and_wavetable_keys_exist() {
        let params = MorphwaveParams::default();
        let ids: Vec<String> = params.param_map().into_iter().map(|(id, _, _)| id).collect();
        for id in ["src_1", "dst_1", "amt_1", "filt_env", "master"] {
            assert!(ids.contains(&id.to_string()), "{id}");
        }
        let fields = params.serialize_fields();
        for f in WAVETABLE_FIELDS.iter().chain(INSTANCE_FIELDS.iter().take(1)) {
            assert!(fields.contains_key(*f), "persisted field {f}");
        }
    }

    #[test]
    fn fill_defaults_completes_a_preset() {
        let params = MorphwaveParams::default();
        let mut state = factory().remove(0).state;
        fill_defaults(&mut state, &params);
        assert_eq!(state.params.len(), params.param_map().len());
        assert_eq!(state.fields["osc_a_wavetable"], "null");
        assert!(matches!(state.params["osc_a_pos"], ParamValue::F32(v) if v == 0.0)); // the preset's own value kept
    }

    #[test]
    fn parse_round_trips_and_rejects_bad_files() {
        let p = factory().remove(1);
        let back = parse(to_json(&p).as_bytes()).unwrap();
        assert_eq!(back.name, p.name);
        assert_eq!(back.state.params.len(), p.state.params.len());
        assert!(parse(b"{}").is_err());
        assert!(parse(b"garbage").is_err());
        assert!(parse(br#"{"morphwave_preset": 99, "name": "x", "category": "Bass", "state": {}}"#).unwrap_err().contains("newer"));
    }

    #[test]
    fn names_are_sanitized() {
        assert_eq!(sanitize("  My: Bass?  ").as_deref(), Some("My Bass"));
        assert_eq!(sanitize("..."), None);
        assert_eq!(sanitize(""), None);
    }

    #[test]
    fn factory_install_scan_and_save() {
        let dir = std::env::temp_dir().join(format!("morphwave-presets-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(install_factory(&dir).unwrap(), factory().len());
        // user edits one; a second install (e.g. after an update) leaves it alone
        let edited = dir.join("Bass").join("Rubber Sub.json");
        std::fs::write(&edited, "user edit").unwrap();
        std::fs::remove_file(dir.join(FACTORY_MARKER)).unwrap();
        install_factory(&dir).unwrap();
        assert_eq!(std::fs::read_to_string(&edited).unwrap(), "user edit");

        let list = scan(&dir);
        assert_eq!(list.len(), factory().len());
        assert_eq!(list[0].category, "Bass"); // category order, not alphabetical
        let mut state = init_preset().state;
        state.fields.insert("editor-state".into(), "{}".into());
        let path = save(&dir, "My/Pad", "Pad", state).unwrap();
        let saved = parse(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved.name, "MyPad");
        assert!(!saved.state.fields.contains_key("editor-state"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
