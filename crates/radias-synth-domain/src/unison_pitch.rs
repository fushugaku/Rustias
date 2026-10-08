//! Original Unison detune coefficient compiler, Master DAB4..DB5C.
use crate::{
    fixed::{high_product, multiply_q15, saturate},
    pitch::PhaseIncrement,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnisonDetuneTable {
    /// Master words 4788..478C; the interpolation's zero index is word 478A.
    pub coefficients: [i16; 5],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnisonPitch {
    pub increments: [PhaseIncrement; 5],
    /// The original sixth coefficient is the center of two averaged increments.
    pub averaging_center: PhaseIncrement,
}

impl UnisonDetuneTable {
    pub const ORIGINAL: Self = Self {
        coefficients: [0x1ea4, 0x1f50, 0x2000, 0x20b3, 0x216a],
    };
    fn interpolate(&self, coordinate: i64) -> i16 {
        let index = (coordinate >> 25) as i32 + 2;
        let fraction = (coordinate >> 10) & 32767;
        let left = i64::from(self.coefficients[index as usize]);
        let right = i64::from(self.coefficients[index as usize + 1]);
        (left + (((right - left) * fraction * 2) >> 16)) as i16
    }

    pub fn compile(&self, increment: PhaseIncrement, detune: u16) -> Option<UnisonPitch> {
        if detune > 32767 {
            return None;
        }
        let positive = self.interpolate(i64::from(detune) << 11);
        let negative = self.interpolate(high_product(detune as i16, 0x8290u16 as i16) >> 5);
        let positive = multiply_q15(increment.0 as i32, positive) << 2;
        let negative = multiply_q15(increment.0 as i32, negative) << 2;
        let center_gain = 0x4000 + (detune >> 5);
        let center = multiply_q15(increment.0 as i32, center_gain as i16) << 1;
        // The saturated memory stores do not narrow the original accumulator
        // inputs to the following averages. Keep those guard bits until storing.
        Some(UnisonPitch {
            increments: [
                increment,
                PhaseIncrement(saturate(positive) as u32),
                PhaseIncrement(saturate(negative) as u32),
                PhaseIncrement(saturate((positive + center) >> 1) as u32),
                PhaseIncrement(saturate((negative + center) >> 1) as u32),
            ],
            averaging_center: PhaseIncrement(saturate(center) as u32),
        })
    }
}

/// Original Unison bandwidth lookup and Pulse coefficient, DA89..DAAE.
pub fn unison_bandwidth(increment: PhaseIncrement) -> (i16, i16) {
    let index = increment.0.min(i32::MAX as u32) >> 24;
    let gain = if index < 2 {
        32767
    } else {
        (32768 / index) as i16
    };
    let square = high_product(gain, gain);
    let shaped = high_product((square >> 16) as i16, 0x7333) + high_product(gain, 0x0ccc);
    (gain, (saturate(shaped) >> 16) as i16)
}

/// Original per-note/phase-edit initialization, DB60..DBD7.
pub fn unison_phases(control2_code: u16, triangle: bool) -> Option<[crate::Phase; 5]> {
    if control2_code > 32767 {
        return None;
    }
    let product = high_product(control2_code as i16, 0x6666) as i32;
    let bias: u32 = if triangle { 0xc000_0000 } else { 0 };
    Some(
        [0, -product, (-product) >> 1, product, product >> 1]
            .map(|offset| crate::Phase(bias.wrapping_add(offset as u32))),
    )
}
