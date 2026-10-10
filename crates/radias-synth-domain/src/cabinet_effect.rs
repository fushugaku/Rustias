//! CabinetSimltr insert dependency compiler and its complete type transition.
use crate::{
    effect_control::{EffectKind, EffectMix},
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    effect_updates::{CoefficientChange, EffectCoefficientAssignments},
    program::Program,
};
pub struct CabinetEffectTables {
    pub coefficients: [[u32; 43]; 11],
    pub air: [[i32; 4]; 11],
    pub trim_range: EffectParameterRange,
    pub trim_first: i32,
    pub trim_second: i32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CabinetEffectRack {
    pub instances: [EffectRoutingInstance; 9],
    pub assignments: EffectCoefficientAssignments,
}
#[derive(Clone, Copy)]
pub struct CabinetParameterEdit {
    pub slot: u8,
    pub parameter: u8,
    pub value: u8,
    pub parameters: [u8; 20],
    pub origin: u16,
    pub owners: [u32; 2],
    pub direct_switch: u32,
}
pub struct PreparedCabinetEdit {
    pub next: CabinetEffectRack,
    pub batch: EffectParameterBatch,
}
impl CabinetEffectTables {
    pub(crate) fn air(
        &self,
        batch: &mut EffectParameterBatch,
        instance: &EffectRoutingInstance,
    ) -> Option<()> {
        let input = self.air.get(usize::from(instance.parameters[1]))?;
        let value = i32::from(instance.parameters[2]) + 127;
        for (index, coefficient) in input.iter().copied().enumerate() {
            // CMP/GT + ADDC + SHAR divides negative coefficients toward zero.
            let half = coefficient.wrapping_add(i32::from(coefficient < 0)) >> 1;
            let product = half.wrapping_mul(value);
            let high = ((i64::from(product) * i64::from(0x81020409u32 as i32)) >> 32) as i32;
            let result = product.wrapping_add(high) >> 6;
            let result = result.wrapping_add(i32::from(result < 0));
            batch.push_direct(u32::from(instance.origin) + 7 + index as u32, result as u32)?;
        }
        Some(())
    }
    pub fn prepare(
        &self,
        rack: &CabinetEffectRack,
        edit: CabinetParameterEdit,
        routing: &EffectRoutingTables,
        program: &Program,
    ) -> Option<PreparedCabinetEdit> {
        const MAX: [u8; 4] = [100, 10, 127, 127];
        let slot = usize::from(edit.slot);
        let parameter = usize::from(edit.parameter);
        if slot >= 8
            || parameter >= 4
            || edit.value > MAX[parameter]
            || edit.parameters[..4]
                .iter()
                .zip(MAX)
                .any(|(&v, max)| v > max)
        {
            return None;
        }
        let mut next = *rack;
        next.instances[slot] = EffectRoutingInstance {
            kind: 8,
            origin: edit.origin,
            parameters: edit.parameters,
        };
        let instance = &next.instances[slot];
        let origin = u32::from(instance.origin);
        let context = EffectRoutingContext::Insert((slot / 2) as u8);
        let mut batch = EffectParameterBatch::from_lfo(None);
        match edit.parameter {
            0 => {
                let mix = EffectMix::compile(EffectKind::new(8)?, edit.value, Default::default())?;
                batch.push_direct(origin, mix.dry as u32)?;
                batch.push_direct(origin + 1, mix.wet as u32)?;
            }
            1 => {
                if edit.direct_switch == 0 {
                    batch.push_direct(origin, 0x7fffff)?;
                    batch.push_direct(origin + 1, 0)?;
                    batch.extend(&routing.prepare(program, &next.instances, context, 1)?)?;
                    batch.push_command(0, 0x01000014)?;
                }
                for (index, &value) in self.coefficients[usize::from(instance.parameters[1])]
                    .iter()
                    .enumerate()
                {
                    batch.push_direct(origin + 11 + index as u32, value)?;
                }
                self.air(&mut batch, instance)?;
                if edit.direct_switch == 0 && program.timbre(slot / 2)?.effect(slot % 2)?.enabled()
                {
                    batch.extend(&routing.prepare_input_mute(
                        program,
                        &next.instances,
                        context,
                    )?)?;
                    batch.push_command(0, 0x01000028)?;
                    let mix = EffectMix::compile(
                        EffectKind::new(8)?,
                        instance.parameters[0],
                        Default::default(),
                    )?;
                    batch.push_direct(origin, mix.dry as u32)?;
                    batch.push_direct(origin + 1, mix.wet as u32)?;
                    batch.extend(&routing.prepare(program, &next.instances, context, 0)?)?;
                }
            }
            2 => self.air(&mut batch, instance)?,
            3 => {
                let interpolation = EffectInterpolationControl::from_owners(
                    edit.direct_switch,
                    edit.parameter,
                    edit.owners[0],
                    edit.owners[1],
                    false,
                );
                let value = self.trim_range.compile(
                    EffectCurve::Linear,
                    i32::from(edit.value),
                    self.trim_first,
                    self.trim_second,
                )? as u32;
                let prepared = next.assignments.prepare(CoefficientChange {
                    direct_switch: edit.direct_switch,
                    standalone: false,
                    enabled_argument: interpolation.enabled_argument,
                    mode: 0,
                    target: origin + 5,
                    value,
                });
                batch.append(&prepared.plan)?;
                next.assignments = prepared.next;
            }
            _ => return None,
        }
        Some(PreparedCabinetEdit { next, batch })
    }
}
