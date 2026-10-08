//! Portamento use cases preserve separate assigned pitch and curve phase.
use radias_synth_domain::portamento::{
    PortamentoCurves, PortamentoProgram, PortamentoRates, PortamentoState,
};

pub struct PortamentoTables {
    pub rates: PortamentoRates,
    pub curves: PortamentoCurves,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VoicePortamento {
    pub program: PortamentoProgram,
    pub state: PortamentoState,
    pub modulation: i16,
    pub manual_offset: i8,
}
impl VoicePortamento {
    pub fn new(program: PortamentoProgram) -> Self {
        Self {
            program,
            state: Default::default(),
            modulation: 0,
            manual_offset: 0,
        }
    }
    pub fn compile_rate(&mut self, tables: &PortamentoRates, switch: bool) {
        self.state.rate = self
            .program
            .rate(tables, switch, self.modulation, self.manual_offset);
    }
    pub fn note_on(
        &mut self,
        note: u8,
        previous_voice: i32,
        previous_timbre: Option<i32>,
        tables: &PortamentoRates,
        switch: bool,
    ) {
        self.compile_rate(tables, switch);
        self.state
            .begin(self.state.rate, note, previous_voice, previous_timbre);
    }
    pub fn edit(&mut self, program: PortamentoProgram, tables: &PortamentoRates, switch: bool) {
        self.program = program;
        self.compile_rate(tables, switch);
    }
    pub fn modulate(&mut self, value: i16, tables: &PortamentoRates, switch: bool) {
        if value != self.modulation {
            self.modulation = value;
            self.compile_rate(tables, switch);
        }
    }
    pub fn tick(&mut self, curves: &PortamentoCurves) {
        self.state.advance(curves, self.program.curve);
    }
}
