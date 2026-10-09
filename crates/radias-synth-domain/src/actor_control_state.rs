//! Native controller storage/OSC1 preparation, SYS0206ee and SYS01e968.
use crate::{
    actor_descriptors::ActorControlCache,
    controller_noise::{formant_control1, noise_control1},
    controller_primary::{PrimaryControl, PrimaryControlState, PrimaryTarget},
};
pub const ACTOR_CONTROLLER_BYTES: usize = 0x1f0;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorControlState {
    pub bytes: [u8; ACTOR_CONTROLLER_BYTES],
}
impl ActorControlState {
    /// Functional SH caller work from whole0206ee. The Formant clamp helper
    /// takes distinct negative/in-range/upper-clipped instruction paths.
    pub fn primary_preparation_clocks(&self) -> u16 {
        let selection = self.bytes[0x1e0] & 63;
        match selection {
            0 | 1 | 3 => 129,
            2 => 130,
            4 => {
                let base = self.primary_inputs().compose().base;
                if base < 0 {
                    302
                } else if base > 65535 {
                    299
                } else {
                    301
                }
            }
            5 => 155,
            16..=19 => 204,
            32..=35 => 248,
            48..=51 => 206,
            _ => 105,
        }
    }
    pub(crate) fn word(&self, offset: usize) -> i16 {
        i16::from_be_bytes(self.bytes[offset..offset + 2].try_into().unwrap())
    }
    pub(crate) fn long(&self, offset: usize) -> i32 {
        i32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
    pub(crate) fn set_word(&mut self, offset: usize, value: i16) {
        self.bytes[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
    }
    pub(crate) fn set_long(&mut self, offset: usize, value: i32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    pub fn primary_inputs(&self) -> PrimaryControl {
        PrimaryControl {
            control1: self.bytes[0x1eb],
            control2: self.bytes[0x1ec],
            control1_manual_offset: self.word(0x182),
            control1_modulation: self.word(0x116),
            control2_modulation: self.word(0x134),
            control2_manual_offset: self.bytes[0x184] as i8,
            lfo1: self.word(0xe0),
        }
    }
    /// Unselected and null callback shadows remain intact.
    pub fn refresh_primary(&mut self) -> PrimaryControlState {
        let state = self.primary_inputs().compose();
        self.set_long(0xc8, state.base);
        self.set_long(0xcc, state.curved);
        self.set_long(0xd0, state.linear);
        let selection = self.bytes[0x1e0] & 63;
        // SYS display names: 4=Formant, 5=Noise. The reused compiler function
        // names predate that identification; numeric algorithm mapping stays.
        if selection == 4 {
            self.set_word(0x16c, noise_control1(state.base));
        } else if selection == 5 {
            let target = formant_control1(state.base);
            self.set_word(0x16e, target.level);
            self.set_word(0x170, target.feedback);
        } else if let Some(target) = state.target(selection) {
            let (offset, value) = match target {
                PrimaryTarget::Waveform(value) => (0x114, value),
                PrimaryTarget::Cross(value) => (0x16a, value),
                PrimaryTarget::Unison(value) => (0x176, value),
                PrimaryTarget::Vpm(value) => (0x174, value),
            };
            self.set_word(offset, value);
        }
        state
    }
    pub fn prepare_from_body(&mut self, body: &[u8; 104], owner_mode: u8) -> PrimaryControlState {
        for (offset, source) in [
            (0x1e0, 22),
            (0x1e1, 27),
            (0x1e2, 33),
            (0x1e3, 46),
            (0x1e4, 47),
            (0x1eb, 23),
            (0x1ec, 24),
        ] {
            self.bytes[offset] = body[source];
        }
        self.bytes[0x1ea] = owner_mode;
        self.refresh_primary()
    }
    pub fn descriptor_cache(&self) -> ActorControlCache {
        ActorControlCache {
            waveform: self.word(0x114),
            cross: self.word(0x16a),
            colored_color: self.word(0x16c),
            formant_level: self.word(0x16e),
            formant_feedback: self.word(0x170),
            vpm: self.word(0x174),
            unison: self.word(0x176),
            pitch: self.word(0xec),
            control2_modulation: self.word(0x134),
            control2_manual: self.bytes[0x184] as i8,
            shaper_manual: self.word(0x1a6),
            shaper_modulation: self.word(0x128),
            filter_type_manual: self.word(0x194),
            filter_type_modulation: self.word(0x120),
        }
    }
}
