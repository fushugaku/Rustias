//! Master Early Reflect type transition and fixed delay-buffer tap addresses.
use crate::{
    early_reflect_time::EarlyReflectTimeEdit,
    effect_control::{EffectKind, EffectMix},
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_program_staging::EffectStagedProgramWrite,
    effect_routing::EffectRoutingInstance,
    master_effect_control::{MasterControlState, MasterControlTables, MasterEdit, publish},
};
impl MasterControlTables {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_early_reflect_action(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        writes: &mut [Option<EffectStagedProgramWrite>; 2],
        edit: MasterEdit,
        action: u8,
        control: EffectInterpolationControl,
    ) -> Option<()> {
        let origin = u32::from(edit.origin);
        if action == 24 {
            return publish(
                state,
                batch,
                control,
                origin + 62,
                *self.early_reflect_decay.get(usize::from(edit.value))?,
                0,
            );
        }
        if action == 59 {
            return batch.extend(&self.early_reflect.time.prepare(EarlyReflectTimeEdit {
                origin: edit.origin,
                buffer_origin: 0x2ee00,
                size: edit.parameters[2],
                pre_delay: edit.parameters[3],
            })?);
        }
        let slot = state.work_slot;
        if slot >= 20 && slot != 28 {
            return None;
        }
        let instance = EffectRoutingInstance {
            kind: 12,
            origin: edit.origin,
            parameters: edit.parameters,
        };
        if edit.direct_switch == 0 {
            // SYS076390 resets routing before bypassing Master, then invokes
            // the prefix half of SYS07D05A (20 ticks).
            batch.extend(&self.flanger.routing.prepare_master(
                edit.stored_effect_type,
                &instance,
                1,
            )?)?;
            batch.push_direct(origin, 0x7fffff)?;
            batch.push_direct(origin + 1, 0)?;
            batch.push_command(0, 0x01000014)?;
            let write = EffectStagedProgramWrite {
                selector: 3 * slot,
                word: self.early_reflect.transition_programs[4][0],
            };
            batch.push_command(edit.prefix_origin, 0x02000000 | u32::from(write.selector))?;
            writes[0] = Some(write);
        }
        for (i, &word) in self
            .early_reflect
            .type_coefficients
            .get(usize::from(edit.value))?
            .iter()
            .enumerate()
        {
            batch.push_direct(origin + 13 + i as u32, word)?;
        }
        if edit.direct_switch == 0 {
            // SYS07CFFC selects Master Early Reflect's 67/68 pair, with the
            // 70-tick wait preceding mute and tail storage.
            batch.push_command(0, 0x01000046)?;
            batch.push_direct(origin + 68, 0)?;
            batch.push_direct(origin + 67, 0x7fffff)?;
            let write = EffectStagedProgramWrite {
                selector: 3 * slot + 2,
                word: self.early_reflect.transition_programs[4][1],
            };
            batch.push_command(edit.prefix_origin, 0x02000000 | u32::from(write.selector))?;
            writes[1] = Some(write);
            batch.push_command(0, 0x01000001)?;
            state.work_slot = if slot >= 19 { 0 } else { slot + 1 };
            batch.push_direct(origin + 68, 0x7f150f)?;
            batch.push_direct(origin + 67, 0xeaf0)?;
            batch.push_command(0, 0x01000001)?;
            let mix =
                EffectMix::compile(EffectKind::new(12)?, edit.parameters[0], Default::default())?;
            batch.push_direct(origin, mix.dry as u32)?;
            batch.push_direct(origin + 1, mix.wet as u32)?;
            batch.extend(&self.flanger.routing.prepare_master(
                edit.stored_effect_type,
                &instance,
                0,
            )?)?;
        }
        Some(())
    }
}
