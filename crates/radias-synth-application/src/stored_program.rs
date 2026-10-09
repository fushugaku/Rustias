//! Compile all four stored timbres. ROM/control tables are explicit ports.
use crate::{
    comb::CombProgram,
    program::{InvalidStoredProgram, StoredProgram},
    voice_envelopes::DynamicFilter,
};
use radias_synth_domain::{
    controller_filter::ControllerFilter,
    filter::FilterCoefficients,
    filter_control::FilterMixTable,
    filter_routing::{Filter2Coefficients, Filter2Output, FilterRouting},
    parameter_template::{ParameterTemplate, ParameterTemplateTables, TemplateCompilationError},
    program::Program,
};

pub struct ProgramFilterTables<'a> {
    pub frequencies: &'a [i32; 128],
    pub resonances: &'a [i32; 128],
    pub input_gains: &'a [i16; 128],
    pub normalization: i32,
    pub mix: &'a FilterMixTable,
}
impl ProgramFilterTables<'_> {
    fn filter(
        &self,
        mut base: FilterCoefficients,
        cutoff: u8,
        resonance: u8,
    ) -> FilterCoefficients {
        let c = radias_synth_domain::filter_control::compile(
            self.frequencies[(cutoff & 127) as usize],
            self.resonances[(resonance & 127) as usize],
            self.normalization,
        );
        base.feedback = c.feedback;
        base.integrator_gain = c.integrator_gain;
        base.post_gain = c.post_gain;
        base.post_feedback = c.post_feedback;
        base.input_gain = self.input_gains[(resonance & 127) as usize];
        base
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramCompilationError {
    Controls(InvalidStoredProgram),
    Generator { timbre: u8, selection: u8 },
}
#[derive(Clone, Copy)]
pub struct CompiledTimbre {
    pub parameter_template: Option<ParameterTemplate>,
    pub filter: FilterCoefficients,
    pub dynamic_filter: DynamicFilter,
    pub filter_routing: Option<FilterRouting>,
    pub filter2: Filter2Coefficients,
    pub comb: Option<CombProgram>,
    pub dynamic_filter2: Option<crate::filter2::Filter2Program>,
}
#[derive(Clone, Copy)]
pub struct CompiledProgram {
    pub stored: StoredProgram,
    pub timbres: [CompiledTimbre; 4],
}
impl CompiledProgram {
    /// Compile cold static banks directly from the lossless stored inputs.
    /// Deferred PCM/input generators keep their existing explicit availability.
    pub fn with_parameter_templates(
        mut self,
        program: &Program,
        tables: &ParameterTemplateTables,
    ) -> Result<Self, TemplateCompilationError> {
        for index in 0..4 {
            let body = program.timbre(index).unwrap().synthesis()[..104]
                .try_into()
                .unwrap();
            self.timbres[index].compile_parameter_template(index, body, tables)?;
        }
        Ok(self)
    }
    pub fn compile(
        program: &Program,
        global_channel: u8,
        controls: &ProgramFilterTables<'_>,
        base: FilterCoefficients,
    ) -> Result<Self, ProgramCompilationError> {
        let stored = StoredProgram::from_program(program, global_channel)
            .map_err(ProgramCompilationError::Controls)?;
        let compile =
            |index: usize| CompiledTimbre::compile(stored.timbres[index].controls, controls, base);
        Ok(Self {
            stored,
            timbres: [compile(0), compile(1), compile(2), compile(3)],
        })
    }
    /// No silent oscillator substitution for a stored PCM/input timbre.
    pub fn validate_native_generators(&self) -> Result<(), ProgramCompilationError> {
        self.validate_native_generators_except(None)
    }
    pub fn validate_native_generators_except(
        &self,
        replaced: Option<u8>,
    ) -> Result<(), ProgramCompilationError> {
        for (index, t) in self.stored.timbres.iter().enumerate() {
            if !t.enabled || replaced == Some(index as u8) {
                continue;
            }
            let selection = t.controls.oscillator_selection & 63;
            if selection & 15 >= 6 || (selection & 15 >= 4 && selection & 48 != 0) {
                return Err(ProgramCompilationError::Generator {
                    timbre: index as u8,
                    selection,
                });
            }
        }
        Ok(())
    }
}
impl CompiledTimbre {
    pub fn compile_parameter_template(
        &mut self,
        index: usize,
        body: &[u8; 104],
        tables: &ParameterTemplateTables,
    ) -> Result<(), TemplateCompilationError> {
        let Some(mut template) = ParameterTemplate::boot(index, [0; 160]) else {
            return Err(TemplateCompilationError::InvalidTemplateIndex);
        };
        match template.compile(body, tables) {
            Err(TemplateCompilationError::PcmOrInputGenerator) => {
                self.parameter_template = None;
                Ok(())
            }
            Err(error) => Err(error),
            Ok(()) => {
                self.filter.mix = core::array::from_fn(|i| template.words[72 + 2 * i] as i16);
                self.dynamic_filter.base.mix = self.filter.mix;
                self.parameter_template = Some(template);
                Ok(())
            }
        }
    }
    pub fn compile(
        c: crate::program::TimbreControls,
        controls: &ProgramFilterTables<'_>,
        base: FilterCoefficients,
    ) -> Self {
        let mut filter = controls.filter(base, c.cutoff[0], c.resonance[0]);
        filter.mix = controls.mix.weights((c.filter_type as u16) << 8);
        let second = controls.filter(base, c.cutoff[1], c.resonance[1]);
        let kind = (c.filter_route >> 4) & 3;
        let filter2 = Filter2Coefficients {
            input_gain: second.input_gain,
            feedback: second.feedback,
            integrator_gain: second.integrator_gain,
            output: [
                Filter2Output::LowPass,
                Filter2Output::HighPass,
                Filter2Output::BandPass,
                Filter2Output::Comb,
            ][kind as usize],
        };
        let route = match c.filter_route & 3 {
            0 => None,
            1 => Some(FilterRouting::Serial),
            2 => Some(FilterRouting::Parallel),
            _ => Some(FilterRouting::Individual),
        };
        let defaults = CombProgram::default();
        let filter2_controls = CombProgram {
            cutoff: radias_synth_domain::controller_comb::CombCutoffControl {
                cutoff: c.cutoff[1],
                linked_cutoff: c.cutoff[0],
                link: c.filter_route & 128 != 0,
                eg1_intensity: c.filter2_eg_intensity,
                linked_eg1_intensity: c.eg1_intensity,
                ..defaults.cutoff
            },
            resonance: radias_synth_domain::controller_comb::CombResonanceControl {
                resonance: c.resonance[1],
                linked_resonance: c.resonance[0],
                link: c.filter_route & 128 != 0,
                ..defaults.resonance
            },
            key_tracking: c.filter2_key_tracking,
            linked_key_tracking: c.filter_key_tracking,
            ..defaults
        };
        let comb = (kind == 3).then_some(filter2_controls);
        CompiledTimbre {
            parameter_template: None,
            filter,
            dynamic_filter: DynamicFilter {
                input: ControllerFilter {
                    cutoff: c.cutoff[0],
                    eg1_intensity: c.eg1_intensity,
                    key_tracking: c.filter_key_tracking,
                    ..Default::default()
                },
                resonance: controls.resonances[(c.resonance[0] & 127) as usize],
                normalization: controls.normalization,
                base: filter,
            },
            filter_routing: route,
            filter2,
            comb,
            dynamic_filter2: (kind != 3).then_some(crate::filter2::Filter2Program {
                route: c.filter_route,
                controls: filter2_controls,
                normalization: controls.normalization,
            }),
        }
    }
}
