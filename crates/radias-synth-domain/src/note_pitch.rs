//! SH3 note initialization and tuning, 01f182..01f4d6, 01f862 and 01f972.
//! Raw program values retain their original centered MIDI representation.
use crate::controller_secondary::FineTuneTable;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PitchProgram {
    pub transpose: u8,
    pub fine_tune: u8,
    pub vibrato_intensity: u8,
    pub bend_range: u8,
    pub bend_enabled: bool,
    pub wheel_enabled: bool,
}
impl Default for PitchProgram {
    fn default() -> Self {
        Self {
            transpose: 64,
            fine_tune: 64,
            vibrato_intensity: 64,
            bend_range: 66,
            bend_enabled: true,
            wheel_enabled: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScaleContext {
    /// Low nibble selects the scale; high nibble selects its root.
    pub selection: u8,
    pub global_transpose: Option<i8>,
    pub custom_cents: [i8; 12],
}

#[derive(Clone, Copy)]
pub struct NotePitchTables {
    pub fine_tune: [i32; 128],
    pub cents: [[i8; 12]; 6],
    pub scaled_root: [[i8; 12]; 2],
    pub scaled_note: [[i8; 12]; 2],
    pub vibrato: FineTuneTable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitializedNote {
    pub wrapped: u8,
    pub clamped_q8: u16,
    pub scale_q16: i32,
}

/// Original 01f2de: preserve pitch class while folding into 0..127.
pub fn fold_note(mut note: i32) -> u8 {
    loop {
        if note < 0 {
            note = note.wrapping_add(12);
        } else if (note as i8) < 0 {
            note = note.wrapping_sub(12);
        } else {
            return note as u8;
        }
    }
}

/// Original top-end snap makes the positive bend endpoint symmetric.
pub fn normalize_bend(raw: u16) -> i16 {
    let raw = if raw > 0x3fc0 { 0x4000 } else { raw };
    raw.wrapping_sub(0x2000) as i16
}

impl PitchProgram {
    pub fn bend_q16(self, bend: i16) -> i32 {
        if self.bend_enabled {
            (self.bend_range as i32 - 64)
                .wrapping_mul(bend as i32)
                .wrapping_shl(3)
        } else {
            0
        }
    }
    pub fn vibrato_depth(self, wheel: u8, table: &FineTuneTable) -> i32 {
        let amount = if self.wheel_enabled {
            (wheel as u32 & 127) * 256 / 127
        } else {
            0
        };
        (amount as i16 as i32)
            .wrapping_mul(table.values[(self.vibrato_intensity & 127) as usize] as i32)
    }
    pub fn tuning_q16(
        self,
        table: &NotePitchTables,
        master_tune: i32,
        virtual_patch: i32,
        manual_offset: i16,
    ) -> i32 {
        table.fine_tune[(self.fine_tune & 127) as usize]
            .wrapping_add(master_tune)
            .wrapping_add(virtual_patch)
            .wrapping_add(manual_offset as i32)
    }
    pub fn initialize(
        self,
        note: u8,
        scale: ScaleContext,
        tables: &NotePitchTables,
        seed: &mut u16,
    ) -> Option<InitializedNote> {
        let note = note as i32 + self.transpose as i32 - 64;
        let wrapped = fold_note(note);
        Some(InitializedNote {
            wrapped,
            clamped_q8: (note.clamp(0, 127) << 8) as u16,
            scale_q16: tables.scale_offset(wrapped, scale, seed)?,
        })
    }
}

impl NotePitchTables {
    /// The original pitch-class ROM has 144 entries. Reject addresses outside
    /// that musical table instead of inventing a tuning for malformed inputs.
    pub fn scale_offset(&self, note: u8, context: ScaleContext, seed: &mut u16) -> Option<i32> {
        let scale = context.selection & 15;
        if scale == 0 || scale > 10 {
            return Some((scale as i32) << 2);
        }
        if scale == 10 {
            let taps = *seed & 0x8805;
            let merged = ((taps & 255) | (taps >> 8)) as u8;
            *seed = seed.wrapping_shl(1) | (merged.count_ones() as u16 & 1);
            return Some(*seed as i16 as i32);
        }
        let root = (context.selection >> 4) as usize;
        if root >= 12 {
            return None;
        }
        let index = note as i32 + 12 - context.global_transpose.unwrap_or(0) as i32 - root as i32;
        if !(0..144).contains(&index) {
            return None;
        }
        let pitch_class = index as usize % 12;
        match scale {
            1 | 2 => {
                let bank = (scale - 1) as usize;
                Some(
                    (self.scaled_note[bank][pitch_class] as i32
                        - self.scaled_root[bank][root] as i32)
                        << 9,
                )
            }
            3..=8 => Some(self.cents[(scale - 3) as usize][pitch_class] as i32 * 65536 / 100),
            9 => Some(context.custom_cents[pitch_class] as i32 * 65536 / 100),
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BasePitch {
    pub assigned_note_q16: i32,
    pub scale_q16: i32,
    pub bend_q16: i32,
    pub tuning_q16: i32,
    pub manual_offset: i16,
    pub drum_transpose: Option<u8>,
}
impl BasePitch {
    pub fn q16(self) -> i32 {
        let pitch = if let Some(transpose) = self.drum_transpose {
            (fold_note(60 + transpose as i32 - 64) as i32) << 16
        } else {
            self.assigned_note_q16
                .wrapping_add(self.scale_q16)
                .wrapping_add(self.bend_q16)
        };
        pitch
            .wrapping_add(self.tuning_q16)
            .wrapping_add((self.manual_offset as i32) << 8)
    }
}
