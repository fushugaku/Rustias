//! St.Decimator's original controller coefficient tables and parameter writes.
//! These coefficients do not define the unimplemented FXD03 audio arithmetic.
use crate::effect_curves::{EffectCurve, EffectParameterRange};
pub use crate::effect_parameters::{EffectInterpolationControl, EffectParameterBatch};
use crate::effect_updates::EffectCoefficientAssignments;
pub type PreparedDecimatorEffectChange = crate::effect_parameters::PreparedEffectParameterChange;
#[derive(Clone)]
pub struct DecimatorEffectTables {
    pub bit_depth: [u32; 21],
    pub sample_rate: [u32; 95],
    pub pre_lpf: [u32; 95],
    pub high_dump_peak: u32,
    pub high_dump_range: EffectParameterRange,
    pub output_peak: u32,
    pub output_range: EffectParameterRange,
    pub fs_mod_peak: u32,
    pub fs_mod_range: EffectParameterRange,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecimatorEffectState {
    /// The source tests nonzero, rather than comparing with one.
    pub pre_lpf: u8,
    pub stored_sample_rate: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecimatorEffectChange {
    Mix {
        value: u8,
    },
    PreLpf {
        value: u8,
        state: DecimatorEffectState,
    },
    HighDump {
        value: u8,
    },
    OutputLevel {
        value: u8,
    },
    FsModulation {
        value: u8,
    },
    BitDepth {
        value: u8,
        coefficient_offset: u32,
    },
    SampleRate {
        value: u8,
        state: DecimatorEffectState,
    },
}
impl EffectInterpolationControl {
    /// PreLPF recomputes the dependent coefficient using Fs's owner selection.
    pub fn for_decimator_parameter(
        direct_switch: u32,
        parameter: u8,
        first: u32,
        second: u32,
        master: bool,
    ) -> Self {
        Self::from_owners(
            direct_switch,
            if parameter == 1 { 3 } else { parameter },
            first,
            second,
            master,
        )
    }
}
impl DecimatorEffectChange {
    pub fn from_parameter(parameter: u8, value: u8, state: DecimatorEffectState) -> Option<Self> {
        Some(match parameter {
            0 => Self::Mix { value },
            1 => Self::PreLpf { value, state },
            2 => Self::HighDump { value },
            3 => Self::SampleRate { value, state },
            4 => Self::BitDepth {
                value,
                coefficient_offset: 7,
            },
            5 => Self::OutputLevel { value },
            6 => Self::FsModulation { value },
            _ => return None,
        })
    }
}
impl DecimatorEffectTables {
    pub fn prepare(
        &self,
        assignments: &EffectCoefficientAssignments,
        origin: u16,
        change: DecimatorEffectChange,
        interpolation: EffectInterpolationControl,
    ) -> Option<PreparedDecimatorEffectChange> {
        let mut batch = EffectParameterBatch::from_lfo(None);
        let mut next = *assignments;
        let mut transitions = [None; 2];
        match change {
            DecimatorEffectChange::Mix { value } => {
                let mix = crate::effect_control::EffectMix::compile(
                    crate::effect_control::EffectKind::new(10)?,
                    value,
                    Default::default(),
                )?;
                transitions[0] = Some((u32::from(origin), mix.dry as u32));
                transitions[1] = Some((u32::from(origin) + 1, mix.wet as u32));
            }
            DecimatorEffectChange::PreLpf { value, state } => {
                if value > 1 {
                    return None;
                }
                let coefficient = if state.pre_lpf != 0 {
                    *self.pre_lpf.get(usize::from(state.stored_sample_rate))?
                } else {
                    0
                };
                transitions[0] = Some((u32::from(origin) + 13, coefficient));
            }
            DecimatorEffectChange::HighDump { value } => {
                if value > 100 {
                    return None;
                }
                let coefficient = self.high_dump_range.compile(
                    EffectCurve::Quadratic,
                    i32::from(value),
                    self.high_dump_peak as i32,
                    0,
                )?;
                transitions[0] = Some((u32::from(origin) + 9, coefficient as u32));
            }
            DecimatorEffectChange::OutputLevel { value } => {
                if value > 127 {
                    return None;
                }
                let coefficient = self.output_range.compile(
                    EffectCurve::Linear,
                    i32::from(value),
                    self.output_peak as i32,
                    0,
                )?;
                transitions[0] = Some((u32::from(origin) + 10, coefficient as u32));
            }
            DecimatorEffectChange::FsModulation { value } => {
                if !(1..=127).contains(&value) {
                    return None;
                }
                let coefficient = self.fs_mod_range.compile(
                    EffectCurve::OffsetQuadratic,
                    i32::from(value),
                    self.fs_mod_peak as i32,
                    0,
                )?;
                transitions[0] = Some((u32::from(origin) + 11, coefficient as u32));
            }
            DecimatorEffectChange::BitDepth {
                value,
                coefficient_offset,
            } => {
                let coefficient = *self.bit_depth.get(usize::from(value))?;
                batch.push_direct(
                    u32::from(origin).wrapping_add(coefficient_offset),
                    coefficient,
                )?;
            }
            DecimatorEffectChange::SampleRate { value, state } => {
                let coefficient = *self.sample_rate.get(usize::from(value))?;
                // The original rereads the stored Fs byte for the dependent LPF.
                // It is separate from the requested argument to the first table.
                let lpf = if state.pre_lpf != 0 {
                    *self.pre_lpf.get(usize::from(state.stored_sample_rate))?
                } else {
                    0
                };
                batch.push_direct(u32::from(origin) + 12, coefficient)?;
                transitions[0] = Some((u32::from(origin) + 13, lpf));
            }
        }
        for (target, value) in transitions.into_iter().flatten() {
            let prepared = next.prepare(crate::effect_updates::CoefficientChange {
                direct_switch: interpolation.direct_switch,
                standalone: false,
                enabled_argument: interpolation.enabled_argument,
                mode: 0,
                // Preserve the complete pre-host-wrap target in the cache.
                target,
                value,
            });
            batch.append(&prepared.plan)?;
            next = prepared.next;
        }
        Some(PreparedDecimatorEffectChange { next, batch })
    }
}
