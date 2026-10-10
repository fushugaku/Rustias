//! Original per-voice stereo routing A200..A222, before final bus scaling.
use crate::{
    Sample,
    fixed::{multiply_q15, saturate},
};

// The original four hardware buses are unchanged in every native build.
pub const TIMBRE_BUSES: usize = if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
    8
} else {
    4
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PanSmoother {
    pub current: i16,
    pub target: i16,
}
impl PanSmoother {
    /// Original A1E4..A200 retains the high word for the next audio sample.
    pub fn next(&mut self, weights: crate::control_slew::SlewWeights) -> i32 {
        let position = saturate(
            crate::fixed::high_product(self.target, weights.target)
                + crate::fixed::high_product(self.current, weights.memory),
        );
        self.current = (position >> 16) as i16;
        position
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StereoFrame {
    pub left: Sample,
    pub right: Sample,
}

/// One of the four stereo pairs in a processor's eight-channel output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VoiceBus(u8);
impl VoiceBus {
    pub fn new(index: u8) -> Option<Self> {
        ((index as usize) < TIMBRE_BUSES).then_some(Self(index))
    }
    pub fn from_offsets(left: u32, right: u32) -> Option<Self> {
        if left <= 12 && left.is_multiple_of(4) && right == left + 2 {
            Some(Self((left / 4) as u8))
        } else {
            None
        }
    }
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

pub fn route(sample: Sample, position: i32, existing: StereoFrame) -> StereoFrame {
    let left = ((0x7fb9_0000i64 - position as i64).max(0) >> 20) as i16;
    let right = (position >> 20) as i16;
    StereoFrame {
        left: Sample(saturate(
            multiply_q15(sample.0, left) + existing.left.0 as i64,
        )),
        right: Sample(saturate(
            multiply_q15(sample.0, right) + existing.right.0 as i64,
        )),
    }
}
