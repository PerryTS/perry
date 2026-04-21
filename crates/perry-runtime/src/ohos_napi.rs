//! HarmonyOS NAPI entry wrapper.
//!
//! HarmonyOS NEXT apps ship as `.so` libraries loaded by the ArkTS runtime,
//! not as standalone executables. When ArkTS executes
//! `import entry from 'libperry_app.so'`, the loader invokes each module's
//! `nm_register_func` (set up via `napi_module_register`) to populate the
//! `exports` object. We expose one function, `run`, which calls Perry's
//! compiled `main()` and returns its exit code to ArkTS as an `int32`.
//!
//! Registration happens via `.init_array` — Rust's equivalent of
//! `__attribute__((constructor))` — so it runs automatically on `dlopen`.
//! The TS entry is *not* invoked at load; ArkTS must explicitly call
//! `entry.run()` from its `EntryAbility.onCreate` (see the ArkTS shim
//! emitted by the compiler alongside the `.so`).
//!
//! Multi-call semantics: `entry.run()` calls `main()` every time, which
//! re-runs module init + user code. For the logic-only v1, that's the
//! correct shape — ArkTS calls it once from `onCreate`. A future
//! lifecycle-aware mode would need a guard to make re-entry a no-op
//! (or restart-friendly), but that's out of scope here.

use std::os::raw::{c_char, c_int, c_uint, c_void};
use std::ptr;

#[repr(C)] pub struct NapiEnv(());
#[repr(C)] pub struct NapiValue(());
#[repr(C)] pub struct NapiCallbackInfo(());

pub type NapiStatus = c_int;
pub type NapiCallback = unsafe extern "C" fn(
    env: *mut NapiEnv,
    info: *mut NapiCallbackInfo,
) -> *mut NapiValue;

#[repr(C)]
pub struct NapiModule {
    pub nm_version: c_int,
    pub nm_flags: c_uint,
    pub nm_filename: *const c_char,
    pub nm_register_func: Option<
        unsafe extern "C" fn(env: *mut NapiEnv, exports: *mut NapiValue) -> *mut NapiValue,
    >,
    pub nm_modname: *const c_char,
    pub nm_priv: *mut c_void,
    pub reserved: [*mut c_void; 4],
}

// SAFETY: NapiModule is only mutated once during .init_array execution,
// before any ArkTS thread can observe it. After that it's read-only.
unsafe impl Sync for NapiModule {}

extern "C" {
    pub fn napi_module_register(m: *mut NapiModule);
    pub fn napi_create_int32(
        env: *mut NapiEnv,
        value: i32,
        result: *mut *mut NapiValue,
    ) -> NapiStatus;
    pub fn napi_create_function(
        env: *mut NapiEnv,
        utf8name: *const c_char,
        length: usize,
        cb: NapiCallback,
        data: *mut c_void,
        result: *mut *mut NapiValue,
    ) -> NapiStatus;
    pub fn napi_set_named_property(
        env: *mut NapiEnv,
        object: *mut NapiValue,
        utf8name: *const c_char,
        value: *mut NapiValue,
    ) -> NapiStatus;
}

// Perry's compiled entry. The TypeScript compiler always emits `main`
// (module init + user top-level code). On HarmonyOS we don't use it as
// the process entry — it's just a regular exported function that the
// NAPI `run` callback invokes.
//
// `-Wl,-Bsymbolic` on the link line ensures this resolves to our own
// `main`, not the ArkTS host process's `main`.
extern "C" {
    fn main() -> c_int;
}

unsafe extern "C" fn run(
    env: *mut NapiEnv,
    _info: *mut NapiCallbackInfo,
) -> *mut NapiValue {
    let exit_code = main();
    let mut out: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_int32(env, exit_code, &mut out);
    out
}

unsafe extern "C" fn napi_init(
    env: *mut NapiEnv,
    exports: *mut NapiValue,
) -> *mut NapiValue {
    // Export a single function, `run`. ArkTS callers do `entry.run()`.
    let name = b"run\0";
    let mut fn_val: *mut NapiValue = ptr::null_mut();
    let _ = napi_create_function(
        env,
        name.as_ptr() as *const c_char,
        3,
        run,
        ptr::null_mut(),
        &mut fn_val,
    );
    let _ = napi_set_named_property(
        env,
        exports,
        name.as_ptr() as *const c_char,
        fn_val,
    );
    exports
}

// ArkTS's `import entry from 'libperry_app.so'` matches this modname. If
// the user customizes the output filename, the compiler also rewrites the
// matching ArkTS shim's `import` to match.
static MODNAME: &[u8] = b"entry\0";

static mut NAPI_MODULE_DESC: NapiModule = NapiModule {
    nm_version: 1,
    nm_flags: 0,
    nm_filename: ptr::null(),
    nm_register_func: Some(napi_init),
    nm_modname: ptr::null(),
    nm_priv: ptr::null_mut(),
    reserved: [ptr::null_mut(); 4],
};

// Runs on .so load, before any ArkTS code executes.
extern "C" fn register_module() {
    unsafe {
        let desc_ptr = &raw mut NAPI_MODULE_DESC;
        (*desc_ptr).nm_modname = MODNAME.as_ptr() as *const c_char;
        napi_module_register(desc_ptr);
    }
}

// The ELF equivalent of `__attribute__((constructor))`. The linker walks
// `.init_array` on `dlopen` and invokes every function pointer.
#[used]
#[link_section = ".init_array"]
static INIT_ARRAY_ENTRY: extern "C" fn() = register_module;
