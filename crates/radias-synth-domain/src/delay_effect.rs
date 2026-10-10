//! Complete L/C/R Delay and Stereo Delay insert parameter compilation.
//! Host coefficient preparation is independent of FXD03 sample arithmetic.
use crate::{
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, encode_delay_frames},
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelayEffectKind {
    Lcr,
    Stereo,
}
impl DelayEffectKind {
    pub fn id(self) -> u8 {
        match self {
            Self::Lcr => 13,
            Self::Stereo => 14,
        }
    }
    pub fn parameter_count(self) -> usize {
        match self {
            Self::Lcr => 17,
            Self::Stereo => 13,
        }
    }
    fn time_ratio_parameter(self) -> u8 {
        match self {
            Self::Lcr => 2,
            Self::Stereo => 3,
        }
    }
}
pub struct DelayEffectDefinition {
    pub ranges: [EffectParameterRange; 17],
    pub dependencies: [u32; 17],
    pub groups: [EffectCoefficientGroup; 31],
    pub group_count: usize,
}
pub struct DelayEffectTables {
    pub definitions: [DelayEffectDefinition; 2],
    pub time: DelayTimeTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelayEffectRack {
    pub states: [DelayTimeState; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct DelayParameterEdit {
    pub kind: DelayEffectKind,
    pub slot: u8,
    pub origin: u16,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock: DelayClock,
}
pub struct PreparedDelayEdit {
    pub next: DelayEffectRack,
    pub batch: EffectParameterBatch,
}
fn publish(
    next: &mut EffectCoefficientAssignments,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
    mode: u8,
    standalone: bool,
) -> Option<()> {
    let change = next.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        enabled_argument: control.enabled_argument,
        target,
        value,
        mode,
        standalone,
    });
    batch.append(&change.plan)?;
    *next = change.next;
    Some(())
}
impl DelayEffectTables {
    fn prepare_time(
        &self,
        next: &mut DelayEffectRack,
        batch: &mut EffectParameterBatch,
        edit: DelayParameterEdit,
    ) -> Option<()> {
        let slot = usize::from(edit.slot);
        let prepared = match edit.kind {
            DelayEffectKind::Lcr => {
                self.time
                    .lcr(&edit.parameters, next.states[slot], edit.clock)?
            }
            DelayEffectKind::Stereo => {
                self.time
                    .stereo(&edit.parameters, next.states[slot], edit.clock)?
            }
        };
        next.states[slot] = prepared.state;
        let origin = u32::from(edit.origin);
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.kind.time_ratio_parameter(),
            edit.owners[0],
            edit.owners[1],
            false,
        );
        match edit.kind {
            DelayEffectKind::Lcr => {
                let all = edit.parameters[1] != 0 || matches!(edit.parameter, 1 | 2);
                for (channel, offset) in [6, 7, 8].into_iter().enumerate() {
                    let selected = match channel {
                        0 => matches!(edit.parameter, 3 | 6),
                        1 => matches!(edit.parameter, 4 | 7),
                        _ => matches!(edit.parameter, 5 | 8),
                    };
                    if !all && !selected {
                        continue;
                    }
                    let value = encode_delay_frames(prepared.frames[channel], 6);
                    if channel == 1 {
                        batch.push_direct(origin + offset, value)?;
                    } else {
                        publish(
                            &mut next.assignments,
                            batch,
                            control,
                            origin + offset,
                            value,
                            1,
                            false,
                        )?;
                    }
                }
            }
            DelayEffectKind::Stereo => {
                // Feedback safety is published before either time update.
                batch.push_direct(
                    origin + 29,
                    self.time.feedback_limit(
                        prepared.frames[0],
                        prepared.frames[1],
                        edit.parameters[8],
                    )?,
                )?;
                let all = edit.parameters[2] != 0 || matches!(edit.parameter, 2 | 3);
                for (channel, offset) in [8, 10].into_iter().enumerate() {
                    let selected = if channel == 0 {
                        matches!(edit.parameter, 4 | 6)
                    } else {
                        matches!(edit.parameter, 5 | 7)
                    };
                    if !all && !selected {
                        continue;
                    }
                    // SYS072DCC constructs u16 target fields before allocation.
                    publish(
                        &mut next.assignments,
                        batch,
                        control,
                        (origin + offset) as u16 as u32,
                        encode_delay_frames(prepared.frames[channel], 7),
                        1,
                        false,
                    )?;
                }
            }
        }
        Some(())
    }
    pub fn prepare(
        &self,
        rack: &DelayEffectRack,
        edit: DelayParameterEdit,
    ) -> Option<PreparedDelayEdit> {
        let definition = &self.definitions[usize::from(edit.kind == DelayEffectKind::Stereo)];
        let count = edit.kind.parameter_count();
        let parameter = usize::from(edit.parameter);
        if edit.slot >= 8 || parameter >= count {
            return None;
        }
        let valid = |raw: u8, range: EffectParameterRange| {
            let decoded = i32::from(raw) - i32::from(range.encoded_zero);
            (i32::from(range.minimum)..=i32::from(range.maximum)).contains(&decoded)
        };
        if !valid(edit.value, definition.ranges[parameter])
            || edit.parameters[..count]
                .iter()
                .zip(definition.ranges)
                .any(|(&raw, range)| !valid(raw, range))
        {
            return None;
        }
        let mut next = *rack;
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
            if definition.dependencies[parameter] & (0x80000000 >> index) == 0 {
                continue;
            }
            if group.action == 38 {
                self.prepare_time(&mut next, &mut batch, edit)?;
                continue;
            }
            let value = if matches!(group.action, 4 | 6) {
                let mix = EffectMix::compile(
                    EffectKind::new(edit.kind.id())?,
                    edit.value,
                    Default::default(),
                )?;
                if group.action == 6 {
                    mix.dry as u32
                } else {
                    mix.wet as u32
                }
            } else {
                let curve = match group.action {
                    9 => EffectCurve::Quadratic,
                    11 => EffectCurve::Linear,
                    13 | 16 => EffectCurve::EaseOut,
                    15 => EffectCurve::Select,
                    _ => return None,
                };
                let coefficient =
                    group
                        .range
                        .compile(curve, i32::from(edit.value), group.first, group.second)?;
                if group.action == 16 {
                    coefficient
                        .wrapping_mul(2)
                        .wrapping_add(0xff800001u32 as i32) as u32
                } else {
                    coefficient as u32
                }
            };
            publish(
                &mut next.assignments,
                &mut batch,
                control,
                u32::from(edit.origin) + index as u32,
                value,
                0,
                false,
            )?;
        }
        Some(PreparedDelayEdit { next, batch })
    }
}
