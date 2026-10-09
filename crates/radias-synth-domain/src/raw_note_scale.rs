//! Original physical scale-table lookups, including adjacent immutable ROM.
//! SYS01f2f8 dispatches by the low nibble; it does not wrap the class index.
use crate::lfo::LfoState;

pub struct RawNoteScaleTables {
    /// Original class-ROM offsets -192..447; byte inputs need -130..395.
    pub pitch_classes: [i8; 640],
    /// Each original table has signed byte indexing -128..127.
    pub cents: [[i8; 256]; 6],
    pub scaled_note: [[i8; 256]; 2],
    pub scaled_root: [[i8; 16]; 2],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RawNoteScaleContext {
    pub selection: u8,
    pub global_transpose: Option<i8>,
    /// Configuration RAM window centered at its custom-tuning base (+12hex).
    pub custom_cents: [i8; 256],
}
impl RawNoteScaleTables {
    /// Returns the scale offset and functional work of the whole scale service.
    pub fn offset(&self, note: u8, context: RawNoteScaleContext, seed: &mut u16) -> (i32, u16) {
        let scale = context.selection & 15;
        if scale == 0 || scale > 10 {
            return (i32::from(scale) << 2, 26);
        }
        if scale == 10 {
            let work = 136 - (*seed & 0x8805).count_ones() as u16;
            return (i32::from(LfoState::next_random(seed)), work);
        }
        let root = usize::from(context.selection >> 4);
        let index =
            i32::from(note) + 12 - i32::from(context.global_transpose.unwrap_or(0)) - root as i32;
        let class = i32::from(self.pitch_classes[(index + 192) as usize]);
        let physical = (class + 128) as usize;
        if scale <= 2 {
            let bank = usize::from(scale - 1);
            return (
                (i32::from(self.scaled_note[bank][physical])
                    - i32::from(self.scaled_root[bank][root]))
                    << 9,
                63,
            );
        }
        let cents = if scale == 9 {
            context.custom_cents[physical]
        } else {
            self.cents[usize::from(scale - 3)][physical]
        };
        (
            i32::from(cents) * 65536 / 100,
            if scale == 8 { 461 } else { 464 },
        )
    }
}
