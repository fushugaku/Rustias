//! Firmware-free native synthesizer C ABI for an AudioWorklet.
use radias_synth_infrastructure::standalone::{PARAMETER_COUNT, StandaloneSynth};
use std::cell::RefCell;

const PRESET_CAPACITY: usize = 65536;
struct WebEngine {
    synth: StandaloneSynth,
    output: [f32; 256],
    preset: Vec<u8>,
    frames: u32,
    peak: f32,
}
thread_local! {
    static ENGINE: RefCell<Option<Box<WebEngine>>> = const { RefCell::new(None) };
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_init() {
    ENGINE.with(|state| {
        *state.borrow_mut() = Some(Box::new(WebEngine {
            synth: StandaloneSynth::new(),
            output: [0.0; 256],
            preset: vec![0; PRESET_CAPACITY],
            frames: 0,
            peak: 0.0,
        }));
    });
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_parameter_count() -> u32 {
    PARAMETER_COUNT as u32
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_control(timbre: u32, parameter: u32, value: i32) -> u32 {
    if timbre >= 4 {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .is_some_and(|e| e.synth.control(timbre as u8, parameter as usize, value))
            as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_value(timbre: u32, parameter: u32) -> i32 {
    if timbre >= 4 || parameter as usize >= PARAMETER_COUNT {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map_or(0, |e| e.synth.value(timbre as u8, parameter as usize))
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_drum_control(index: u32, parameter: u32, value: i32) -> u32 {
    if index >= 16 {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .is_some_and(|e| e.synth.drum_control(index as u8, parameter as usize, value))
            as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_note(timbre: u32, note: u32, velocity: u32) -> u32 {
    if timbre >= 4 || note > 127 || velocity > 127 {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .is_some_and(|e| e.synth.note(timbre as u8, note as u8, velocity as u8)) as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_drum_pad(index: u32, velocity: u32) -> u32 {
    if index >= 16 || velocity > 127 {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .is_some_and(|e| e.synth.drum_pad(index as u8, velocity as u8)) as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_midi(status: u32, first: u32, second: u32) {
    if status > 255 || first > 127 || second > 127 {
        return;
    }
    ENGINE.with(|state| {
        if let Some(e) = state.borrow_mut().as_mut() {
            e.synth.midi(status as u8, first as u8, second as u8);
        }
    });
}
/// Shared input/output buffer for complete JSON programs. No external data is loaded.
#[unsafe(no_mangle)]
pub extern "C" fn rustias_preset_buffer() -> *mut u8 {
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .map_or(std::ptr::null_mut(), |e| e.preset.as_mut_ptr())
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_preset_capacity() -> u32 {
    PRESET_CAPACITY as u32
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_save() -> u32 {
    ENGINE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(e) = state.as_mut() else {
            return 0;
        };
        let bytes = e.synth.save();
        if bytes.len() > PRESET_CAPACITY {
            return 0;
        }
        e.preset[..bytes.len()].copy_from_slice(&bytes);
        bytes.len() as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_load(length: u32) -> u32 {
    if length as usize > PRESET_CAPACITY {
        return 0;
    }
    ENGINE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(e) = state.as_mut() else {
            return 0;
        };
        let Some(synth) = StandaloneSynth::load(&e.preset[..length as usize]) else {
            return 0;
        };
        e.synth = synth;
        1
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_stop() {
    ENGINE.with(|state| {
        if let Some(e) = state.borrow_mut().as_mut() {
            e.synth.stop();
        }
    });
}
/// 128 interleaved stereo frames at 48 kHz; reread memory after each render.
#[unsafe(no_mangle)]
pub extern "C" fn rustias_render() -> *const f32 {
    ENGINE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(e) = state.as_mut() else {
            return std::ptr::null();
        };
        e.peak = 0.0;
        for frame in e.output.chunks_exact_mut(2) {
            let sample = e.synth.engine.sample();
            frame[0] = sample.left.0 as f32 / 2147483648.0;
            frame[1] = sample.right.0 as f32 / 2147483648.0;
            e.peak = e.peak.max(frame[0].abs()).max(frame[1].abs());
        }
        e.frames = e.frames.wrapping_add(128);
        e.output.as_ptr()
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_voices() -> u32 {
    ENGINE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map_or(0, |e| e.synth.engine.active_count() as u32)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_frames() -> u32 {
    ENGINE.with(|state| state.borrow().as_ref().map_or(0, |e| e.frames))
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_peak() -> f32 {
    ENGINE.with(|state| state.borrow().as_ref().map_or(0.0, |e| e.peak))
}
