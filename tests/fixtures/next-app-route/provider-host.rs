use std::ffi::{c_char, c_int, c_void, CString};

const RTLD_NOW: c_int = 2;
#[cfg(target_os = "macos")]
const RTLD_GLOBAL: c_int = 8;
#[cfg(target_os = "linux")]
const RTLD_GLOBAL: c_int = 0x100;
#[cfg(target_os = "macos")]
const RTLD_LOCAL: c_int = 4;
#[cfg(target_os = "linux")]
const RTLD_LOCAL: c_int = 0;

unsafe extern "C" {
    fn dlopen(path: *const c_char, mode: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

type VoidFn = unsafe extern "C" fn();
type TickFn = unsafe extern "C" fn() -> i32;
type ProbeFn = unsafe extern "C" fn() -> usize;

fn loader_error(context: &str) -> String {
    let message = unsafe {
        let error = dlerror();
        if error.is_null() {
            "unknown loader error".to_string()
        } else {
            std::ffi::CStr::from_ptr(error)
                .to_string_lossy()
                .into_owned()
        }
    };
    format!("{context}: {message}")
}

fn open(path: &str, mode: c_int) -> Result<*mut c_void, String> {
    let path = CString::new(path).map_err(|_| "library path contains NUL".to_string())?;
    let handle = unsafe { dlopen(path.as_ptr(), mode) };
    if handle.is_null() {
        Err(loader_error("dlopen failed"))
    } else {
        Ok(handle)
    }
}

unsafe fn symbol<T: Copy>(handle: *mut c_void, name: &str) -> Result<T, String> {
    let name = CString::new(name).map_err(|_| "symbol name contains NUL".to_string())?;
    let address = unsafe { dlsym(handle, name.as_ptr()) };
    if address.is_null() {
        return Err(loader_error("dlsym failed"));
    }
    Ok(unsafe { std::mem::transmute_copy(&address) })
}

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 4 {
        return Err("usage: provider-host runtime stdlib app".into());
    }

    let runtime = open(&arguments[1], RTLD_NOW | RTLD_GLOBAL)?;
    let stdlib = open(&arguments[2], RTLD_NOW | RTLD_GLOBAL)?;
    let app = open(&arguments[3], RTLD_NOW | RTLD_LOCAL)?;

    let gc_init: VoidFn = unsafe { symbol(runtime, "js_gc_init")? };
    let provider_probe: ProbeFn =
        unsafe { symbol(stdlib, "next_app_route_provider_runtime_probe")? };
    if unsafe { provider_probe() } != gc_init as usize {
        return Err("stdlib provider is bound to a different runtime image".into());
    }

    let module_init: VoidFn = unsafe { symbol(app, "perry_module_init")? };
    let run_microtasks: TickFn =
        unsafe { symbol(runtime, "js_promise_run_microtasks_event_loop")? };
    let timer_tick: TickFn = unsafe { symbol(runtime, "js_timer_tick")? };
    let callback_timer_tick: TickFn = unsafe { symbol(runtime, "js_callback_timer_tick")? };
    let interval_timer_tick: TickFn = unsafe { symbol(runtime, "js_interval_timer_tick")? };
    let run_stdlib_pump: VoidFn = unsafe { symbol(runtime, "js_run_stdlib_pump")? };
    let wait_for_event: VoidFn = unsafe { symbol(runtime, "js_wait_for_event")? };

    unsafe {
        gc_init();
        module_init();
    }
    loop {
        unsafe {
            run_microtasks();
            timer_tick();
            callback_timer_tick();
            interval_timer_tick();
            run_stdlib_pump();
            wait_for_event();
        }
    }
}
