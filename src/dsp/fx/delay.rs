use super::{Enable, Smooth, Status};
use crate::dsp::fast_tanh;

/// Longest delay time (1 bar at 60 BPM).
const MAX_S: f32 = 4.0;

pub struct Settings {
    pub time_s: f32,
    /// 0..0.95
    pub feedback: f32,
    pub pingpong: bool,
    /// Feedback-path low-pass cutoff, Hz.
    pub tone: f32,
    pub mix: f32,
}

/// Stereo or ping-pong delay with a low-pass in the feedback path.
pub struct Delay {
    on: Enable,
    mix: Smooth,
    feedback: Smooth,
    /// Delay in samples; glides when the time changes (tape-style) instead of clicking.
    time: Smooth,
    buf: [Vec<f32>; 2],
    pos: usize,
    lp: [f32; 2],
    sr: f32,
}

impl Delay {
    pub fn new(sr: f32) -> Self {
        let len = (MAX_S * sr) as usize + 4;
        let mut time = Smooth::new(0.3 * sr, sr);
        time.coef = 1.0 - (-1.0 / (0.05 * sr)).exp(); // slower glide for time changes
        Self {
            on: Enable::new(sr),
            mix: Smooth::new(0.25, sr),
            feedback: Smooth::new(0.35, sr),
            time,
            buf: [vec![0.0; len], vec![0.0; len]],
            pos: 0,
            lp: [0.0; 2],
            sr,
        }
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|b| b.fill(0.0));
        self.lp = [0.0; 2];
    }

    pub fn process(&mut self, on: bool, s: &Settings, left: &mut [f32], right: &mut [f32]) {
        match self.on.begin(on) {
            Status::Run => {}
            Status::Clear => return self.clear(),
            Status::Skip => return,
        }
        let len = self.buf[0].len();
        let target = (s.time_s * self.sr).clamp(1.0, (len - 2) as f32);
        let lp_coef = 1.0 - (-std::f32::consts::TAU * s.tone.clamp(200.0, 20000.0) / self.sr).exp();
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let d = self.time.next(target);
            let fb = self.feedback.next(s.feedback);
            let amount = self.mix.next(s.mix) * self.on.next(on);
            let read = (self.pos + len) as f32 - d;
            let i = read as usize;
            let t = read - i as f32;
            let mut wet = [0.0; 2];
            for c in 0..2 {
                let b = &self.buf[c];
                let raw = b[i % len] + (b[(i + 1) % len] - b[i % len]) * t;
                self.lp[c] += (raw - self.lp[c]) * lp_coef;
                wet[c] = self.lp[c];
            }
            // tanh in the loop keeps runaway feedback (e.g. from modulation) bounded
            let (in_l, in_r) = if s.pingpong {
                ((*l + *r) * 0.5 + fb * wet[1], fb * wet[0])
            } else {
                (*l + fb * wet[0], *r + fb * wet[1])
            };
            self.buf[0][self.pos] = fast_tanh(in_l);
            self.buf[1][self.pos] = fast_tanh(in_r);
            self.pos = (self.pos + 1) % len;
            *l += (wet[0] - *l) * amount;
            *r += (wet[1] - *r) * amount;
        }
    }
}
