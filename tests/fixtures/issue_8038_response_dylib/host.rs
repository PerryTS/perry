use std::ffi::{c_char, c_int, c_void, CStr, CString};

const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;

#[cfg(target_os = "linux")]
const RTLD_GLOBAL: c_int = 0x100;
#[cfg(target_os = "macos")]
const RTLD_GLOBAL: c_int = 0x8;
#[cfg(target_os = "linux")]
const RTLD_LOCAL: c_int = 0;
#[cfg(target_os = "macos")]
const RTLD_LOCAL: c_int = 0x4;
const RTLD_NOW: c_int = 2;

#[cfg_attr(target_os = "linux", link(name = "dl"))]
unsafe extern "C" {
    fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlerror() -> *const c_char;
}

type GcInit = unsafe extern "C" fn();
type ModuleInit = unsafe extern "C" fn();
type Entry = unsafe extern "C" fn(i64) -> f64;
type PromiseState = unsafe extern "C" fn(*mut c_void) -> i32;
type RunMicrotasks = unsafe extern "C" fn() -> i32;
type TempRootPush = unsafe extern "C" fn(u64) -> u32;
type TempRootGet = unsafe extern "C" fn(u32) -> u64;
type TempRootTruncate = unsafe extern "C" fn(u32);
type RuntimeProbe = unsafe extern "C" fn() -> usize;

fn dynamic_error(context: &str) -> String {
    let detail = unsafe {
        let error = dlerror();
        if error.is_null() {
            "unknown loader error".into()
        } else {
            CStr::from_ptr(error).to_string_lossy().into_owned()
        }
    };
    format!("{context}: {detail}")
}

fn open(path: &str, flags: c_int) -> Result<usize, String> {
    let path = CString::new(path).map_err(|_| format!("NUL in library path {path:?}"))?;
    let handle = unsafe { dlopen(path.as_ptr(), flags) };
    if handle.is_null() {
        Err(dynamic_error("dlopen failed"))
    } else {
        Ok(handle as usize)
    }
}

unsafe fn symbol<T: Copy>(handle: usize, name: &str) -> Result<T, String> {
    let name = CString::new(name).map_err(|_| format!("NUL in symbol {name:?}"))?;
    let pointer = dlsym(handle as *mut c_void, name.as_ptr());
    if pointer.is_null() {
        return Err(dynamic_error("dlsym failed"));
    }
    Ok(std::mem::transmute_copy::<*mut c_void, T>(&pointer))
}

fn main() -> Result<(), String> {
    let arguments: Vec<String> = std::env::args().collect();
    if arguments.len() != 5 {
        return Err("usage: host runtime stdlib app entry-symbol".into());
    }

    let runtime = open(&arguments[1], RTLD_NOW | RTLD_GLOBAL)?;
    let stdlib = open(&arguments[2], RTLD_NOW | RTLD_GLOBAL)?;
    let app = open(&arguments[3], RTLD_NOW | RTLD_LOCAL)?;
    let gc_init: GcInit = unsafe { symbol(runtime, "js_gc_init")? };
    let promise_state: PromiseState = unsafe { symbol(runtime, "js_promise_state")? };
    let run_microtasks: RunMicrotasks =
        unsafe { symbol(runtime, "js_promise_run_microtasks_event_loop")? };
    let temp_root_push: TempRootPush = unsafe { symbol(runtime, "js_gc_temp_root_push")? };
    let temp_root_get: TempRootGet = unsafe { symbol(runtime, "js_gc_temp_root_get")? };
    let temp_root_truncate: TempRootTruncate =
        unsafe { symbol(runtime, "js_gc_temp_root_truncate")? };
    let probe: RuntimeProbe = unsafe { symbol(stdlib, "issue_8075_stdlib_runtime_probe")? };
    if unsafe { probe() } != gc_init as usize {
        return Err("stdlib provider is bound to a different runtime image".into());
    }

    let module_init: ModuleInit = unsafe { symbol(app, "perry_module_init")? };
    let entry: Entry = unsafe { symbol(app, &arguments[4])? };
    unsafe {
        gc_init();
        module_init();
    }

    let promise = unsafe { entry(0) };
    let promise_bits = promise.to_bits();
    if promise_bits & !POINTER_MASK != POINTER_TAG {
        return Err(format!(
            "runIssue8038 returned a non-Promise value: {promise_bits:#018x}"
        ));
    }
    let root = unsafe { temp_root_push(promise_bits) };

    let mut state = 0;
    for _ in 0..10_000 {
        let rooted_bits = unsafe { temp_root_get(root) };
        let promise_pointer = (rooted_bits & POINTER_MASK) as *mut c_void;
        state = unsafe { promise_state(promise_pointer) };
        if state != 0 {
            break;
        }
        unsafe { run_microtasks() };
    }
    unsafe { temp_root_truncate(root) };

    match state {
        1 => Ok(()),
        2 => Err("runIssue8038 rejected".into()),
        other => Err(format!(
            "runIssue8038 did not settle after 10,000 microtask drains (state {other})"
        )),
    }
}
