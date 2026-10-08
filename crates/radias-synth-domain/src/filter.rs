//! Original resonant filter core, Master B4C0..B57F.
use crate::{
    Sample,
    fixed::{multiply_q15, multiply_q31, saturate, weighted_sum},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterCoefficients {
    pub input_gain: i16,
    pub feedback: i32,
    pub integrator_gain: i32,
    pub post_gain: i16,
    pub post_feedback: i16,
    pub mix: [i16; 5],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterState {
    pub first: i32,
    pub second: i32,
    pub post: [i32; 2],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResonantFilter {
    pub state: FilterState,
}

impl ResonantFilter {
    pub fn next_sample(&mut self, input: Sample, c: FilterCoefficients) -> Sample {
        let dry = input.0;
        let first_drive = saturate(
            multiply_q15(dry, c.input_gain)
                - 2 * multiply_q31(self.state.first, c.feedback)
                - self.state.second as i64,
        );
        let first =
            saturate(2 * multiply_q31(first_drive, c.integrator_gain) + self.state.first as i64);
        let second =
            saturate(2 * multiply_q31(first, c.integrator_gain) + self.state.second as i64);
        let post_first = saturate(weighted_sum(
            [second, self.state.post[0]],
            [c.post_gain, c.post_feedback],
        ));
        let post_second = saturate(weighted_sum(
            [post_first, self.state.post[1]],
            [c.post_gain, c.post_feedback],
        ));
        self.state = FilterState {
            first,
            second,
            post: [post_first, post_second],
        };
        Sample(saturate(weighted_sum(
            [post_second, first_drive, dry, second, first],
            c.mix,
        )))
    }
}
