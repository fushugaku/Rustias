//! SYS016C4C waveform distribution and complete SYS077ED4 callbacks.
use crate::{
    effect_lfo_program::EffectLfoProgram,
    effect_lfo_values::{EffectLfoValueState, EffectLfoValueTables},
    effect_parameters::EffectParameterBatch,
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    filter_effect::{FilterEffectCache, FilterEffectFrequency, FilterEffectTables},
    lfo::LfoTables,
    program::Program,
};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectModulationInstance {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub program: EffectLfoProgram,
    pub blocks_next_insert: u8,
    pub pending_coefficients: [u32; 2],
    pub control_argument: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GrainModulationHistory {
    pub left: [i16; 8],
    pub right: [i16; 8],
    pub left_read: u8,
    pub left_write: u8,
    pub right_read: u8,
    pub right_write: u8,
}
impl Default for GrainModulationHistory {
    fn default() -> Self {
        Self {
            left: [i16::MIN; 8],
            right: [i16::MIN; 8],
            left_read: 0,
            left_write: 1,
            right_read: 0,
            right_write: 1,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectModulationRack {
    pub instances: [EffectModulationInstance; 9],
    pub caches: [FilterEffectCache; 9],
    pub grain_history: [GrainModulationHistory; 9],
    pub assignments: EffectCoefficientAssignments,
}
pub struct EffectModulationTables<'a> {
    pub coefficients: &'a FilterEffectTables,
    pub lfo: &'a LfoTables,
    pub values: &'a EffectLfoValueTables,
    pub available: [bool; 31],
}
pub struct PreparedEffectModulation {
    pub next: EffectModulationRack,
    pub batch: EffectParameterBatch,
    pub computed_pairs: [[i32; 2]; 9],
    pub evaluated: u16,
    pub callback_mask: u16,
}
fn unipolar(value: i16) -> u32 {
    ((((i32::from(value) + i32::from(value < 0)) >> 1) + 16384) as u32) << 8
}
impl EffectModulationRack {
    pub fn prepare(
        &self,
        program: &Program,
        states: [EffectLfoValueState; 9],
        tables: &EffectModulationTables<'_>,
        direct_switch: u32,
    ) -> Option<PreparedEffectModulation> {
        let mut next = *self;
        let mut batch = EffectParameterBatch::from_lfo(None);
        let mut pairs = [[0; 2]; 9];
        let mut evaluated = 0;
        let mut callback_mask = 0;
        for slot in 0..9 {
            let instance = self.instances[slot];
            let available = *tables.available.get(usize::from(instance.kind))?;
            if !available {
                continue;
            }
            let enabled = if slot == 8 {
                program.master_effect().enabled()
            } else {
                let timbre = program.timbre(slot / 2)?;
                timbre.enabled() && timbre.effect(slot % 2)?.enabled()
            };
            if !enabled {
                continue;
            }
            pairs[slot] = tables
                .values
                .values(tables.lfo, instance.program, states[slot]);
            evaluated |= 1 << slot;
            if slot < 8 && slot % 2 == 1 && self.instances[slot - 1].blocks_next_insert == 1 {
                continue;
            }
            callback_mask |= 1 << slot;
            let level = pairs[slot].map(|value| value as i16);
            let origin = u32::from(instance.origin);
            match instance.kind {
                4 => {
                    if instance.parameters[5] != 0 {
                        continue;
                    }
                    let prepared = tables.coefficients.prepare_frequency(
                        next.caches[slot],
                        FilterEffectFrequency {
                            origin: instance.origin,
                            cutoff: instance.parameters[2],
                            resonance: instance.parameters[3],
                            modulation_depth: instance.parameters[6],
                            modulation: level[0],
                        },
                    )?;
                    next.caches[slot] = prepared.next;
                    batch.extend(&prepared.batch)?;
                }
                5 => {
                    if instance.parameters[4] == 1 {
                        batch.push_direct(origin + 2, unipolar(level[0]))?;
                    }
                }
                30 => {
                    if instance.parameters[7] == 1 {
                        batch.push_direct(origin + 2, unipolar(level[0]))?;
                    }
                }
                10 | 15 | 19 => batch.push_direct(origin + 2, unipolar(level[0]))?,
                16 | 22 | 23 | 24 | 28 => {
                    let packed = direct_switch == 0;
                    batch.push_command(
                        instance.origin.wrapping_add(2),
                        (if packed { 0x82000000 } else { 0 }) | unipolar(level[0]),
                    )?;
                    batch.push_command(
                        instance.origin.wrapping_add(3),
                        (if packed { 0x81000000 } else { 0 }) | unipolar(level[1]),
                    )?;
                }
                25 => {
                    batch.push_direct(origin + 2, i32::from(level[0]).wrapping_mul(127) as u32)?
                }
                27 => next.grain_modulation(&mut batch, slot, level, direct_switch)?,
                _ => return None,
            }
        }
        Some(PreparedEffectModulation {
            next,
            batch,
            computed_pairs: pairs,
            evaluated,
            callback_mask,
        })
    }
    fn grain_modulation(
        &mut self,
        batch: &mut EffectParameterBatch,
        slot: usize,
        levels: [i16; 2],
        direct_switch: u32,
    ) -> Option<()> {
        let history = &mut self.grain_history[slot];
        let instance = &mut self.instances[slot];
        if [
            history.left_read,
            history.left_write,
            history.right_read,
            history.right_write,
        ]
        .into_iter()
        .any(|p| p >= 8)
        {
            return None;
        }
        for (channel, (buffer, read, write)) in [
            (
                &mut history.left,
                &mut history.left_read,
                &mut history.left_write,
            ),
            (
                &mut history.right,
                &mut history.right_read,
                &mut history.right_write,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let old = buffer[usize::from(*write)];
            if buffer[usize::from(*read)] != old {
                batch.push_direct(
                    u32::from(instance.origin) + 2 + channel as u32,
                    unipolar(old),
                )?;
                if instance.pending_coefficients[channel] & 0x80000000 != 0 {
                    instance.pending_coefficients[channel] &= 0xffffff;
                    let prepared = self.assignments.prepare(CoefficientChange {
                        direct_switch,
                        standalone: false,
                        enabled_argument: instance.control_argument,
                        mode: 1,
                        target: u32::from(instance.origin) + 6 + 2 * channel as u32,
                        value: instance.pending_coefficients[channel],
                    });
                    batch.append(&prepared.plan)?;
                    self.assignments = prepared.next;
                }
            }
            buffer[usize::from(*read)] = levels[channel];
            *read = read.wrapping_add(1) & 7;
            *write = write.wrapping_add(1) & 7;
        }
        Some(())
    }
}
