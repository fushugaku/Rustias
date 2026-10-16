//! Per-instance native host for the exact same engine entry points as WASM.
//! The caller serializes access to each host; instances can move between threads.
use super::*;
use std::cell::Cell;

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
];

struct Region {
    token: u32,
    pointer: *mut u8,
    length: usize,
    writable: bool,
}

pub struct NativeHost {
    engine: RefCell<Option<Box<WebEngine>>>,
    region: Option<Region>,
    serial: u32,
}
impl Default for NativeHost {
    fn default() -> Self {
        Self {
            engine: RefCell::new(None),
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
        Some(match (method.invoke)(args) {
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
