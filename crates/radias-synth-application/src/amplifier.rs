//! Native amplitude use case. Files/devices/MIDI decoding stay in adapters.
use radias_synth_domain::{
    amp_envelope::{AmpEnvelope, AmpEnvelopeParameters, EnvelopeStage},
    amplifier_control::{AmplifierControl, AmplifierTables},
    envelope_segment::{EnvelopeCurves, EnvelopeTimingTables},
};

pub struct ControllerTables {
    pub curves: EnvelopeCurves,
    pub timing: EnvelopeTimingTables,
    pub amplifier: AmplifierTables,
}

/// Complete stored EG2/amplifier inputs, independent of a note or device.
/// Velocity changes envelope timing and amplitude through separate controls.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmplifierProgram {
    pub envelope: crate::voice_envelopes::ModEnvelopeProgram,
    pub level: u8,
    pub level_offset: i8,
    pub key_tracking: u8,
    pub source_gain: u16,
    pub midi_volume: Option<u8>,
    pub program_volume: u8,
}
impl Default for AmplifierProgram {
    fn default() -> Self {
        Self {
            envelope: crate::voice_envelopes::ModEnvelopeProgram {
                velocity_level_sensitivity: 127,
                ..Default::default()
            },
            level: 100,
            level_offset: 0,
            key_tracking: 64,
            source_gain: 0x7f00,
            midi_volume: None,
            program_volume: 0,
        }
    }
}
impl AmplifierProgram {
    pub fn parameters(self, note: u8, velocity: u8) -> AmpEnvelopeParameters {
        self.envelope.parameters(note, velocity)
    }
    pub fn control(self, velocity: u8) -> AmplifierControl {
        AmplifierControl {
            level: self.level,
            level_offset: self.level_offset,
            source_gain: self.source_gain,
            envelope_level: 0,
            velocity,
            velocity_sensitivity: self.envelope.velocity_level_sensitivity,
            modulation: [0; 2],
            midi_volume: self.midi_volume,
            program_volume: self.program_volume,
        }
    }
}

#[derive(Clone, Copy)]
pub struct AmplifierController {
    pub envelope: AmpEnvelope,
    pub parameters: AmpEnvelopeParameters,
    control: AmplifierControl,
    sample_phase: u8,
    target: i16,
    pending_compile: bool,
    key_tracking: u8,
    relative_pitch: i16,
}

impl AmplifierController {
    /// Single-trigger Mono changes note inputs without restarting EG2 or
    /// recompiling the already published amplifier target.
    pub fn retarget(&mut self, note: u8, velocity: u8) {
        self.parameters.note = note;
        self.parameters.velocity = velocity;
        self.control.velocity = velocity;
    }
    pub fn from_program(
        program: AmplifierProgram,
        note: u8,
        velocity: u8,
        tables: &ControllerTables,
    ) -> Self {
        let mut controller = Self::new(
            program.parameters(note, velocity),
            program.control(velocity),
            tables,
        );
        controller.key_tracking = program.key_tracking;
        controller.relative_pitch((note as i16 - 60) * 256, tables);
        controller.compile(tables);
        controller
    }
    pub fn level_sensitivity(&self) -> u8 {
        self.control.velocity_sensitivity
    }
    pub fn control(&self) -> AmplifierControl {
        self.control
    }
    pub fn edit_program(&mut self, program: AmplifierProgram, tables: &ControllerTables) {
        let next = program.parameters(self.parameters.note, self.parameters.velocity);
        self.envelope.edit(self.parameters, next, &tables.timing);
        self.parameters = next;
        let modulation = self.control.modulation;
        self.control = program.control(self.parameters.velocity);
        self.control.modulation = modulation;
        self.key_tracking = program.key_tracking;
        self.relative_pitch(self.relative_pitch, tables);
        self.compile(tables);
    }
    pub fn new(
        parameters: AmpEnvelopeParameters,
        control: AmplifierControl,
        tables: &ControllerTables,
    ) -> Self {
        Self::new_at_phase(parameters, control, tables, 0)
    }
    /// A reused physical actor can retain a fractional controller phase.
    /// Its clock owner supplies that state independently of DSP sample phase.
    pub fn new_at_phase(
        parameters: AmpEnvelopeParameters,
        control: AmplifierControl,
        tables: &ControllerTables,
        initial_phase: u32,
    ) -> Self {
        let mut envelope = AmpEnvelope::default();
        envelope.note_on(parameters, &tables.curves, &tables.timing, initial_phase);
        let mut controller = Self {
            envelope,
            parameters,
            control,
            sample_phase: 0,
            target: 0,
            pending_compile: false,
            key_tracking: 64,
            relative_pitch: (parameters.note as i16 - 60) * 256,
        };
        controller.compile(tables);
        controller
    }
    fn compile(&mut self, tables: &ControllerTables) {
        self.control.envelope_level = self.envelope.segment.level;
        self.control.velocity = self.parameters.velocity;
        self.target = tables.amplifier.target(self.control);
        self.pending_compile = false;
    }
    /// The legacy program_volume index is the original per-actor1EA
    /// Unison gain bank. Recompile without advancing the envelope clock.
    pub fn set_group_gain_bank(&mut self, bank: u8) {
        if self.control.program_volume != bank {
            self.control.program_volume = bank;
            self.pending_compile = true;
        }
    }
    pub fn release(&mut self, tables: &ControllerTables) {
        self.envelope.release(self.parameters, &tables.timing);
    }
    /// One explicit controller service; sample scheduling and source HPI
    /// delivery clocks are supplied by the caller.
    pub fn service(&mut self, tables: &ControllerTables, release_acknowledged: bool) -> i16 {
        self.envelope.publish();
        self.envelope.tick(
            self.parameters,
            &tables.curves,
            &tables.timing,
            release_acknowledged,
        );
        self.compile(tables);
        self.target
    }
    /// 48 kHz / 24 = 2 kHz nominal controller service cadence. The original
    /// interpreter's HPI latency/jitter is qualified with a separate timeline.
    pub fn next_target(&mut self, tables: &ControllerTables) -> i16 {
        if self.pending_compile {
            self.compile(tables);
        }
        if self.sample_phase == 24 {
            self.sample_phase = 0;
            self.service(tables, true);
        }
        self.sample_phase += 1;
        self.target
    }
    pub fn finished(&self) -> bool {
        self.envelope.stage == EnvelopeStage::Finished
    }
    pub fn edit_adsr(&mut self, values: [u8; 4], tables: &ControllerTables) {
        let mut next = self.parameters;
        [next.attack, next.decay, next.sustain, next.release] = values;
        self.envelope.edit(self.parameters, next, &tables.timing);
        self.parameters = next;
        self.compile(tables);
    }
    pub fn modulation(&mut self, value: i16, tables: &ControllerTables) -> i16 {
        self.control.modulation[1] = value;
        self.compile(tables);
        self.target
    }
    pub fn modulations(&mut self, values: [i16; 2], tables: &ControllerTables) -> i16 {
        self.control.modulation = values;
        self.compile(tables);
        self.target
    }
    pub fn edit_level(&mut self, level: u8, tables: &ControllerTables) {
        self.control.level = level;
        self.compile(tables);
    }
    pub fn relative_pitch(&mut self, pitch: i16, tables: &ControllerTables) {
        self.relative_pitch = pitch;
        let modulation = tables.amplifier.key_modulation(self.key_tracking, pitch);
        if self.control.modulation[0] != modulation {
            self.control.modulation[0] = modulation;
            self.pending_compile = true;
        }
    }
}
