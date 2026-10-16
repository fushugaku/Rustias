//! AOT native DSP, linked directly into the AU/VST3/CLAP binary.
#[cfg(not(rustias_plugin))]
compile_error!("Build with --cfg rustias_plugin; use scripts/build-plugin-engine.py");

use radias_web::native::{METHODS, NativeHost};

#[unsafe(no_mangle)]
pub extern "C" fn rustias_native_create() -> *mut NativeHost {
    Box::into_raw(Box::new(NativeHost::default()))
}
/// # Safety
/// The handle must be live, exclusively owned, and not in use on another thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustias_native_destroy(host: *mut NativeHost) {
    if !host.is_null() {
        drop(unsafe { Box::from_raw(host) });
    }
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_native_method_count() -> u32 {
    METHODS.len() as u32
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_native_method_name(index: u32) -> *const u8 {
    METHODS
        .get(index as usize)
        .map_or(std::ptr::null(), |m| m.name.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn rustias_native_method_arguments(index: u32) -> u32 {
    METHODS
        .get(index as usize)
        .map_or(0, |m| m.arguments as u32)
}
/// # Safety
/// All pointers and lengths must be valid; calls to one handle are serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustias_native_call(
    host: *mut NativeHost,
    method: u32,
    args: *const f64,
    count: u32,
    output: *mut f64,
) -> bool {
    if host.is_null() || output.is_null() || count > 16 || (count != 0 && args.is_null()) {
        return false;
    }
    let args = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args, count as usize) }
    };
    let Some(result) = (unsafe { &mut *host }).call(method as usize, args) else {
        return false;
    };
    unsafe {
        *output = result;
    }
    true
}
/// # Safety
/// The destination is writable for length bytes; calls to the handle are serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustias_native_read(
    host: *const NativeHost,
    token: u32,
    data: *mut u8,
    length: usize,
) -> bool {
    if host.is_null() || data.is_null() {
        return false;
    }
    (unsafe { &*host }).read(token, unsafe {
        std::slice::from_raw_parts_mut(data, length)
    })
}
/// # Safety
/// The source is readable for length bytes; calls to the handle are serialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustias_native_write(
    host: *mut NativeHost,
    token: u32,
    data: *const u8,
    length: usize,
) -> bool {
    if host.is_null() || data.is_null() {
        return false;
    }
    (unsafe { &mut *host }).write(token, unsafe { std::slice::from_raw_parts(data, length) })
}
