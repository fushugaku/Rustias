//! Original shared synthesis/effect LFO phase service, SYS 016fa4/017160.
use crate::lfo::{LFO_COUNT, LfoPairController, LfoParameters};
use radias_synth_domain::{
    lfo::{LfoState, LfoTables},
    lfo_tempo::{LfoTempoState, LfoTempoTables},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectLfoParameters {
    /// Effect control byte 0: bit 7 selects the half-rate phase mode.
    pub mode: u8,
    pub frequency: u8,
    pub phase_sync: u8,
    pub beat: u8,
    /// State byte +1e is supplied by the effect controller, not phase advance.
    pub alternate_phase: u8,
}
impl EffectLfoParameters {
    pub fn with_program(
        self,
        program: radias_synth_domain::effect_lfo_program::EffectLfoProgram,
    ) -> Self {
        Self {
            mode: program.bytes[0],
            frequency: program.bytes[2],
            phase_sync: program.bytes[3],
            beat: program.bytes[4],
            ..self
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectLfoController {
    pub state: LfoState,
    pub tempo: LfoTempoState,
}
impl EffectLfoController {
    pub fn tick_free(&mut self, p: EffectLfoParameters, lfo: &LfoTables, seed: &mut u16) {
        let half = p.mode & 128 != 0;
        let increment = lfo.frequency[(p.frequency & 127) as usize] >> u32::from(half);
        let offset = if half && p.alternate_phase != 0 {
            32768
        } else {
            0
        };
        self.state
            .advance(lfo, p.phase_sync, increment, offset, seed);
        self.tempo.phase = self.state.phase;
    }
    pub fn tick(
        &mut self,
        p: EffectLfoParameters,
        lfo: &LfoTables,
        tables: &LfoTempoTables,
        seed: &mut u16,
    ) {
        let half = p.mode & 128 != 0;
        let mut increment = if p.phase_sync & 128 != 0 {
            self.tempo.phase = self.state.phase;
            let correction = self
                .tempo
                .phase_correction((p.beat & 31) | if half { 32 } else { 0 }, tables);
            self.state.phase = self.tempo.phase;
            self.tempo.increment(correction, tables)
        } else {
            lfo.frequency[(p.frequency & 127) as usize]
        };
        if half {
            increment >>= 1;
        }
        let offset = if half && p.alternate_phase != 0 {
            32768
        } else {
            0
        };
        self.state
            .advance(lfo, p.phase_sync, increment, offset, seed);
        self.tempo.phase = self.state.phase;
    }
}

pub struct SharedTimbreLfo {
    pub synthesis: LfoPairController,
    pub tempo: [LfoTempoState; LFO_COUNT],
    pub effects: [EffectLfoController; 2],
}
impl Default for SharedTimbreLfo {
    fn default() -> Self {
        Self {
            synthesis: LfoPairController {
                states: [Default::default(); LFO_COUNT],
                parameters: [LfoParameters::default(); LFO_COUNT],
            },
            tempo: [Default::default(); LFO_COUNT],
            effects: [Default::default(); 2],
        }
    }
}
impl SharedTimbreLfo {
    pub fn tick_free(
        &mut self,
        enabled: bool,
        effects: [EffectLfoParameters; 2],
        lfo: &LfoTables,
        seed: &mut u16,
    ) {
        if !enabled {
            return;
        }
        let original_parameters = self.synthesis.parameters;
        for p in &mut self.synthesis.parameters {
            p.frequency_offset = 0;
            p.frequency_modulation = 0;
        }
        let _ = self.synthesis.tick(lfo, seed);
        self.synthesis.parameters = original_parameters;
        for (controller, p) in self.effects.iter_mut().zip(effects) {
            controller.tick_free(p, lfo, seed);
        }
    }
    pub fn tick(
        &mut self,
        enabled: bool,
        divisions: [u8; LFO_COUNT],
        effects: [EffectLfoParameters; 2],
        lfo: &LfoTables,
        tables: &LfoTempoTables,
        seed: &mut u16,
    ) {
        if !enabled {
            return;
        }
        // Shared synthesis rates have no private-voice frequency offsets or
        // virtual-patch feedback inputs in original 016fa4.
        let original_parameters = self.synthesis.parameters;
        for p in &mut self.synthesis.parameters {
            p.frequency_offset = 0;
            p.frequency_modulation = 0;
        }
        self.synthesis
            .tick_with_tempo(lfo, tables, divisions, &mut self.tempo, seed);
        self.synthesis.parameters = original_parameters;
        for (controller, p) in self.effects.iter_mut().zip(effects) {
            controller.tick(p, lfo, tables, seed);
        }
    }
    pub fn pulse(&mut self, four: bool) {
        for i in 0..LFO_COUNT {
            self.tempo[i].phase = self.synthesis.states[i].phase;
            if four {
                self.tempo[i].clock_pulse_four();
            } else {
                self.tempo[i].clock_pulse_one();
            }
            if let Some(effect) = self.effects.get_mut(i) {
                effect.tempo.phase = effect.state.phase;
                if four {
                    effect.tempo.clock_pulse_four();
                } else {
                    effect.tempo.clock_pulse_one();
                }
            }
        }
    }
}
