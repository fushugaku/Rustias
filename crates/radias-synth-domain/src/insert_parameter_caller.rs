//! Parameter branch of SYS07B3DC for all eight physical Insert slots.
use crate::{
    effect_parameter_caller::{
        EffectParameterCallerTables, PreparedEffectParameterCaller, mode_owner_change,
    },
    insert_effect_control::{
        InsertControlContext, InsertControlState, InsertControlStep, InsertControlTables,
    },
    program::Program,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertParameterChange {
    pub slot: u8,
    pub parameter: u8,
}
impl InsertControlTables {
    pub fn prepare_insert_parameter_caller(
        &self,
        state: &InsertControlState,
        edit: InsertParameterChange,
        context: InsertControlContext<'_>,
        tables: &EffectParameterCallerTables,
    ) -> Option<PreparedEffectParameterCaller> {
        let slot = usize::from(edit.slot);
        let p = usize::from(edit.parameter);
        let instance = state.midi.inserts.get(slot)?;
        let kind = usize::from(instance.buffer.kind);
        let definition = self.definitions.get(kind)?;
        if p >= definition.parameter_count {
            return None;
        }
        let offset = 168 + 228 * (slot / 2) + 24 * (slot % 2);
        let mut out = PreparedEffectParameterCaller {
            next: *state,
            program: context.program.clone(),
            steps: [None; 2],
            step_count: 0,
        };
        let mut bytes = *out.program.bytes();
        if state.midi.master.control.update_marker != 0 {
            let batch = self.common.release_master_assignments(
                &mut out.next.midi.master.control,
                context.midi.direct_switch,
            )?;
            out.steps[0] = Some(InsertControlStep {
                batch,
                program_writes: [None; 2],
                body_program: None,
            });
            out.step_count = 1;
        }
        if bytes[offset] & 127 != 0 || edit.parameter == 0 {
            let [mode, time] = *tables.time_parameters.get(kind)?;
            let i = &mut out.next.midi.inserts[slot];
            if mode != 0
                && edit.parameter == mode
                && i.buffer.parameters[p] != i.previous_parameters[p]
            {
                let time = usize::from(time);
                let old = i.controller_offset;
                i.controller_offset = definition.ranges[time]
                    .clamp_encoded(bytes[offset + 4 + time])
                    .wrapping_sub(definition.ranges[time].encoded_zero);
                bytes[offset + 4 + time] = old;
                i.buffer.parameters[time] = old;
            }
            let program = Program::from_bytes(&bytes).ok()?;
            let (next, step) = self.prepare_stored_parameter(
                &out.next,
                edit.slot,
                edit.parameter,
                bytes[offset + 4 + p],
                InsertControlContext {
                    program: &program,
                    ..context
                },
            )?;
            out.next = next;
            out.steps[out.step_count] = Some(step);
            out.step_count += 1;
            let i = &mut out.next.midi.inserts[slot];
            i.previous_parameters[p] = i.buffer.parameters[p];
        }
        let i = &mut out.next.midi.inserts[slot];
        if let Some([zero, nonzero, selected]) = mode_owner_change(
            *tables.owner_modes.get(kind)?,
            edit.parameter,
            i.buffer.parameters[p],
        ) {
            for (index, owner) in i.owners.iter_mut().enumerate() {
                if *owner == u32::from(zero) || *owner == u32::from(nonzero) {
                    *owner = u32::from(selected);
                    bytes[offset + 2 + index] =
                        (bytes[offset + 2 + index] & 0xe0) | (selected & 31);
                }
            }
        }
        out.program = Program::from_bytes(&bytes).ok()?;
        Some(out)
    }
}
