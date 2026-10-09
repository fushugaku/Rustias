//! Firmware-free native synthesizer C ABI for an AudioWorklet.
use radias_synth_infrastructure::standalone::{PARAMETER_COUNT, StandaloneSynth};
use std::cell::RefCell;
mod rdl_import;
mod sampler;
use sampler::Sampler;

const PRESET_CAPACITY: usize = 65536;
struct WebEngine {
    synth: StandaloneSynth,
    sampler: Sampler,
    held_drums: [[u16; 128]; 4],
    unavailable_timbres: u8,
    unavailable_drums: u16,
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
        let synth = StandaloneSynth::new();
        let sampler = Sampler::new(&synth);
        *state.borrow_mut() = Some(Box::new(WebEngine {
            synth,
            sampler,
            held_drums: [[0; 128]; 4],
            unavailable_timbres: 0,
            unavailable_drums: 0,
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
        state.borrow_mut().as_mut().is_some_and(|e| {
            let global = e.synth.settings[0];
            let instrument = global[142] as usize;
            let sample_amplifier = global[140] != 0
                && global[141] == timbre as i32
                && e.sampler.assigned(instrument)
                && matches!(parameter, 114..=117);
            let accepted = if sample_amplifier {
                e.drum_control(instrument, parameter as usize, value)
            } else {
                e.synth.control(timbre as u8, parameter as usize, value)
            };
            if accepted {
                if matches!(parameter, 140 | 141) {
                    e.sampler.stop();
                    e.held_drums = [[0; 128]; 4];
                }
                e.sampler.sync(&e.synth);
            }
            accepted
        }) as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_value(timbre: u32, parameter: u32) -> i32 {
    if timbre >= 4 || parameter as usize >= PARAMETER_COUNT {
        return 0;
    }
    ENGINE.with(|state| {
        state.borrow().as_ref().map_or(0, |e| {
            let global = e.synth.settings[0];
            let instrument = global[142] as usize;
            if global[140] != 0 && global[141] == timbre as i32 && e.sampler.assigned(instrument) {
                match parameter {
                    114..=117 => return e.synth.drum_settings[instrument][parameter as usize],
                    118 => return 0, // PCM voices do not allocate native Unison actors.
                    _ => {}
                }
            }
            e.synth.value(timbre as u8, parameter as usize)
        })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_drum_control(index: u32, parameter: u32, value: i32) -> u32 {
    if index >= 16 {
        return 0;
    }
    ENGINE.with(|state| {
        state.borrow_mut().as_mut().is_some_and(|e| {
            let accepted = e.drum_control(index as usize, parameter as usize, value);
            if accepted {
                e.sampler.sync(&e.synth);
            }
            accepted
        }) as u32
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
            .is_some_and(|e| e.note(timbre as u8, note as u8, velocity as u8)) as u32
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
            .is_some_and(|e| e.drum_pad(index as usize, velocity as u8)) as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_midi(status: u32, first: u32, second: u32) {
    if status > 255 || first > 127 || second > 127 {
        return;
    }
    ENGINE.with(|state| {
        if let Some(e) = state.borrow_mut().as_mut() {
            e.midi(status as u8, first as u8, second as u8);
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
        e.sampler.stop();
        e.held_drums = [[0; 128]; 4];
        e.synth = synth;
        e.unavailable_timbres = 0;
        e.unavailable_drums = 0;
        e.sampler.sync(&e.synth);
        1
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_stop() {
    ENGINE.with(|state| {
        if let Some(e) = state.borrow_mut().as_mut() {
            e.synth.stop();
            e.sampler.stop();
            e.held_drums = [[0; 128]; 4];
        }
    });
}
/// Browser-imported programs can refer to PCM/input sources absent from this
/// build. Block their instruments until the user selects an available source.
#[unsafe(no_mangle)]
pub extern "C" fn rustias_rdl_mute(timbres: u32, drums: u32) {
    ENGINE.with(|state| {
        if let Some(e) = state.borrow_mut().as_mut() {
            let timbres = (timbres & 15) as u8;
            let drums = (drums & 65535) as u16;
            if e.unavailable_timbres != timbres || e.unavailable_drums != drums {
                e.synth.stop();
                e.sampler.stop();
                e.held_drums = [[0; 128]; 4];
                e.unavailable_timbres = timbres;
                e.unavailable_drums = drums;
            }
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
            let pcm = e.sampler.sample(&e.synth);
            frame[0] = (sample.left.0 as f64 + pcm.left.0 as f64) as f32 / 2147483648.0;
            frame[1] = (sample.right.0 as f64 + pcm.right.0 as f64) as f32 / 2147483648.0;
            e.peak = e.peak.max(frame[0].abs()).max(frame[1].abs());
        }
        e.frames = e.frames.wrapping_add(128);
        e.output.as_ptr()
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_voices() -> u32 {
    ENGINE.with(|state| {
        state.borrow().as_ref().map_or(0, |e| {
            (e.synth.engine.active_count() + e.sampler.active_count()) as u32
        })
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

impl WebEngine {
    // These extra PCM amplifier settings live in the existing per-drum rows.
    // The desktop/native drum controller continues to use its original inputs.
    fn drum_control(&mut self, instrument: usize, parameter: usize, value: i32) -> bool {
        let range = match parameter {
            114 => Some(-64..=63),
            115 => Some(0..=32767),
            116 => Some(0..=1),
            117 => Some(0..=127),
            _ => None,
        };
        if let Some(range) = range {
            if !range.contains(&value) {
                return false;
            }
            self.synth.drum_settings[instrument][parameter] = value;
            true
        } else {
            self.synth.drum_control(instrument as u8, parameter, value)
        }
    }
    fn channel(&self, t: usize) -> u8 {
        let v = self.synth.settings[t];
        if v[72] == 16 {
            v[148] as u8
        } else {
            v[72] as u8
        }
    }
    fn note(&mut self, t: u8, note: u8, velocity: u8) -> bool {
        let global = self.synth.settings[0];
        if global[140] == 0 || global[141] != t as i32 {
            if velocity != 0 && self.unavailable_timbres & (1 << t) != 0 {
                return true;
            }
            return self.synth.note(t, note, velocity);
        }
        let settings = self.synth.settings[t as usize];
        if velocity != 0
            && (settings[71] == 0 || (note as i32) < settings[119] || (note as i32) > settings[120])
        {
            return true;
        }
        let mask = if velocity == 0 {
            let held = self.held_drums[t as usize][note as usize];
            self.held_drums[t as usize][note as usize] = 0;
            held
        } else {
            let mut mask = 0;
            for i in 0..16 {
                if self.synth.drum_settings[i][146] + global[145] - 64 == note as i32 {
                    mask |= 1 << i;
                }
            }
            self.held_drums[t as usize][note as usize] |= mask;
            mask
        };
        for i in 0..16 {
            if mask & (1 << i) != 0 {
                self.drum_pad(i, velocity);
            }
        }
        true
    }
    fn drum_pad(&mut self, instrument: usize, velocity: u8) -> bool {
        if self.synth.settings[0][140] == 0 {
            return false;
        }
        if velocity != 0 && self.unavailable_drums & (1 << instrument) != 0 {
            return true;
        }
        if velocity != 0 {
            self.sampler.choke(&self.synth, instrument);
        }
        if self.sampler.assigned(instrument) {
            if velocity != 0 {
                let group = self.synth.drum_settings[instrument][147];
                if group != 0 {
                    for i in 0..16 {
                        if !self.sampler.assigned(i) && self.synth.drum_settings[i][147] == group {
                            self.synth.drum_pad(i as u8, 0);
                        }
                    }
                }
            }
            self.sampler.trigger(&self.synth, instrument, velocity);
            true
        } else {
            self.synth.drum_pad(instrument as u8, velocity)
        }
    }
    fn midi(&mut self, status: u8, first: u8, second: u8) {
        let channel = status & 15;
        if matches!(status & 240, 0x80 | 0x90) {
            let velocity = if status & 240 == 0x80 { 0 } else { second };
            for t in 0..4 {
                if self.channel(t) == channel {
                    self.note(t as u8, first, velocity);
                }
            }
        } else {
            self.synth.midi(status, first, second);
            let owner = self.synth.settings[0][141] as usize;
            if status & 240 == 0xb0 && self.channel(owner) == channel {
                if first == 7 && self.synth.settings[0][140] != 0 {
                    for instrument in &mut self.synth.drum_settings {
                        if instrument[116] != 0 {
                            instrument[117] = second as i32;
                        }
                    }
                }
                if matches!(first, 120 | 123) {
                    self.held_drums[owner] = [0; 128];
                }
                if first == 120 {
                    self.sampler.stop();
                }
                if first == 123 {
                    for i in 0..16 {
                        self.sampler.trigger(&self.synth, i, 0);
                    }
                }
            }
            self.sampler.sync(&self.synth);
        }
    }
}

/// Staging memory for locally decoded mono PCM at the native 48 kHz rate.
#[unsafe(no_mangle)]
pub extern "C" fn rustias_sample_buffer(instrument: u32, frames: u32) -> *mut f32 {
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .map_or(std::ptr::null_mut(), |e| {
                e.sampler.buffer(instrument as usize, frames as usize)
            })
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_sample_commit(instrument: u32, frames: u32, mode: u32) -> u32 {
    if mode > 2 {
        return 0;
    }
    ENGINE.with(|state| {
        state.borrow_mut().as_mut().is_some_and(|e| {
            let accepted = e
                .sampler
                .commit(instrument as usize, frames as usize, mode as u8);
            if accepted {
                e.synth.drum_pad(instrument as u8, 0);
            }
            accepted
        }) as u32
    })
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_sample_clear(instrument: u32) {
    ENGINE.with(|state| {
        if let Some(e) = state.borrow_mut().as_mut() {
            e.sampler.clear(instrument as usize);
        }
    });
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_sample_mode(instrument: u32, mode: u32) -> u32 {
    if mode > 2 {
        return 0;
    }
    ENGINE.with(|state| {
        state
            .borrow_mut()
            .as_mut()
            .is_some_and(|e| e.sampler.set_mode(instrument as usize, mode as u8)) as u32
    })
}
