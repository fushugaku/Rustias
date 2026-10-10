//! Master parameter branch of SYS07C17E, including its mode/history ordering.
use crate::{
    effect_parameter_caller::mode_owner_change,
    insert_effect_control::{InsertControlState, InsertControlStep, InsertControlTables},
    master_initial_mask::MasterInitialMaskContext,
    program::Program,
};

pub use crate::effect_parameter_caller::{
    EffectParameterCallerTables as MasterParameterCallerTables,
    PreparedEffectParameterCaller as PreparedMasterParameterCaller,
};
impl InsertControlTables {
    /// Current parameters are the caller's already compiled object. The stored
    /// Program remains independent and supplies SYS07BBFA's requested byte.
    pub fn prepare_master_parameter_caller(
        &self,
        state: &InsertControlState,
        parameter: u8,
        context: MasterInitialMaskContext<'_>,
        tables: &MasterParameterCallerTables,
    ) -> Option<PreparedMasterParameterCaller> {
        let kind = usize::from(state.midi.master.kind);
        let definition = self.common.definitions.get(kind)?;
        let p = usize::from(parameter);
        if p >= definition.parameter_count {
            return None;
        }
        let mut out = PreparedMasterParameterCaller {
            next: *state,
            program: context.common.program.clone(),
            steps: [None; 2],
            step_count: 0,
        };
        let mut bytes = *out.program.bytes();
        if state.midi.master.control.update_marker != 0 {
            let batch = self.common.release_master_assignments(
                &mut out.next.midi.master.control,
                context.common.midi.direct_switch,
            )?;
            out.steps[0] = Some(InsertControlStep {
                batch,
                program_writes: [None; 2],
                body_program: None,
            });
            out.step_count = 1;
        }
        let stored_kind = bytes[1038] & 127;
        if stored_kind != 0 || parameter == 0 {
            let [mode, time] = *tables.time_parameters.get(kind)?;
            if mode != 0
                && parameter == mode
                && out.next.midi.master.parameters[p] != out.next.midi.master.previous_parameters[p]
            {
                let time = usize::from(time);
                let old = out.next.midi.master.controller_offset;
                out.next.midi.master.controller_offset = definition.ranges[time]
                    .clamp_encoded(bytes[1040 + time])
                    .wrapping_sub(definition.ranges[time].encoded_zero);
                bytes[1040 + time] = old;
                out.next.midi.master.parameters[time] = old;
            }
            let program = Program::from_bytes(&bytes).ok()?;
            let (next, step) = self.prepare_master_stored_parameter(
                &out.next,
                parameter,
                bytes[1040 + p],
                MasterInitialMaskContext {
                    common: crate::insert_effect_control::InsertControlContext {
                        program: &program,
                        ..context.common
                    },
                    ..context
                },
            )?;
            out.next = next;
            out.steps[out.step_count] = Some(step);
            out.step_count += 1;
            out.next.midi.master.previous_parameters[p] = out.next.midi.master.parameters[p];
        }
        // SYS0759DA follows the parameter callback and history copy. Each
        // triple switches between two owner numbers according to its mode.
        if let Some([zero, nonzero, selected]) = mode_owner_change(
            *tables.owner_modes.get(kind)?,
            parameter,
            out.next.midi.master.parameters[p],
        ) {
            let owner = &mut out.next.midi.master.control.owner;
            if *owner == u32::from(zero) || *owner == u32::from(nonzero) {
                *owner = u32::from(selected);
                bytes[1039] = (bytes[1039] & 0xf0) | (selected & 15);
            }
        }
        out.program = Program::from_bytes(&bytes).ok()?;
        Some(out)
    }
}
