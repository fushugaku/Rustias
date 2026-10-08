//! Original oscillator mixer B472..B494.
use crate::{
    Sample,
    fixed::{high_product, multiply_q15, saturate},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OscillatorMix {
    pub primary_gain: i16,
    pub secondary_gain: i16,
    pub noise_gain: i16,
}

impl OscillatorMix {
    pub fn sample(self, primary: Sample, secondary: Sample, noise: i16, bias: i16) -> Sample {
        let noise_product = high_product(noise, self.noise_gain) as i32;
        Sample(saturate(
            multiply_q15(primary.0, self.primary_gain)
                + multiply_q15(secondary.0, self.secondary_gain)
                + noise_product as i64
                + ((bias as i64) << 16),
        ))
    }
}
