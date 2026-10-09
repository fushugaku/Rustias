//! St.Decimator's original controller coefficient tables and parameter writes.
//! These coefficients do not define the unimplemented FXD03 audio arithmetic.
use crate::effect_updates::{CoefficientQueueWord, EffectCoefficientAssignments};
#[derive(Clone)]
pub struct DecimatorEffectTables {
    pub bit_depth: [u32; 21],
    pub sample_rate: [u32; 95],
    pub pre_lpf: [u32; 95],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecimatorEffectState {
    /// The source tests nonzero, rather than comparing with one.
    pub pre_lpf: u8,
    pub stored_sample_rate: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecimatorEffectChange {
    BitDepth {
        value: u8,
        coefficient_offset: u32,
    },
    SampleRate {
        value: u8,
        state: DecimatorEffectState,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectParameterBatch {
    words: [CoefficientQueueWord; 6],
    count: u8,
}
impl EffectParameterBatch {
    pub fn words(&self) -> &[CoefficientQueueWord] {
        &self.words[..usize::from(self.count)]
    }
}
pub struct PreparedDecimatorEffectChange {
    pub next: EffectCoefficientAssignments,
    pub batch: EffectParameterBatch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectInterpolationControl {
    pub direct_switch: u16,
    pub enabled_argument: u32,
}
impl DecimatorEffectTables {
    pub fn prepare(
        &self,
        assignments: &EffectCoefficientAssignments,
        origin: u16,
        change: DecimatorEffectChange,
        interpolation: EffectInterpolationControl,
    ) -> Option<PreparedDecimatorEffectChange> {
        let mut batch = EffectParameterBatch {
            words: [CoefficientQueueWord::default(); 6],
            count: 1,
        };
        let mut next = *assignments;
        match change {
            DecimatorEffectChange::BitDepth {
                value,
                coefficient_offset,
            } => {
                let coefficient = *self.bit_depth.get(usize::from(value))?;
                batch.words[0] = CoefficientQueueWord {
                    address: origin.wrapping_add(coefficient_offset as u16),
                    tagged_value: coefficient & 0xffffff,
                };
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
                batch.words[0] = CoefficientQueueWord {
                    address: origin.wrapping_add(12),
                    tagged_value: coefficient & 0xffffff,
                };
                let prepared = assignments.prepare(crate::effect_updates::CoefficientChange {
                    direct_switch: interpolation.direct_switch,
                    standalone: false,
                    enabled_argument: interpolation.enabled_argument,
                    mode: 1,
                    // Preserve the complete pre-host-wrap target in the cache.
                    target: u32::from(origin) + 13,
                    value: lpf,
                });
                let count = usize::from(prepared.plan.count);
                batch.words[1..1 + count].copy_from_slice(&prepared.plan.entries[..count]);
                batch.count += prepared.plan.count;
                next = prepared.next;
            }
        }
        Some(PreparedDecimatorEffectChange { next, batch })
    }
}
