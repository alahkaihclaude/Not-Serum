use super::{Enable, Smooth, Status};
use crate::dsp::fast_tanh;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DistMode {
    /// tanh saturation
    Soft,
    /// hard clipping
    Hard,
    /// sine wavefolder
    Fold,
    /// asymmetric (tube-like, adds even harmonics)
    Asym,
}

impl DistMode {
    /// Same order as the `Distortion Mode` parameter.
    pub const ALL: [Self; 4] = [Self::Soft, Self::Hard, Self::Fold, Self::Asym];

    #[inline]
    pub fn shape(self, x: f32) -> f32 {
        match self {
            Self::Soft => fast_tanh(x),
            Self::Hard => x.clamp(-1.0, 1.0),
            Self::Fold => (x * std::f32::consts::FRAC_PI_2).sin(),
            Self::Asym => {
                if x >= 0.0 {
                    fast_tanh(x)
                } else {
                    fast_tanh(0.5 * x) * 1.4
                }
            }
        }
    }
}

/// Drive 0..1 maps to 0..36 dB of gain into the shaper.
pub fn drive_gain(drive: f32) -> f32 {
    (drive * 36.0 / 20.0 * std::f32::consts::LN_10).exp()
}

pub struct Distortion {
    on: Enable,
    drive: Smooth,
    mix: Smooth,
    /// DC blocker state per channel: (x1, y1)
    dc: [(f32, f32); 2],
}

impl Distortion {
    pub fn new(sr: f32) -> Self {
        Self { on: Enable::new(sr), drive: Smooth::new(0.0, sr), mix: Smooth::new(1.0, sr), dc: [(0.0, 0.0); 2] }
    }

    pub fn clear(&mut self) {
        self.dc = [(0.0, 0.0); 2];
    }

    pub fn process(&mut self, on: bool, mode: DistMode, drive: f32, mix: f32, left: &mut [f32], right: &mut [f32]) {
        match self.on.begin(on) {
            Status::Run => {}
            Status::Clear => return self.clear(),
            Status::Skip => return,
        }
        for (l, r) in left.iter_mut().zip(right.iter_mut()) {
            let g = drive_gain(self.drive.next(drive));
            let amount = self.mix.next(mix) * self.on.next(on);
            // rough loudness compensation so turning drive up doesn't just make it louder
            let comp = g.powf(-0.5).max(0.3);
            for (c, x) in [l, r].into_iter().enumerate() {
                let wet = mode.shape(*x * g) * comp;
                // DC blocker (asymmetric shaping adds offset)
                let (x1, y1) = self.dc[c];
                let y = wet - x1 + 0.995 * y1;
                self.dc[c] = (wet, y);
                *x += (y - *x) * amount;
            }
        }
    }
}
