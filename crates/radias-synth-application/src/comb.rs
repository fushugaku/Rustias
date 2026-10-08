//! Per-timbre Comb parameters; domain compilers own numeric transfers.
use radias_synth_domain::{
    amplifier_control::AmplifierTables,
    controller_comb::{CombControlTables, CombCutoffControl, CombResonanceControl},
    controller_filter::{ControllerFilter, ControllerFilterTables},
    filter_routing::{Filter2Coefficients, Filter2Output},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CombProgram {
    pub cutoff: CombCutoffControl,
    pub resonance: CombResonanceControl,
    pub key_tracking: u8,
    pub linked_key_tracking: u8,
    pub key_manual_offset: i8,
    pub key_modulation: i16,
}
impl Default for CombProgram {
    fn default() -> Self {
        Self {
            cutoff: CombCutoffControl {
                cutoff: 127,
                linked_cutoff: 64,
                eg1_intensity: 64,
                linked_eg1_intensity: 64,
                eg1_velocity_sensitivity: 64,
                ..Default::default()
            },
            resonance: CombResonanceControl {
                linked_resonance: 48,
                ..Default::default()
            },
            key_tracking: 64,
            linked_key_tracking: 64,
            key_manual_offset: 0,
            key_modulation: 0,
        }
    }
}
impl CombProgram {
    pub fn coefficients(
        self,
        tables: &CombControlTables,
        amplitude: &AmplifierTables,
    ) -> Filter2Coefficients {
        let code = self.cutoff.code(amplitude);
        Filter2Coefficients {
            input_gain: 0,
            feedback: tables.compile_feedback(code, self.resonance) as i32,
            integrator_gain: tables.delay(code) as i32,
            output: Filter2Output::Comb,
        }
    }
    pub fn for_voice(mut self, input: CombVoiceControl, tables: &ControllerFilterTables) -> Self {
        self.cutoff.eg1_level = input.eg1_level;
        self.cutoff.velocity = input.velocity;
        self.cutoff.eg1_velocity_sensitivity = input.eg1_velocity_sensitivity;
        self.cutoff.cutoff_modulation = self
            .cutoff
            .cutoff_modulation
            .wrapping_add(input.modulation[0]);
        self.resonance.modulation = self.resonance.modulation.wrapping_add(input.modulation[1]);
        self.cutoff.eg1_depth_modulation = self
            .cutoff
            .eg1_depth_modulation
            .wrapping_add(input.modulation[2]);
        let key = ControllerFilter {
            key_tracking: if self.cutoff.link {
                self.linked_key_tracking
            } else {
                self.key_tracking
            },
            key_manual_offset: self.key_manual_offset,
            key_modulation: self.key_modulation.wrapping_add(input.modulation[3]),
            relative_pitch: input.relative_pitch,
            ..Default::default()
        };
        self.cutoff.key_offset = self.cutoff.key_offset.wrapping_add(key.key_offset(tables));
        self
    }
}
#[derive(Clone, Copy, Debug)]
pub struct CombVoiceControl {
    pub eg1_level: u16,
    pub velocity: u8,
    pub eg1_velocity_sensitivity: u8,
    pub relative_pitch: i16,
    /// Filter2 cutoff, resonance, EG1 depth and key tracking.
    pub modulation: [i16; 4],
}
