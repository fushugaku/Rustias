//! Complete eleven-property Reverb insert dependency compiler.
use crate::{
    delay_time::DelayClock,
    effect_buffer_allocation::{EffectBufferAllocationTables, EffectBufferInstance},
    effect_control::{EffectBank, EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    reverb_time::{ReverbTimeEdit, ReverbTimeTables},
};
pub struct ReverbEffectTables {
    pub ranges: [EffectParameterRange; 11],
    pub dependencies: [[u32; 2]; 11],
    pub groups: [EffectCoefficientGroup; 49],
    pub allocation: EffectBufferAllocationTables,
    pub time: ReverbTimeTables,
    pub type_coefficients: [[u32; 4]; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReverbEffectRack {
    pub instances: [EffectBufferInstance; 8],
    pub assignments: EffectCoefficientAssignments,
    pub scratch: [u32; 49],
}
#[derive(Clone, Copy)]
pub struct ReverbParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub secondary_switch: u32,
    pub clock: DelayClock,
}
pub struct PreparedReverbEdit {
    pub next: ReverbEffectRack,
    pub batch: EffectParameterBatch,
}
impl ReverbEffectTables {
    pub fn prepare(
        &self,
        rack: &ReverbEffectRack,
        edit: ReverbParameterEdit,
    ) -> Option<PreparedReverbEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 11 {
            return None;
        }
        let valid = |raw: u8, r: EffectParameterRange| {
            let v = i32::from(raw) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&v)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit.parameters[..11]
                .iter()
                .zip(self.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *rack;
        next.instances[slot].kind = 11;
        next.instances[slot].origin = edit.origin;
        next.instances[slot].parameters = edit.parameters;
        let mut batch = EffectParameterBatch::from_lfo(None);
        let time = |batch: &mut EffectParameterBatch| {
            batch.extend(&self.time.prepare(ReverbTimeEdit {
                bank: EffectBank::Insert,
                origin: edit.origin,
                effect_type: edit.parameters[1],
                time: edit.parameters[2],
            })?)
        };
        let interpolation = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        for (index, group) in self.groups.iter().enumerate() {
            if self.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            if group.action == 54 {
                if edit.direct_switch == 0 || edit.secondary_switch != 0 {
                    let template =
                        &self.allocation.reverb_templates[usize::from(edit.parameters[1])][..49];
                    let allocation = self.allocation.prepare(
                        &next.instances,
                        edit.slot,
                        template,
                        edit.clock,
                    )?;
                    next.instances = allocation.instances;
                    next.scratch
                        .copy_from_slice(&allocation.template.words[..49]);
                    batch.extend(&allocation.batch)?;
                    for (i, &v) in next.scratch[6..30].iter().enumerate() {
                        batch.push_direct(u32::from(edit.origin) + 6 + i as u32, v)?;
                    }
                    for (i, &v) in self.type_coefficients[usize::from(edit.parameters[1])]
                        .iter()
                        .enumerate()
                    {
                        batch.push_direct(u32::from(edit.origin) + 32 + i as u32, v)?;
                    }
                    if edit.direct_switch == 0 {
                        batch.push_command(0, 0x01000028)?;
                    }
                }
                time(&mut batch)?;
                continue;
            }
            if group.action == 55 {
                time(&mut batch)?;
                continue;
            }
            let value = if matches!(group.action, 4 | 6) {
                let mix = EffectMix::compile(EffectKind::new(11)?, edit.value, Default::default())?;
                if group.action == 6 {
                    mix.dry as u32
                } else {
                    mix.wet as u32
                }
            } else if group.action == 9 {
                group.range.compile(
                    EffectCurve::Quadratic,
                    i32::from(edit.value),
                    group.first,
                    group.second,
                )? as u32
            } else {
                return None;
            };
            let p = next.assignments.prepare(CoefficientChange {
                direct_switch: edit.direct_switch,
                standalone: false,
                enabled_argument: interpolation.enabled_argument,
                mode: 0,
                target: u32::from(edit.origin) + index as u32,
                value,
            });
            batch.append(&p.plan)?;
            next.assignments = p.next;
        }
        Some(PreparedReverbEdit { next, batch })
    }
}
