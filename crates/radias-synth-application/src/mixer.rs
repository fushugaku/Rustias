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
    #[cfg(all(feature = "web-expanded", target_arch = "wasm32"))]
    pub fn compile_with_third(
        self,
        tables: &MixerScales,
        modulation: [i16; 3],
        selection: u8,
    ) -> OscillatorMix {
        let mut mix = self.compile(tables, modulation);
        // OSC 3 has OSC 1's generator; the former Noise scale attenuated it by ~8 dB.
        mix.noise_gain = MixerLevel {
            level: self.levels[2],
            manual_offset: self.manual_offsets[2],
            modulation: modulation[2],
            scale: tables.primary(selection),
        }
        .gain();
        mix
    }
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
