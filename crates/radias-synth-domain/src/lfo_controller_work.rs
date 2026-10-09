//! Functional instruction work of SYS0164f4..0165ba and its waveform helpers.
//! The branch predicates use waveform inputs, never observed execution times.
use crate::lfo::{LfoTables, LfoWave};

fn warp_work(tables: &LfoTables, phase: u16, shape: i8) -> u16 {
    if shape == 0 {
        return 7;
    }
    let p = u32::from(phase);
    let shape = i32::from(shape);
    if shape < 0 {
        // Both negative branches complement and narrow before the limit test.
        return if p < (-shape as u32) * 1024 { 46 } else { 47 };
    }
    let (index, argument, prefix) = if p < 65536 - shape as u32 * 1024 {
        (shape as usize, p, 21)
    } else {
        ((64 - shape) as usize, !p & 65535, 25)
    };
    let product = (u32::from(tables.warp[index]) * argument) >> 8;
    prefix + if product > 65535 { 17 } else { 18 }
}
fn hold_work(phase: u16, shape: i8) -> u16 {
    if shape == 0 {
        8
    } else if shape > 0 {
        36
    } else if (!u32::from(phase) & 65535) >= (-i32::from(shape)) as u32 * 1024 {
        32
    } else {
        55
    }
}
fn quarter_sine_work(phase: u16) -> u16 {
    if phase >= 16384 {
        36
    } else if phase & 31 == 0 {
        19
    } else {
        25
    }
}
pub(crate) fn waveform_work(tables: &LfoTables, wave: LfoWave, phase: u16, shape: i8) -> u16 {
    match wave {
        LfoWave::Saw => 11 + warp_work(tables, phase, shape),
        LfoWave::Pulse | LfoWave::BipolarPulse => 13,
        LfoWave::Triangle => {
            let shifted = phase.wrapping_add(16384);
            let prefix = if shifted as i16 >= 0 { 18 } else { 17 };
            prefix + if shape == 0 { 7 } else { 27 }
        }
        LfoWave::SampleHold => 8 + hold_work(phase, shape),
        LfoWave::Sine => {
            let negative = (phase as i16) < 0;
            let folded = phase & 32767;
            let prefix = if negative { 16 } else { 14 };
            prefix + quarter_sine_work(folded) + if shape == 0 { 11 } else { 17 + 2 * 101 }
        }
        LfoWave::Zero => 3,
    }
}
