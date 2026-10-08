//! Noise/Formant pitch coefficients, original Master DBDF..DD92.
use crate::{
    fixed::{multiply_q15, saturate},
    pitch::{PhaseIncrement, PitchCode},
};

/// Immutable Noise curve scale words at original Master 484B.
pub struct NoisePitchTable {
    pub curve_scales: [i16; 128],
}

impl NoisePitchTable {
    pub fn curve_scale(&self, pitch: PitchCode) -> i16 {
        self.curve_scales[(pitch.raw() >> 8) as usize]
    }
}

/// Formant's frequency follows the compiled primary phase increment. CTRL2 is
/// a separate smoothed input to the excitation path.
pub fn formant_frequency(increment: PhaseIncrement) -> i16 {
    let frequency = saturate(multiply_q15(increment.0 as i32, 0x63d7) * 4);
    (frequency.min(0x7eb8_0000) >> 16) as i16
}
