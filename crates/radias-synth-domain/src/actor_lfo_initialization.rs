//! Owned, raw per-voice LFO state and note initialization.
use crate::{
    actor_control_state::ActorControlState,
    lfo::{LfoState, LfoTables, LfoWave},
    lfo_tempo::{LfoTempoState, LfoTempoTables},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ActorLfo {
    First = 0,
    Second = 1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorLfoValue {
    pub level: i16,
    pub controller_clocks: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidLfoShape;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorLfoState {
    pub bytes: [u8; 32],
}
impl ActorLfoState {
    /// Whole SYS016b84/016b98. Original shape bytes are signed loads with
    /// a centered 0..127 stored domain; malformed shapes are rejected.
    pub fn controller_value(
        self,
        lfo: ActorLfo,
        body: &[u8; 104],
        tables: &LfoTables,
    ) -> Result<ActorLfoValue, InvalidLfoShape> {
        let base = 76 + 5 * lfo as usize;
        if body[base + 1] > 127 {
            return Err(InvalidLfoShape);
        }
        let shape = (i16::from(body[base + 1]) - 64) as i8;
        let wave = if lfo == ActorLfo::First {
            [
                LfoWave::Saw,
                LfoWave::BipolarPulse,
                LfoWave::Triangle,
                LfoWave::SampleHold,
            ]
        } else {
            [
                LfoWave::Saw,
                LfoWave::Pulse,
                LfoWave::Sine,
                LfoWave::SampleHold,
            ]
        }[(body[base] & 3) as usize];
        let sync = body[base + 3];
        let phase =
            ((self.phase_state().phase >> 16) as u16).wrapping_add(tables.phase_offset(sync));
        Ok(ActorLfoValue {
            level: self.phase_state().value(tables, wave, sync, shape),
            controller_clocks: 51
                + if sync & 0x60 == 0 { 9 } else { 12 }
                + crate::lfo_controller_work::waveform_work(tables, wave, phase, shape),
        })
    }
    fn word(&self, offset: usize) -> u16 {
        u16::from_be_bytes(self.bytes[offset..offset + 2].try_into().unwrap())
    }
    fn long(&self, offset: usize) -> u32 {
        u32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
    fn set_word(&mut self, offset: usize, value: u16) {
        self.bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    fn set_long(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    fn phase_state(self) -> LfoState {
        LfoState {
            phase: self.long(0),
            previous_random: self.word(12) as i16,
            random: self.word(14) as i16,
            half_cycle: self.bytes[31],
        }
    }
    fn tempo_state(self) -> LfoTempoState {
        LfoTempoState {
            phase: self.long(0),
            previous_increment: self.long(4),
            reference_phase: self.long(8),
            clock_count: self.word(16),
            observed_clock_count: self.word(18),
            correction_active: self.bytes[28],
            correction_hold: self.bytes[27],
            division: self.bytes[29],
        }
    }
    /// Whole SYS015b42/015bce. The compiled rate and unused fields survive.
    /// Returns functional controller work, including conditional PRNG work.
    pub fn initialize_note(&mut self, sync: u8, division: u8, shared: Self, seed: &mut u16) -> u16 {
        let note_sync = sync & 0x60 == 0x40;
        let work = if note_sync {
            // SYS016d70: eight parity iterations; set taps omit one taken BT.
            154 - (*seed & 0x8805).count_ones() as u16
        } else {
            46
        };
        let mut phase = self.phase_state();
        phase.initialize_note(sync, shared.phase_state(), seed);
        let mut tempo = self.tempo_state();
        tempo.initialize_note(sync, division, shared.tempo_state());
        self.set_long(0, phase.phase);
        self.set_long(8, tempo.reference_phase);
        self.set_word(12, phase.previous_random as u16);
        self.set_word(14, phase.random as u16);
        self.set_word(16, tempo.clock_count);
        self.set_word(18, tempo.observed_clock_count);
        self.bytes[27] = tempo.correction_hold;
        self.bytes[28] = tempo.correction_active;
        self.bytes[29] = tempo.division;
        work
    }
}

impl ActorControlState {
    pub fn publish_lfo_level(
        &mut self,
        lfo: ActorLfo,
        state: ActorLfoState,
        body: &[u8; 104],
        tables: &LfoTables,
    ) -> Result<ActorLfoValue, InvalidLfoShape> {
        let value = state.controller_value(lfo, body, tables)?;
        self.set_word(0xe0 + 2 * lfo as usize, value.level);
        Ok(value)
    }
}

/// Whole SYS017730: two initial rates, with signed 64-bit fixed-point products.
pub fn initialize_lfo_rates(
    states: &mut [ActorLfoState; 2],
    body: &[u8; 104],
    clock_rate: u32,
    tables: &LfoTempoTables,
) -> u16 {
    let mut work = 49;
    for (index, state) in states.iter_mut().enumerate() {
        let division = body[80 + 5 * index] & 31;
        let (_, increment) = tables.compile_increment(i32::from(division), 0, clock_rate);
        state.set_long(4, increment);
        work += 97 + 5 * u16::from(division > 16);
    }
    work
}
