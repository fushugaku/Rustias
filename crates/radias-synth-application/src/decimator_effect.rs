//! Submit a whole St.Decimator parameter change before committing assignment
//! state. DSP compilation belongs to the domain; transport belongs to the port.
pub use crate::effect_parameters::{
    EffectControlPort, EffectParameterQueue, dispatch_complete_parameter_batch,
    dispatch_parameter_batch,
};
use crate::effects::EffectUpdateController;
use radias_synth_domain::{
    decimator_effect::{
        DecimatorEffectChange, DecimatorEffectState, DecimatorEffectTables,
        EffectInterpolationControl, EffectParameterBatch,
    },
    effect_lfo_program::{EffectLfoMapping, EffectLfoProgram, EffectLfoSlot},
    lfo_tempo::LfoTempoTables,
};
#[derive(Debug, PartialEq, Eq)]
pub enum DecimatorEffectError<E> {
    ParameterValue,
    UnsupportedParameter(u8),
    Queue(E),
}
#[derive(Clone, Copy)]
pub struct DecimatorParameterRequest {
    pub origin: u16,
    pub parameter: u8,
    pub value: u8,
    pub state: DecimatorEffectState,
    pub direct_switch: u32,
    pub owners: [u32; 2],
    pub master: bool,
}
#[derive(Clone, Copy)]
pub struct DecimatorLfoEdit {
    /// SYS's complete stored parameter snapshot; the argument value is separate.
    pub parameters: [u8; 20],
    pub mapping: EffectLfoMapping,
    pub slot: EffectLfoSlot,
    pub offset_control: u32,
    pub clock_rate: u32,
}
impl EffectUpdateController {
    pub fn change_decimator_complete<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &DecimatorEffectTables,
        request: DecimatorParameterRequest,
        current_lfo: &mut EffectLfoProgram,
        edit: DecimatorLfoEdit,
        tempo: &LfoTempoTables,
    ) -> Result<(), DecimatorEffectError<Q::Error>> {
        if request.parameter < 7 {
            return self.change_decimator_parameter(queue, tables, request);
        }
        let valid = match request.parameter {
            7 | 12 => request.value <= 1,
            8 => request.value <= 127,
            9 => request.value <= 16,
            10 => request.value <= 4,
            11 => (1..=127).contains(&request.value),
            13 => request.value <= 18,
            _ => {
                return Err(DecimatorEffectError::UnsupportedParameter(
                    request.parameter,
                ));
            }
        };
        if !valid {
            return Err(DecimatorEffectError::ParameterValue);
        }
        let publication = current_lfo
            .prepare(
                &edit.parameters,
                edit.mapping,
                edit.slot,
                edit.offset_control,
                edit.clock_rate,
                tempo,
            )
            .ok_or(DecimatorEffectError::ParameterValue)?;
        let batch = EffectParameterBatch::from_lfo(publication);
        queue
            .enqueue_parameter(&batch)
            .map_err(DecimatorEffectError::Queue)?;
        if let Some(p) = publication {
            *current_lfo = p.program;
        }
        Ok(())
    }
    /// Compile property bindings, dependent coefficients, owner routing and the
    /// complete queue batch before accepting a patch edit.
    pub fn change_decimator_parameter<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &DecimatorEffectTables,
        request: DecimatorParameterRequest,
    ) -> Result<(), DecimatorEffectError<Q::Error>> {
        let change =
            DecimatorEffectChange::from_parameter(request.parameter, request.value, request.state)
                .ok_or(DecimatorEffectError::UnsupportedParameter(
                    request.parameter,
                ))?;
        let interpolation = EffectInterpolationControl::for_decimator_parameter(
            request.direct_switch,
            request.parameter,
            request.owners[0],
            request.owners[1],
            request.master,
        );
        self.change_decimator(queue, tables, request.origin, change, interpolation)
    }
    pub fn change_decimator<Q: EffectParameterQueue>(
        &mut self,
        queue: &mut Q,
        tables: &DecimatorEffectTables,
        origin: u16,
        change: DecimatorEffectChange,
        interpolation: EffectInterpolationControl,
    ) -> Result<(), DecimatorEffectError<Q::Error>> {
        let prepared = tables
            .prepare(&self.assignments, origin, change, interpolation)
            .ok_or(DecimatorEffectError::ParameterValue)?;
        queue
            .enqueue_parameter(&prepared.batch)
            .map_err(DecimatorEffectError::Queue)?;
        self.assignments = prepared.next;
        Ok(())
    }
}
