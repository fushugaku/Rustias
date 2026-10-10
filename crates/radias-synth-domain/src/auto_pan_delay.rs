//! AutoPanDelay and St.AutoPanDly complete native insert controllers.
use crate::{
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, encode_delay_frames},
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
};
pub struct AutoPanDelayDefinition {
    pub ranges: [EffectParameterRange; 20],
    pub dependencies: [u32; 20],
    pub groups: [EffectCoefficientGroup; 32],
    pub lfo_mapping: EffectLfoMapping,
}
pub struct AutoPanDelayTables {
    pub definitions: [AutoPanDelayDefinition; 2],
    pub time: DelayTimeTables,
    pub tempo: LfoTempoTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AutoPanDelayRack {
    pub times: [DelayTimeState; 8],
    pub lfos: [EffectLfoProgram; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct AutoPanDelayEdit {
    pub stereo: bool,
    pub slot: u8,
    pub origin: u16,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock: DelayClock,
    pub clock_rate: u32,
}
pub struct PreparedAutoPanDelayEdit {
    pub next: AutoPanDelayRack,
    pub batch: EffectParameterBatch,
}
fn publish(
    next: &mut EffectCoefficientAssignments,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
    mode: u8,
) -> Option<()> {
    let p = next.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        enabled_argument: control.enabled_argument,
        target,
        value,
        mode,
        standalone: false,
    });
    batch.append(&p.plan)?;
    *next = p.next;
    Some(())
}
impl AutoPanDelayTables {
    fn time(
        &self,
        next: &mut AutoPanDelayRack,
        batch: &mut EffectParameterBatch,
        edit: AutoPanDelayEdit,
    ) -> Option<()> {
        let slot = usize::from(edit.slot);
        let p = self
            .time
            .auto_pan(&edit.parameters, next.times[slot], edit.clock, edit.stereo)?;
        next.times[slot] = p.state;
        let origin = u32::from(edit.origin);
        if edit.stereo {
            batch.push_direct(
                origin + 32,
                self.time
                    .feedback_limit(p.frames[0], p.frames[1], edit.parameters[7])?,
            )?;
        }
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            2,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        let all = edit.parameters[1] != 0 || matches!(edit.parameter, 1 | 2);
        for (channel, offset) in [8, 10].into_iter().enumerate() {
            let selected = if channel == 0 {
                matches!(edit.parameter, 3 | 5)
            } else {
                matches!(edit.parameter, 4 | 6)
            };
            if !all && !selected {
                continue;
            }
            publish(
                &mut next.assignments,
                batch,
                control,
                (origin + offset) as u16 as u32,
                encode_delay_frames(p.frames[channel], if edit.stereo { 7 } else { 6 }),
                1,
            )?;
        }
        Some(())
    }
    pub fn prepare(
        &self,
        rack: &AutoPanDelayRack,
        edit: AutoPanDelayEdit,
    ) -> Option<PreparedAutoPanDelayEdit> {
        let slot = usize::from(edit.slot);
        let count = if edit.stereo { 20 } else { 19 };
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= count {
            return None;
        }
        let definition = &self.definitions[usize::from(edit.stereo)];
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
        for (index, group) in definition.groups.iter().enumerate() {
            if definition.dependencies[parameter] & (0x80000000 >> index) == 0 {
                continue;
            }
            if group.action == 38 {
                self.time(&mut next, &mut batch, edit)?;
                continue;
            }
            if group.action == 34 {
                let publication = next.lfos[slot].prepare(
                    &edit.parameters,
                    definition.lfo_mapping,
                    EffectLfoSlot::new(edit.slot)?,
                    0,
                    edit.clock_rate,
                    &self.tempo,
                )?;
                if let Some(p) = publication {
                    next.lfos[slot] = p.program;
                }
                batch.extend(&EffectParameterBatch::from_lfo(publication))?;
                continue;
            }
            let value = if matches!(group.action, 4 | 6) {
                let mix = EffectMix::compile(
                    EffectKind::new(if edit.stereo { 16 } else { 15 })?,
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
            )?;
        }
        Some(PreparedAutoPanDelayEdit { next, batch })
    }
}
