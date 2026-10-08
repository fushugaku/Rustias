//! Inactive physical voice tail, original A0EC..A142.
use crate::{
    Sample,
    fixed::{multiply_q15, saturate},
    pan::StereoFrame,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StereoCache {
    pub gain: i16,
    pub samples: StereoFrame,
}
impl StereoCache {
    /// E232..E23A: accepted transition delta plus signed Q15 -32767.
    pub fn set_decay(&mut self, delta: i16) {
        self.gain = (i32::from(delta) - 32767).max(i32::from(i16::MIN)) as i16;
    }
    pub fn initialize(&mut self, amplified: Sample, pan: i16, rate: i16) {
        self.set_decay(rate);
        let left_gain = (32767 - i32::from(pan)).min(32767) as i16;
        self.samples = StereoFrame {
            left: Sample(saturate(multiply_q15(amplified.0, left_gain) >> 4)),
            right: Sample(saturate(multiply_q15(amplified.0, pan) >> 4)),
        };
    }
    pub fn advance(&mut self, existing: StereoFrame) -> StereoFrame {
        let transition = |sample: Sample| {
            let first = saturate(multiply_q15(sample.0, self.gain));
            Sample(saturate(multiply_q15(first, -32767)))
        };
        self.samples = StereoFrame {
            left: transition(self.samples.left),
            right: transition(self.samples.right),
        };
        StereoFrame {
            left: Sample(saturate(
                existing.left.0 as i64 + self.samples.left.0 as i64,
            )),
            right: Sample(saturate(
                existing.right.0 as i64 + self.samples.right.0 as i64,
            )),
        }
    }
}
