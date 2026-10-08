//! Compile independent drum bodies through the shared synthesis compiler.
use crate::{
    program::{InvalidPatchDestination, TimbreControls},
    stored_program::{CompiledTimbre, ProgramFilterTables},
};
use radias_synth_domain::{
    drum::{DrumKit, DrumProgram},
    filter::FilterCoefficients,
    program::Program,
};

#[derive(Clone, Copy)]
pub struct DrumInstrumentProgram {
    pub controls: TimbreControls,
    pub graph: CompiledTimbre,
}
pub struct CompiledDrumKit {
    pub kit: DrumKit,
    pub program: DrumProgram,
    pub instruments: [DrumInstrumentProgram; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrumCompilationError {
    NoOwningTimbre,
    Controls {
        instrument: u8,
        patch: InvalidPatchDestination,
    },
}
impl CompiledDrumKit {
    pub fn compile(
        program: &Program,
        kit: DrumKit,
        tables: &ProgramFilterTables<'_>,
        base: FilterCoefficients,
    ) -> Result<Self, DrumCompilationError> {
        let selection = program.drum_program();
        let owner = program
            .timbre(
                selection
                    .timbre
                    .ok_or(DrumCompilationError::NoOwningTimbre)? as usize,
            )
            .unwrap();
        let compile = |index: usize| {
            let controls =
                TimbreControls::from_drum_instrument(owner, kit.instrument(index).unwrap())
                    .map_err(|patch| DrumCompilationError::Controls {
                        instrument: index as u8,
                        patch,
                    })?;
            Ok(DrumInstrumentProgram {
                controls,
                graph: CompiledTimbre::compile(controls, tables, base),
            })
        };
        let instruments = [
            compile(0)?,
            compile(1)?,
            compile(2)?,
            compile(3)?,
            compile(4)?,
            compile(5)?,
            compile(6)?,
            compile(7)?,
            compile(8)?,
            compile(9)?,
            compile(10)?,
            compile(11)?,
            compile(12)?,
            compile(13)?,
            compile(14)?,
            compile(15)?,
        ];
        Ok(Self {
            kit,
            program: selection,
            instruments,
        })
    }
}
