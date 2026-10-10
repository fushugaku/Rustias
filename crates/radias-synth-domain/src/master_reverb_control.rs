//! Master Reverb algorithm, coefficient scratch, program and parameter lifecycle.
use crate::{
    effect_control::{
        EffectBank, EffectKind, EffectLoad, EffectMix, EffectOrigins, EffectRequest, PreparedEffect,
    },
    effect_curves::{EffectCurve, EffectParameterRange},
    effect_parameters::EffectParameterBatch,
    effect_program_staging::EffectStagedProgramWrite,
    effect_routing::EffectRoutingInstance,
    master_effect_buffers::{MasterBufferState, relocate_master_template},
    master_effect_control::{MasterControlState, MasterControlTables, MasterEdit},
    reverb_time::{ReverbTimeEdit, ReverbTimeTables},
};
pub struct MasterReverbTables {
    pub time: ReverbTimeTables,
    pub coefficients: [[u32; 73]; 6],
    pub programs: [[u8; 720]; 2],
    pub pre_delay: [u8; 128],
    pub damping_range: EffectParameterRange,
    pub depth_range: EffectParameterRange,
}
impl MasterControlTables {
    fn reverb_depth(
        &self,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        intensity: bool,
    ) -> Option<()> {
        if edit.parameters[1] < 4 {
            return Some(());
        }
        let origin = u32::from(edit.origin);
        let parameter = if intensity { 10 } else { 9 };
        let peaks: &[i32] = if intensity {
            &[0x7fffff]
        } else {
            &[0x600000, 0x7fffff, 0x300000, 0x480000]
        };
        for (i, &peak) in peaks.iter().enumerate() {
            batch.push_direct(
                origin + if intensity { 55 } else { 56 + i as u32 },
                self.reverb.depth_range.compile(
                    EffectCurve::Linear,
                    i32::from(edit.parameters[parameter]),
                    peak,
                    0,
                )? as u32,
            )?;
        }
        Some(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_reverb_action(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        writes: &mut [Option<EffectStagedProgramWrite>; 2],
        body: &mut Option<PreparedEffect>,
        edit: MasterEdit,
        action: u8,
    ) -> Option<()> {
        let p = edit.parameters;
        let origin = u32::from(edit.origin);
        if action == 38 {
            let frames = u32::from(*self.reverb.pre_delay.get(usize::from(p[4]))?) * 48;
            return batch.push_direct(origin + 5, frames.saturating_sub(1).max(1) + 0x2ee00);
        }
        if action == 56 {
            let peaks = if p[1] < 4 {
                [0x7f1d74, 0x7b1a75, 0x710707, 0x6fe86c]
            } else {
                [0x7f097e, 0x659e2f, 0x747052, 0x77f652]
            };
            for (i, peak) in peaks.into_iter().enumerate() {
                batch.push_direct(
                    origin + 40 + i as u32,
                    self.reverb.damping_range.compile(
                        EffectCurve::Quadratic,
                        i32::from(p[3]),
                        peak,
                        0,
                    )? as u32,
                )?;
            }
            return Some(());
        }
        if action == 57 || action == 58 {
            return self.reverb_depth(batch, edit, action == 58);
        }
        if action == 54 {
            let slot = state.work_slot;
            if slot >= 20 && slot != 28 {
                return None;
            }
            let instance = EffectRoutingInstance {
                kind: 11,
                origin: edit.origin,
                parameters: p,
            };
            if edit.direct_switch == 0 {
                batch.push_direct(origin, 0x7fffff)?;
                batch.push_direct(origin + 1, 0)?;
                batch.extend(&self.flanger.routing.prepare_master(
                    edit.stored_effect_type,
                    &instance,
                    1,
                )?)?;
                batch.push_command(0, 0x01000014)?;
                let write = EffectStagedProgramWrite {
                    selector: 3 * slot,
                    word: self.early_reflect.transition_programs[4][0],
                };
                batch.push_command(edit.prefix_origin, 0x02000000 | u32::from(write.selector))?;
                writes[0] = Some(write);
            }
            if (edit.direct_switch == 0 || edit.transition_marker != 0)
                && ((p[1] >= 4) != (edit.previous_parameters[1] >= 4))
            {
                let prepared = PreparedEffect::compile(
                    EffectRequest {
                        kind: EffectKind::new(11)?,
                        bank: EffectBank::Master,
                        load: if slot == 28 {
                            EffectLoad::ParameterSelected
                        } else {
                            EffectLoad::Default
                        },
                        work_slot: u16::from(slot),
                        selector_byte: p[1],
                        origins: EffectOrigins {
                            program: edit.body_origin,
                            data: edit.origin,
                            coefficients: edit.relocation_origin,
                        },
                    },
                    &self.reverb.programs[usize::from(p[1] >= 4)],
                    self.talking.program_layout,
                )
                .ok()?;
                batch.push_command(prepared.blocks[0].destination, prepared.blocks[0].tag)?;
                *body = Some(prepared);
            }
            let template = relocate_master_template(
                11,
                MasterBufferState {
                    capacity: state.delay.capacity,
                },
                self.reverb.coefficients.get(usize::from(p[1]))?,
            )?;
            state
                .coefficient_scratch
                .copy_from_slice(&template.words[..73]);
            state.delay.capacity = template.next.capacity;
            for (i, &word) in state.coefficient_scratch.iter().enumerate().skip(2) {
                if matches!(i, 5 | 31 | 36..=43 | 47..=53) || (p[1] >= 4 && matches!(i, 55..=59)) {
                    continue;
                }
                batch.push_direct(origin + i as u32, word)?;
            }
            self.reverb_depth(batch, edit, false)?;
            self.reverb_depth(batch, edit, true)?;
            if edit.direct_switch == 0 {
                // SYS079EB0's algorithm coefficient settle wait is distinct
                // from the following SYS07D05A tail wait.
                batch.push_command(0, 0x01000046)?;
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
                batch.extend(&self.flanger.routing.prepare_master(
                    edit.stored_effect_type,
                    &instance,
                    0,
                )?)?;
                if edit.stored_enabled {
                    let mix = EffectMix::compile(EffectKind::new(11)?, p[0], Default::default())?;
                    batch.push_direct(origin, mix.dry as u32)?;
                    batch.push_direct(origin + 1, mix.wet as u32)?;
                }
            }
        }
        batch.extend(&self.reverb.time.prepare(ReverbTimeEdit {
            bank: EffectBank::Master,
            origin: edit.origin,
            effect_type: p[1],
            time: p[2],
        })?)
    }
}
