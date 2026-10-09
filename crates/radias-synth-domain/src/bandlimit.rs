//! Original oscillator bandwidth coefficients, D63C..D68E.
use crate::{
    fixed::{high_product, saturate},
    pitch::{PhaseIncrement, PitchCode},
};

pub struct BandwidthTable {
    pub gains: [i16; 129],
}

/// Interpolate the original two signed ROM gain words. The scalar MAC narrows
/// its product before the accumulator addition, including the17-bit delta.
pub fn interpolate_words(left: i16, right: i16, fraction: u16) -> i16 {
    let left = i64::from(left) << 16;
    let delta = (i64::from(right) << 16) - left;
    let product = ((delta >> 16) * i64::from(fraction & 32767) * 2) as i32;
    ((left + i64::from(product)) >> 16) as i16
}

impl BandwidthTable {
    pub fn coefficient(&self, increment: PhaseIncrement) -> i16 {
        let phase = increment.0.min(i32::MAX as u32);
        let index = (phase >> 24) as usize;
        let fraction = (phase >> 9) & 32767;
        interpolate_words(self.gains[index], self.gains[index + 1], fraction as u16)
    }
}

pub fn edge_coefficient(pitch: PitchCode, sixth_power: bool) -> i16 {
    let difference = 32767 - pitch.raw() as i32;
    let product = high_product(difference as i16, 20480) as i32;
    let root = saturate(((product as i64) << 1) + (7372i64 << 16));
    let square = |value: i32| high_product((value >> 16) as i16, (value >> 16) as i16) as i32;
    let second = square(root);
    let fourth = square(second);
    let result = if sixth_power {
        let fifth = high_product((root >> 16) as i16, (fourth >> 16) as i16) as i32;
        high_product((fifth >> 16) as i16, (root >> 16) as i16) as i32
    } else {
        square(fourth)
    };
    (result >> 16) as i16
}
