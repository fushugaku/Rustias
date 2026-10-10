//! Master Talking coefficient and selected/normal program transition controller.
use crate::{
    effect_control::{
        EffectBank, EffectKind, EffectLoad, EffectMix, EffectOrigins, EffectRequest, PreparedEffect,
    },
    effect_curves::EffectCurve,
    effect_midi::effect_controller_level,
    effect_parameters::{EffectInterpolationControl, EffectParameterBatch},
    effect_program_staging::EffectStagedProgramWrite,
    effect_routing::EffectRoutingInstance,
    master_effect_control::{
        MasterControlState, MasterControlTables, MasterEdit, PreparedMasterEdit, publish,
    },
    talking_effect::TalkingEdit,
};
impl MasterControlTables {
    pub fn prepare_talking_midi(
        &self,
        state: &MasterControlState,
        edit: MasterEdit,
    ) -> Option<PreparedMasterEdit> {
        if edit.kind != 30 {
            return None;
        }
        for (&value, range) in edit.parameters.iter().zip(self.definitions[30].ranges) {
            let decoded = i32::from(value) - i32::from(range.encoded_zero);
            if !(i32::from(range.minimum)..=i32::from(range.maximum)).contains(&decoded) {
                return None;
            }
        }
        let mut next = *state;
        let mut batch = EffectParameterBatch::from_lfo(None);
        if next.midi_binding.source != 0 {
            let value = edit
                .midi
                .value(4, next.midi_binding.source.try_into().ok()?)?;
            if value != next.midi_binding.values[0] {
                if edit.parameters[7] == 2 {
                    let level = effect_controller_level(value);
                    let level = if edit.polarity.bipolar(edit.parameters[19]) {
                        let x = level.wrapping_add(0x7fffff);
                        x.wrapping_add(i32::from(x < 0)) >> 1
                    } else {
                        level
                    };
                    batch.push_direct(u32::from(edit.origin) + 5, level as u32)?;
                }
                next.midi_binding.values[0] = value;
            }
        }
        Some(PreparedMasterEdit {
            next,
            batch,
            program_writes: [None; 2],
            body_program: None,
        })
    }
    pub(crate) fn prepare_talking_coefficient(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        edit: MasterEdit,
        action: u8,
        control: EffectInterpolationControl,
    ) -> Option<()> {
        let p = edit.parameters;
        if action == 74 {
            self.talking.voice(
                batch,
                TalkingEdit {
                    slot: 0,
                    parameter: edit.parameter,
                    value: edit.value,
                    parameters: p,
                    previous_parameters: edit.previous_parameters,
                    origin: edit.origin,
                    relocation_origin: edit.relocation_origin,
                    prefix_origin: edit.prefix_origin,
                    body_origin: edit.body_origin,
                    owners: [state.owner, 0],
                    direct_switch: edit.direct_switch,
                    update_marker: edit.transition_marker,
                    clock_rate: edit.clock_rate,
                },
            )?;
        } else {
            let response = if p[7] == 0 {
                self.talking.positive_range.compile(
                    EffectCurve::Linear,
                    i32::from(p[9]),
                    12800,
                    256,
                )? as u32
            } else {
                self.talking.response[usize::from(p[9])]
            };
            let value = self.talking.centered_range.compile(
                EffectCurve::OffsetScale,
                i32::from(p[8]),
                response as i32,
                (response as i32).wrapping_neg(),
            )? as u32;
            publish(state, batch, control, u32::from(edit.origin) + 62, value, 0)?;
            if matches!(edit.parameter, 7 | 9) {
                batch.push_direct(u32::from(edit.origin) + 15, response)?;
                batch.push_direct(
                    u32::from(edit.origin) + 16,
                    self.talking.damping[usize::from(p[9])],
                )?;
            }
        }
        Some(())
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_talking_mode(
        &self,
        state: &mut MasterControlState,
        batch: &mut EffectParameterBatch,
        writes: &mut [Option<EffectStagedProgramWrite>; 2],
        body: &mut Option<PreparedEffect>,
        edit: MasterEdit,
    ) -> Option<()> {
        let p = edit.parameters;
        let previous = edit.previous_parameters[7];
        let changed = p[7] != previous && (p[7] == 0 || previous == 0);
        let slot = state.work_slot;
        if slot >= 20 && slot != 28 {
            return None;
        }
        let instance = EffectRoutingInstance {
            kind: 30,
            origin: edit.origin,
            parameters: p,
        };
        let origin = u32::from(edit.origin);
        if changed {
            if edit.direct_switch == 0 {
                // SYS07CBE4 ignores stored enable and always restores the
                // current nonzero object kind, rather than an insert pair.
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
                    word: self.talking.transition_programs[4][0],
                };
                batch.push_command(edit.prefix_origin, 0x02000000 | u32::from(write.selector))?;
                writes[0] = Some(write);
            }
            if edit.direct_switch == 0 || edit.transition_marker != 0 {
                let prepared = PreparedEffect::compile(
                    EffectRequest {
                        kind: EffectKind::new(30)?,
                        bank: EffectBank::Master,
                        load: if slot == 28 {
                            EffectLoad::ParameterSelected
                        } else {
                            EffectLoad::Default
                        },
                        work_slot: u16::from(slot),
                        selector_byte: p[7],
                        origins: EffectOrigins {
                            program: edit.body_origin,
                            data: edit.origin,
                            coefficients: edit.relocation_origin,
                        },
                    },
                    &self.master_talking_programs[usize::from(p[7] != 0)],
                    self.talking.program_layout,
                )
                .ok()?;
                batch.push_command(prepared.blocks[0].destination, prepared.blocks[0].tag)?;
                *body = Some(prepared);
            }
        }
        let mode = match p[7] {
            0 => [0, 0],
            1 => [0, 0x7fffff],
            2 => [0x7fffff, 0],
            _ => return None,
        };
        batch.push_direct(origin + 19, mode[0])?;
        batch.push_direct(origin + 20, mode[1])?;
        if p[7] == 0 {
            batch.push_direct(
                origin + 17,
                self.talking.positive_range.compile(
                    EffectCurve::Linear,
                    i32::from(p[10]),
                    0x7fffff,
                    0,
                )? as u32,
            )?;
            batch.push_direct(
                origin + 61,
                self.talking.centered_range.compile(
                    EffectCurve::OffsetQuadratic,
                    i32::from(p[10]),
                    0x7fffff,
                    -0x7fffff,
                )? as u32,
            )?;
        }
        if changed && edit.direct_switch == 0 {
            // Master mute offsets for Talking are 56/57, SYS07CFFC.
            batch.push_command(0, 0x01000046)?;
            batch.push_direct(origin + 57, 0)?;
            batch.push_direct(origin + 56, 0x7fffff)?;
            let write = EffectStagedProgramWrite {
                selector: 3 * slot + 2,
                word: self.talking.transition_programs[4][1],
            };
            batch.push_command(edit.prefix_origin, 0x02000000 | u32::from(write.selector))?;
            writes[1] = Some(write);
            batch.push_command(0, 0x01000001)?;
            state.work_slot = if slot >= 19 { 0 } else { slot + 1 };
            batch.push_direct(origin + 57, 0x7f150f)?;
            batch.push_direct(origin + 56, 0xeaf0)?;
            batch.push_command(0, 0x01000001)?;
            batch.extend(&self.flanger.routing.prepare_master(
                edit.stored_effect_type,
                &instance,
                0,
            )?)?;
            let mix = EffectMix::compile(EffectKind::new(30)?, p[0], Default::default())?;
            batch.push_direct(origin, mix.dry as u32)?;
            batch.push_direct(origin + 1, mix.wet as u32)?;
        }
        Some(())
    }
}
