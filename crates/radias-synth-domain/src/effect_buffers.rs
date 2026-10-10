//! Original effect buffer pairing and all insert coefficient-template callbacks.
pub const EFFECT_TEMPLATE_WORDS: usize = 80;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectBufferSlice {
    pub offset: u32,
    pub frames: u32,
}
pub struct EffectBufferTables {
    pub profiles: [u8; 31],
    pub frames: [u32; 10],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedEffectBufferTemplate {
    pub layout: EffectBufferSlice,
    pub words: [u32; EFFECT_TEMPLATE_WORDS],
    pub count: usize,
}
impl PreparedEffectBufferTemplate {
    pub fn words(&self) -> &[u32] {
        &self.words[..self.count]
    }
}
/// Whole SYS074212: Rotary has a buffer callback despite profile zero.
pub fn effect_uses_buffer(kind: u32) -> bool {
    matches!(kind,8|11..=22|26..=29)
}
/// SYS070F16 shifts the template address at most four times; SYS070EA8
/// then preserves positive values or substitutes the signed minimum 640.
pub(crate) fn limit_template_time(mut word: u32, frames: u32) -> u32 {
    let limit = frames.wrapping_shl(7);
    for _ in 0..4 {
        if word <= limit {
            break;
        }
        word >>= 1;
    }
    if (word as i32) < 1 { 640 } else { word }
}
impl EffectBufferTables {
    pub fn pair(&self, first: u8, second: u8) -> Option<[EffectBufferSlice; 2]> {
        let a = usize::from(*self.profiles.get(usize::from(first))?);
        let b = usize::from(*self.profiles.get(usize::from(second))?);
        let fixed_a = *self.frames.get(a)?;
        let fixed_b = *self.frames.get(b)?;
        let (a, b) = if a == 0 {
            (0, fixed_b)
        } else if a == 9 {
            if b == 9 {
                (24000, 24000)
            } else {
                (48000u32.wrapping_sub(fixed_b), fixed_b)
            }
        } else if b == 9 {
            (fixed_a, 48000u32.wrapping_sub(fixed_a))
        } else {
            (fixed_a, fixed_b)
        };
        Some([
            EffectBufferSlice {
                offset: 0,
                frames: a,
            },
            EffectBufferSlice {
                offset: a,
                frames: b,
            },
        ])
    }
    pub fn relocate_template(
        &self,
        kind: u8,
        layout: EffectBufferSlice,
        buffer_origin: u32,
        input: &[u32],
    ) -> Option<PreparedEffectBufferTemplate> {
        if kind >= 31 || input.len() > EFFECT_TEMPLATE_WORDS {
            return None;
        }
        let needed = match kind {
            8 => 7,
            11 => 30,
            12 => 61,
            13 => 9,
            14..=16 => 11,
            17 | 18 | 20 | 22 => 10,
            19 => 9,
            21 => 20,
            26 => 23,
            27 | 28 => 9,
            29 => 53,
            _ => 0,
        };
        if input.len() < needed {
            return None;
        }
        let mut p = PreparedEffectBufferTemplate {
            layout,
            words: [0; EFFECT_TEMPLATE_WORDS],
            count: input.len(),
        };
        p.words[..input.len()].copy_from_slice(input);
        let base = layout.offset.wrapping_add(buffer_origin);
        let add = |words: &mut [u32], indices: &[usize]| {
            for &i in indices {
                words[i] = words[i].wrapping_add(base);
            }
        };
        let limit = |words: &mut [u32], indices: &[usize], frames| {
            for &i in indices {
                words[i] = limit_template_time(words[i], frames);
            }
        };
        match kind {
            8 => p.words[6] = base,
            11 => {
                for word in &mut p.words[6..30] {
                    *word = word.wrapping_add(base);
                }
            }
            12 => {
                for word in &mut p.words[33..50] {
                    *word = word.wrapping_add(base);
                }
                add(&mut p.words, &[57, 58, 59, 60]);
            }
            13 => {
                p.words[5] = base;
                limit(&mut p.words, &[6, 7, 8], layout.frames);
            }
            14 | 16 => {
                p.words[7] = base;
                p.words[9] = base.wrapping_add(layout.frames >> 1);
                limit(&mut p.words, &[8, 10], layout.frames >> 1);
            }
            15 => {
                p.words[7] = base;
                p.words[9] = base;
                limit(&mut p.words, &[8, 10], layout.frames);
            }
            17 => {
                p.words[6] = base;
                p.words[8] = base;
                p.layout.frames = p.layout.frames.wrapping_sub(1920);
                limit(&mut p.words, &[7, 9], p.layout.frames);
            }
            18 | 20 => {
                p.words[6] = base;
                p.words[8] = base.wrapping_add(layout.frames >> 1);
                p.layout.frames =
                    p.layout
                        .frames
                        .wrapping_sub(if kind == 18 { 3840 } else { 2880 });
                limit(&mut p.words, &[7, 9], p.layout.frames >> 1);
            }
            19 => {
                p.words[6] = base;
                p.layout.frames = p.layout.frames.wrapping_sub(960);
                limit(&mut p.words, &[7, 8], p.layout.frames);
            }
            21 => add(&mut p.words, &[13, 14, 15, 19]),
            22 => {
                p.words[6] = base;
                p.words[8] = base.wrapping_add(layout.frames >> 1);
                limit(&mut p.words, &[7, 9], 2400);
            }
            26 => add(&mut p.words, &[22]),
            27 => {
                p.words[5] = base;
                p.words[7] = base.wrapping_add(layout.frames >> 1);
                limit(&mut p.words, &[6, 8], layout.frames >> 1);
            }
            28 => {
                p.words[6] = base;
                p.words[8] = base.wrapping_add(layout.frames >> 1);
            }
            29 => add(&mut p.words, &[41, 42, 49, 50, 52]),
            _ => {}
        }
        Some(p)
    }
}
