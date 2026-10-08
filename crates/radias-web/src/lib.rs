//! Small C ABI for a firmware-free native synth in an AudioWorklet.
//! No filesystem, network, CPU interpreter or JavaScript sound generation.
use radias_synth_infrastructure::{standalone::StandaloneSynth, synthesizer::Command};
use std::cell::RefCell;

struct WebEngine {
    synth: StandaloneSynth,
    output: [f32; 256],
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
            frames: 0,
            peak: 0.0,
        }))
    });
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_control(timbre: u32, parameter: u32, value: u32) -> u32 {
    if timbre >= 4 || value > 127 {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .is_some_and(|engine| engine.synth.control(timbre as u8, parameter, value as u8))
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
            .is_some_and(|engine| engine.synth.note(timbre as u8, note as u8, velocity as u8))
            as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_midi(status: u32, first: u32, second: u32) {
    if status > 255 || first > 127 || second > 127 {
        return;
    }
    ENGINE.with(|state| {
        if let Some(engine) = state.borrow_mut().as_mut() {
            let channel = (status & 15) as u8;
            let command = match status & 0xf0 {
                0x90 => Command::Midi(channel, first as u8, second as u8),
                0x80 => Command::Midi(channel, first as u8, 0),
                0xb0 if first == 64 => Command::Sustain(channel, second as u8),
                0xb0 if first == 120 => Command::AllSoundOff(channel),
                0xb0 if first == 123 => Command::AllNotesOff(channel),
                _ => return,
            };
            engine.synth.engine.apply(command);
        }
    });
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_stop() {
    ENGINE.with(|state| {
        if let Some(engine) = state.borrow_mut().as_mut() {
            engine.synth.stop();
        }
    });
}
/// Always fills 128 interleaved stereo frames at 48 kHz. The pointer remains
/// valid until reinitialization; JavaScript rereads memory after each call.
#[unsafe(no_mangle)]
pub extern "C" fn rustias_render() -> *const f32 {
    ENGINE.with(|state| {
        let mut state = state.borrow_mut();
        let Some(engine) = state.as_mut() else {
            return std::ptr::null();
        };
        engine.peak = 0.0;
        for frame in engine.output.chunks_exact_mut(2) {
            let sample = engine.synth.engine.sample();
            frame[0] = sample.left.0 as f32 / 2147483648.0;
            frame[1] = sample.right.0 as f32 / 2147483648.0;
            engine.peak = engine.peak.max(frame[0].abs()).max(frame[1].abs());
        }
        engine.frames = engine.frames.wrapping_add(128);
        engine.output.as_ptr()
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_voices() -> u32 {
    ENGINE.with(|state| {
        state
            .borrow()
            .as_ref()
            .map_or(0, |engine| engine.synth.engine.active_count() as u32)
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_frames() -> u32 {
    ENGINE.with(|state| state.borrow().as_ref().map_or(0, |engine| engine.frames))
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_peak() -> f32 {
    ENGINE.with(|state| state.borrow().as_ref().map_or(0.0, |engine| engine.peak))
}
