//! Dynamics edits are accepted atomically before assignment state is committed.
use crate::{effect_parameters::EffectParameterQueue, effects::EffectUpdateController};
use radias_synth_domain::{
    dynamics_effect::{DynamicsEffectKind, DynamicsEffectTables, DynamicsParameterChange},
    effect_parameters::EffectInterpolationControl,
};

#[derive(Clone, Copy)]
pub struct DynamicsParameterRequest {
    pub kind: DynamicsEffectKind,
    pub origin: u16,
    pub parameter: u8,
    pub value: u8,
    pub direct_switch: u32,
    pub owners: [u32; 2],
    pub master: bool,
}
#[derive(Debug, PartialEq, Eq)]
pub enum DynamicsEffectError<E> {
    ParameterValue,
    Queue(E),
}
impl EffectUpdateController {
    pub fn change_dynamics_parameter<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &DynamicsEffectTables,
        request: DynamicsParameterRequest,
    ) -> Result<(), DynamicsEffectError<Q::Error>> {
        let interpolation = EffectInterpolationControl::from_owners(
            request.direct_switch,
            request.parameter,
            request.owners[0],
            request.owners[1],
            request.master,
        );
        let prepared = tables
            .prepare(
                &self.assignments,
                DynamicsParameterChange {
                    kind: request.kind,
                    origin: request.origin,
                    parameter: request.parameter,
                    value: request.value,
                    interpolation,
                },
            )
            .ok_or(DynamicsEffectError::ParameterValue)?;
        queue
            .enqueue_parameter(&prepared.batch)
            .map_err(DynamicsEffectError::Queue)?;
        self.assignments = prepared.next;
        Ok(())
    }
}
