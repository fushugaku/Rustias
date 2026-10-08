//! Original primary VA generators, Master BE88..CB8E.
use crate::{
    Phase, Sample,
    fixed::{high_product, multiply_q15, saturate},
    pitch::PhaseIncrement,
    waveform::WaveformTable,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryRampParameters {
    pub increment: PhaseIncrement,
    pub shape: i16,
    pub blend: i16,
    pub offset_target: i16,
    pub target_gain: i16,
    pub memory_gain: i16,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimaryRampOscillator {
    pub phase: Phase,
    pub offset: i16,
}

fn curve(table: &WaveformTable, phase: i32, shape: i16) -> i64 {
    let product = ((phase >> 16) as i64 * shape as i64 * 2) as i32;
    let coordinate = saturate((product as i64) << 7);
    table.correction_at(coordinate) + phase as i64
}

impl PrimaryRampOscillator {
    pub fn next_sample(&mut self, table: &WaveformTable, p: PrimaryRampParameters) -> Sample {
        self.phase.retreat(p.increment);
        let phase = self.phase.0 as i32;
        let shifted = phase.wrapping_add((self.offset as i32) << 16);
        let first = curve(table, phase, p.shape) as i32;
        let second = saturate(curve(table, shifted, p.shape));
        let target = high_product(p.offset_target, p.target_gain) as i32;
        let memory = high_product(self.offset, p.memory_gain) as i32;
        self.offset = (saturate(target as i64 + memory as i64) >> 16) as i16;
        Sample(saturate(
            (multiply_q15(second, p.blend) + first as i64) >> 1,
        ))
    }

    /// Original primary pulse, BF1C..BFC2. Width smoothing is the same
    /// state transition as the ramp; the phase offsets and output polarity differ.
    pub fn next_pulse(&mut self, table: &WaveformTable, p: PrimaryRampParameters) -> Sample {
        self.phase.retreat(p.increment);
        let phase = self.phase.0 as i32;
        let width = saturate(high_product(self.offset, 16392) * 2);
        let first = curve(table, phase.wrapping_add(0x7fff0000), p.shape) as i32;
        let second = saturate(curve(table, phase.wrapping_add(width), p.shape));
        let target = high_product(p.offset_target, p.target_gain) as i32;
        let memory = high_product(self.offset, p.memory_gain) as i32;
        self.offset = (saturate(target as i64 + memory as i64) >> 16) as i16;
        Sample(saturate(
            -((multiply_q15(second, p.blend) + first as i64) >> 1),
        ))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryTriangleParameters {
    pub increment: PhaseIncrement,
    pub center: i16,
    pub gain: i16,
    pub edge_gain: i16,
    pub upper: i16,
    pub upper_reflection: i16,
    pub lower: i16,
    pub lower_reflection: i16,
}

/// Original primary folded triangle, BFC4..C016.
pub fn triangle_sample(phase: &mut Phase, p: PrimaryTriangleParameters) -> Sample {
    phase.retreat(p.increment);
    let folded = (p.center as i64 * 65536 - ((phase.0 as i32 | 1) as i64).abs()) as i32;
    let mut output =
        saturate(multiply_q15(folded, p.gain) + 2 * multiply_q15(folded, p.edge_gain)) as i64;
    if output - p.upper as i64 * 65536 >= 0 {
        output = p.upper_reflection as i64 * 65536 - output;
    }
    output = saturate(output) as i64;
    if p.lower as i64 * 65536 - output >= 0 {
        output = p.lower_reflection as i64 * 65536 - output;
    }
    Sample(saturate(output * 3))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimarySineParameters {
    pub increment: PhaseIncrement,
    pub modulation_gain: i16,
    pub control: [i16; 2],
    pub center: i16,
    pub polynomial: [i32; 3],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimarySineOscillator {
    pub phase: Phase,
    pub modulated_phase: Phase,
    pub control_feedback: i16,
}

impl PrimarySineOscillator {
    /// Original primary phase-modulated polynomial sine, C018..C0A6.
    pub fn next_sample(&mut self, p: PrimarySineParameters) -> Sample {
        self.phase.retreat(p.increment);
        self.control_feedback = (saturate(high_product(p.control[0], p.control[1])) >> 16) as i16;
        let folded = (p.center as i64 * 65536 - ((self.phase.0 as i32 | 1) as i64).abs()) as i32;
        let step = saturate(multiply_q15(folded, p.modulation_gain) + p.increment.0 as i32 as i64);
        self.modulated_phase.0 = self.modulated_phase.0.wrapping_add(step as u32);
        let centered = saturate(
            2 * (p.center as i64 * 65536 - ((self.modulated_phase.0 as i32 | 1) as i64).abs()),
        );
        let square = (high_product((centered >> 16) as i16, (centered >> 16) as i16) >> 16) as i16;
        let first = saturate(multiply_q15(p.polynomial[0], square) + p.polynomial[1] as i64);
        let second = (multiply_q15(first, square) + p.polynomial[2] as i64) as i32;
        Sample(saturate(
            -((multiply_q15(second, (centered >> 16) as i16) + centered as i64) | 1),
        ))
    }
}

/// Original Sine pitch coefficient, Master D88D..D89C. The DSP shifts the
/// increment four bits, saturates the result, and caps it at Q30 one before
/// storing its high word. This is independent of CONTROL 1's modulation depth.
pub fn sine_pitch_coefficient(increment: PhaseIncrement) -> i16 {
    ((u64::from(increment.0) * 16).min(0x4000_0000) >> 16) as i16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryParameters {
    Ramp(PrimaryRampParameters),
    Pulse(PrimaryRampParameters),
    Triangle(PrimaryTriangleParameters),
    Sine(PrimarySineParameters),
    Noise(PrimaryNoiseParameters),
    Formant(PrimaryFormantParameters),
    Cross(PrimaryCrossParameters),
    CrossTriangle(PrimaryCrossTriangleParameters),
    CrossSine(PrimaryCrossSineParameters),
    Unison(crate::unison::UnisonParameters),
    UnisonCarrier(PrimaryUnisonCarrierParameters),
    Vpm(PrimaryVpmParameters),
    VpmCarrier(PrimaryVpmCarrierParameters),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryNoiseParameters {
    pub increment: PhaseIncrement,
    pub generator: crate::noise::ColoredNoiseParameters,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryFormantParameters {
    pub increment: PhaseIncrement,
    pub generator: crate::noise::FormantParameters,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryVpmParameters {
    pub increment: PhaseIncrement,
    pub modulation_gain: i16,
    pub ratio: i16,
    pub limit: i16,
    pub center: i32,
    pub shape: i16,
    pub offset: i16,
    pub blend: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryVpmModulatorParameters {
    pub increment: PhaseIncrement,
    pub modulation_gain: i16,
    pub ratio: i16,
    pub limit: i16,
    pub center: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrimaryVpmCarrier {
    Triangle(PrimaryTriangleParameters),
    Sine { center: i16, polynomial: [i32; 3] },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryVpmCarrierParameters {
    pub modulator: PrimaryVpmModulatorParameters,
    pub carrier: PrimaryVpmCarrier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryCrossParameters {
    pub increment: PhaseIncrement,
    pub modulation_gain: i16,
    pub shape: i16,
    pub offset: i16,
    pub blend: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryCrossTriangleParameters {
    pub carrier: PrimaryTriangleParameters,
    pub modulation_gain: i16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryCrossSineParameters {
    pub increment: PhaseIncrement,
    pub modulation_gain: i16,
    pub center: i16,
    pub polynomial: [i32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrimaryUnisonCarrierParameters {
    pub parameters: crate::unison::UnisonParameters,
    pub waveform: crate::unison::UnisonWaveform,
}

impl PrimaryParameters {
    /// Original Noise/Formant initialization descriptors, SYS 0420e8/042120.
    /// Note pitch and controller targets are compiled separately.
    pub fn noise_waveform(selection: u8, increment: PhaseIncrement) -> Option<Self> {
        Some(match selection {
            4 => Self::Noise(PrimaryNoiseParameters {
                increment,
                generator: crate::noise::ColoredNoiseParameters {
                    phase_gain: 0,
                    seed_bias: 0,
                    curve_scale: 0,
                    phase_offset: 0x6000,
                    feedback: 0x0ccc,
                    curve_limit: 0x7fff_7fff,
                    limits: [i32::MAX, i32::MIN],
                },
            }),
            5 => Self::Formant(PrimaryFormantParameters {
                increment,
                generator: crate::noise::FormantParameters {
                    seed_gain: 0,
                    seed_bias: 0,
                    frequency: crate::noise_control::formant_frequency(increment),
                    input_gain: 0,
                    feedback: 0,
                    limits: [i32::MAX, i32::MIN],
                },
            }),
            _ => return None,
        })
    }
    /// Original VPM initialization descriptors, SYS 0421e0..04223c.
    pub fn vpm_waveform(selection: u8, increment: PhaseIncrement, bandwidth: i16) -> Option<Self> {
        Some(match selection {
            0 | 1 => Self::Vpm(PrimaryVpmParameters {
                increment,
                modulation_gain: 0,
                ratio: 0,
                limit: 0x7f7c,
                center: i32::MAX,
                shape: bandwidth,
                offset: if selection == 0 { 0 } else { i16::MIN },
                blend: if selection == 0 { i16::MAX } else { i16::MIN },
            }),
            2 | 3 => Self::VpmCarrier(PrimaryVpmCarrierParameters {
                modulator: PrimaryVpmModulatorParameters {
                    increment,
                    modulation_gain: 0,
                    ratio: 0,
                    limit: 0x7f7c,
                    center: i32::MAX,
                },
                carrier: if selection == 2 {
                    match Self::waveform(2, increment, bandwidth)? {
                        Self::Triangle(p) => PrimaryVpmCarrier::Triangle(p),
                        _ => unreachable!(),
                    }
                } else {
                    PrimaryVpmCarrier::Sine {
                        center: 0x4000,
                        polynomial: [0x0932_c2b5, 0xadcf_3319u32 as i32, 0x4900_41ae],
                    }
                },
            }),
            _ => return None,
        })
    }
    pub fn unison_waveform(selection: u8, increment: PhaseIncrement) -> Option<Self> {
        use crate::unison::{UnisonParameters, UnisonWaveform};
        let mut parameters = UnisonParameters {
            increments: [increment; 5],
            detune: 0,
            correction_gain: 32767,
            level: 0x1999,
        };
        parameters.retune(increment);
        Some(match selection {
            0 => Self::Unison(parameters),
            1..=3 => Self::UnisonCarrier(PrimaryUnisonCarrierParameters {
                parameters,
                waveform: match selection {
                    1 => UnisonWaveform::Pulse {
                        bandwidth: crate::unison_pitch::unison_bandwidth(increment).1,
                    },
                    2 => UnisonWaveform::Triangle,
                    _ => UnisonWaveform::Sine {
                        normalization: i32::MAX,
                    },
                },
            }),
            _ => return None,
        })
    }
    /// Cross uses a distinct carrier generator for Triangle and Sine.
    pub fn cross_waveform(
        selection: u8,
        increment: PhaseIncrement,
        bandwidth: i16,
    ) -> Option<Self> {
        Some(match selection {
            0 | 1 => Self::Cross(PrimaryCrossParameters {
                increment,
                modulation_gain: 0,
                shape: bandwidth,
                offset: if selection == 0 { 0 } else { i16::MIN },
                blend: if selection == 0 { i16::MAX } else { i16::MIN },
            }),
            2 => {
                let Self::Triangle(carrier) = Self::waveform(2, increment, bandwidth)? else {
                    return None;
                };
                Self::CrossTriangle(PrimaryCrossTriangleParameters {
                    carrier,
                    modulation_gain: 0,
                })
            }
            3 => {
                let Self::Sine(sine) = Self::waveform(3, increment, bandwidth)? else {
                    return None;
                };
                Self::CrossSine(PrimaryCrossSineParameters {
                    increment,
                    modulation_gain: 0,
                    center: sine.center,
                    polynomial: sine.polynomial,
                })
            }
            _ => return None,
        })
    }

    /// Ordinary Waveform initialization from SYS 041608/042088..0420C4.
    /// These constants belong to the algorithm, not to an observed patch.
    pub fn waveform(selection: u8, increment: PhaseIncrement, bandwidth: i16) -> Option<Self> {
        Some(match selection {
            0 | 1 => {
                let ramp = PrimaryRampParameters {
                    increment,
                    shape: bandwidth,
                    blend: if selection == 0 { 0x7fff } else { i16::MIN },
                    offset_target: 0,
                    target_gain: 0x01d4,
                    memory_gain: 0x7e2d,
                };
                if selection == 0 {
                    Self::Ramp(ramp)
                } else {
                    Self::Pulse(ramp)
                }
            }
            2 => Self::Triangle(PrimaryTriangleParameters {
                increment,
                center: 0x4000,
                gain: 0x5555,
                edge_gain: 0,
                upper: 0x2aaa,
                upper_reflection: 0x5555,
                lower: 0xd555u16 as i16,
                lower_reflection: 0xaaaau16 as i16,
            }),
            3 => Self::Sine(PrimarySineParameters {
                increment,
                modulation_gain: 0,
                control: [sine_pitch_coefficient(increment), 0x6000],
                center: 0x4000,
                polynomial: [0x0932_c2b5, 0xadcf_3319u32 as i32, 0x4900_41ae],
            }),
            _ => return None,
        })
    }

    pub fn base_increment(self) -> PhaseIncrement {
        match self {
            Self::Ramp(p) | Self::Pulse(p) => p.increment,
            Self::Triangle(p) => p.increment,
            Self::Sine(p) => p.increment,
            Self::Noise(p) => p.increment,
            Self::Formant(p) => p.increment,
            Self::Cross(p) => p.increment,
            Self::CrossTriangle(p) => p.carrier.increment,
            Self::CrossSine(p) => p.increment,
            Self::Unison(p) => p.increments[0],
            Self::UnisonCarrier(p) => p.parameters.increments[0],
            Self::Vpm(p) => p.increment,
            Self::VpmCarrier(p) => p.modulator.increment,
        }
    }
    pub fn retune(&mut self, increment: PhaseIncrement, bandwidth: i16) {
        match self {
            Self::Ramp(p) | Self::Pulse(p) => {
                p.increment = increment;
                p.shape = bandwidth;
            }
            Self::Triangle(p) => p.increment = increment,
            Self::Noise(p) => p.increment = increment,
            Self::Formant(p) => p.increment = increment,
            Self::Sine(p) => {
                p.increment = increment;
                p.control[0] = sine_pitch_coefficient(increment);
            }
            Self::Cross(p) => {
                p.increment = increment;
                p.shape = bandwidth;
            }
            Self::CrossTriangle(p) => p.carrier.increment = increment,
            Self::CrossSine(p) => p.increment = increment,
            Self::Unison(p) => p.retune(increment),
            Self::UnisonCarrier(p) => {
                p.parameters.retune(increment);
                if let crate::unison::UnisonWaveform::Pulse { bandwidth } = &mut p.waveform {
                    *bandwidth = crate::unison_pitch::unison_bandwidth(increment).1;
                }
            }
            Self::Vpm(p) => {
                p.increment = increment;
                p.shape = bandwidth;
            }
            Self::VpmCarrier(p) => p.modulator.increment = increment,
        }
    }
}

/// State owned by one primary oscillator, independent of its selected waveform.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrimaryOscillator {
    pub phase: Phase,
    pub offset: i16,
    pub modulated_phase: Phase,
    pub control_feedback: i16,
    pub unison: crate::unison::UnisonOscillator,
    pub noise: crate::noise::NoiseFilterState,
    pub formant: crate::noise::FormantState,
}

impl From<PrimaryRampOscillator> for PrimaryOscillator {
    fn from(value: PrimaryRampOscillator) -> Self {
        Self {
            phase: value.phase,
            offset: value.offset,
            ..Self::default()
        }
    }
}

impl PrimaryOscillator {
    pub fn next_sample(&mut self, table: &WaveformTable, p: PrimaryParameters) -> Sample {
        self.next_with_modulator(table, p, Sample(0))
    }
    pub fn next_with_modulator(
        &mut self,
        table: &WaveformTable,
        p: PrimaryParameters,
        modulator: Sample,
    ) -> Sample {
        self.next_with_modulator_and_bias(table, p, modulator, 0)
    }
    pub fn next_with_modulator_and_bias(
        &mut self,
        table: &WaveformTable,
        p: PrimaryParameters,
        modulator: Sample,
        excitation_bias: i16,
    ) -> Sample {
        match p {
            PrimaryParameters::Ramp(p) => self.ramp_sample(table, p, false),
            PrimaryParameters::Pulse(p) => self.ramp_sample(table, p, true),
            PrimaryParameters::Triangle(p) => triangle_sample(&mut self.phase, p),
            PrimaryParameters::Cross(p) => {
                self.phase.retreat(p.increment);
                self.phase.0 = self
                    .phase
                    .0
                    .wrapping_add(multiply_q15(modulator.0, p.modulation_gain) as u32);
                let first = curve(table, self.phase.0 as i32, p.shape) as i32;
                let shifted = self.phase.0.wrapping_add((p.offset as i32 as u32) << 16);
                let second = saturate(curve(table, shifted as i32, p.shape));
                Sample(saturate(
                    (multiply_q15(second, p.blend) + first as i64) >> 1,
                ))
            }
            PrimaryParameters::CrossTriangle(p) => {
                self.phase.retreat(p.carrier.increment);
                self.phase.0 = self
                    .phase
                    .0
                    .wrapping_add(multiply_q15(modulator.0, p.modulation_gain) as u32);
                triangle_sample(
                    &mut self.phase,
                    PrimaryTriangleParameters {
                        increment: PhaseIncrement(0),
                        edge_gain: 0,
                        ..p.carrier
                    },
                )
            }
            PrimaryParameters::CrossSine(p) => {
                self.phase.retreat(p.increment);
                self.phase.0 = self
                    .phase
                    .0
                    .wrapping_add(multiply_q15(modulator.0, p.modulation_gain) as u32);
                let centered = saturate(
                    2 * (p.center as i64 * 65536 - ((self.phase.0 as i32 | 1) as i64).abs()),
                );
                let square =
                    (high_product((centered >> 16) as i16, (centered >> 16) as i16) >> 16) as i16;
                let first =
                    saturate(multiply_q15(p.polynomial[0], square) + p.polynomial[1] as i64);
                let second = (multiply_q15(first, square) + p.polynomial[2] as i64) as i32;
                Sample(saturate(
                    multiply_q15(second, (centered >> 16) as i16) + centered as i64,
                ))
            }
            PrimaryParameters::Unison(p) => {
                let sample = self.unison.next_sample(p);
                self.phase = self.unison.phases[0];
                sample
            }
            PrimaryParameters::UnisonCarrier(p) => {
                let sample = self.unison.next_sample_waveform(p.parameters, p.waveform);
                self.phase = self.unison.phases[0];
                sample
            }
            PrimaryParameters::Vpm(p) => {
                self.phase.retreat(p.increment);
                let step = saturate(multiply_q15(p.increment.0 as i32, p.ratio) << 6)
                    .min((p.limit as i32) << 16);
                self.modulated_phase.retreat(PhaseIncrement(step as u32));
                let folded = p.center as i64 - ((self.modulated_phase.0 as i32 | 1) as i64).abs();
                let modulator = saturate(
                    multiply_q15(self.modulated_phase.0 as i32, (folded >> 16) as i16) << 2,
                );
                self.phase.0 = self
                    .phase
                    .0
                    .wrapping_add(multiply_q15(modulator, p.modulation_gain) as u32);
                let first = curve(table, self.phase.0 as i32, p.shape) as i32;
                let shifted = self.phase.0.wrapping_add((p.offset as i32 as u32) << 16);
                let second = saturate(curve(table, shifted as i32, p.shape));
                Sample(saturate(
                    (multiply_q15(second, p.blend) + first as i64) >> 1,
                ))
            }
            PrimaryParameters::VpmCarrier(p) => {
                let m = p.modulator;
                self.phase.retreat(m.increment);
                let step = saturate(multiply_q15(m.increment.0 as i32, m.ratio) << 6)
                    .min((m.limit as i32) << 16);
                self.modulated_phase.retreat(PhaseIncrement(step as u32));
                let folded = m.center as i64 - ((self.modulated_phase.0 as i32 | 1) as i64).abs();
                let modulator = saturate(
                    multiply_q15(self.modulated_phase.0 as i32, (folded >> 16) as i16) << 2,
                );
                self.phase.0 = self
                    .phase
                    .0
                    .wrapping_add(multiply_q15(modulator, m.modulation_gain) as u32);
                match p.carrier {
                    PrimaryVpmCarrier::Triangle(carrier) => triangle_sample(
                        &mut self.phase,
                        PrimaryTriangleParameters {
                            increment: PhaseIncrement(0),
                            edge_gain: 0,
                            ..carrier
                        },
                    ),
                    PrimaryVpmCarrier::Sine { center, polynomial } => {
                        let centered = saturate(
                            2 * (center as i64 * 65536 - ((self.phase.0 as i32 | 1) as i64).abs()),
                        );
                        let square =
                            (high_product((centered >> 16) as i16, (centered >> 16) as i16) >> 16)
                                as i16;
                        let first =
                            saturate(multiply_q15(polynomial[0], square) + polynomial[1] as i64);
                        let second = (multiply_q15(first, square) + polynomial[2] as i64) as i32;
                        Sample(saturate(
                            multiply_q15(second, (centered >> 16) as i16) + centered as i64,
                        ))
                    }
                }
            }
            PrimaryParameters::Sine(p) => {
                let mut sine = PrimarySineOscillator {
                    phase: self.phase,
                    modulated_phase: self.modulated_phase,
                    control_feedback: self.control_feedback,
                };
                let output = sine.next_sample(p);
                self.phase = sine.phase;
                self.modulated_phase = sine.modulated_phase;
                self.control_feedback = sine.control_feedback;
                output
            }
            PrimaryParameters::Noise(p) => {
                self.phase.retreat(p.increment);
                self.noise.colored(self.phase.0, p.generator)
            }
            PrimaryParameters::Formant(p) => {
                self.phase.retreat(p.increment);
                self.formant.sample(p.generator, excitation_bias)
            }
        }
    }

    fn ramp_sample(
        &mut self,
        table: &WaveformTable,
        p: PrimaryRampParameters,
        pulse: bool,
    ) -> Sample {
        let mut ramp = PrimaryRampOscillator {
            phase: self.phase,
            offset: self.offset,
        };
        let output = if pulse {
            ramp.next_pulse(table, p)
        } else {
            ramp.next_sample(table, p)
        };
        self.phase = ramp.phase;
        self.offset = ramp.offset;
        output
    }
}
