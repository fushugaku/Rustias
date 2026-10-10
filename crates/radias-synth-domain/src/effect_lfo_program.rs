//! Original SYS077984 effect LFO program construction and SYS01767A rate store.
//! The six configuration bytes are separate from running phase/random state.
use crate::lfo_tempo::LfoTempoTables;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectLfoProgram {
    pub bytes: [u8; 6],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectLfoMapping {
    pub fields: [u8; 8],
    pub definition_mode: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectLfoSlot(u8);
impl EffectLfoSlot {
    pub fn new(value: u8) -> Option<Self> {
        (value < 9).then_some(Self(value))
    }
    pub fn raw(self) -> u8 {
        self.0
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectLfoPublication {
    pub slot: EffectLfoSlot,
    pub program: EffectLfoProgram,
    pub tempo_increment: u32,
}
impl EffectLfoProgram {
    pub fn prepare(
        self,
        parameters: &[u8],
        mapping: EffectLfoMapping,
        slot: EffectLfoSlot,
        offset_control: u32,
        clock_rate: u32,
        tempo: &LfoTempoTables,
    ) -> Option<Option<EffectLfoPublication>> {
        let mode = mapping.definition_mode;
        if mode == 0 {
            return Some(None);
        }
        let mut values = [0u8; 8];
        for (out, index) in values.iter_mut().zip(mapping.fields) {
            *out = *parameters.get(usize::from(index))?;
        }
        let mut b = self.bytes;
        b[0] = (b[0] & 0xf0) | (values[0] & 15);
        b[1] = values[1];
        b[2] = values[2];
        b[3] = (b[3] & 0xe0) | (values[3] & 31);
        b[3] = (b[3] & 0x9f) | ((values[4] & 3) << 5);
        b[3] = (b[3] & 0x7f) | ((values[5] & 1) << 7);
        b[4] = (b[4] & 0xe0) | (values[6] & 31);
        b[5] = values[7];
        b[0] &= 0x7f;
        if mode == 1 {
            b[5] = 64;
        } else if mode != 2 {
            b[1] = 64;
            if mode == 5 {
                b[0] = (b[0] & 0xf0) | 0x81;
                if offset_control == 0 {
                    b[5] = 64;
                }
            } else {
                b[3] &= 0xe0;
                b[0] = (b[0] & 0xf0) | if mode == 3 { 2 } else { 3 };
                b[3] &= 0x9f;
                b[3] &= 0x7f;
                b[4] &= 0xe0;
            }
        }
        let program = Self { bytes: b };
        let tempo_increment = tempo
            .compile_increment(i32::from(b[4] & 31), 0, clock_rate)
            .1;
        Some(Some(EffectLfoPublication {
            slot,
            program,
            tempo_increment,
        }))
    }
}
