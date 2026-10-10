//! Complete SYS078A12 St.Vibrato insert parameter control.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    effect_parameters::{EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    lfo_tempo::LfoTempoTables,
};
pub struct VibratoTables {
    pub ranges: [EffectParameterRange; 10],
    pub dependencies: [[u32; 2]; 10],
    pub groups: [EffectCoefficientGroup; 28],
    pub lfo_mapping: EffectLfoMapping,
    pub tempo: LfoTempoTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VibratoRack {
    pub lfos: [EffectLfoProgram; 8],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct VibratoEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
    pub clock_rate: u32,
}
pub struct PreparedVibratoEdit {
    pub next: VibratoRack,
    pub batch: EffectParameterBatch,
}
impl VibratoTables {
    pub fn prepare(&self, rack: &VibratoRack, edit: VibratoEdit) -> Option<PreparedVibratoEdit> {
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8 || parameter >= 10 {
            return None;
        }
        let valid = |v: u8, r: EffectParameterRange| {
            let x = i32::from(v) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&x)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit.parameters[..10]
                .iter()
                .zip(self.ranges)
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
        for (index, group) in self.groups.iter().enumerate() {
            if self.dependencies[parameter][index / 32] & (0x80000000 >> (index % 32)) == 0 {
                continue;
            }
            if group.action == 34 {
                let p = next.lfos[slot].prepare(
                    &edit.parameters,
                    self.lfo_mapping,
                    EffectLfoSlot::new(edit.slot)?,
                    0,
                    edit.clock_rate,
                    &self.tempo,
                )?;
                if let Some(p) = p {
                    next.lfos[slot] = p.program;
                }
                batch.extend(&EffectParameterBatch::from_lfo(p))?;
                continue;
            }
            let value = match group.action {
                4 | 6 => {
                    let mix =
                        EffectMix::compile(EffectKind::new(28)?, edit.value, Default::default())?;
                    if group.action == 6 {
                        mix.dry as u32
                    } else {
                        mix.wet as u32
                    }
                }
                76 => group.range.compile(
                    EffectCurve::Quadratic,
                    i32::from(edit.value),
                    group.first,
                    group.second,
                )? as u32,
                _ => return None,
            };
            let p = next.assignments.prepare(CoefficientChange {
                direct_switch: control.direct_switch,
                standalone: false,
                enabled_argument: control.enabled_argument,
                target: u32::from(edit.origin) + index as u32,
                value,
                mode: u8::from(group.action == 76),
            });
            batch.append(&p.plan)?;
            next.assignments = p.next;
        }
        Some(PreparedVibratoEdit { next, batch })
    }
}
