//! Decode the physical OSC1 parameter bank shared by constructor and sample jobs.
use crate::{
    pitch::PhaseIncrement,
    primary_oscillator::{
        PrimaryCrossParameters, PrimaryCrossSineParameters, PrimaryCrossTriangleParameters,
        PrimaryParameters, PrimaryRampParameters, PrimarySineParameters, PrimaryTriangleParameters,
        PrimaryUnisonCarrierParameters, PrimaryVpmParameters,
    },
};

pub fn decode(words: &[u16; 160]) -> Option<PrimaryParameters> {
    let p = |i: usize| words[i] as i16;
    let q = |i: usize| ((u32::from(words[i]) << 16) | u32::from(words[i + 1])) as i32;
    let ramp = PrimaryRampParameters {
        increment: PhaseIncrement(q(4) as u32),
        shape: p(10),
        blend: p(12),
        offset_target: p(6),
        target_gain: p(13),
        memory_gain: p(14),
    };
    Some(match p(1) as u16 {
        0xbe88 => PrimaryParameters::Ramp(ramp),
        0xbf1c => PrimaryParameters::Pulse(ramp),
        0xbfc4 => PrimaryParameters::Triangle(PrimaryTriangleParameters {
            increment: ramp.increment,
            center: p(10),
            gain: p(11),
            edge_gain: p(7),
            upper: p(12),
            upper_reflection: p(13),
            lower: p(14),
            lower_reflection: p(15),
        }),
        0xc018 => PrimaryParameters::Sine(PrimarySineParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            control: [p(10), p(11)],
            center: p(12),
            polynomial: [
                ((p(15) as u16 as u32) << 16 | p(14) as u16 as u32) as i32,
                q(16),
                q(18),
            ],
        }),
        0xc0a8 => PrimaryParameters::Noise(crate::primary_oscillator::PrimaryNoiseParameters {
            increment: ramp.increment,
            generator: crate::noise::ColoredNoiseParameters {
                phase_gain: p(7),
                seed_bias: p(9),
                curve_scale: p(10),
                phase_offset: p(11),
                curve_limit: q(12),
                feedback: p(18),
                limits: [q(20), q(22)],
            },
        }),
        0xc1a8 => PrimaryParameters::Formant(crate::primary_oscillator::PrimaryFormantParameters {
            increment: ramp.increment,
            generator: crate::noise::FormantParameters {
                seed_gain: p(7),
                seed_bias: p(9),
                frequency: p(11),
                input_gain: p(16),
                feedback: p(17),
                limits: [q(18), q(20)],
            },
        }),
        0xc430 => PrimaryParameters::Cross(PrimaryCrossParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            shape: p(10),
            offset: p(11),
            blend: p(12),
        }),
        0xc4c4 => PrimaryParameters::CrossTriangle(PrimaryCrossTriangleParameters {
            carrier: PrimaryTriangleParameters {
                increment: ramp.increment,
                center: p(10),
                gain: p(11),
                edge_gain: 0,
                upper: p(12),
                upper_reflection: p(13),
                lower: p(14),
                lower_reflection: p(15),
            },
            modulation_gain: p(7),
        }),
        0xc51c => PrimaryParameters::CrossSine(PrimaryCrossSineParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            center: p(12),
            polynomial: [
                ((p(15) as u16 as u32) << 16 | p(14) as u16 as u32) as i32,
                q(16),
                q(18),
            ],
        }),
        0xc584 => PrimaryParameters::Unison(crate::unison::UnisonParameters {
            increments: [4, 10, 12, 16, 18].map(|i| PhaseIncrement(q(i) as u32)),
            detune: p(6) as u16,
            correction_gain: p(20),
            level: p(21),
        }),
        0xc66c | 0xc790 | 0xc868 => {
            PrimaryParameters::UnisonCarrier(PrimaryUnisonCarrierParameters {
                parameters: crate::unison::UnisonParameters {
                    increments: [4, 10, 12, 16, 18].map(|i| PhaseIncrement(q(i) as u32)),
                    detune: p(6) as u16,
                    correction_gain: p(20),
                    level: p(21),
                },
                waveform: match p(1) as u16 {
                    0xc66c => crate::unison::UnisonWaveform::Pulse { bandwidth: p(34) },
                    0xc790 => crate::unison::UnisonWaveform::Triangle,
                    _ => crate::unison::UnisonWaveform::Sine {
                        normalization: i32::MAX,
                    },
                },
            })
        }
        0xc97c => PrimaryParameters::Vpm(PrimaryVpmParameters {
            increment: ramp.increment,
            modulation_gain: p(7),
            ratio: p(8),
            limit: p(13),
            center: q(14),
            shape: p(10),
            offset: p(11),
            blend: p(12),
        }),
        0xca50 | 0xcae8 => {
            use crate::primary_oscillator::{
                PrimaryVpmCarrier, PrimaryVpmCarrierParameters, PrimaryVpmModulatorParameters,
            };
            PrimaryParameters::VpmCarrier(PrimaryVpmCarrierParameters {
                modulator: PrimaryVpmModulatorParameters {
                    increment: ramp.increment,
                    modulation_gain: p(7),
                    ratio: p(8),
                    limit: p(10),
                    center: q(12),
                },
                carrier: if p(1) as u16 == 0xca50 {
                    PrimaryVpmCarrier::Triangle(PrimaryTriangleParameters {
                        increment: ramp.increment,
                        center: p(14),
                        gain: p(15),
                        edge_gain: 0,
                        upper: p(16),
                        upper_reflection: p(17),
                        lower: p(18),
                        lower_reflection: p(19),
                    })
                } else {
                    PrimaryVpmCarrier::Sine {
                        center: p(14),
                        polynomial: [
                            ((p(17) as u16 as u32) << 16 | p(16) as u16 as u32) as i32,
                            q(18),
                            q(20),
                        ],
                    }
                },
            })
        }
        _ => return None,
    })
}
