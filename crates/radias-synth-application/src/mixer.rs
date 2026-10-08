//! Timbre mixer controls compile independently of files, devices and voices.
use radias_synth_domain::{
    controller_mixer::{MixerLevel, MixerScales},
    mixer::OscillatorMix,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MixerProgram {
    pub selections: [u8; 2],
    pub levels: [u8; 3],
    pub manual_offsets: [i16; 3],
}
impl Default for MixerProgram {
    fn default() -> Self {
        Self {
            selections: [0; 2],
            levels: [127, 0, 0],
            manual_offsets: [0; 3],
        }
    }
}
impl MixerProgram {
    pub fn compile(self, tables: &MixerScales, modulation: [i16; 3]) -> OscillatorMix {
        let scales = [
            tables.primary(self.selections[0]),
            tables.secondary(self.selections[1]),
            0x3333,
        ];
        let values: [i16; 3] = core::array::from_fn(|i| {
            MixerLevel {
                level: self.levels[i],
                manual_offset: self.manual_offsets[i],
                modulation: modulation[i],
                scale: scales[i],
            }
            .gain()
        });
        OscillatorMix {
            primary_gain: values[0],
            secondary_gain: values[1],
            noise_gain: values[2],
        }
    }
}
