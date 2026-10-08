//! SYS006B50 direct instrument key input and retained key/channel release.
use crate::drum::DrumProgram;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrumPadState {
    pub note_flags: [u8; 16],
    pub channels: [u8; 16],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumPadInput {
    pub instrument: u8,
    pub velocity: u8,
    pub owning_timbre_enabled: bool,
    pub owning_channel: u8,
    pub key: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DrumPadEvent {
    pub instrument: u8,
    pub timbre: u8,
    pub event: u32,
}
impl DrumPadState {
    pub fn input(&mut self, program: DrumProgram, input: DrumPadInput) -> Option<DrumPadEvent> {
        // The direct-pad entry only checks zero. Preserve its raw selector,
        // including extra table entries; native four-timbre loading validates it.
        let timbre = (program.selection >> 5).checked_sub(1)?;
        if !input.owning_timbre_enabled {
            return None;
        }
        let instrument = input.instrument & 15;
        let index = instrument as usize;
        let event = if input.velocity != 0 {
            let key = input.key as i8 as i32 + (program.transpose & 127) as i32 - 64;
            let note = key as u8 | 128;
            self.note_flags[index] = note;
            self.channels[index] = input.owning_channel;
            ((input.owning_channel as u32) << 24)
                | (((input.velocity & 127) as u32) << 8)
                | note as u32
        } else {
            if self.note_flags[index] & 128 == 0 {
                return None;
            }
            self.note_flags[index] &= 127;
            (((self.channels[index] & 15) as u32) << 24) | 0x4000 | self.note_flags[index] as u32
        };
        Some(DrumPadEvent {
            instrument,
            timbre,
            event,
        })
    }
}
