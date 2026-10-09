//! SYS01BD62/01C592 regular Filter2 targets, including LINK and Serial gain.
use crate::{
    amplifier_control::AmplifierTables,
    controller_comb::{CombCutoffControl, CombResonanceControl},
    controller_filter::ControllerFilterTables,
    filter_control,
    filter_routing::{Filter2Coefficients, Filter2Output},
};

#[derive(Clone, Copy)]
pub struct Filter2ControlTables {
    pub resonance: [i32; 128],
    pub input_gain: [i16; 128],
    pub linked_serial_input_gain: [i16; 128],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter2Targets {
    pub frequency: i32,
    pub resonance: i32,
    pub input_gain: i16,
    pub output: Filter2Output,
}
impl Filter2ControlTables {
    pub fn resonance_targets(&self, route: u8, resonance: CombResonanceControl) -> (i32, i16) {
        let index = resonance.level() as usize;
        let gain = if route & 0x83 == 0x81 {
            self.linked_serial_input_gain[index]
        } else {
            self.input_gain[index]
        };
        (self.resonance[index], gain)
    }
    pub fn targets(
        &self,
        route: u8,
        cutoff: CombCutoffControl,
        resonance: CombResonanceControl,
        frequencies: &ControllerFilterTables,
        amplitude: &AmplifierTables,
    ) -> Option<Filter2Targets> {
        let output = match (route >> 4) & 3 {
            0 => Filter2Output::LowPass,
            1 => Filter2Output::HighPass,
            2 => Filter2Output::BandPass,
            _ => return None,
        };
        let (resonance, input_gain) = self.resonance_targets(route, resonance);
        Some(Filter2Targets {
            frequency: frequencies.frequency(cutoff.code(amplitude)) as i32,
            resonance,
            input_gain,
            output,
        })
    }
    pub fn coefficients(
        &self,
        route: u8,
        cutoff: CombCutoffControl,
        resonance: CombResonanceControl,
        frequencies: &ControllerFilterTables,
        amplitude: &AmplifierTables,
        normalization: i32,
    ) -> Option<Filter2Coefficients> {
        let target = self.targets(route, cutoff, resonance, frequencies, amplitude)?;
        let c = filter_control::compile(target.frequency, target.resonance, normalization);
        Some(Filter2Coefficients {
            input_gain: target.input_gain,
            feedback: c.feedback,
            integrator_gain: c.integrator_gain,
            output: target.output,
        })
    }
}
