//! SYS02c168 assignment initialization and whole SYS02b6e8 note initialization.
use crate::{
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorPlan,
    manual_parameters::{ManualCompilationError, ManualCompilerPorts, ManualCompilerTables},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionControlState {
    /// Three twelve-byte tracks, followed by the shared motion controller flags.
    pub bytes: [u8; 48],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionInitializationError {
    InvalidTrack,
    Manual(ManualCompilationError),
}

pub struct CompiledMotionInitialization {
    pub controller: ActorControlState,
    pub motion: MotionControlState,
    pub publication: DescriptorPlan,
}

#[derive(Clone, Copy)]
pub struct MotionNoteRequest<'a> {
    pub assignments: [u8; 3],
    pub program_flags: u8,
    pub global_flags: u8,
    pub body: &'a [u8; 104],
    pub ports: ManualCompilerPorts,
}

fn clipped_manual(parameter: u8, raw: u16) -> (i16, u16) {
    let limit = if matches!(parameter, 1 | 5) {
        0x1800
    } else {
        0x3f00
    };
    let value = i32::from(raw) - 0x4000;
    // SYS02b62c selects the limit before calling SYS02c1dc.
    let selection = match parameter {
        1 => 9,
        5 => 11,
        _ => 10,
    };
    let clipping = if value > limit {
        7
    } else if value < -limit {
        10
    } else {
        9
    };
    (
        value.clamp(-limit, limit) as i16,
        selection + 3 + clipping + 4,
    )
}

impl ActorControlState {
    pub fn initialize_motion_assignment(
        &self,
        assignment: u8,
        track: u8,
        mut motion: MotionControlState,
        body: &[u8; 104],
        ports: ManualCompilerPorts,
        tables: &ManualCompilerTables<'_>,
    ) -> Result<CompiledMotionInitialization, MotionInitializationError> {
        if track >= 3 {
            return Err(MotionInitializationError::InvalidTrack);
        }
        let mut publication = DescriptorPlan::default();
        if assignment == 0 || assignment > 41 {
            publication.work(if assignment == 0 { 25 } else { 28 });
            return Ok(CompiledMotionInitialization {
                controller: *self,
                motion,
                publication,
            });
        }
        let base = usize::from(track) * 12;
        let special = assignment == 4 && matches!(self.bytes[0x1e0] & 15, 6 | 7);
        let input = base + if special { 8 } else { 4 };
        let raw = u16::from_be_bytes(motion.bytes[input..input + 2].try_into().unwrap());
        let previous = u16::from_be_bytes(motion.bytes[base + 6..base + 8].try_into().unwrap());
        // The original publishes the chosen raw value to history before comparison.
        motion.bytes[base + 6..base + 8].copy_from_slice(&raw.to_be_bytes());
        let selection_work = if assignment == 4 { 32 } else { 25 };
        if raw == previous {
            publication.work(selection_work + 16);
            return Ok(CompiledMotionInitialization {
                controller: *self,
                motion,
                publication,
            });
        }
        let (value, clipping_work) = clipped_manual(assignment, raw);
        let compiled = self
            .compile_manual_parameter(assignment, value, body, ports, tables)
            .map_err(MotionInitializationError::Manual)?;
        publication.work(selection_work + 16 + clipping_work);
        publication.append_compacted(&compiled.publication);
        publication.work(8);
        Ok(CompiledMotionInitialization {
            controller: compiled.controller,
            motion,
            publication,
        })
    }

    pub fn initialize_motion_note(
        &self,
        request: MotionNoteRequest<'_>,
        motion: MotionControlState,
        tables: &ManualCompilerTables<'_>,
    ) -> Result<CompiledMotionInitialization, MotionInitializationError> {
        let mut controller = *self;
        controller.set_word(0x180, 0);
        controller.set_word(0x184, 0);
        let mut publication = DescriptorPlan::default();
        let drum = i32::from((request.ports.pitch.midi_mode & 0xe0) >> 5) - 1
            == i32::from(request.ports.pitch.timbre & 3);
        let gated_work = if motion.bytes[0x2c] & 128 == 0 {
            Some(27)
        } else if request.program_flags & 128 == 0 {
            Some(31)
        } else if drum {
            Some(52)
        } else if request.global_flags & 2 != 0 {
            Some(56)
        } else {
            None
        };
        if let Some(work) = gated_work {
            publication.work(work);
            return Ok(CompiledMotionInitialization {
                controller,
                motion,
                publication,
            });
        }
        publication.work(48);
        let mut motion = motion;
        for (track, assignment) in request.assignments.into_iter().enumerate() {
            publication.work(5);
            let initialized = controller.initialize_motion_assignment(
                assignment,
                track as u8,
                motion,
                request.body,
                request.ports,
                tables,
            )?;
            publication.append_compacted(&initialized.publication);
            controller = initialized.controller;
            motion = initialized.motion;
        }
        publication.work(6);
        Ok(CompiledMotionInitialization {
            controller,
            motion,
            publication,
        })
    }
}
