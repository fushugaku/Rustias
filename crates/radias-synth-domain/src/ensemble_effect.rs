//! Complete three-property Ensemble insert dependency compiler.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::{
        EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch,
        PreparedEffectParameterChange,
    },
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};
pub struct EnsembleEffectTables {
    pub ranges: [EffectParameterRange; 3],
    pub dependencies: [u32; 3],
    pub groups: [EffectCoefficientGroup; 27],
    pub speed_words: [[u32; 127]; 2],
    pub speed_shape_range: EffectParameterRange,
}
#[derive(Clone, Copy)]
pub struct EnsembleParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
}
fn publish(
    assignments: &mut EffectCoefficientAssignments,
    batch: &mut EffectParameterBatch,
    control: EffectInterpolationControl,
    target: u32,
    value: u32,
    mode: u8,
) -> Option<()> {
    let p = assignments.prepare(CoefficientChange {
        direct_switch: control.direct_switch,
        enabled_argument: control.enabled_argument,
        standalone: false,
        target,
        value,
        mode,
    });
    batch.append(&p.plan)?;
    *assignments = p.next;
    Some(())
}
impl EnsembleEffectTables {
    pub fn prepare(
        &self,
        assignments: &EffectCoefficientAssignments,
        edit: EnsembleParameterEdit,
    ) -> Option<PreparedEffectParameterChange> {
        let parameter = usize::from(edit.parameter);
        if edit.slot >= 8 || parameter >= 3 {
            return None;
        }
        let valid = |raw: u8, r: EffectParameterRange| {
            let value = i32::from(raw) - i32::from(r.encoded_zero);
            (i32::from(r.minimum)..=i32::from(r.maximum)).contains(&value)
        };
        if !valid(edit.value, self.ranges[parameter])
            || edit.parameters[..3]
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
        let mut next = *assignments;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (index, group) in self.groups.iter().enumerate() {
            if self.dependencies[parameter] & (0x80000000 >> index) == 0 {
                continue;
            }
            if group.action == 60 {
                // SYS07704C always rereads stored Speed, independently of the
                // changed argument. The third coefficient is a direct write.
                let speed = edit.parameters[2];
                let lookup = usize::from(speed.checked_sub(1)?);
                for (i, table) in self.speed_words.iter().enumerate() {
                    publish(
                        &mut next,
                        &mut batch,
                        control,
                        u32::from(edit.origin) + 6 + i as u32,
                        *table.get(lookup)?,
                        0,
                    )?;
                }
                let shape = self.speed_shape_range.compile(
                    EffectCurve::EaseOut,
                    i32::from(speed),
                    0xaaaa,
                    0x0e0000,
                )?;
                batch.push_direct(u32::from(edit.origin) + 10, shape as u32)?;
                continue;
            }
            if group.action == 38 {
                continue;
            } // Ensemble's SYS072C4A returns.
            let value = match group.action {
                4 | 6 => {
                    let mix =
                        EffectMix::compile(EffectKind::new(21)?, edit.value, Default::default())?;
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
            publish(
                &mut next,
                &mut batch,
                control,
                u32::from(edit.origin) + index as u32,
                value,
                u8::from(group.action == 76),
            )?;
        }
        Some(PreparedEffectParameterChange { next, batch })
    }
}
