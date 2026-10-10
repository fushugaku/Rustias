//! Complete SYS0743E2 Master coefficient-template callback bank.
use crate::effect_buffers::{EFFECT_TEMPLATE_WORDS, limit_template_time};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MasterBufferState {
    pub capacity: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedMasterBufferTemplate {
    pub next: MasterBufferState,
    pub words: [u32; EFFECT_TEMPLATE_WORDS],
    pub count: usize,
}
pub fn relocate_master_template(
    kind: u8,
    state: MasterBufferState,
    input: &[u32],
) -> Option<PreparedMasterBufferTemplate> {
    let needed = match kind {
        8 => 7,
        11 => 31,
        12 => 54,
        13 => 9,
        14..=16 => 11,
        17 | 18 | 20 | 22 => 10,
        19 => 9,
        21 => 20,
        26 => 38,
        27 => 13,
        28 => 9,
        29 => 53,
        _ => 0,
    };
    if kind >= 31 || input.len() < needed || input.len() > EFFECT_TEMPLATE_WORDS {
        return None;
    }
    let mut result = PreparedMasterBufferTemplate {
        next: state,
        words: [0; EFFECT_TEMPLATE_WORDS],
        count: input.len(),
    };
    result.words[..input.len()].copy_from_slice(input);
    let base = 0x2ee00u32;
    let add = |words: &mut [u32], indices: &[usize]| {
        for &i in indices {
            words[i] = words[i].wrapping_add(base);
        }
    };
    let limit = |words: &mut [u32], indices: &[usize], capacity| {
        for &i in indices {
            words[i] = limit_template_time(words[i], capacity);
        }
    };
    let w = &mut result.words;
    match kind {
        8 => add(w, &[6]),
        11 => {
            for v in &mut w[5..31] {
                *v = v.wrapping_add(base);
            }
            result.next.capacity = 0x11200;
        }
        12 => {
            for v in &mut w[33..54] {
                *v = v.wrapping_add(base);
            }
            result.next.capacity = 0x11200;
        }
        13 => {
            result.next.capacity = 0x11200;
            w[5] = base;
            limit(w, &[6, 7, 8], result.next.capacity);
        }
        14 | 16 => {
            result.next.capacity = 0x8900;
            w[7] = base;
            w[9] = 0x37700;
            limit(w, &[8, 10], result.next.capacity);
        }
        15 => {
            result.next.capacity = 0x11200;
            w[7] = base;
            w[9] = base;
            limit(w, &[8, 10], result.next.capacity);
        }
        17 => {
            result.next.capacity = 0x10a80;
            w[6] = base;
            w[8] = base;
            limit(w, &[7, 9], result.next.capacity);
        }
        18 => {
            result.next.capacity = 0x8180;
            w[6] = base;
            w[8] = 0x37700;
            limit(w, &[7, 9], result.next.capacity);
        }
        19 => {
            result.next.capacity = 0x10e40;
            w[6] = base;
            limit(w, &[7, 8], result.next.capacity);
        }
        20 | 22 => {
            result.next.capacity = 0x960;
            w[6] = base;
            w[8] = 0x37700;
            limit(w, &[7, 9], result.next.capacity);
        }
        21 => {
            result.next.capacity = 0x11200;
            add(w, &[13, 14, 15, 19]);
        }
        26 => {
            result.next.capacity = 0x5dc0;
            add(w, &[22, 23, 36, 37]);
        }
        27 => {
            result.next.capacity = 0x4480;
            w[5] = base;
            w[7] = 0x33280;
            w[9] = 0x37700;
            w[11] = 0x3bb80;
            limit(w, &[6, 8, 10, 12], result.next.capacity);
        }
        28 => {
            result.next.capacity = 480;
            w[6] = base;
            w[8] = base + 480;
        }
        29 => {
            result.next.capacity = 0x11200;
            add(w, &[41, 42, 49, 50, 52]);
        }
        _ => {}
    }
    Some(result)
}
