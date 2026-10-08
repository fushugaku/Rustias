//! Original B5A0/B884/BB80 two-filter graphs with disabled waveshaper.
use crate::{
    Sample,
    filter::{FilterCoefficients, ResonantFilter},
    fixed::{multiply_q15, multiply_q31, saturate},
    mixer::OscillatorMix,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter2State {
    pub first: i32,
    pub second: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter2Coefficients {
    pub input_gain: i16,
    pub feedback: i32,
    pub integrator_gain: i32,
    pub output: Filter2Output,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter2Output {
    LowPass,
    HighPass,
    BandPass,
    Comb,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Filter2 {
    pub state: Filter2State,
}
impl Filter2 {
    pub fn sample(&mut self, input: Sample, c: Filter2Coefficients) -> Sample {
        let drive = saturate(
            multiply_q15(input.0, c.input_gain)
                - 2 * multiply_q31(self.state.first, c.feedback)
                - self.state.second as i64,
        );
        let first = saturate(2 * multiply_q31(drive, c.integrator_gain) + self.state.first as i64);
        let second =
            saturate(2 * multiply_q31(first, c.integrator_gain) + self.state.second as i64);
        self.state = Filter2State { first, second };
        Sample(match c.output {
            Filter2Output::LowPass => second,
            Filter2Output::HighPass => drive,
            Filter2Output::BandPass => first,
            Filter2Output::Comb => panic!("Comb requires its delay memory"),
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterRouting {
    Serial,
    Parallel,
    Individual,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualFilterParameters {
    pub route: FilterRouting,
    pub first: FilterCoefficients,
    pub second: Filter2Coefficients,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DualFilterGraph {
    pub first: ResonantFilter,
    pub second: Filter2,
}
pub struct Filter2Frame<'a> {
    pub coefficients: Filter2Coefficients,
    pub comb: Option<&'a mut crate::comb::Comb>,
}
impl DualFilterGraph {
    pub fn sample(
        &mut self,
        route: FilterRouting,
        mix: OscillatorMix,
        first: FilterCoefficients,
        second: Filter2Coefficients,
        inputs: (Sample, Sample, i16, i16),
    ) -> Sample {
        self.sample_with_prefilter(route, mix, first, second, inputs, |x| x)
    }

    pub fn sample_with_prefilter(
        &mut self,
        route: FilterRouting,
        mix: OscillatorMix,
        first: FilterCoefficients,
        second: Filter2Coefficients,
        inputs: (Sample, Sample, i16, i16),
        transform: impl FnMut(Sample) -> Sample,
    ) -> Sample {
        self.sample_with_prefilter_and_comb(
            route,
            mix,
            first,
            Filter2Frame {
                coefficients: second,
                comb: None,
            },
            inputs,
            transform,
        )
    }
    pub fn sample_with_prefilter_and_comb(
        &mut self,
        route: FilterRouting,
        mix: OscillatorMix,
        first: FilterCoefficients,
        mut second: Filter2Frame<'_>,
        inputs: (Sample, Sample, i16, i16),
        mut transform: impl FnMut(Sample) -> Sample,
    ) -> Sample {
        let (primary, secondary, noise, bias) = inputs;
        let mixed = mix.sample(primary, secondary, noise, bias);
        let mut filter2 = |input| {
            if second.coefficients.output == Filter2Output::Comb {
                second
                    .comb
                    .as_deref_mut()
                    .expect("Comb delay memory configured")
                    .sample(
                        input,
                        second.coefficients.feedback,
                        second.coefficients.integrator_gain as u32,
                    )
            } else {
                self.second.sample(input, second.coefficients)
            }
        };
        match route {
            FilterRouting::Serial => filter2(self.first.next_sample(transform(mixed), first)),
            FilterRouting::Parallel => {
                let input = transform(mixed);
                let a = self.first.next_sample(input, first);
                // The original fork retains the unshaped mix for Filter2.
                let b = filter2(mixed);
                Sample(saturate((a.0 as i64 + b.0 as i64) >> 1))
            }
            FilterRouting::Individual => {
                let a = self.first.next_sample(
                    transform(Sample(saturate(
                        multiply_q15(primary.0, mix.primary_gain) + ((bias as i64) << 16),
                    ))),
                    first,
                );
                let b = filter2(
                    OscillatorMix {
                        primary_gain: 0,
                        ..mix
                    }
                    .sample(primary, secondary, noise, bias),
                );
                Sample(saturate(a.0 as i64 + b.0 as i64))
            }
        }
    }
}
