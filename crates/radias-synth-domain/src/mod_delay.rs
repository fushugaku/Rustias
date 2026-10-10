//! Complete Mod Delay, St.Mod Delay and Tape Echo insert controllers.
use crate::{
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, encode_delay_frames},
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModDelayKind {
    Mod,
    StereoMod,
    TapeEcho,
}
impl ModDelayKind {
    pub fn index(self) -> usize {
        match self {
            Self::Mod => 0,
            Self::StereoMod => 1,
            Self::TapeEcho => 2,
        }
    }
    pub fn id(self) -> u8 {
        17 + self.index() as u8
    }
    pub fn parameter_count(self) -> usize {
        if self == Self::TapeEcho { 18 } else { 11 }
    }
}
pub struct ModDelayDefinition {
    pub ranges: [EffectParameterRange; 18],
    pub dependencies: [[u32; 2]; 18],
    pub groups: [EffectCoefficientGroup; 33],
    pub group_count: usize,
    pub lfo_mapping: EffectLfoMapping,
}
pub struct ModDelayTables {
    pub definitions: [ModDelayDefinition; 3],
    pub milliseconds: [[u16; 128]; 2],
    pub modulation_rate: [u32; 128],
    pub time: DelayTimeTables,
    pub tempo: LfoTempoTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModDelayRack {
    pub times: [DelayTimeState; 8],
    pub lfos: [EffectLfoProgram; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct ModDelayEdit {
    pub kind: ModDelayKind,
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
pub struct PreparedModDelayEdit {
    pub next: ModDelayRack,
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
impl ModDelayTables {
    fn time(
        &self,
        next: &mut ModDelayRack,
        batch: &mut EffectParameterBatch,
        edit: ModDelayEdit,
    ) -> Option<()> {
        let slot = usize::from(edit.slot);
        let stereo = edit.kind == ModDelayKind::StereoMod;
        let mut mapped = [0; 20];
        mapped[2..8].copy_from_slice(&edit.parameters[1..7]);
        let state = next.times[slot];
        let p = self.time.two_channel(
            &mapped,
            state,
            edit.clock,
            if stereo {
                state.capacity >> 1
            } else {
                state.capacity
            },
            &self.milliseconds[usize::from(stereo)],
        )?;
        next.times[slot] = p.state;
        let origin = u32::from(edit.origin);
        if stereo {
            batch.push_direct(
                origin + 28,
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
        let offsets = if edit.kind == ModDelayKind::TapeEcho {
            [7, 8]
        } else {
            [7, 9]
        };
        for (channel, offset) in offsets.into_iter().enumerate() {
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
                encode_delay_frames(p.frames[channel], if stereo { 7 } else { 6 }),
                1,
            )?;
        }
        Some(())
    }
    pub fn prepare(&self, rack: &ModDelayRack, edit: ModDelayEdit) -> Option<PreparedModDelayEdit> {
        let slot = usize::from(edit.slot);
        let count = edit.kind.parameter_count();
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= count {
            return None;
        }
        let definition = &self.definitions[edit.kind.index()];
        let valid = |raw: u8, range: EffectParameterRange| {
            let v = i32::from(raw) - i32::from(range.encoded_zero);
            (i32::from(range.minimum)..=i32::from(range.maximum)).contains(&v)
        };
        if !valid(edit.value, definition.ranges[parameter])
            || edit.parameters[..count]
                .iter()
                .zip(definition.ranges)
                .any(|(&raw, r)| !valid(raw, r))
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
            if definition.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
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
                    EffectKind::new(edit.kind.id())?,
                    edit.value,
                    Default::default(),
                )?;
                if group.action == 6 {
                    mix.dry as u32
                } else {
                    mix.wet as u32
                }
            } else if group.action == 32 {
                *self.modulation_rate.get(usize::from(edit.value))?
            } else {
                let curve = match group.action {
                    9 | 76 => EffectCurve::Quadratic,
                    11 => EffectCurve::Linear,
                    12 => EffectCurve::OffsetScale,
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
            publish(
                &mut next.assignments,
                &mut batch,
                control,
                u32::from(edit.origin) + index as u32,
                value,
                u8::from(group.action == 76),
            )?;
        }
        Some(PreparedModDelayEdit { next, batch })
    }
}
