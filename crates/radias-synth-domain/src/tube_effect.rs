//! Original TubePreAmpSim insert parameter dependency compiler.
use crate::{
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::{
        EffectCoefficientGroup, EffectInterpolationControl, EffectParameterBatch,
        PreparedEffectParameterChange,
    },
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
};
pub struct TubeEffectTables {
    pub parameter_ranges: [EffectParameterRange; 13],
    pub dependencies: [u32; 13],
    pub groups: [EffectCoefficientGroup; 24],
    pub gain: [u32; 66],
}
#[derive(Clone, Copy)]
pub struct TubeParameterEdit {
    pub origin: u16,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub interpolation: EffectInterpolationControl,
}
impl TubeEffectTables {
    pub fn prepare(
        &self,
        assignments: &EffectCoefficientAssignments,
        edit: TubeParameterEdit,
    ) -> Option<PreparedEffectParameterChange> {
        let parameter = usize::from(edit.parameter);
        let range = *self.parameter_ranges.get(parameter)?;
        let decoded = i32::from(edit.value) - i32::from(range.encoded_zero);
        if !(i32::from(range.minimum)..=i32::from(range.maximum)).contains(&decoded) {
            return None;
        }
        for (value, range) in edit.parameters.iter().zip(&self.parameter_ranges) {
            let value = i32::from(*value) - i32::from(range.encoded_zero);
            if !(i32::from(range.minimum)..=i32::from(range.maximum)).contains(&value) {
                return None;
            }
        }
        let mut next = *assignments;
        let mut batch = EffectParameterBatch::from_lfo(None);
        for (index, group) in self.groups.iter().enumerate() {
            if self.dependencies[parameter] & (0x80000000 >> index) == 0 {
                continue;
            }
            if group.action == 42 {
                // Complete SYS078910 rereads stored saturation and bias.
                let first_stage = matches!(index, 14 | 16);
                let source = &self.groups[if first_stage { 14 } else { 15 }];
                let bias = edit.parameters[if first_stage { 4 } else { 10 }];
                let saturation = edit.parameters[if first_stage { 5 } else { 11 }];
                let coefficient = source.range.compile(
                    EffectCurve::Linear,
                    i32::from(saturation),
                    source.first,
                    source.second,
                )?;
                let product = coefficient.wrapping_mul(i32::from(bias));
                let half = product.wrapping_add(i32::from(product < 0)) >> 1;
                let divisor = i32::from(group.range.maximum) - i32::from(group.range.minimum);
                if divisor == 0 {
                    return None;
                }
                let value = half.wrapping_div(divisor);
                batch.push_direct(
                    u32::from(edit.origin) + if first_stage { 16 } else { 17 },
                    value as u32,
                )?;
                continue;
            }
            let curve = match group.action {
                9 => Some(EffectCurve::Quadratic),
                11 => Some(EffectCurve::Linear),
                13 | 16 => Some(EffectCurve::EaseOut),
                14 => Some(EffectCurve::InverseScale),
                15 => Some(EffectCurve::Select),
                _ => None,
            };
            let value = if let Some(curve) = curve {
                let value =
                    group
                        .range
                        .compile(curve, i32::from(edit.value), group.first, group.second)?;
                if group.action == 16 {
                    value.wrapping_mul(2).wrapping_add(0xff800001u32 as i32) as u32
                } else {
                    value as u32
                }
            } else if group.action == 18 {
                *self.gain.get(usize::from(edit.value.checked_sub(23)?))?
            } else {
                return None;
            };
            let prepared = next.prepare(CoefficientChange {
                direct_switch: edit.interpolation.direct_switch,
                standalone: false,
                enabled_argument: edit.interpolation.enabled_argument,
                mode: 0,
                target: u32::from(edit.origin) + index as u32,
                value,
            });
            batch.append(&prepared.plan)?;
            next = prepared.next;
        }
        Some(PreparedEffectParameterChange { next, batch })
    }
}
