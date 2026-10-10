//! Insert type constructors SYS07B12C and SYS07B21C.
use crate::{
    effect_curves::EffectParameterRange,
    effect_modulation::GrainModulationHistory,
    insert_effect_construction::{InsertEffectInstance, InsertPatch},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertTypeConstruction {
    Defaults,
    StoredWithDefaultHistory,
}
pub struct InsertTypeConstructionTables {
    pub parameter_counts: [u8; 31],
    pub defaults: [[u8; 20]; 31],
    pub owners: [[u8; 2]; 31],
    pub ranges: [[EffectParameterRange; 20]; 31],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedInsertTypeConstruction {
    pub next: InsertEffectInstance,
    pub patch: InsertPatch,
}
impl InsertTypeConstructionTables {
    pub fn prepare(
        &self,
        prior: &InsertEffectInstance,
        mut patch: InsertPatch,
        kind: u8,
        mode: InsertTypeConstruction,
    ) -> Option<PreparedInsertTypeConstruction> {
        if prior.slot >= 8 {
            return None;
        }
        let index = usize::from(kind);
        let count = usize::from(*self.parameter_counts.get(index)?);
        if count > 20 {
            return None;
        }
        let defaults = self.defaults.get(index)?;
        let mut next = *prior;
        next.buffer.kind = kind;
        if mode == InsertTypeConstruction::Defaults {
            let owners = self.owners.get(index)?;
            next.owners = owners.map(u32::from);
            for (header, owner) in patch.header[2..4].iter_mut().zip(owners) {
                *header = (*header & 0xe0) | (owner & 31);
            }
            patch.parameters[..count].copy_from_slice(&defaults[..count]);
        } else {
            next.owners = [
                u32::from(patch.header[2] & 31),
                u32::from(patch.header[3] & 31),
            ];
        }
        next.buffer.parameters[..count].copy_from_slice(&patch.parameters[..count]);
        next.previous_parameters[..count].copy_from_slice(&defaults[..count]);
        if mode == InsertTypeConstruction::StoredWithDefaultHistory {
            for (value, range) in next.buffer.parameters[..count]
                .iter_mut()
                .zip(self.ranges[index])
            {
                *value = range.clamp_encoded(*value);
            }
        }
        next.buffer.ratio = 0;
        next.buffer.limited = 0;
        next.controller_offset = 64;
        next.enabled_argument = u32::from(patch.header[0] & 128 != 0);
        if kind == 29 {
            next.rotary_mode = u32::from(next.buffer.parameters[1]);
            next.rotary_speed = u32::from(next.buffer.parameters[5]);
        } else if kind == 27 {
            next.grain_history = GrainModulationHistory::default();
        } else {
            next.rotary_mode = 0;
            next.rotary_speed = 0;
        }
        Some(PreparedInsertTypeConstruction { next, patch })
    }
}
