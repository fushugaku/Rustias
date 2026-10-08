//! Original phase-driven Noise and recursive Formant generators, Master C0A8/C1A8.
use crate::{
    Sample,
    fixed::{high_product, multiply_q15, saturate},
};

/// Per-voice frame22 state; Master A19A..A1B1 also commits the current
/// output to frame28. Its low fraction is stored but not fed back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MixerNoise {
    pub state: i32,
}

/// The original Master initializes three separate banks of twelve physical
/// frame states from two accepted boot input words, E0D4..E13E.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoiseFrameSeeds {
    pub primary: [crate::Phase; 12],
    pub secondary: [crate::Phase; 12],
    pub mixer: [MixerNoise; 12],
}

impl NoiseFrameSeeds {
    pub fn from_inputs(first: i16, second: i16) -> Self {
        let mut state = (first as u16).wrapping_add(second as u16);
        let mut next = || {
            state = state.wrapping_mul(5).wrapping_add(1);
            crate::Phase(u32::from(state) << 16)
        };
        let primary = core::array::from_fn(|_| next());
        state = state.wrapping_add(1);
        let mut next = || {
            state = state.wrapping_mul(5).wrapping_add(1);
            crate::Phase(u32::from(state) << 16)
        };
        let secondary = core::array::from_fn(|_| next());
        state = state.wrapping_add(1);
        let mixer = core::array::from_fn(|_| {
            state = state.wrapping_mul(5).wrapping_add(1);
            MixerNoise {
                state: (u32::from(state) << 16) as i32,
            }
        });
        Self {
            primary,
            secondary,
            mixer,
        }
    }
}
impl MixerNoise {
    pub fn next_word(&mut self, excitation_bias: i16) -> i16 {
        let next = (self.state >> 16) as i64 * 5 * 65536 + excitation_bias as i64 * 256;
        self.state = next as i32;
        (self.state >> 16) as i16
    }
}

/// Original unsigned low sample half followed by signed high/gain product.
#[inline]
fn signed_gain(sample: i32, gain: i16) -> i64 {
    multiply_q15(sample, gain)
}

#[inline]
fn folded_quadratic(value: i64, limits: [i32; 2]) -> Sample {
    let current = saturate(value);
    let bound = if current < 0 { limits[1] } else { limits[0] };
    let offset = current as i64 - bound as i64;
    let stored = saturate(offset);
    let magnitude = (saturate((offset | 1).abs()) >> 16) as i16;
    Sample(saturate(signed_gain(stored, magnitude) + bound as i64))
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoiseFilterState {
    pub first: i32,
    pub second: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ColoredNoiseParameters {
    pub phase_gain: i16,
    pub seed_bias: i16,
    pub curve_scale: i16,
    pub phase_offset: i16,
    pub curve_limit: i32,
    pub feedback: i16,
    pub limits: [i32; 2],
}
impl NoiseFilterState {
    pub fn colored(&mut self, phase: u32, p: ColoredNoiseParameters) -> Sample {
        let shifted = phase.wrapping_add((p.phase_offset as i32 as u32) << 16) as i32;
        let phase = phase as i32;
        let first_curve = saturate(
            signed_gain(
                saturate(p.curve_limit as i64 - (phase as i64 | 1).abs()),
                p.curve_scale,
            ) << 8,
        );
        let second_curve = saturate(
            signed_gain(
                saturate(p.curve_limit as i64 - (shifted as i64 | 1).abs()),
                p.curve_scale,
            ) << 8,
        );
        let difference = signed_gain(first_curve, (phase as u32 >> 16) as i16)
            - signed_gain(second_curve, (shifted as u32 >> 16) as i16);
        let excitation = saturate(
            signed_gain(saturate(difference), p.phase_gain) + ((p.seed_bias as i64) << 16),
        )
        .max(0);
        let gain = (signed_gain(excitation, (excitation as u32 >> 16) as i16) >> 16) as i16;
        let driven = (difference >> 2) - signed_gain(self.first, p.feedback) - self.second as i64;
        self.first = saturate(self.first as i64 + signed_gain(saturate(driven), gain));
        self.second = saturate(self.second as i64 + signed_gain(self.first, gain));
        folded_quadratic(driven + signed_gain(self.second, p.feedback), p.limits)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormantState {
    pub counter: i16,
    pub filter: NoiseFilterState,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormantParameters {
    pub seed_gain: i16,
    pub seed_bias: i16,
    pub frequency: i16,
    pub input_gain: i16,
    pub feedback: i16,
    pub limits: [i32; 2],
}
impl FormantState {
    pub fn sample(&mut self, p: FormantParameters, excitation_bias: i16) -> Sample {
        let counter = self.counter as i64 * 5 * 65536 + excitation_bias as i64 * 256;
        self.counter = (counter >> 16) as i16;
        let driven = high_product(p.input_gain, self.counter)
            + signed_gain(self.filter.first, p.feedback)
            - self.filter.second as i64;
        self.filter.first =
            saturate(self.filter.first as i64 + signed_gain(saturate(driven), p.frequency));
        self.filter.second =
            saturate(self.filter.second as i64 + signed_gain(self.filter.first, p.frequency));
        let first = signed_gain(saturate(driven), p.seed_bias);
        let gain = ((p.limits[0] >> 16) as i16).wrapping_sub(p.seed_bias);
        let combined = first + signed_gain(self.filter.second, gain);
        let stored = saturate(combined);
        // The original low product is signed in its coefficient; the high
        // product is also signed, and the old wide result joins before shifting.
        let low = (stored as u32 & 65535) as i64 * p.seed_gain as i64 * 2;
        let shaped = ((combined + low) >> 16) + high_product((stored >> 16) as i16, p.seed_gain);
        folded_quadratic(shaped << 3, p.limits)
    }
}
