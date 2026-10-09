//! Raw note identity and primary pitch preparation, SYS014d48 and SYS01f4d6.
use crate::{
    actor_control_state::ActorControlState,
    controller_pitch::ControllerPitch,
    controller_secondary::FineTuneTable,
    note_pitch::{BasePitch, PitchProgram},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorPitchPorts {
    pub timbre: u8,
    /// Original MIDI cache byte18; high bits select the drum timbre.
    pub midi_mode: u8,
    pub bend_q16: i32,
    pub wheel: u8,
    /// Actor+8 common-header byte5, distinct from the context's owner program.
    pub common_receive_flags: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedPrimaryPitch {
    pub code: u16,
    pub controller_clocks: u16,
}
impl ActorControlState {
    fn pitch_long(&self, offset: usize) -> i32 {
        i32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
    /// Uses the assigned note and retained Unison detune; no DSP publication.
    pub fn prepare_note_identity(&mut self) -> u16 {
        let pitch = (i32::from(self.bytes[0x36]) << 16).wrapping_add(self.pitch_long(0x8c));
        self.set_long(0x14, pitch);
        self.set_word(0xea, (pitch.wrapping_add(-60 * 65536) >> 8) as i16);
        21
    }
    pub fn prepare_primary_pitch(
        &mut self,
        body: &[u8; 104],
        ports: ActorPitchPorts,
        fine: &FineTuneTable,
    ) -> PreparedPrimaryPitch {
        let drum = i32::from(ports.midi_mode >> 5) - 1 == i32::from(ports.timbre & 3);
        let base = BasePitch {
            assigned_note_q16: self.pitch_long(0x14),
            scale_q16: self.pitch_long(0x18),
            bend_q16: ports.bend_q16,
            tuning_q16: self.pitch_long(0x1c),
            manual_offset: self.word(0x17e),
            drum_transpose: drum.then_some(body[19]),
        }
        .q16();
        self.set_long(0x10, base);
        let wheel_enabled = ports.common_receive_flags & 0x10 != 0;
        let depth = PitchProgram {
            vibrato_intensity: body[21],
            wheel_enabled,
            ..Default::default()
        }
        .vibrato_depth(ports.wheel, fine);
        self.set_long(0x20, depth);
        let code = ControllerPitch {
            base_q16: base,
            vibrato_depth: depth,
            lfo2: self.word(0xe2),
            virtual_patch_q16: self.pitch_long(0xa4),
        }
        .code();
        self.set_word(0xec, code as i16);
        let fold_work = if drum {
            let mut note = 60 + i32::from(body[19]) - 64;
            let mut work = 11;
            loop {
                if note < 0 {
                    note += 12;
                    work += 7;
                } else if (note as i8) < 0 {
                    note -= 12;
                    work += 10;
                } else {
                    break;
                }
            }
            6 + work
        } else {
            0
        };
        PreparedPrimaryPitch {
            code,
            // Gate/body + wheel compiler + unsigned divide + pitch narrowing.
            controller_clocks: 56 + if wheel_enabled { 51 } else { 47 } + 332 + 43 + fold_work,
        }
    }
}
