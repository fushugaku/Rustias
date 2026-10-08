//! Original oscillator bandwidth coefficients, D63C..D68E.
use crate::{
    fixed::{high_product, saturate},
    pitch::{PhaseIncrement, PitchCode},
};

pub struct BandwidthTable {
    pub gains: [i16; 129],
}

impl BandwidthTable {
    pub fn coefficient(&self, increment: PhaseIncrement) -> i16 {
        let phase = increment.0.min(i32::MAX as u32);
        let index = (phase >> 24) as usize;
        let fraction = (phase >> 9) & 32767;
        let left = (self.gains[index] as i64) << 16;
        let delta = ((self.gains[index + 1] as i64) << 16) - left;
        let product = ((delta >> 16) * fraction as i64 * 2) as i32;
        ((left + product as i64) >> 16) as i16
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
