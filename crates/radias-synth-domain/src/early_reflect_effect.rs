//! Complete nine-property Early Reflect insert dependency compiler.
use crate::{
    early_reflect_time::{EarlyReflectTimeEdit, EarlyReflectTimeTables},
    effect_buffer_allocation::EffectBufferInstance,
    effect_control::{EffectKind, EffectMix, MixContext},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_pair_transition::EffectPairTransitionTables,
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_program_staging::{EffectProgramStaging, EffectStagedProgramWrite},
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    program::Program,
};
pub struct EarlyReflectEffectTables {
    pub ranges: [EffectParameterRange; 9],
    pub dependencies: [[u32; 2]; 9],
    pub groups: [EffectCoefficientGroup; 61],
    pub time: EarlyReflectTimeTables,
    pub routing: EffectRoutingTables,
    pub pair_transition: EffectPairTransitionTables,
    pub type_coefficients: [[u32; 16]; 4],
    pub transition_programs: [[u64; 2]; 5],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EarlyReflectEffectRack {
    pub instances: [EffectBufferInstance; 8],
    pub program_origins: [u16; 8],
    pub assignments: EffectCoefficientAssignments,
    pub staging: EffectProgramStaging,
}
#[derive(Clone, Copy)]
pub struct EarlyReflectParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
}
pub struct PreparedEarlyReflectEdit {
    pub next: EarlyReflectEffectRack,
    pub batch: EffectParameterBatch,
    pub program_writes: [Option<EffectStagedProgramWrite>; 2],
}
fn routing_instances(rack: &EarlyReflectEffectRack) -> [EffectRoutingInstance; 9] {
    core::array::from_fn(|i| {
        if i < 8 {
            EffectRoutingInstance {
                kind: rack.instances[i].kind,
                origin: rack.instances[i].origin,
                parameters: rack.instances[i].parameters,
            }
        } else {
            EffectRoutingInstance::default()
        }
    })
}
fn bypass_mix(
    program: &Program,
    instances: &[EffectRoutingInstance; 9],
    part: u8,
    bypass: bool,
) -> Option<EffectParameterBatch> {
    let timbre = program.timbre(usize::from(part))?;
    let first_kind = timbre.effect(0)?.kind()?.raw();
    let mut batch = EffectParameterBatch::from_lfo(None);
    for role in 0..2 {
        if role == 1 && matches!(first_kind, 29 | 30) {
            break;
        }
        let instance = &instances[usize::from(part) * 2 + role];
        let mix = if !bypass && instance.kind != 0 && timbre.effect(role)?.enabled() {
            EffectMix::compile(
                EffectKind::new(instance.kind)?,
                instance.parameters[0],
                MixContext {
                    byte1: instance.parameters[1],
                    byte5: instance.parameters[5],
                    byte6: instance.parameters[6],
                },
            )?
        } else {
            EffectMix {
                dry: 0x7fffff,
                wet: 0,
            }
        };
        batch.push_direct(u32::from(instance.origin), mix.dry as u32)?;
        batch.push_direct(u32::from(instance.origin) + 1, mix.wet as u32)?;
    }
    // SYS07C7EE requests owner for parameter zero, so both publications are
    // direct and preserve assignment state. Its extra wet stack flag is ignored.
    Some(batch)
}
impl EarlyReflectEffectTables {
    fn type_change(
        &self,
        next: &mut EarlyReflectEffectRack,
        program: &Program,
        edit: EarlyReflectParameterEdit,
        batch: &mut EffectParameterBatch,
        writes: &mut [Option<EffectStagedProgramWrite>; 2],
    ) -> Option<()> {
        let part = edit.slot / 2;
        let first = usize::from(part) * 2;
        let instances = routing_instances(next);
        let pair = [instances[first], instances[first + 1]];
        if edit.direct_switch == 0 {
            batch.extend(&self.routing.prepare(
                program,
                &instances,
                EffectRoutingContext::Insert(part),
                1,
            )?)?;
            batch.extend(&bypass_mix(program, &instances, part, true)?)?;
            batch.push_command(0, 0x01000014)?;
            let write = next
                .staging
                .store(self.transition_programs[usize::from(part)][0], false)?;
            batch.push_command(
                next.program_origins[first],
                0x02000000 | u32::from(write.selector),
            )?;
            writes[0] = Some(write);
        }
        for (i, &word) in self
            .type_coefficients
            .get(usize::from(edit.value))?
            .iter()
            .enumerate()
        {
            batch.push_direct(u32::from(edit.origin) + 13 + i as u32, word)?;
        }
        if edit.direct_switch == 0 {
            batch.extend(&self.pair_transition.prepare(&pair, 1)?)?;
            batch.push_command(0, 0x01000028)?;
            let write = next
                .staging
                .store(self.transition_programs[usize::from(part)][1], true)?;
            batch.push_command(
                next.program_origins[first],
                0x02000000 | u32::from(write.selector),
            )?;
            writes[1] = Some(write);
            batch.push_command(0, 0x01000001)?;
            next.staging.advance();
            batch.extend(&self.pair_transition.prepare(&pair, 0)?)?;
            batch.push_command(0, 0x01000001)?;
            batch.extend(&bypass_mix(program, &instances, part, false)?)?;
            batch.extend(&self.routing.prepare(
                program,
                &instances,
                EffectRoutingContext::Insert(part),
                0,
            )?)?;
        }
        Some(())
    }
    pub fn prepare(
        &self,
        rack: &EarlyReflectEffectRack,
        program: &Program,
        edit: EarlyReflectParameterEdit,
    ) -> Option<PreparedEarlyReflectEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 9 || rack.staging.cursor >= 20 {
            return None;
        }
        let valid = |raw: u8, r: EffectParameterRange| {
            let decoded = i32::from(raw) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&decoded)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit.parameters[..9]
                .iter()
                .zip(self.ranges)
                .any(|(&raw, r)| !valid(raw, r))
        {
            return None;
        }
        let mut next = *rack;
        next.instances[slot].kind = 12;
        next.instances[slot].origin = edit.origin;
        next.instances[slot].parameters = edit.parameters;
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        let mut batch = EffectParameterBatch::from_lfo(None);
        let mut program_writes = [None; 2];
        for (index, group) in self.groups.iter().enumerate() {
            if self.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            if group.action == 23 {
                self.type_change(&mut next, program, edit, &mut batch, &mut program_writes)?;
                continue;
            }
            if group.action == 59 {
                let instance = next.instances[slot];
                batch.extend(&self.time.prepare(EarlyReflectTimeEdit {
                    origin: edit.origin,
                    buffer_origin: instance.buffer_origin.wrapping_add(instance.layout.offset),
                    size: edit.parameters[2],
                    pre_delay: edit.parameters[3],
                })?)?;
                continue;
            }
            let value = if matches!(group.action, 4 | 6) {
                let mix = EffectMix::compile(EffectKind::new(12)?, edit.value, Default::default())?;
                if group.action == 6 {
                    mix.dry as u32
                } else {
                    mix.wet as u32
                }
            } else {
                let curve = match group.action {
                    11 => EffectCurve::Linear,
                    13 | 16 => EffectCurve::EaseOut,
                    _ => return None,
                };
                let value =
                    group
                        .range
                        .compile(curve, i32::from(edit.value), group.first, group.second)?;
                if group.action == 16 {
                    value.wrapping_mul(2).wrapping_add(0xff800001u32 as i32) as u32
                } else {
                    value as u32
                }
            };
            let p = next.assignments.prepare(CoefficientChange {
                direct_switch: edit.direct_switch,
                standalone: false,
                enabled_argument: control.enabled_argument,
                mode: 0,
                target: u32::from(edit.origin) + index as u32,
                value,
            });
            batch.append(&p.plan)?;
            next.assignments = p.next;
        }
        Some(PreparedEarlyReflectEdit {
            next,
            batch,
            program_writes,
        })
    }
}
