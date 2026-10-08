//! Steady waveform oscillator, verified against original continuous phase blocks.
//! Initial note phase and dynamic parameter compilation remain separate paths.
use crate::{
    Phase, Sample,
    pitch::PhaseIncrement,
    waveform::{ShapeParameters, Transfer, WaveformTable},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Oscillator {
    phase: Phase,
    increment: PhaseIncrement,
    edge_offset: u32,
    transfer: Transfer,
    parameters: ShapeParameters,
}

impl Oscillator {
    /// Restore a prepared oscillator. The reference adapter supplies observed
    /// state; future note-on compilation will supply the original reset state.
    pub const fn new(
        phase: Phase,
        increment: PhaseIncrement,
        edge_offset: u32,
        transfer: Transfer,
        parameters: ShapeParameters,
    ) -> Self {
        Self {
            phase,
            increment,
            edge_offset,
            transfer,
            parameters,
        }
    }

    pub const fn phase(&self) -> Phase {
        self.phase
    }
    pub const fn increment(&self) -> PhaseIncrement {
        self.increment
    }

    pub fn set_increment(&mut self, increment: PhaseIncrement) {
        self.increment = increment;
    }
    pub fn sync_phase(&mut self, phase: Phase) {
        self.phase = phase;
    }
    pub fn set_gain(&mut self, gain: i16) {
        self.parameters.gain = gain;
    }
    pub fn set_sync_window(&mut self, enabled: bool) {
        self.parameters.subtract_edge = enabled;
    }

    pub fn set_parameters(&mut self, parameters: ShapeParameters) {
        self.parameters = parameters;
    }
    pub fn retune(&mut self, increment: PhaseIncrement, edge: i16, bandwidth: i16) {
        // Original A158..A198 advances the phase before the waveform call.
        // This oscillator retains the next sample's phase, so a rate edit
        // must replace that already-applied old decrement with the new one.
        self.phase.0 = self
            .phase
            .0
            .wrapping_add(self.increment.0)
            .wrapping_sub(increment.0);
        self.increment = increment;
        self.parameters.edge_coefficient = edge;
        self.parameters.waveform_control = bandwidth;
    }
    pub fn select_transfer(&mut self, transfer: Transfer, offset: u32) {
        self.transfer = transfer;
        self.edge_offset = offset;
    }

    pub fn next_sample(&mut self, table: &WaveformTable) -> Sample {
        self.next_with_edge(table, None)
    }
    pub fn next_with_edge(&mut self, table: &WaveformTable, edge: Option<Phase>) -> Sample {
        let sample = table.sample(
            self.transfer,
            self.phase.0 as i32,
            edge.map_or(self.phase.0.wrapping_add(self.edge_offset), |phase| phase.0) as i32,
            self.parameters,
        );
        // Original sustained phase blocks descend by the stored Q0.32 increment.
        self.phase.retreat(self.increment);
        sample
    }
}
