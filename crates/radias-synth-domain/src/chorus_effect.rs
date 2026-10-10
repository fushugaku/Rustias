//! Complete nine-property St.Chorus insert controller.
use crate::{
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, encode_delay_frames},
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::EffectEqualizerTables,
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};
pub struct ChorusEffectTables {
    pub ranges: [EffectParameterRange; 9],
    pub dependencies: [u32; 9],
    pub groups: [EffectCoefficientGroup; 31],
    pub time: DelayTimeTables,
    pub milliseconds: [u16; 128],
    pub modulation_rate: [u32; 128],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChorusEffectRack {
    pub times: [DelayTimeState; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct ChorusParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock: DelayClock,
}
pub struct PreparedChorusEdit {
    pub next: ChorusEffectRack,
    pub batch: EffectParameterBatch,
}
impl ChorusEffectTables {
    pub fn prepare(
        &self,
        rack: &ChorusEffectRack,
        edit: ChorusParameterEdit,
        equalizer: &EffectEqualizerTables,
    ) -> Option<PreparedChorusEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 9 {
            return None;
        }
        let valid = |raw: u8, r: EffectParameterRange| {
            let value = i32::from(raw) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&value)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit.parameters[..9]
                .iter()
                .zip(self.ranges)
                .any(|(&v, r)| !valid(v, r))
        {
            return None;
        }
        let control = EffectInterpolationControl::from_owners(
            edit.direct_switch,
            edit.parameter,
            edit.owners[0],
            edit.owners[1],
            false,
        );
        let mut next = *rack;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (index, group) in self.groups.iter().enumerate() {
            if self.dependencies[parameter] & (0x80000000 >> index) == 0 {
                continue;
            }
            if group.action == 38 {
                let mut mapped = [0; 20];
                mapped[3] = 64;
                mapped[4] = edit.parameters[4];
                mapped[5] = edit.parameters[5];
                let state = next.times[slot];
                let prepared = self.time.two_channel_scaled(
                    &mapped,
                    state,
                    edit.clock,
                    state.capacity >> 1,
                    &self.milliseconds,
                    10,
                )?;
                next.times[slot] = prepared.state;
                // SYS075618's Chorus table entry is zero: time writes remain
                // direct even when a PreDelay controller is assigned.
                let channel = match edit.parameter {
                    4 => 0,
                    5 => 1,
                    _ => return None,
                };
                batch.push_direct(
                    u32::from(edit.origin) + if channel == 0 { 7 } else { 9 },
                    encode_delay_frames(prepared.frames[channel], 7),
                )?;
                continue;
            }
            if group.action == 35 {
                // Common-effect shelves use actual stored gain and publish
                // three scalar words; the EQ/Distortion six-word path differs.
                let values = match edit.parameter {
                    7 => equalizer.low_shelf(27, (i32::from(edit.parameters[7]) - 64) as i8)?,
                    8 => equalizer.high_shelf(36, (i32::from(edit.parameters[8]) - 64) as i8)?,
                    _ => return None,
                };
                for (i, value) in values.into_iter().enumerate() {
                    batch.push_direct(u32::from(edit.origin) + index as u32 + i as u32, value)?;
                }
                continue;
            }
            let value = match group.action {
                4 | 6 => {
                    let mix =
                        EffectMix::compile(EffectKind::new(20)?, edit.value, Default::default())?;
                    if group.action == 6 {
                        mix.dry as u32
                    } else {
                        mix.wet as u32
                    }
                }
                32 => *self.modulation_rate.get(usize::from(edit.value))?,
                9 | 76 | 11 | 12 => {
                    let curve = match group.action {
                        9 | 76 => EffectCurve::Quadratic,
                        11 => EffectCurve::Linear,
                        _ => EffectCurve::OffsetScale,
                    };
                    group
                        .range
                        .compile(curve, i32::from(edit.value), group.first, group.second)?
                        as u32
                }
                _ => return None,
            };
            let p = next.assignments.prepare(CoefficientChange {
                direct_switch: edit.direct_switch,
                standalone: false,
                enabled_argument: control.enabled_argument,
                target: u32::from(edit.origin) + index as u32,
                value,
                mode: u8::from(group.action == 76),
            });
            batch.append(&p.plan)?;
            next.assignments = p.next;
        }
        Some(PreparedChorusEdit { next, batch })
    }
}
