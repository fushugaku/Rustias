//! Complete SYS016A0C effect waveform pair; phase advance is a separate service.
use crate::{
    effect_lfo_program::EffectLfoProgram,
    lfo::{LfoState, LfoTables, LfoWave},
};
pub struct EffectLfoValueTables {
    pub waves: [LfoWave; 8],
    /// Signed -18..18 table entries from the original center pointer.
    pub stereo_phase: [u16; 37],
    pub hold_offset: [i16; 37],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectLfoValueState {
    pub oscillator: LfoState,
    pub alternate_phase: u8,
}
fn hold_fold(random: i16, offset: i16) -> i32 {
    let value = random.wrapping_add(offset) as i32;
    if value >= 0 {
        let doubled = value * 2;
        let mask = if doubled as i16 >= 0 { 0 } else { -1 };
        (doubled ^ mask) as u16 as i32
    } else {
        // BF/S executes ADD R0,R0 on both branches before the negative fold.
        let magnitude = -value * 2;
        let mask = if magnitude as i16 >= 0 { 0 } else { -1 };
        -((magnitude ^ mask).wrapping_sub(mask) as u16 as i32)
    }
}
fn hold_value(tables: &LfoTables, phase: u16, shape: i8, state: LfoState, offset: i16) -> i32 {
    let previous = hold_fold(state.previous_random, offset);
    let current = hold_fold(state.random, offset);
    if shape == 0 {
        return current;
    }
    if shape > 0 {
        let amount = ((u32::from(shape as u8) * 4 * u32::from(phase)) >> 8) as u16 as u32;
        let product = (current - previous).wrapping_mul(amount as i32) as u32;
        previous.wrapping_add((product >> 16) as i16 as i32)
    } else {
        let argument = !phase;
        if u32::from(argument) >= (-i32::from(shape)) as u32 * 1024 {
            return previous;
        }
        let amount =
            (u32::from(tables.warp[(64 + i32::from(shape)) as usize]) * u32::from(argument)) >> 8;
        let product = (current - previous).wrapping_mul(i32::from(amount as u16)) as u32;
        current.wrapping_sub((product >> 16) as i16 as i32)
    }
}
impl EffectLfoValueTables {
    pub fn values(
        &self,
        tables: &LfoTables,
        program: EffectLfoProgram,
        state: EffectLfoValueState,
    ) -> [i32; 2] {
        let bytes = program.bytes;
        let wave = self.waves[usize::from(bytes[0] & 7)];
        let shape = ((bytes[1] & 127) as i16 - 64) as i8;
        let mut phase =
            ((state.oscillator.phase >> 16) as u16).wrapping_add(tables.phase_offset(bytes[3]));
        if bytes[0] & 128 != 0 && state.alternate_phase != 0 {
            phase = phase.wrapping_add(32768);
        }
        let index = (((bytes[5] & 127) as i16 - 64).clamp(-18, 18) + 18) as usize;
        if wave == LfoWave::SampleHold {
            [
                hold_value(tables, phase, shape, state.oscillator, 0),
                hold_value(
                    tables,
                    phase,
                    shape,
                    state.oscillator,
                    self.hold_offset[index],
                ),
            ]
        } else {
            [
                tables.value_raw(wave, phase, shape, state.oscillator),
                tables.value_raw(
                    wave,
                    phase.wrapping_add(self.stereo_phase[index]),
                    shape,
                    state.oscillator,
                ),
            ]
        }
    }
}
