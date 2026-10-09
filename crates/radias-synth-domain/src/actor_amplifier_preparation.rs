//! Whole initial AMP key and target caches, SYS0020e2 and SYS002202.
use crate::{
    actor_control_state::ActorControlState,
    amplifier_control::{AmplifierControl, AmplifierTables},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorAmplifierPorts {
    /// Global configuration byte0f chooses the owner-program receive mask.
    pub configuration_mode: u8,
    pub owner_receive_flags: u8,
    pub context_gain: u16,
    pub midi_volume: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidUnisonGainBank;
impl ActorControlState {
    pub fn prepare_amplifier_key(&mut self, body: &[u8; 104], tables: &AmplifierTables) -> u16 {
        let tracking = body[50];
        let relative = self.word(0xea);
        let depth = i32::from(tables.key_tracking[(tracking & 127) as usize]);
        let linear = ((depth * i32::from(relative)) >> 11).clamp(-32767, 32767);
        self.set_word(0x178, tables.key_modulation(tracking, relative));
        if depth == 0 {
            24
        } else if linear == 0 {
            45
        } else if linear < 0 {
            67
        } else {
            68
        }
    }
    /// This compiles actor+f0; the subsequent activation service owns DSP
    /// delivery. The context gain and per-actor MIDI gain have separate gates.
    pub fn prepare_amplifier_target(
        &mut self,
        body: &[u8; 104],
        ports: ActorAmplifierPorts,
        tables: &AmplifierTables,
    ) -> Result<u16, InvalidUnisonGainBank> {
        let bank = self.bytes[0x1ea];
        if bank > 127 {
            return Err(InvalidUnisonGainBank);
        }
        let mask = if ports.configuration_mode == 1 {
            0x20
        } else {
            0x40
        };
        let midi_enabled = self.bytes[0x1e7] != 0;
        let control = AmplifierControl {
            level: body[45],
            level_offset: self.bytes[0x1a2] as i8,
            source_gain: if ports.owner_receive_flags & mask != 0 {
                ports.context_gain
            } else {
                0x7f00
            },
            envelope_level: self.word(0x60) as u16,
            velocity: self.bytes[0x37],
            velocity_sensitivity: body[65],
            modulation: [self.word(0x178), self.word(0x12a)],
            midi_volume: midi_enabled.then_some(ports.midi_volume),
            program_volume: bank,
        };
        self.set_word(0xf0, tables.target(control));
        let envelope_work = 17
            + match body[65] & 127 {
                64 => 17,
                0..=63 => 42,
                _ => 39,
            };
        Ok(188
            + envelope_work
            + u16::from(ports.configuration_mode == 1)
            + if midi_enabled { 23 } else { 2 }
            + if tables.program_volume[bank as usize] == 0 {
                9
            } else {
                23
            }
            + 10)
    }
}
