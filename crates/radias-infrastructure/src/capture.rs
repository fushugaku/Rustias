use radias_domain::{board::Board, dsp::c55::WordWrite};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct Captures {
    pub recording: bool,
    pub dry: Vec<[i32; 2]>,
    pub mix: [Vec<[i32; 8]>; 2],
    pub vocoder: [Vec<[i32; 2]>; 2],
    pub dry_enabled: bool,
    pub mix_enabled: bool,
    pub vocoder_enabled: bool,
}
pub type SharedCaptures = Arc<Mutex<Captures>>;
pub fn attach(board: &mut Board, capture: SharedCaptures) {
    for chip in 0..2 {
        let shared = capture.clone();
        board.dsp[chip].store_observer = Some(Box::new(
            move |w: &WordWrite, memory: &[u16], xar: &[u32; 8], t: &[u16; 4]| {
                if w.host || w.dma || (w.pc != 0xa333 && w.pc != 0xd52a) {
                    return;
                }
                let mut c = shared.lock().unwrap();
                if !c.recording {
                    return;
                }
                if w.pc == 0xd52a {
                    if c.vocoder_enabled {
                        let bank = xar[0] & 0x7f0000;
                        let left = bank | xar[0].wrapping_add(t[0] as u32) & 0xffff;
                        let right = bank | xar[0].wrapping_add(t[1] as u32) & 0xffff;
                        if w.address == (right ^ 1)
                            && ((left | 1).max(right | 1) as usize) < memory.len()
                        {
                            c.vocoder[chip].push([
                                ((memory[left as usize] as u32) << 16
                                    | memory[(left ^ 1) as usize] as u32)
                                    as i32,
                                ((memory[right as usize] as u32) << 16
                                    | memory[(right ^ 1) as usize] as u32)
                                    as i32,
                            ]);
                        }
                    }
                    return;
                }
                if c.mix_enabled && memory.len() > 0x4001 {
                    let base = memory[0x4001].wrapping_add(8) as usize;
                    if base + 15 < memory.len() && w.address as usize == base + 15 {
                        let mut frame = [0; 8];
                        for i in 0..8 {
                            frame[i] = ((memory[base + 2 * i] as u32) << 16
                                | memory[base + 2 * i + 1] as u32)
                                as i32;
                        }
                        c.mix[chip].push(frame);
                    }
                }
                if chip == 0
                    && c.dry_enabled
                    && w.address >= 0x46d
                    && w.address <= 0x4cd
                    && (w.address - 0x46d) % 0x20 == 0
                {
                    let a = w.address as usize - 3;
                    c.dry.push([
                        ((memory[a] as u32) << 16 | memory[a + 1] as u32) as i32,
                        ((memory[a + 2] as u32) << 16 | memory[a + 3] as u32) as i32,
                    ]);
                }
            },
        ));
    }
}
