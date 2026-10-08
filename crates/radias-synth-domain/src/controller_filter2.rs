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
    pub fn coefficients(
        &self,
        route: u8,
        cutoff: CombCutoffControl,
        resonance: CombResonanceControl,
        frequencies: &ControllerFilterTables,
        amplitude: &AmplifierTables,
        normalization: i32,
    ) -> Option<Filter2Coefficients> {
        let output = match (route >> 4) & 3 {
            0 => Filter2Output::LowPass,
            1 => Filter2Output::HighPass,
            2 => Filter2Output::BandPass,
            _ => return None,
        };
        let (resonance, input_gain) = self.resonance_targets(route, resonance);
        let c = filter_control::compile(
            frequencies.frequency(cutoff.code(amplitude)) as i32,
            resonance,
            normalization,
        );
        Some(Filter2Coefficients {
            input_gain,
            feedback: c.feedback,
            integrator_gain: c.integrator_gain,
            output,
        })
    }
}
