//! Whole SYS07C0B8 initial Master program/coefficient/LFO publication.
use crate::{
    effect_buffers::effect_uses_buffer,
    effect_control::{
        EffectBank, EffectBufferLayout, EffectKind, EffectLoad, EffectOrigins, EffectRequest,
        PreparedEffect,
    },
    effect_lfo_program::EffectLfoSlot,
    effect_parameters::EffectParameterBatch,
    effect_routing::EffectRoutingInstance,
    effect_updates::UNASSIGNED_TARGET,
    master_effect_buffers::{MasterBufferState, relocate_master_template},
    master_effect_control::{MasterControlState, MasterControlTables},
};
pub struct MasterInitialProgram {
    pub bytes: [u8; 720],
    pub source_words: u8,
}
#[derive(Clone, Copy, Debug)]
pub struct MasterInitialCoefficients {
    pub words: [u32; 73],
    pub count: u8,
}
pub struct MasterInitializationTables {
    pub programs: [MasterInitialProgram; 31],
    pub coefficients: [MasterInitialCoefficients; 31],
    pub reverb: [MasterInitialCoefficients; 6],
    pub talking: [MasterInitialCoefficients; 3],
    pub program_layout: EffectBufferLayout,
}
#[derive(Clone, Copy)]
pub struct MasterInitialization {
    pub kind: u8,
    pub parameters: [u8; 20],
    pub origins: EffectOrigins,
    pub direct_switch: u32,
    pub clock_rate: u32,
}
pub struct PreparedMasterInitialization {
    pub next: MasterControlState,
    pub batch: EffectParameterBatch,
    pub program: PreparedEffect,
}
impl MasterControlTables {
    pub(crate) fn release_master_assignments(
        &self,
        next: &mut MasterControlState,
        direct_switch: u32,
    ) -> Option<EffectParameterBatch> {
        let mut batch = EffectParameterBatch::from_lfo(None);
        for record in &mut next.assignments.slots {
            if record.target == UNASSIGNED_TARGET {
                continue;
            }
            batch.push_direct(record.target, record.last_value)?;
            record.target = UNASSIGNED_TARGET;
            record.last_value = 0;
            for (i, value) in [UNASSIGNED_TARGET, 0, 0x7a9765, 0x5689a]
                .into_iter()
                .enumerate()
            {
                batch.push_command(
                    record.indices[i],
                    (value & 0xffffff)
                        | if direct_switch == 0 {
                            (0x84 - i as u32) << 24
                        } else {
                            0
                        },
                )?;
            }
        }
        next.update_marker = 0;
        Some(batch)
    }
    pub fn prepare_master_initialization(
        &self,
        state: &MasterControlState,
        tables: &MasterInitializationTables,
        edit: MasterInitialization,
    ) -> Option<PreparedMasterInitialization> {
        let definition = self.definitions.get(usize::from(edit.kind))?;
        if state.work_slot >= 20 && state.work_slot != 28 {
            return None;
        }
        let mut next = *state;
        let mut batch = self.release_master_assignments(&mut next, edit.direct_switch)?;
        batch.extend(&self.flanger.routing.prepare_master(
            edit.kind,
            &EffectRoutingInstance {
                kind: edit.kind,
                origin: edit.origins.data,
                parameters: edit.parameters,
            },
            1,
        )?)?;
        let template = tables.programs.get(usize::from(edit.kind))?;
        let program = PreparedEffect::compile(
            EffectRequest {
                kind: EffectKind::new(edit.kind)?,
                bank: EffectBank::Master,
                load: if state.work_slot == 28 {
                    EffectLoad::ParameterSelected
                } else {
                    EffectLoad::Default
                },
                work_slot: u16::from(state.work_slot),
                selector_byte: 0,
                origins: edit.origins,
            },
            template
                .bytes
                .get(..usize::from(template.source_words) * 6)?,
            tables.program_layout,
        )
        .ok()?;
        batch.push_command(program.blocks[0].destination, program.blocks[0].tag)?;
        let coefficients = match edit.kind {
            11 => tables.reverb.get(usize::from(edit.parameters[1]))?,
            30 => tables.talking.get(usize::from(edit.parameters[7]))?,
            _ => tables.coefficients.get(usize::from(edit.kind))?,
        };
        let count = usize::from(coefficients.count);
        next.coefficient_scratch
            .get_mut(..count)?
            .copy_from_slice(coefficients.words.get(..count)?);
        next.coefficient_scratch[0] = 0x7fffff;
        next.coefficient_scratch[1] = 0;
        let lfo = next.lfo.prepare(
            &edit.parameters,
            definition.lfo_mapping,
            EffectLfoSlot::new(8)?,
            1,
            edit.clock_rate,
            &self.tempo,
        )?;
        if let Some(publication) = lfo {
            next.lfo = publication.program;
        }
        batch.extend(&EffectParameterBatch::from_lfo(lfo))?;
        if effect_uses_buffer(u32::from(edit.kind)) {
            let relocated = relocate_master_template(
                edit.kind,
                MasterBufferState {
                    capacity: next.delay.capacity,
                },
                &next.coefficient_scratch[..count],
            )?;
            next.delay.capacity = relocated.next.capacity;
            next.coefficient_scratch[..count].copy_from_slice(&relocated.words[..count]);
        }
        // SYS07AC32 patches two scratch pointers beyond Distortion's uploaded
        // 43-word range; those writes still affect later initializations.
        if edit.kind == 7 {
            next.coefficient_scratch[46] = u32::from(edit.origins.data) + 6;
            next.coefficient_scratch[47] = u32::from(edit.origins.data) + 11;
        }
        for (i, &value) in next.coefficient_scratch[..count].iter().enumerate() {
            batch.push_direct(u32::from(edit.origins.data.wrapping_add(i as u16)), value)?;
        }
        batch.push_command(0, 0x01000046)?;
        Some(PreparedMasterInitialization {
            next,
            batch,
            program,
        })
    }
}
