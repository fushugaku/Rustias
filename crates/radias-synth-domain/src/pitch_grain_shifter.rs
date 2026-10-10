//! Complete Pitch Shifter and Grain Shifter insert parameter controllers.
use crate::{
    delay_time::{DelayClock, DelayTimeState, DelayTimeTables, divide_192, encode_delay_frames},
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_equalizer::multiply,
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PitchGrainKind {
    Pitch,
    Grain,
}
impl PitchGrainKind {
    pub fn index(self) -> usize {
        usize::from(self == Self::Grain)
    }
    pub fn id(self) -> u8 {
        26 + self.index() as u8
    }
    pub fn parameter_count(self) -> usize {
        if self == Self::Pitch { 12 } else { 11 }
    }
}
pub struct PitchGrainDefinition {
    pub ranges: [EffectParameterRange; 12],
    pub dependencies: [[u32; 2]; 12],
    pub groups: [EffectCoefficientGroup; 42],
    pub group_count: usize,
    pub lfo_mapping: EffectLfoMapping,
}
pub struct PitchGrainTables {
    pub definitions: [PitchGrainDefinition; 2],
    pub pitch_ratio: [u32; 128],
    pub fine_ratio: [u32; 128],
    pub feedback_range: EffectParameterRange,
    pub time: DelayTimeTables,
    pub grain_period: [u32; 128],
    pub clock_notes: [u32; 17],
    pub tempo: LfoTempoTables,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GrainPendingTime {
    pub coefficients: [u32; 2],
    pub control_argument: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchGrainRack {
    pub times: [DelayTimeState; 8],
    pub pending: [GrainPendingTime; 8],
    pub lfos: [EffectLfoProgram; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct PitchGrainEdit {
    pub kind: PitchGrainKind,
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock: DelayClock,
    pub clock_rate: u32,
}
pub struct PreparedPitchGrainEdit {
    pub next: PitchGrainRack,
    pub batch: EffectParameterBatch,
}
fn publish(
    rack: &mut PitchGrainRack,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
) -> Option<()> {
    let p = rack.assignments.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        standalone: false,
        enabled_argument: control.enabled_argument,
        target,
        value,
        mode: 0,
    });
    batch.append(&p.plan)?;
    rack.assignments = p.next;
    Some(())
}
impl PitchGrainTables {
    pub fn pitch_word(&self, p: &[u8; 20]) -> Option<u32> {
        let ratio = multiply(
            *self.pitch_ratio.get(usize::from(p[1]))?,
            *self.fine_ratio.get(usize::from(p[2]))?,
            9,
        );
        Some(if ratio == 0x100000 {
            0
        } else {
            0xfffffu32.wrapping_sub(ratio).wrapping_mul(2)
        })
    }
    fn grain_time(&self, rack: &mut PitchGrainRack, edit: PitchGrainEdit) -> Option<()> {
        let slot = usize::from(edit.slot);
        let p = edit.parameters;
        let state = rack.times[slot];
        let mut mapped = [0; 20];
        mapped[2] = p[1];
        mapped[3] = p[2];
        mapped[4] = p[3];
        mapped[5] = p[3];
        mapped[6] = p[4];
        mapped[7] = p[4];
        let prepared = self.time.two_channel(
            &mapped,
            state,
            edit.clock,
            state.capacity >> 1,
            &self.time.stereo_milliseconds,
        )?;
        let tempo = if p[1] == 0 {
            edit.clock.tempo
        } else {
            prepared.state.cached_tempo
        };
        if tempo == 0 {
            return None;
        }
        let beat = 600000 / u32::from(tempo);
        let period = if p[5] == 0 {
            *self.grain_period.get(usize::from(p[6]))?
        } else {
            divide_192(
                beat.wrapping_mul(*self.clock_notes.get(usize::from(p[7]))?)
                    .wrapping_mul(48),
            )
        };
        rack.times[slot] = prepared.state;
        rack.pending[slot] = GrainPendingTime {
            coefficients: core::array::from_fn(|i| {
                (encode_delay_frames(prepared.frames[i].min(period), 7).max(0x1700) & 0xffffff)
                    | 0x80000000
            }),
            control_argument: 0,
        };
        Some(())
    }
    pub fn prepare(
        &self,
        rack: &PitchGrainRack,
        edit: PitchGrainEdit,
    ) -> Option<PreparedPitchGrainEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        let definition = &self.definitions[edit.kind.index()];
        if slot >= 8 || parameter >= edit.kind.parameter_count() {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let x = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&x)
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
                38 => {
                    if edit.kind == PitchGrainKind::Grain {
                        self.grain_time(&mut next, edit)?;
                    }
                } // Pitch's SYS072C4A returns.
                69 => {
                    let owner = EffectInterpolationControl::from_owners(
                        edit.direct_switch,
                        1,
                        edit.owners[0],
                        edit.owners[1],
                        false,
                    );
                    publish(
                        &mut next,
                        &mut batch,
                        owner,
                        u32::from(edit.origin) + index as u32,
                        self.pitch_word(&edit.parameters)?,
                    )?;
                }
                70 => {
                    let words = match edit.value {
                        0 => [0x1fff, 0xfffff],
                        1 => [0xfff, 0x1fffff],
                        2 => [0x3ff, 0x7fffff],
                        _ => return None,
                    };
                    for (i, v) in words.into_iter().enumerate() {
                        batch.push_direct(u32::from(edit.origin) + 9 + i as u32, v)?;
                    }
                }
                71 => {
                    let position = edit.parameters[7] != 0;
                    let value = self.feedback_range.compile(
                        EffectCurve::Quadratic,
                        i32::from(edit.parameters[8]),
                        if position { 0x7fffff } else { 0x4ccccc },
                        0,
                    )? as u32;
                    for (i, v) in if position { [0, value] } else { [value, 0] }
                        .into_iter()
                        .enumerate()
                    {
                        publish(
                            &mut next,
                            &mut batch,
                            control,
                            u32::from(edit.origin) + 13 + i as u32,
                            v,
                        )?;
                    }
                }
                4 | 6 | 11 | 13 | 14 | 72 => {
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
                            11 => EffectCurve::Linear,
                            14 => EffectCurve::InverseScale,
                            _ => EffectCurve::EaseOut,
                        };
                        let v = group.range.compile(
                            curve,
                            i32::from(edit.value),
                            group.first,
                            group.second,
                        )? as u32;
                        if group.action == 72 {
                            0x7fffffu32.wrapping_sub(v)
                        } else {
                            v
                        }
                    };
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + index as u32,
                        value,
                    )?;
                }
                _ => return None,
            }
        }
        Some(PreparedPitchGrainEdit { next, batch })
    }
}
