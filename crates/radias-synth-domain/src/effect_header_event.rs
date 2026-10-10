//! Enable and owner branches of original SYS07B3DC / SYS07C17E.
use crate::{
    effect_parameter_caller::PreparedEffectParameterCaller,
    insert_effect_control::{InsertControlState, InsertControlStep, InsertControlTables},
    mixed_effect_parameter::EffectParameterTarget,
    program::Program,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectHeaderEvent {
    Enabled,
    Owner(u8),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectHeaderChange {
    pub target: EffectParameterTarget,
    pub event: EffectHeaderEvent,
}
impl InsertControlTables {
    /// The generic setter has already updated the stored header. These source
    /// event branches update the runtime object and coefficient assignments.
    pub fn prepare_effect_header_event(
        &self,
        state: &InsertControlState,
        program: &Program,
        edit: EffectHeaderChange,
        direct_switch: u32,
    ) -> Option<PreparedEffectParameterCaller> {
        let (offset, slot) = match edit.target {
            EffectParameterTarget::Insert(slot) if slot < 8 => (
                168 + 228 * usize::from(slot / 2) + 24 * usize::from(slot % 2),
                Some(usize::from(slot)),
            ),
            EffectParameterTarget::Insert(_) => return None,
            EffectParameterTarget::Master => (1038, None),
        };
        let mut out = PreparedEffectParameterCaller {
            next: *state,
            program: program.clone(),
            steps: [None; 2],
            step_count: 0,
        };
        if out.next.midi.master.control.update_marker != 0 {
            let batch = self
                .common
                .release_master_assignments(&mut out.next.midi.master.control, direct_switch)?;
            out.steps[0] = Some(InsertControlStep {
                batch,
                program_writes: [None; 2],
                body_program: None,
            });
            out.step_count = 1;
        }
        match edit.event {
            EffectHeaderEvent::Enabled => {
                let enabled = u32::from(program.bytes()[offset] & 128 != 0);
                if let Some(slot) = slot {
                    out.next.midi.inserts[slot].enabled_argument = enabled;
                } else {
                    out.next.midi.master.enabled_argument = enabled;
                }
            }
            EffectHeaderEvent::Owner(index) => {
                if let Some(slot) = slot {
                    if index >= 2 {
                        return None;
                    }
                    out.next.midi.inserts[slot].owners[usize::from(index)] =
                        u32::from(program.bytes()[offset + 2 + usize::from(index)] & 31);
                } else {
                    if index != 0 {
                        return None;
                    }
                    out.next.midi.master.control.owner =
                        u32::from(program.bytes()[offset + 1] & 31);
                }
                let batch = self
                    .common
                    .release_master_assignments(&mut out.next.midi.master.control, direct_switch)?;
                out.steps[out.step_count] = Some(InsertControlStep {
                    batch,
                    program_writes: [None; 2],
                    body_program: None,
                });
                out.step_count += 1;
            }
        }
        Some(out)
    }
}
