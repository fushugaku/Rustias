//! Per-instance native host for the exact same engine entry points as WASM.
//! The caller serializes access to each host; instances can move between threads.
use super::*;
use std::cell::Cell;
mod modulation;
use modulation::Modulation;

thread_local! {
    static ACTIVE: Cell<*const NativeHost> = const { Cell::new(std::ptr::null()) };
}

pub(super) fn with_active<R>(f: impl FnOnce(&RefCell<Option<Box<WebEngine>>>) -> R) -> R {
    ACTIVE.with(|active| {
        let host = active.get();
        if host.is_null() {
            ENGINE.with(f)
        } else {
            // Set only for the duration of a serialized NativeHost::call.
            f(unsafe { &(*host).engine })
        }
    })
}

struct Scope(*const NativeHost);
impl Drop for Scope {
    fn drop(&mut self) {
        ACTIVE.with(|active| active.set(self.0));
    }
}

enum Value {
    Number(f64),
    Buffer(*mut u8, usize, bool),
}
pub struct Method {
    pub name: &'static [u8],
    pub arguments: usize,
    invoke: fn(&[f64]) -> Value,
}
macro_rules! number {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        Method { name: concat!(stringify!($name), "\0").as_bytes(),
            arguments: 0 $(+ { let _ = stringify!($arg); 1 })*,
            invoke: |args| { #[allow(unused_mut)] let mut index = 0; $(let $arg = {let value = args[index] as $ty; index += 1; value};)*
                let _ = (args, index); Value::Number($name($($arg),*) as f64) } }
    };
}
macro_rules! action {
    ($name:ident($($arg:ident: $ty:ty),*)) => {
        Method { name: concat!(stringify!($name), "\0").as_bytes(),
            arguments: 0 $(+ { let _ = stringify!($arg); 1 })*,
            invoke: |args| { #[allow(unused_mut)] let mut index = 0; $(let $arg = {let value = args[index] as $ty; index += 1; value};)*
                let _ = (args, index); $name($($arg),*); Value::Number(0.0) } }
    };
}

/// This table forwards to shared Rust functions; it contains no synthesis code.
pub static METHODS: &[Method] = &[
    action!(rustias_init()),
    number!(rustias_effect_catalog()),
    Method {
        name: b"rustias_effect_catalog_buffer\0",
        arguments: 0,
        invoke: |_| {
            Value::Buffer(
                rustias_effect_catalog_buffer().cast_mut(),
                rustias_effect_catalog() as usize,
                false,
            )
        },
    },
    number!(rustias_effect(slot: u32, length: u32)),
    number!(rustias_effect_chain(timbre: u32, length: u32)),
    number!(rustias_chain_effect(timbre: u32, length: u32)),
    number!(rustias_timbre_capacity()),
    number!(rustias_parameter_count()),
    number!(rustias_poly_motion(length: u32)),
    number!(rustias_poly_motion_position(timbre: u32, lane: u32)),
    action!(rustias_poly_motion_tempo(tempo: f64)),
    number!(rustias_osc3_control(timbre: u32, control: u32, value: i32)),
    number!(rustias_control(timbre: u32, parameter: u32, value: i32)),
    number!(rustias_value(timbre: u32, parameter: u32)),
    number!(rustias_drum_control(index: u32, parameter: u32, value: i32)),
    number!(rustias_note(timbre: u32, note: u32, velocity: u32)),
    number!(rustias_drum_pad(index: u32, velocity: u32)),
    action!(rustias_midi(status: u32, first: u32, second: u32)),
    number!(rustias_circuit(timbre: u32, length: u32)),
    Method {
        name: b"rustias_preset_buffer\0",
        arguments: 0,
        invoke: |_| {
            Value::Buffer(
                rustias_preset_buffer(),
                rustias_preset_capacity() as usize,
                true,
            )
        },
    },
    number!(rustias_preset_capacity()),
    number!(rustias_preset_reserve(length: u32)),
    number!(rustias_save()),
    number!(rustias_load(length: u32)),
    action!(rustias_stop()),
    action!(rustias_rdl_mute(timbres: u32, drums: u32)),
    Method {
        name: b"rustias_render\0",
        arguments: 0,
        invoke: |_| {
            Value::Buffer(
                rustias_render().cast_mut().cast(),
                256 * size_of::<f32>(),
                false,
            )
        },
    },
    number!(rustias_voices()),
    number!(rustias_frames()),
    number!(rustias_peak()),
    number!(rustias_effect_delay_bytes()),
    Method {
        name: b"rustias_sample_buffer\0",
        arguments: 2,
        invoke: |a| {
            Value::Buffer(
                rustias_sample_buffer(a[0] as u32, a[1] as u32).cast(),
                a[1] as u32 as usize * size_of::<f32>(),
                true,
            )
        },
    },
    number!(rustias_sample_commit(instrument: u32, frames: u32, mode: u32)),
    action!(rustias_sample_clear(instrument: u32)),
    number!(rustias_sample_mode(instrument: u32, mode: u32)),
    number!(rustias_voice_capacity()),
    number!(rustias_drum_gain(db: i32)),
    Method {
        name: b"rustias_library_sample_buffer\0",
        arguments: 2,
        invoke: |a| {
            Value::Buffer(
                rustias_library_sample_buffer(a[0] as u32, a[1] as u32).cast(),
                a[1] as u32 as usize * size_of::<f32>(),
                true,
            )
        },
    },
    number!(rustias_library_sample_commit(asset: u32, frames: u32)),
    number!(rustias_library_profile(id: u32, asset: u32, timbre: u32, mode: u32)),
    number!(rustias_library_control(id: u32, parameter: u32, value: i32)),
    number!(rustias_library_value(id: u32, parameter: u32)),
    action!(rustias_library_sync()),
    action!(rustias_library_reset()),
    number!(rustias_library_note(timbre: u32, id: u32, velocity: u32)),
    number!(rustias_modulation_state(length: u32)),
    action!(rustias_modulation_restore()),
    action!(rustias_modulation_play()),
    action!(rustias_modulation_stop()),
    action!(rustias_modulation_reset()),
    action!(rustias_modulation_tempo(tempo: f64)),
    action!(rustias_modulation_transport(playing: u32, tempo: f64, beat: f64, seek: u32)),
    action!(rustias_modulation_parameter(index: u32, value: f64)),
    number!(rustias_modulation_gain()),
    number!(rustias_modulation_profile(timbre: u32, id: u32, length: u32)),
    number!(rustias_modulation_status()),
    action!(rustias_modulation_accept_values()),
];

fn with_modulation<R>(f: impl FnOnce(&mut Modulation) -> R) -> R {
    ACTIVE.with(|active| {
        let host = active.get();
        assert!(!host.is_null(), "Native modulation requires a NativeHost");
        f(&mut unsafe { &*host }.modulation.borrow_mut())
    })
}
fn rustias_modulation_state(length: u32) -> u32 {
    modulation::bytes(length).is_some_and(|bytes| with_modulation(|m| m.set_state(&bytes))) as u32
}
fn rustias_modulation_restore() {
    with_modulation(Modulation::restore);
}
fn rustias_modulation_accept_values() {
    with_modulation(Modulation::accept_values);
}
fn rustias_modulation_play() {
    with_modulation(Modulation::play);
}
fn rustias_modulation_stop() {
    with_modulation(Modulation::stop);
}
fn rustias_modulation_reset() {
    with_modulation(|m| m.reset(0.));
}
fn rustias_modulation_tempo(tempo: f64) {
    with_modulation(|m| m.set_tempo(tempo));
}
fn rustias_modulation_transport(playing: u32, tempo: f64, beat: f64, seek: u32) {
    with_modulation(|m| m.transport(playing != 0, tempo, beat, seek != 0));
}
fn rustias_modulation_parameter(index: u32, value: f64) {
    with_modulation(|m| m.parameter(index as usize, value));
}
fn rustias_modulation_gain() -> f64 {
    with_modulation(|m| m.gain())
}
fn rustias_modulation_profile(timbre: u32, id: u32, length: u32) -> u32 {
    let Some(bytes) = modulation::bytes(length) else {
        return 0;
    };
    let Ok(source) = serde_json::from_slice::<String>(&bytes) else {
        return 0;
    };
    if timbre as usize >= TIMBRE_COUNT || source.len() > 200 {
        return 0;
    }
    with_modulation(|m| m.set_profile(timbre as usize, id, source));
    1
}
fn rustias_modulation_status() -> u32 {
    let bytes = serde_json::to_vec(&with_modulation(|m| m.status())).unwrap();
    if modulation::stage(&bytes) {
        bytes.len() as u32
    } else {
        0
    }
}

struct Region {
    token: u32,
    pointer: *mut u8,
    length: usize,
    writable: bool,
}

pub struct NativeHost {
    engine: RefCell<Option<Box<WebEngine>>>,
    modulation: RefCell<Modulation>,
    region: Option<Region>,
    serial: u32,
}
impl Default for NativeHost {
    fn default() -> Self {
        Self {
            engine: RefCell::new(None),
            modulation: RefCell::new(Modulation::default()),
            region: None,
            serial: 0,
        }
    }
}
impl NativeHost {
    pub fn call(&mut self, method: usize, args: &[f64]) -> Option<f64> {
        let method = METHODS.get(method)?;
        if args.len() != method.arguments || args.iter().any(|v| !v.is_finite()) {
            return None;
        }
        // A token never exposes a process address to JavaScript. Any subsequent
        // engine operation revokes it, including operations that reallocate PCM.
        self.region = None;
        let _scope = Scope(ACTIVE.with(|active| active.replace(self)));
        match method.name {
            b"rustias_init\0" => *self.modulation.borrow_mut() = Modulation::default(),
            b"rustias_render\0" => self.modulation.borrow_mut().before_render(128),
            b"rustias_save\0" => self.modulation.borrow_mut().restore(),
            b"rustias_stop\0" => self.modulation.borrow_mut().stop(),
            b"rustias_load\0" => {
                // Restoring modulated FX can use the shared JSON staging buffer.
                // Preserve the caller's incoming program across that restoration.
                let bytes = modulation::bytes(args[0] as u32);
                self.modulation.borrow_mut().stop();
                if let Some(bytes) = bytes {
                    modulation::stage(&bytes);
                }
            }
            b"rustias_midi\0" => self.modulation.borrow_mut().restore(),
            _ => {}
        }
        let result = (method.invoke)(args);
        match method.name {
            b"rustias_note\0" if matches!(result, Value::Number(v) if v != 0.) => self
                .modulation
                .borrow_mut()
                .note(args[0] as usize, args[1] as u64, args[2] as u32),
            b"rustias_library_note\0" if matches!(result, Value::Number(v) if v != 0.) => {
                self.modulation.borrow_mut().note(
                    args[0] as usize,
                    (1u64 << 32) | args[1] as u64,
                    args[2] as u32,
                )
            }
            b"rustias_drum_pad\0" if matches!(result, Value::Number(v) if v != 0.) => {
                self.modulation.borrow_mut().note(
                    rustias_value(0, 141) as usize,
                    128 + args[0] as u64,
                    args[1] as u32,
                )
            }
            b"rustias_midi\0" => {
                let command = args[0] as u32 & 240;
                if command == 128 || command == 144 {
                    for t in 0..TIMBRE_COUNT {
                        let channel = rustias_value(t as u32, 72);
                        let channel = if channel == 16 {
                            rustias_value(0, 148)
                        } else {
                            channel
                        };
                        if rustias_value(t as u32, 71) != 0
                            && channel == (args[0] as i32 & 15)
                            && (rustias_value(t as u32, 119)..=rustias_value(t as u32, 120))
                                .contains(&(args[1] as i32))
                        {
                            self.modulation.borrow_mut().note(
                                t,
                                args[1] as u64,
                                if command == 144 { args[2] as u32 } else { 0 },
                            );
                        }
                    }
                } else if command == 176 || command == 224 {
                    self.modulation.borrow_mut().accept_values();
                    if command == 176 && (args[1] == 120. || args[1] == 123.) {
                        for t in 0..TIMBRE_COUNT {
                            let channel = rustias_value(t as u32, 72);
                            let channel = if channel == 16 {
                                rustias_value(0, 148)
                            } else {
                                channel
                            };
                            if channel == (args[0] as i32 & 15) {
                                self.modulation.borrow_mut().release_keys(t);
                            }
                        }
                    }
                }
            }
            b"rustias_library_reset\0" => self.modulation.borrow_mut().clear_profiles(),
            b"rustias_control\0" if args[1] == 89. => self
                .modulation
                .borrow_mut()
                .set_tempo(rustias_value(0, 89) as f64 / 10.),
            _ => {}
        }
        Some(match result {
            Value::Number(value) => value,
            Value::Buffer(pointer, length, writable) => {
                if pointer.is_null() {
                    return Some(0.0);
                }
                self.serial = self.serial.wrapping_add(1).max(1);
                self.region = Some(Region {
                    token: self.serial,
                    pointer,
                    length,
                    writable,
                });
                self.serial as f64
            }
        })
    }
    pub fn read(&self, token: u32, bytes: &mut [u8]) -> bool {
        let Some(region) = self
            .region
            .as_ref()
            .filter(|r| r.token == token && bytes.len() <= r.length)
        else {
            return false;
        };
        // This region belongs to the live engine and remains valid until call().
        bytes.copy_from_slice(unsafe { std::slice::from_raw_parts(region.pointer, bytes.len()) });
        true
    }
    pub fn write(&mut self, token: u32, bytes: &[u8]) -> bool {
        let Some(region) = self
            .region
            .as_ref()
            .filter(|r| r.writable && r.token == token && bytes.len() <= r.length)
        else {
            return false;
        };
        unsafe { std::slice::from_raw_parts_mut(region.pointer, bytes.len()) }
            .copy_from_slice(bytes);
        true
    }
}
