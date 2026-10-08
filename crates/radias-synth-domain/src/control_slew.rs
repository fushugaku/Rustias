//! Original control interpolation, Master A238..A2F6.
use crate::filter::FilterCoefficients;
use crate::fixed::{saturate, weighted_sum};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlewWeights {
    pub target: i16,
    pub memory: i16,
}
impl SlewWeights {
    pub fn word(self, current: i16, target: i16) -> i16 {
        (saturate(2 * (target as i64 * self.target as i64 + current as i64 * self.memory as i64))
            >> 16) as i16
    }
    pub fn wide(self, current: i32, target: i32) -> i32 {
        saturate(weighted_sum([target, current], [self.target, self.memory]))
    }
    pub fn filter(self, current: &mut FilterCoefficients, target: FilterCoefficients) {
        current.input_gain = self.word(current.input_gain, target.input_gain);
        current.feedback = self.wide(current.feedback, target.feedback);
        current.integrator_gain = self.wide(current.integrator_gain, target.integrator_gain);
        current.post_gain = self.word(current.post_gain, target.post_gain);
        current.post_feedback = self.word(current.post_feedback, target.post_feedback);
        for (current, target) in current.mix.iter_mut().zip(target.mix) {
            *current = self.word(*current, target);
        }
    }
    pub fn mixer(
        self,
        current: &mut crate::mixer::OscillatorMix,
        target: crate::mixer::OscillatorMix,
    ) {
        current.primary_gain = self.word(current.primary_gain, target.primary_gain);
        current.secondary_gain = self.word(current.secondary_gain, target.secondary_gain);
        current.noise_gain = self.word(current.noise_gain, target.noise_gain);
    }
}
