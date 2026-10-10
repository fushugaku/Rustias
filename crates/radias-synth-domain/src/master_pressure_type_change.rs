//! Complete busy type-change branch of SYS07C17E, after optional release.
use crate::{
    effect_parameters::EffectParameterBatch,
    effect_rack_rebuild::{
        EffectRackRebuildContext, EffectRackRebuildStep, EffectRackRebuildTables,
        PreparedEffectRackRebuild,
    },
    effect_transition_queue::EffectTransitionQueueState,
    insert_effect_control::{InsertControlState, InsertControlStep},
    master_assignment_release::MasterAssignmentRelease,
};
/// The source optional release can suspend inside its full producer queue.
/// Retain that operation until the threshold test and stored-rack rebuild.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterPressureTypeChangeOperation {
    release: Option<MasterAssignmentRelease>,
    prefix_complete: bool,
    complete: bool,
}
impl MasterPressureTypeChangeOperation {
    pub fn new(state: &InsertControlState, direct_switch: u32) -> Self {
        Self {
            release: (state.midi.master.control.update_marker != 0)
                .then(|| MasterAssignmentRelease::new(direct_switch)),
            prefix_complete: false,
            complete: false,
        }
    }
    pub fn pending_release(&mut self) -> Option<&mut MasterAssignmentRelease> {
        if self.prefix_complete {
            None
        } else {
            self.release.as_mut()
        }
    }
    pub fn finish_prefix(&mut self) {
        self.prefix_complete = true;
    }
    pub fn is_complete(&self) -> bool {
        self.complete
    }
    pub fn finish(&mut self) {
        self.complete = true;
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MasterPressureTypeError {
    BelowThreshold,
    QueueServiceRequired,
    InvalidPreparation,
}
pub struct PreparedMasterPressureTypeChange {
    pub rebuild: PreparedEffectRackRebuild,
    pub release: Option<EffectParameterBatch>,
    pub pressure_marker: u32,
}
impl PreparedMasterPressureTypeChange {
    pub fn steps(&self) -> impl Iterator<Item = EffectRackRebuildStep> + '_ {
        self.release
            .map(|batch| {
                EffectRackRebuildStep::Effect(InsertControlStep {
                    batch,
                    program_writes: [None; 2],
                    body_program: None,
                })
            })
            .into_iter()
            .chain(self.rebuild.steps().copied())
    }
}
impl EffectRackRebuildTables {
    pub fn prepare_master_pressure_type_change(
        &self,
        state: &InsertControlState,
        context: EffectRackRebuildContext<'_>,
    ) -> Result<PreparedMasterPressureTypeChange, MasterPressureTypeError> {
        let mut next = *state;
        let mut queue = context.queue;
        let release = if next.midi.master.control.update_marker != 0 {
            Some(
                self.rack
                    .control
                    .common
                    .release_master_assignments(
                        &mut next.midi.master.control,
                        context.rack.common.midi.direct_switch,
                    )
                    .ok_or(MasterPressureTypeError::InvalidPreparation)?,
            )
        } else {
            None
        };
        advance_release_queue(&mut queue, release.as_ref().map_or(0, |b| b.words().len()))?;
        if queue.rings[usize::from(queue.control & 1)].count < 1843 {
            return Err(MasterPressureTypeError::BelowThreshold);
        }
        // The busy source branch reconstructs the stored Program. It never
        // executes the idle default constructor using the incoming R7 kind.
        let rebuild = self
            .prepare(&next, EffectRackRebuildContext { queue, ..context })
            .ok_or(MasterPressureTypeError::InvalidPreparation)?;
        Ok(PreparedMasterPressureTypeChange {
            rebuild,
            release,
            pressure_marker: 1,
        })
    }
}
fn advance_release_queue(
    queue: &mut EffectTransitionQueueState,
    count: usize,
) -> Result<(), MasterPressureTypeError> {
    let ring = &mut queue.rings[usize::from(queue.control & 1)];
    if usize::from(ring.count) + count > 2046 {
        return Err(MasterPressureTypeError::QueueServiceRequired);
    }
    ring.write_index = ((usize::from(ring.write_index) + count) & 2047) as u16;
    ring.count += count as u16;
    Ok(())
}
