//! The six-route controller retains the previous published feedback depths.
use radias_synth_domain::modulation::{
    AppliedModulationTargets, ModulationDestination, ModulationSource, ModulationTables,
    VirtualPatch,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchRoute {
    pub source: u8,
    pub destination: ModulationDestination,
    pub intensity: u8,
}
pub struct VirtualPatchController {
    pub routes: [PatchRoute; 6],
    pub manual_offsets: [i8; 6],
    pub targets: AppliedModulationTargets,
}

/// Immutable input program for one timbre's native modulation controller.
/// Tempo synchronization is validated before a voice/controller is changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModulationProgram {
    pub lfo: [crate::lfo::LfoParameters; 2],
    pub tempo_divisions: [u8; 2],
    pub routes: [PatchRoute; 6],
    pub manual_offsets: [i8; 6],
    pub vibrato_depth: i32,
}
impl Default for ModulationProgram {
    fn default() -> Self {
        Self {
            lfo: [crate::lfo::LfoParameters::default(); 2],
            tempo_divisions: [8; 2],
            routes: [PatchRoute {
                source: 3,
                destination: ModulationDestination::new(0).unwrap(),
                intensity: 64,
            }; 6],
            manual_offsets: [0; 6],
            vibrato_depth: 0,
        }
    }
}
impl ModulationProgram {
    pub fn validate(&self) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        self.validate_with_clock(false)
    }
    pub fn validate_with_clock(
        &self,
        available: bool,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        if !available && self.lfo.iter().any(|p| p.phase_sync & 128 != 0) {
            Err(crate::lfo::TempoSynchronizationPending)
        } else {
            Ok(())
        }
    }
}

/// Voice state and source routing; the instrument supplies its service clock.
pub struct VoiceModulation {
    pub pair: crate::lfo::LfoPairController,
    pub patches: VirtualPatchController,
    pub base_pitch_q16: i32,
    pub vibrato_depth: i32,
    pub tempo_divisions: [u8; 2],
}
impl VoiceModulation {
    pub fn new(
        program: ModulationProgram,
        base_pitch_q16: i32,
        shared: [radias_synth_domain::lfo::LfoState; 2],
        seed: &mut u16,
    ) -> Result<Self, crate::lfo::TempoSynchronizationPending> {
        Self::from_prior(
            program,
            base_pitch_q16,
            shared,
            [Default::default(); 2],
            seed,
        )
    }
    pub fn from_prior(
        program: ModulationProgram,
        base_pitch_q16: i32,
        shared: [radias_synth_domain::lfo::LfoState; 2],
        prior: [radias_synth_domain::lfo::LfoState; 2],
        seed: &mut u16,
    ) -> Result<Self, crate::lfo::TempoSynchronizationPending> {
        Self::from_prior_with_clock(program, base_pitch_q16, shared, prior, seed, false)
    }
    pub fn from_prior_with_clock(
        program: ModulationProgram,
        base_pitch_q16: i32,
        shared: [radias_synth_domain::lfo::LfoState; 2],
        prior: [radias_synth_domain::lfo::LfoState; 2],
        seed: &mut u16,
        available: bool,
    ) -> Result<Self, crate::lfo::TempoSynchronizationPending> {
        program.validate_with_clock(available)?;
        let mut pair = crate::lfo::LfoPairController {
            states: prior,
            parameters: program.lfo,
        };
        pair.retrigger(shared, seed);
        Ok(Self {
            pair,
            patches: VirtualPatchController {
                routes: program.routes,
                manual_offsets: program.manual_offsets,
                targets: AppliedModulationTargets::default(),
            },
            base_pitch_q16,
            vibrato_depth: program.vibrato_depth,
            tempo_divisions: program.tempo_divisions,
        })
    }
    pub fn edit(
        &mut self,
        program: ModulationProgram,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        self.edit_with_clock(program, false)
    }
    pub fn edit_with_clock(
        &mut self,
        program: ModulationProgram,
        available: bool,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        program.validate_with_clock(available)?;
        self.pair.parameters = program.lfo;
        self.patches.routes = program.routes;
        self.patches.manual_offsets = program.manual_offsets;
        self.vibrato_depth = program.vibrato_depth;
        self.tempo_divisions = program.tempo_divisions;
        Ok(())
    }
    pub fn lfo_tick(
        &mut self,
        tables: &radias_synth_domain::lfo::LfoTables,
        seed: &mut u16,
    ) -> Result<(), crate::lfo::TempoSynchronizationPending> {
        self.pair.tick(tables, seed)?;
        Ok(())
    }
    /// All source values are normalized controller signals. Override the two
    /// LFO source slots with this voice's independently generated waveforms.
    pub fn service(
        &mut self,
        lfo: &radias_synth_domain::lfo::LfoTables,
        matrix: &ModulationTables,
        sources: [i32; 16],
    ) -> AppliedModulationTargets {
        let values = self.pair.values(lfo);
        self.service_published(matrix, sources, values)
    }
    /// Firmware publishes LFO words separately from phase initialization.
    /// A controller scheduler supplies those independently generated words.
    pub fn service_published(
        &mut self,
        matrix: &ModulationTables,
        mut sources: [i32; 16],
        values: [i16; 2],
    ) -> AppliedModulationTargets {
        sources[3] = (values[0] as i32) >> 1;
        sources[4] = (values[1] as i32) >> 1;
        let targets = self.patches.tick(matrix, &sources);
        self.pair.parameters[0].frequency_modulation = targets.controls[11];
        self.pair.parameters[1].frequency_modulation = targets.controls[12];
        targets
    }
    pub fn pitch_code(&self, tables: &radias_synth_domain::lfo::LfoTables) -> u16 {
        self.pitch_code_published(self.pair.values(tables))
    }
    pub fn pitch_code_published(&self, values: [i16; 2]) -> u16 {
        radias_synth_domain::controller_pitch::ControllerPitch {
            base_q16: self.base_pitch_q16,
            vibrato_depth: self.vibrato_depth,
            lfo2: values[1],
            virtual_patch_q16: self.patches.targets.oscillator_pitch_q16[0],
        }
        .code()
    }
}

pub struct VoiceModulationTables {
    pub lfo: radias_synth_domain::lfo::LfoTables,
    pub matrix: ModulationTables,
    pub pitch: radias_synth_domain::pitch::PitchTable,
    pub bandwidth: radias_synth_domain::bandlimit::BandwidthTable,
}
impl VirtualPatchController {
    /// Sources have already been normalized by the original source algorithms.
    /// Feedback destinations 34..39 apply on the following controller service.
    pub fn tick(
        &mut self,
        tables: &ModulationTables,
        sources: &[i32; 16],
    ) -> AppliedModulationTargets {
        let patches = core::array::from_fn(|i| {
            let p = self.routes[i];
            let selector = p.source & 15;
            VirtualPatch {
                source: ModulationSource {
                    selector,
                    value: sources[selector as usize],
                },
                destination: p.destination,
                intensity: p.intensity,
                manual_offset: self.manual_offsets[i],
                dynamic_offset: self.targets.controls[32 + i],
            }
        });
        self.targets = tables.route(&patches).applied();
        self.targets
    }
}
