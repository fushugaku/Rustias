//! Original03bc18..03bdc8 vocoder source getters. Actor ownership is resolved
//! before entering this data port; envelope/MIDI use cases supply their values.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VocoderActorSources {
    pub envelope_outputs: [i32; 3],
    pub lfo_outputs: [i16; 2],
    pub velocity: u8,
    pub keyboard_tracking: i16,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VocoderSources {
    pub actor: Option<VocoderActorSources>,
    pub pitch_bend: i16,
    pub timbre_controller: u8,
    pub global_controller: i16,
    pub performance: [i32; 5],
}
impl VocoderSources {
    pub fn read(self, selector: u8) -> i16 {
        let value = match selector & 15 {
            0..=2 => self.actor.map_or(0, |actor| {
                actor.envelope_outputs[usize::from(selector & 15)] >> 5
            }),
            3 | 4 => self.actor.map_or(0, |actor| {
                i32::from(actor.lfo_outputs[usize::from((selector & 15) - 3)]) >> 6
            }),
            5 => self
                .actor
                .map_or(0, |actor| i32::from(actor.velocity & 127) << 2),
            6 => i32::from(self.pitch_bend) >> 4,
            7 => i32::from(self.timbre_controller & 127) << 2,
            8 => self.actor.map_or(0, |actor| {
                // The original divides the absolute signed product, restores
                // the sign, then performs an arithmetic six-bit shift.
                (i32::from(actor.keyboard_tracking) * 32767 / 3072) >> 6
            }),
            9 | 15 => i32::from(self.global_controller) >> 6,
            10..=14 => self.performance[usize::from((selector & 15) - 10)].wrapping_shl(2),
            _ => unreachable!(),
        };
        value as i16
    }
}
