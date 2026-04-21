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

// OHOS's NativeModuleManager resolves `import X from 'libfoo.so'` by
// stripping `lib`/`.so` from the filename and looking up a module whose
// `nm_modname` equals the result. If they don't match, the import silently
// no-ops and the ArkTS side crashes on first method access with a confusing
// "cannot read property of undefined."
//
// Rather than hardcode a name (which locks users into a specific `-o` flag),
// we derive the modname at load time via `dladdr` on the register function:
// walk back from our own constructor address to the `.so` path, extract the
// filename, strip `lib`/`.so`, copy into a static buffer. Works regardless
// of what the user named their output.

#[repr(C)]
struct DlInfo {
    dli_fname: *const c_char,
    dli_fbase: *mut c_void,
    dli_sname: *const c_char,
    dli_saddr: *mut c_void,
}

extern "C" {
    fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
    fn strlen(s: *const c_char) -> usize;
}

// 256 bytes is enough for any realistic `.so` filename. Static mut because
// we only write once during .init_array (single-threaded), and it must
// outlive napi_module_register's read of the pointer.
const MODNAME_CAP: usize = 256;
static mut MODNAME_BUF: [u8; MODNAME_CAP] = [0; MODNAME_CAP];

/// Derive modname from the `.so` path reported by dladdr. Strips the
/// leading `lib` and trailing `.so` if present; otherwise uses the
/// filename as-is. Copies into the static buffer and returns a pointer
/// suitable for `nm_modname`. Falls back to "entry" if dladdr fails.
unsafe fn derive_modname() -> *const c_char {
    // Fallback — also what DevEco's hvigor-generated template uses.
    let fallback = b"entry\0";

    let mut info: DlInfo = DlInfo {
        dli_fname: ptr::null(),
        dli_fbase: ptr::null_mut(),
        dli_sname: ptr::null(),
        dli_saddr: ptr::null_mut(),
    };
    let ok = dladdr(derive_modname as *const c_void, &mut info as *mut DlInfo);
    let buf_ptr = &raw mut MODNAME_BUF as *mut u8;
    if ok == 0 || info.dli_fname.is_null() {
        std::ptr::copy_nonoverlapping(fallback.as_ptr(), buf_ptr, fallback.len());
        return buf_ptr as *const c_char;
    }

    // Extract basename: the substring after the last '/'.
    let fname_len = strlen(info.dli_fname);
    let mut base = info.dli_fname;
    let mut probe = info.dli_fname;
    for _ in 0..fname_len {
        if *probe == b'/' as c_char {
            base = probe.add(1);
        }
        probe = probe.add(1);
    }

    // base now points at "libfoo.so" (or whatever). Strip "lib" prefix and
    // ".so" suffix if present.
    let base_len = strlen(base);
    let mut start = base;
    let mut len = base_len;
    if len >= 3 {
        let b0 = *start as u8;
        let b1 = *start.add(1) as u8;
        let b2 = *start.add(2) as u8;
        if b0 == b'l' && b1 == b'i' && b2 == b'b' {
            start = start.add(3);
            len -= 3;
        }
    }
    if len >= 3 {
        let tail = start.add(len - 3);
        let t0 = *tail as u8;
        let t1 = *tail.add(1) as u8;
        let t2 = *tail.add(2) as u8;
        if t0 == b'.' && t1 == b's' && t2 == b'o' {
            len -= 3;
        }
    }

    // Clamp to buffer capacity leaving room for null terminator.
    if len >= MODNAME_CAP {
        len = MODNAME_CAP - 1;
    }

    // Zero the buffer (already zeroed at static init, but reassigning in
    // case of repeated constructor runs — unlikely, but cheap).
    std::ptr::write_bytes(buf_ptr, 0, MODNAME_CAP);
    std::ptr::copy_nonoverlapping(start as *const u8, buf_ptr, len);
    // Null terminator is implicit — buffer is zeroed.

    buf_ptr as *const c_char
}

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
        (*desc_ptr).nm_modname = derive_modname();
        napi_module_register(desc_ptr);
    }
}

// The ELF equivalent of `__attribute__((constructor))`. The linker walks
// `.init_array` on `dlopen` and invokes every function pointer.
#[used]
#[link_section = ".init_array"]
static INIT_ARRAY_ENTRY: extern "C" fn() = register_module;
