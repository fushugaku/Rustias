//! EG1/EG3 use cases and the original Filter1 cutoff/coefficient chain.
use crate::amplifier::ControllerTables;
use radias_synth_domain::{
    controller_filter::{ControllerFilter, ControllerFilterTables},
    filter::FilterCoefficients,
    mod_envelope::{ModEnvelope, ModEnvelopeParameters},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModEnvelopeProgram {
    pub adsr: [u8; 4],
    pub curve: u8,
    pub velocity_time_sensitivity: u8,
    pub key_tracking: u8,
    pub velocity_level_sensitivity: u8,
}
impl Default for ModEnvelopeProgram {
    fn default() -> Self {
        Self {
            adsr: [0, 0, 127, 10],
            curve: 1,
            velocity_time_sensitivity: 64,
            key_tracking: 64,
            velocity_level_sensitivity: 64,
        }
    }
}
impl ModEnvelopeProgram {
    pub fn parameters(self, note: u8, velocity: u8) -> ModEnvelopeParameters {
        ModEnvelopeParameters {
            attack: self.adsr[0],
            decay: self.adsr[1],
            sustain: self.adsr[2],
            release: self.adsr[3],
            curve: self.curve,
            velocity_sensitivity: self.velocity_time_sensitivity,
            key_tracking: self.key_tracking,
            note,
            velocity,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DynamicFilter {
    pub input: ControllerFilter,
    pub resonance: i32,
    pub normalization: i32,
    pub base: FilterCoefficients,
}
#[derive(Clone, Copy)]
pub struct VoiceEnvelopes {
    pub envelopes: [ModEnvelope; 2],
    pub programs: [ModEnvelopeProgram; 2],
    pub filter: Option<DynamicFilter>,
    note: u8,
    velocity: u8,
    sample_phase: u8,
}
impl VoiceEnvelopes {
    /// Original single-trigger Mono keeps the envelope segments and cadence.
    pub fn retarget(&mut self, note: u8, velocity: u8) {
        self.note = note;
        self.velocity = velocity;
    }
    pub fn new(
        programs: [ModEnvelopeProgram; 2],
        filter: Option<DynamicFilter>,
        note: u8,
        velocity: u8,
        tables: &ControllerTables,
    ) -> Self {
        let mut envelopes = [ModEnvelope::default(); 2];
        for i in 0..2 {
            envelopes[i].note_on(
                programs[i].parameters(note, velocity),
                &tables.curves,
                &tables.timing,
                0,
            );
        }
        Self {
            envelopes,
            programs,
            filter,
            note,
            velocity,
            sample_phase: 0,
        }
    }
    pub fn next(&mut self, tables: &ControllerTables) {
        if self.sample_phase == 24 {
            self.sample_phase = 0;
            for i in 0..2 {
                self.envelopes[i].publish();
                self.envelopes[i].tick(
                    self.programs[i].parameters(self.note, self.velocity),
                    &tables.curves,
                    &tables.timing,
                );
            }
        }
        self.sample_phase += 1;
    }
    pub fn release(&mut self, tables: &ControllerTables) {
        for i in 0..2 {
            self.envelopes[i].release(
                self.programs[i].parameters(self.note, self.velocity),
                &tables.timing,
            );
        }
    }
    pub fn edit(&mut self, programs: [ModEnvelopeProgram; 2], tables: &ControllerTables) {
        for (i, program) in programs.iter().enumerate() {
            self.envelopes[i].edit(
                self.programs[i].parameters(self.note, self.velocity),
                program.parameters(self.note, self.velocity),
                &tables.timing,
            );
        }
        self.programs = programs;
    }
    pub fn levels(&self) -> [u16; 2] {
        self.envelopes.map(|e| e.envelope.segment.level)
    }
    pub fn filter_target(
        &self,
        table: &ControllerFilterTables,
        tables: &ControllerTables,
        modulation: [i16; 3],
    ) -> Option<FilterCoefficients> {
        self.filter_target_with_pitch(table, tables, modulation, (self.note as i16 - 60) * 256)
    }
    pub fn filter_target_with_pitch(
        &self,
        table: &ControllerFilterTables,
        tables: &ControllerTables,
        modulation: [i16; 3],
        relative_pitch: i16,
    ) -> Option<FilterCoefficients> {
        let mut f = self.filter?;
        f.input.eg1_level = self.envelopes[0].envelope.segment.level;
        f.input.eg1_velocity_sensitivity = self.programs[0].velocity_level_sensitivity;
        f.input.velocity = self.velocity;
        f.input.relative_pitch = relative_pitch;
        [
            f.input.cutoff_modulation,
            f.input.eg1_depth_modulation,
            f.input.key_modulation,
        ] = modulation;
        let frequency = f.input.frequency(table, &tables.amplifier);
        let c = radias_synth_domain::filter_control::compile(
            frequency as i32,
            f.resonance,
            f.normalization,
        );
        f.base.feedback = c.feedback;
        f.base.integrator_gain = c.integrator_gain;
        f.base.post_gain = c.post_gain;
        f.base.post_feedback = c.post_feedback;
        Some(f.base)
    }
}
