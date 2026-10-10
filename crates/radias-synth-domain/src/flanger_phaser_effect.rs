//! Complete St.Flanger/St.Phaser insert dependency controllers.
use crate::{
    delay_time::encode_delay_frames,
    effect_buffer_allocation::divide_127,
    effect_control::{EffectKind, EffectMix, MixContext},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments, UNASSIGNED_TARGET},
    lfo_tempo::LfoTempoTables,
    program::Program,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlangerPhaserKind {
    Flanger,
    Phaser,
}
impl FlangerPhaserKind {
    pub fn id(self) -> u8 {
        if self == Self::Flanger { 22 } else { 23 }
    }
    pub fn index(self) -> usize {
        usize::from(self == Self::Phaser)
    }
    pub fn parameter_count(self) -> usize {
        if self == Self::Flanger { 16 } else { 15 }
    }
}
pub struct FlangerPhaserDefinition {
    pub ranges: [EffectParameterRange; 16],
    pub dependencies: [[u32; 2]; 16],
    pub groups: [EffectCoefficientGroup; 34],
    pub group_count: usize,
    pub lfo_mapping: EffectLfoMapping,
}
pub struct FlangerPhaserTables {
    pub definitions: [FlangerPhaserDefinition; 2],
    pub feedback_range: EffectParameterRange,
    pub response: [u32; 128],
    pub milliseconds: [u16; 114],
    pub cutoff: [u32; 128],
    pub routing: EffectRoutingTables,
    pub tempo: LfoTempoTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlangerPhaserRack {
    pub instances: [EffectRoutingInstance; 9],
    pub lfos: [EffectLfoProgram; 8],
    pub assignments: EffectCoefficientAssignments,
    pub update_marker: u32,
}
#[derive(Clone, Copy)]
pub struct FlangerPhaserEdit {
    pub kind: FlangerPhaserKind,
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock_rate: u32,
}
pub struct PreparedFlangerPhaserEdit {
    pub next: FlangerPhaserRack,
    pub batch: EffectParameterBatch,
}
fn publish(
    rack: &mut FlangerPhaserRack,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
    mode: u8,
) -> Option<()> {
    let p = rack.assignments.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        enabled_argument: control.enabled_argument,
        standalone: false,
        target,
        value,
        mode,
    });
    batch.append(&p.plan)?;
    rack.assignments = p.next;
    Some(())
}
fn release_assignments(
    rack: &mut FlangerPhaserRack,
    batch: &mut EffectParameterBatch,
    direct: u32,
) -> Option<()> {
    for record in &mut rack.assignments.slots {
        if record.target == UNASSIGNED_TARGET {
            continue;
        }
        batch.push_direct(record.target, record.last_value)?;
        record.target = UNASSIGNED_TARGET;
        record.last_value = 0;
        for (i, value) in [UNASSIGNED_TARGET, 0, 0x7a9765, 0x5689a]
            .into_iter()
            .enumerate()
        {
            batch.push_command(
                record.indices[i],
                (value & 0xffffff)
                    | if direct == 0 {
                        (0x84 - i as u32) << 24
                    } else {
                        0
                    },
            )?;
        }
    }
    // SYS075316 retains the LRU order and clears SYS07513A's separate marker.
    rack.update_marker = 0;
    Some(())
}
impl FlangerPhaserTables {
    pub(crate) fn feedback(&self, p: &[u8; 20]) -> Option<u32> {
        if p[1] == 0 {
            let value = self.feedback_range.compile(
                EffectCurve::Quadratic,
                i32::from(p[5]),
                0x7fffff,
                0,
            )?;
            return Some(if p[6] == 1 {
                value.wrapping_neg()
            } else {
                value
            } as u32);
        }
        let center = (i32::from(p[5]) - divide_127((127 - i32::from(p[3])) * 120)).clamp(-127, 127);
        let index = divide_127(center.wrapping_mul(63).wrapping_add(8128));
        Some(divide_127(
            (*self.response.get(index as usize)? as i32)
                .wrapping_mul((i32::from(p[5]) * 8).clamp(0, 127)),
        ) as u32)
    }
    fn time_owner(edit: FlangerPhaserEdit) -> EffectInterpolationControl {
        EffectInterpolationControl::from_owners(
            edit.direct_switch,
            if edit.parameters[1] == 1 { 3 } else { 2 },
            edit.owners[0],
            edit.owners[1],
            false,
        )
    }
    pub fn prepare(
        &self,
        rack: &FlangerPhaserRack,
        program: &Program,
        edit: FlangerPhaserEdit,
    ) -> Option<PreparedFlangerPhaserEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        let definition = &self.definitions[edit.kind.index()];
        if slot >= 8 || parameter >= edit.kind.parameter_count() {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let value = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&value)
        };
        if !valid(edit.value, definition.ranges[parameter])
            || edit.parameters[..edit.kind.parameter_count()]
                .iter()
                .zip(definition.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let mut next = *rack;
        next.instances[slot] = EffectRoutingInstance {
            kind: edit.kind.id(),
            origin: edit.origin,
            parameters: edit.parameters,
        };
        let mut batch = EffectParameterBatch::from_lfo(None);
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        for (index, group) in definition.groups[..definition.group_count]
            .iter()
            .enumerate()
        {
            if definition.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            match group.action {
                34 => {
                    let p = next.lfos[slot].prepare(
                        &edit.parameters,
                        definition.lfo_mapping,
                        EffectLfoSlot::new(edit.slot)?,
                        0,
                        edit.clock_rate,
                        &self.tempo,
                    )?;
                    if let Some(p) = p {
                        next.lfos[slot] = p.program;
                    }
                    batch.extend(&EffectParameterBatch::from_lfo(p))?;
                }
                77 => {
                    if Self::time_owner(edit).enabled_argument != 0 {
                        release_assignments(&mut next, &mut batch, edit.direct_switch)?;
                    }
                }
                38 => {
                    let feedback = self.feedback(&edit.parameters)?;
                    let time = if edit.parameters[1] == 0 {
                        encode_delay_frames(
                            u32::from(
                                *self
                                    .milliseconds
                                    .get(usize::from(edit.parameters[2].min(113)))?,
                            ) * 48
                                / 10,
                            7,
                        )
                    } else {
                        *self.cutoff.get(usize::from(edit.parameters[3]))?
                    };
                    batch.push_direct(u32::from(edit.origin) + 10, feedback)?;
                    let owner = Self::time_owner(edit);
                    for offset in [7, 9] {
                        publish(
                            &mut next,
                            &mut batch,
                            owner,
                            u32::from(edit.origin) + offset,
                            time,
                            1,
                        )?;
                    }
                }
                37 => {
                    let value = self.feedback(&edit.parameters)?;
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + 10,
                        value,
                        0,
                    )?;
                }
                39 => {
                    if index == 0 || index == 1 {
                        let mix = EffectMix::compile(
                            EffectKind::new(edit.kind.id())?,
                            edit.parameters[0],
                            MixContext {
                                byte1: edit.parameters[1],
                                byte5: edit.parameters[5],
                                byte6: edit.parameters[6],
                            },
                        )?;
                        for (offset, value) in [mix.dry, mix.wet].into_iter().enumerate() {
                            batch.push_direct(
                                u32::from(edit.origin) + offset as u32,
                                value as u32,
                            )?;
                        }
                    } else if edit.kind == FlangerPhaserKind::Phaser && index == 6 {
                        let value = self.feedback_range.compile(
                            EffectCurve::Linear,
                            i32::from(edit.parameters[4]),
                            0x7fffff,
                            0,
                        )?;
                        let value = if edit.parameters[5] == 1 {
                            value.wrapping_neg()
                        } else {
                            value
                        };
                        let owner = EffectInterpolationControl::from_owners(
                            edit.direct_switch,
                            4,
                            edit.owners[0],
                            edit.owners[1],
                            false,
                        );
                        publish(
                            &mut next,
                            &mut batch,
                            owner,
                            u32::from(edit.origin) + 6,
                            value as u32,
                            0,
                        )?;
                    } else {
                        return None;
                    }
                }
                40 => {
                    let context = EffectRoutingContext::Insert(edit.slot / 2);
                    if edit.direct_switch == 0 {
                        batch.extend(&self.routing.prepare(
                            program,
                            &next.instances,
                            context,
                            1,
                        )?)?;
                    }
                    for (i, value) in if edit.parameters[1] == 0 {
                        [0x7fffff, 0]
                    } else {
                        [0, 0x7fffff]
                    }
                    .into_iter()
                    .enumerate()
                    {
                        batch.push_direct(u32::from(edit.origin) + 18 + i as u32, value)?;
                    }
                    if edit.direct_switch == 0 {
                        batch.extend(&self.routing.prepare(
                            program,
                            &next.instances,
                            context,
                            0,
                        )?)?;
                    }
                }
                9 | 13 => {
                    let value = group.range.compile(
                        if group.action == 9 {
                            EffectCurve::Quadratic
                        } else {
                            EffectCurve::EaseOut
                        },
                        i32::from(edit.value),
                        group.first,
                        group.second,
                    )? as u32;
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + index as u32,
                        value,
                        0,
                    )?;
                }
                _ => return None,
            }
        }
        Some(PreparedFlangerPhaserEdit { next, batch })
    }
}
