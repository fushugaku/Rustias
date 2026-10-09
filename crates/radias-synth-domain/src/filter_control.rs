//! Original filter coefficient preparation, DDD8..DE9D.
use crate::fixed::{high_product, multiply_q31, saturate};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterControlCoefficients {
    pub feedback: i32,
    pub integrator_gain: i32,
    pub post_gain: i16,
    pub post_feedback: i16,
}

pub fn compile(frequency: i32, resonance: i32, normalization: i32) -> FilterControlCoefficients {
    let excess = (frequency as i64 - (20866i64 << 16)).max(0);
    let squared = high_product((excess >> 16) as i16, (excess >> 16) as i16) as i32;
    let shaped = saturate((1i64 << 31) - (squared as i64 + (squared as i64 >> 1)));
    let damped = multiply_q31(resonance, shaped);
    let factor = ((damped >> 1) + (1i64 << 30)) as i32;
    let integrator_gain = saturate(multiply_q31(frequency, factor));
    let high = (integrator_gain >> 16) as i16;
    let post_value = saturate((high_product(21845, high) as i32 as i64) << 2);
    let attenuation = ((1i64 << 30) - integrator_gain as i64).max(0);
    // DF39/DE6f multiply memory's signed16 integrator high word by the
    // accumulator's signed17 high operand. Negative frequencies can make the
    // nonnegative attenuation exceed Q31; narrowing it to i16 wraps its guard
    // bit and corrupts the post-feedback coefficient.
    // This single memory/accumulator MPYM then narrows its product to signed32
    // before the following accumulator addition; the four-part Q31 products
    // above have a different wide-product boundary.
    let correction = ((attenuation >> 16) * i64::from(high) * 2) as i32;
    let post_feedback =
        (saturate(normalization as i64 - post_value as i64 + correction as i64) >> 16) as i16;
    let left = (damped - (1i64 << 31)) as i32;
    let right = ((frequency as i64 >> 1) - (1i64 << 31)) as i32;
    FilterControlCoefficients {
        feedback: saturate(multiply_q31(left, right)),
        integrator_gain,
        post_gain: (post_value >> 16) as i16,
        post_feedback,
    }
}

pub struct FilterMixTable {
    pub weights: [[i16; 128]; 5],
}
impl FilterMixTable {
    pub fn weights(&self, code: u16) -> [i16; 5] {
        let index = ((code >> 8) & 127) as usize;
        core::array::from_fn(|row| self.weights[row][index])
    }
}
