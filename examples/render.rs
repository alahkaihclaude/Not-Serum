//! Offline renderer: `cargo run --release --example render -- <preset> out.wav`
//! Presets: init, supersaw, pluck, bass, pad. `bench` instead of a preset measures CPU.
//!
//! Renders a held chord, a filter sweep, then a fast scale up to C8 (listen for aliasing up top).

use morphwave::dsp::filter::FilterMode;
use morphwave::voice::{Patch, Synth, VoiceMode};
use std::time::Instant;

const SR: f32 = 48000.0;

fn preset(name: &str) -> Option<Patch> {
    let mut p = Patch::default();
    match name {
        "init" => {}
        "supersaw" => {
            p.osc[0].unison = 8;
            p.osc[0].detune = 0.3;
            p.osc[1] = Patch::default().osc[0];
            p.osc[1].unison = 8;
            p.osc[1].detune = 0.2;
            p.osc[1].octave = -1;
            p.osc[1].level = 0.5;
            p.env[0].release = 0.5;
        }
        "pluck" => {
            p.osc[0].position = 1.0; // square
            p.filter_on = true;
            p.cutoff = 300.0;
            p.resonance = 0.4;
            p.env2_amount = 0.6;
            p.env[1] = morphwave::voice::EnvPatch { attack: 0.001, decay: 0.25, sustain: 0.0, release: 0.2 };
            p.env[0] = morphwave::voice::EnvPatch { attack: 0.001, decay: 0.6, sustain: 0.0, release: 0.3 };
        }
        "bass" => {
            p.voice_mode = VoiceMode::Legato;
            p.glide = 0.08;
            p.osc[0].table = 1; // PWM
            p.osc[0].position = 0.4;
            p.osc[0].octave = -2;
            p.sub_level = 0.6;
            p.sub_octave = -3;
            p.filter_on = true;
            p.filter_mode = FilterMode::Lp24;
            p.cutoff = 600.0;
            p.drive = 0.4;
        }
        "pad" => {
            p.osc[0].table = 2; // Formant
            p.osc[0].position = 0.5;
            p.osc[0].unison = 4;
            p.osc[0].detune = 0.15;
            p.osc[1].table = 3; // Harmonics
            p.osc[1].position = 0.6;
            p.osc[1].level = 0.5;
            p.osc[1].octave = 1;
            p.noise_level = 0.03;
            p.env[0] = morphwave::voice::EnvPatch { attack: 0.8, decay: 1.0, sustain: 0.8, release: 1.5 };
        }
        _ => return None,
    }
    Some(p)
}

struct Renderer {
    synth: Synth,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl Renderer {
    fn run(&mut self, patch: &Patch, seconds: f32) {
        let n = (seconds * SR) as usize;
        let (mut l, mut r) = (vec![0.0; n], vec![0.0; n]);
        self.synth.render(patch, &mut l, &mut r);
        self.left.extend(l);
        self.right.extend(r);
    }
}

fn bench() {
    let mut p = Patch::default();
    for o in &mut p.osc {
        o.level = 0.5;
        o.unison = 8;
    }
    p.filter_on = true;
    p.filter_mode = FilterMode::Lp24;
    let mut synth = Synth::new(SR);
    for n in 0..16 {
        synth.note_on(&p, 40 + n * 3, 0.8);
    }
    let seconds = 10.0;
    let (mut l, mut r) = (vec![0.0; 512], vec![0.0; 512]);
    let blocks = (seconds * SR / 512.0) as usize;
    let t = Instant::now();
    for _ in 0..blocks {
        synth.render(&p, &mut l, &mut r);
    }
    let cpu = t.elapsed().as_secs_f32() / seconds * 100.0;
    println!("16 voices x 2 osc x 8 unison, LP24 on: {cpu:.1}% of one core (target < 15%)");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("bench") {
        return bench();
    }
    let (Some(name), Some(out)) = (args.first(), args.get(1)) else {
        eprintln!("usage: render <init|supersaw|pluck|bass|pad|bench> out.wav");
        std::process::exit(2);
    };
    let Some(mut patch) = preset(name) else {
        eprintln!("unknown preset '{name}'");
        std::process::exit(2);
    };

    let mut r = Renderer { synth: Synth::new(SR), left: vec![], right: vec![] };

    // chord
    for n in [48, 60, 64, 67, 71] {
        r.synth.note_on(&patch, n, 0.8);
    }
    r.run(&patch, 2.0);
    for n in [48, 60, 64, 67, 71] {
        r.synth.note_off(&patch, n);
    }
    r.run(&patch, 1.5);

    // filter sweep 60 Hz -> 15 kHz on a held note
    let mut sweep = patch;
    sweep.filter_on = true;
    sweep.resonance = sweep.resonance.max(0.5);
    r.synth.note_on(&sweep, 36, 0.8);
    let steps = 300;
    for i in 0..steps {
        sweep.cutoff = 60.0 * 250f32.powf(i as f32 / steps as f32);
        r.run(&sweep, 3.0 / steps as f32);
    }
    r.synth.note_off(&sweep, 36);
    r.run(&sweep, 1.0);

    // fast scale C5 -> C8
    patch.voice_mode = VoiceMode::Poly;
    for n in (72..=108).step_by(2) {
        r.synth.note_on(&patch, n, 0.8);
        r.run(&patch, 0.12);
        r.synth.note_off(&patch, n);
    }
    r.run(&patch, 1.5);

    let spec = hound::WavSpec { channels: 2, sample_rate: SR as u32, bits_per_sample: 32, sample_format: hound::SampleFormat::Float };
    let mut w = hound::WavWriter::create(out, spec).expect("create wav");
    for (l, rr) in r.left.iter().zip(&r.right) {
        w.write_sample(*l).unwrap();
        w.write_sample(*rr).unwrap();
    }
    w.finalize().unwrap();
    let peak = r.left.iter().chain(&r.right).fold(0.0f32, |m, x| m.max(x.abs()));
    let clipped = r.left.iter().chain(&r.right).filter(|x| x.abs() > 0.8).count();
    println!("wrote {out}: {:.1} s, peak {peak:.2}, {clipped} samples in the soft-clip knee", r.left.len() as f32 / SR);
}
