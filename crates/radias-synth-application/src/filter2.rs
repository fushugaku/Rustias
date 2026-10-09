//! Ordinary Filter2 uses the same per-voice cutoff inputs as Comb.
use crate::comb::{CombProgram, CombVoiceControl};
use radias_synth_domain::{
    amplifier_control::AmplifierTables,
    controller_filter::ControllerFilterTables,
    controller_filter2::{Filter2ControlTables, Filter2Targets},
    filter_routing::Filter2Coefficients,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter2Program {
    pub route: u8,
    pub controls: CombProgram,
    pub normalization: i32,
}
impl Filter2Program {
    pub fn targets(
        self,
        input: CombVoiceControl,
        frequencies: &ControllerFilterTables,
        tables: &Filter2ControlTables,
        amplitude: &AmplifierTables,
    ) -> Option<Filter2Targets> {
        let c = self.controls.for_voice(input, frequencies);
        tables.targets(self.route, c.cutoff, c.resonance, frequencies, amplitude)
    }
    pub fn coefficients(
        self,
        input: CombVoiceControl,
        frequencies: &ControllerFilterTables,
        tables: &Filter2ControlTables,
        amplitude: &AmplifierTables,
    ) -> Option<Filter2Coefficients> {
        let c = self.controls.for_voice(input, frequencies);
        tables.coefficients(
            self.route,
            c.cutoff,
            c.resonance,
            frequencies,
            amplitude,
            self.normalization,
        )
    }
}
