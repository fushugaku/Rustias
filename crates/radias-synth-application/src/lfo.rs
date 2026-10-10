//! Controller-rate LFO pair. Sample/device cadence and destination routing are
//! supplied by the instrument; this use case has no audio/device dependencies.
use radias_synth_domain::lfo::{LfoState, LfoTables, LfoWave};

pub const LFO_COUNT: usize = if cfg!(all(feature = "web-expanded", target_arch = "wasm32")) {
    3
} else {
    2
};
/// Keep the original effect clock slots 2/3 stable; LFO 3 uses slot 4.
pub const fn synthesis_clock_slot(index: usize) -> usize {
    if index < 2 { index } else { index + 2 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LfoParameters {
    pub waveform: u8,
    pub shape: u8,
    pub frequency: u8,
    pub phase_sync: u8,
    pub frequency_offset: i8,
    pub frequency_modulation: i16,
}
impl Default for LfoParameters {
    fn default() -> Self {
        Self {
            waveform: 0,
            shape: 64,
            frequency: 0,
            phase_sync: 0,
            frequency_offset: 0,
            frequency_modulation: 0,
        }
    }
}
pub struct LfoPairController {
    pub states: [LfoState; LFO_COUNT],
    pub parameters: [LfoParameters; LFO_COUNT],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TempoSynchronizationPending;
impl LfoPairController {
    /// Original tempo branch of 016db8. Clock-delivery state is supplied by
    /// the instrument, independently of waveform and phase calculations.
    pub fn tick_with_tempo(
        &mut self,
        tables: &LfoTables,
        tempo_tables: &radias_synth_domain::lfo_tempo::LfoTempoTables,
        divisions: [u8; LFO_COUNT],
        tempo: &mut [radias_synth_domain::lfo_tempo::LfoTempoState; LFO_COUNT],
        seed: &mut u16,
    ) -> [i16; LFO_COUNT] {
        for (i, (state, p)) in self.states.iter_mut().zip(self.parameters).enumerate() {
            let increment = if p.phase_sync & 128 != 0 {
                tempo[i].phase = state.phase;
                let correction = tempo[i].phase_correction(divisions[i] & 31, tempo_tables);
                state.phase = tempo[i].phase;
                tempo[i].increment(correction, tempo_tables)
            } else {
                let rate =
                    ((p.frequency & 127) as i16 + p.frequency_offset as i16).clamp(0, 127) as usize;
                tables
                    .modulated_frequency(tables.frequency[rate], p.frequency_modulation as i32 * 2)
            };
            state.advance(tables, p.phase_sync, increment, 0, seed);
            tempo[i].phase = state.phase;
        }
        self.values(tables)
    }
    pub fn tick(
        &mut self,
        tables: &LfoTables,
        seed: &mut u16,
    ) -> Result<[i16; LFO_COUNT], TempoSynchronizationPending> {
        if self.parameters.iter().any(|p| p.phase_sync & 128 != 0) {
            return Err(TempoSynchronizationPending);
        }
        for (state, p) in self.states.iter_mut().zip(self.parameters) {
            let rate =
                ((p.frequency & 127) as i16 + p.frequency_offset as i16).clamp(0, 127) as usize;
            let increment = tables
                .modulated_frequency(tables.frequency[rate], p.frequency_modulation as i32 * 2);
            state.advance(tables, p.phase_sync, increment, 0, seed);
        }
        Ok(self.values(tables))
    }
    pub fn values(&self, tables: &LfoTables) -> [i16; LFO_COUNT] {
        core::array::from_fn(|i| {
            let p = self.parameters[i];
            let waveform = if i != 1 {
                [
                    LfoWave::Saw,
                    LfoWave::BipolarPulse,
                    LfoWave::Triangle,
                    LfoWave::SampleHold,
                ][(p.waveform & 3) as usize]
            } else {
                [
                    LfoWave::Saw,
                    LfoWave::Pulse,
                    LfoWave::Sine,
                    LfoWave::SampleHold,
                ][(p.waveform & 3) as usize]
            };
            self.states[i].value(
                tables,
                waveform,
                p.phase_sync,
                ((p.shape & 127) as i16 - 64) as i8,
            )
        })
    }
    pub fn retrigger(&mut self, shared: [LfoState; LFO_COUNT], seed: &mut u16) {
        for ((state, p), shared) in self.states.iter_mut().zip(self.parameters).zip(shared) {
            state.initialize_note(p.phase_sync, shared, seed);
        }
    }
}
