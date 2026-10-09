//! Whole EG1/EG2/EG3 note-on stores, SYS01512c/01548a/0157f4.
//! Publishing levels and advancing the controller clock belong to the caller.
use crate::{
    actor_control_state::ActorControlState,
    envelope_segment::{EnvelopeCurves, EnvelopeTimingTables},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ActorEnvelope {
    Filter = 0,
    Amplifier = 1,
    Modulation = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvelopeNoteOn {
    pub attack: u8,
    pub phase: u32,
    pub level: u16,
    pub controller_clocks: u16,
}

impl ActorControlState {
    /// Whole SYS0142f2/014896/014c8a. Both equality paths take thirteen clocks.
    /// This publishes the controller shadow; it sends no DSP command.
    pub fn publish_envelope_level(&mut self, envelope: ActorEnvelope) -> u16 {
        let segment = 0x38 + 24 * envelope as usize;
        self.set_word(segment + 20, self.word(segment + 16));
        13
    }

    pub fn initialize_envelope(
        &mut self,
        envelope: ActorEnvelope,
        body: &[u8; 104],
        curves: &EnvelopeCurves,
        timing: &EnvelopeTimingTables,
    ) -> EnvelopeNoteOn {
        let index = envelope as usize;
        let segment = 0x38 + 24 * index;
        let attack = (i32::from(body[52 + 8 * index] & 127)
            + i32::from(self.word(0x140 + 8 * index))
            + i32::from(self.bytes[0x1a8 + 8 * index] as i8))
        .clamp(0, 127) as u8;
        let increment = timing.increments[3][attack as usize];
        let initial = u32::from_be_bytes(self.bytes[segment + 4..segment + 8].try_into().unwrap());
        let next = initial.wrapping_add(increment.wrapping_mul(2));
        // SYS015c5a uses a signed CMP/GT, including wrapped initial phases.
        let clipped = next as i32 > 0x00ff_ffff;
        let phase = if clipped { 0x00ff_ffff } else { next };
        let curve_phase = phase >> 8;
        let level = (curves.evaluate(5, curve_phase).wrapping_mul(32766) >> 16) as u16;

        self.bytes[0x94 + index] = 0;
        self.bytes[0x98 + 2 * index] = 2;
        self.bytes[0x99 + 2 * index] = 2;
        if envelope == ActorEnvelope::Amplifier {
            self.bytes[0x9e] = 1;
        }
        self.set_word(segment + 12, 0);
        self.set_word(segment + 22, 0);
        self.set_word(segment + 14, 32766);
        self.set_long(segment + 8, increment as i32);
        self.set_long(segment, phase as i32);
        self.set_word(segment + 16, level as i16);

        // Instruction work: note-on body, linear table helper, signed clamp,
        // level evaluator, then SYS014d8a's zero/interior/final-bin path.
        let interpolation = if curve_phase == 0 {
            7
        } else if (curve_phase as u16 >> 8) == 255 {
            42
        } else {
            43
        };
        EnvelopeNoteOn {
            attack,
            phase,
            level,
            controller_clocks: match envelope {
                ActorEnvelope::Filter => 60,
                ActorEnvelope::Amplifier => 62,
                ActorEnvelope::Modulation => 59,
            } + 5
                + 9
                + u16::from(clipped)
                + 27
                + interpolation,
        }
    }
}
