//! `dlopen` / symbol call stubs / `ptr` / `CString`.
//!
//! Library handles and prepared symbols live in process-wide registries
//! (`LIBS` / `SYMS`); a symbol's JS call stub is a runtime closure whose
//! single capture is its `SYMS` index, so the shared per-arity thunks
//! (`sym_thunk_0..=16`) stay signature-compatible with the closure call
//! ABI (`extern "C" fn(*const ClosureHeader, f64 × arity) -> f64`,
//! arity-padded by `closure/dispatch` via `js_register_closure_arity`).
//!
//! `close()` calls `dlclose` and poisons the library's symbols: later
//! calls throw instead of jumping through a dangling handle. This is
//! deliberately stricter than Bun (which leaves use-after-close as UB).

use super::call::{self, MAX_ARGS, MAX_FLOAT_ARGS, MAX_INT_ARGS};
use super::types::{self, T_BUFFER, T_FUNCTION, T_NAPI_ENV, T_NAPI_VALUE, T_VOID};
use crate::closure::ClosureHeader;
use crate::value::JSValue;
use std::sync::Mutex;

// ── platform dlopen/dlsym/dlclose ───────────────────────────────────────────
// Mirrors `plugin.rs` (which keeps its helpers private). Windows is absent on
// purpose: `call::platform_supported()` gates every entry point, and stage 1
// supports unix x86_64/aarch64 only.

#[cfg(unix)]
unsafe fn open_library(path: &str) -> Result<usize, String> {
    let c_path = match std::ffi::CString::new(path) {
        Ok(p) => p,
        Err(_) => return Err("path contains a NUL byte".to_string()),
    };
    // Clear any stale error state, then capture dlerror on failure.
    libc::dlerror();
    let h = libc::dlopen(c_path.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL);
    if h.is_null() {
        let err = libc::dlerror();
        let msg = if err.is_null() {
            "unknown dlopen error".to_string()
        } else {
            std::ffi::CStr::from_ptr(err).to_string_lossy().into_owned()
        };
        Err(msg)
    } else {
        Ok(h as usize)
    }
}

#[cfg(unix)]
unsafe fn find_symbol(handle: usize, name: &str) -> Option<usize> {
    let c_name = std::ffi::CString::new(name).ok()?;
    let sym = libc::dlsym(handle as *mut libc::c_void, c_name.as_ptr());
    if sym.is_null() {
        None
    } else {
        Some(sym as usize)
    }
}

#[cfg(unix)]
unsafe fn close_library(handle: usize) {
    libc::dlclose(handle as *mut libc::c_void);
}

#[cfg(not(unix))]
unsafe fn open_library(_path: &str) -> Result<usize, String> {
    Err("bun:ffi is not supported on this platform".to_string())
}
#[cfg(not(unix))]
unsafe fn find_symbol(_handle: usize, _name: &str) -> Option<usize> {
    None
}
#[cfg(not(unix))]
unsafe fn close_library(_handle: usize) {}

// ── registries ──────────────────────────────────────────────────────────────

struct LibRecord {
    /// Raw dlopen handle. Only dereferenced by dlsym/dlclose on the JS
    /// thread; stored as usize so the registry is Send.
    handle: usize,
    path: String,
    closed: bool,
}

/// One prepared symbol: everything a call stub needs, `Copy`-cheap.
#[derive(Clone, Copy)]
pub(crate) struct SymRecord {
    fn_ptr: usize,
    lib: usize,
    ret: u8,
    argc: u8,
    args: [u8; MAX_ARGS],
    /// Leaked once per dlopen'd symbol — used in error messages and as the
    /// stable closure display name.
    name: &'static str,
}

static LIBS: Mutex<Vec<LibRecord>> = Mutex::new(Vec::new());
static SYMS: Mutex<Vec<SymRecord>> = Mutex::new(Vec::new());

fn lib_is_closed(lib: usize) -> Option<String> {
    let libs = LIBS.lock().unwrap();
    let rec = libs.get(lib)?;
    if rec.closed {
        Some(rec.path.clone())
    } else {
        None
    }
}

// ── small JS-value helpers ──────────────────────────────────────────────────

unsafe fn value_to_owned_string(v: f64) -> Option<String> {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = crate::string::js_string_key_bytes(jv, &mut sso)?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

unsafe fn object_ptr_of(v: f64) -> Option<*mut crate::object::ObjectHeader> {
    let jv = JSValue::from_bits(v.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let addr = crate::value::js_nanbox_get_pointer(f64::from_bits(jv.bits()));
    if addr == 0 {
        return None;
    }
    Some(addr as usize as *mut crate::object::ObjectHeader)
}

unsafe fn get_field(obj: *mut crate::object::ObjectHeader, name: &str) -> f64 {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    f64::from_bits(crate::object::js_object_get_field_by_name(obj, key).bits())
}

fn set_field(obj: *mut crate::object::ObjectHeader, name: &str, value: f64) {
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::object::js_object_set_field_by_name(obj, key, value);
}

// ── the per-arity call-stub thunks ──────────────────────────────────────────

/// Shared body: resolve the closure's captured `SYMS` index, marshal, call.
unsafe fn invoke_from_closure(closure: *const ClosureHeader, js_args: &[f64]) -> f64 {
    let sym_index = crate::closure::js_closure_get_capture_bits(closure, 0) as usize;
    let sym = {
        let syms = SYMS.lock().unwrap();
        match syms.get(sym_index) {
            Some(&s) => s,
            None => {
                crate::fs::validate::throw_error_with_code(
                    "bun:ffi: internal error: unknown symbol stub",
                    "ERR_INVALID_STATE",
                );
            }
        }
    };
    if let Some(path) = lib_is_closed(sym.lib) {
        crate::fs::validate::throw_error_with_code(
            &format!(
                "bun:ffi: symbol \"{}\" was called after close() on \"{path}\"",
                sym.name
            ),
            "ERR_INVALID_STATE",
        );
    }
    let image = call::marshal_args(&sym.args[..sym.argc as usize], js_args);
    call::call_and_convert(sym.fn_ptr, sym.ret, &image)
}

macro_rules! sym_thunk {
    ($name:ident $(, $a:ident)*) => {
        extern "C" fn $name(closure: *const ClosureHeader $(, $a: f64)*) -> f64 {
            let args = [$($a),*];
            unsafe { invoke_from_closure(closure, &args) }
        }
    };
}

sym_thunk!(sym_thunk_0);
sym_thunk!(sym_thunk_1, a0);
sym_thunk!(sym_thunk_2, a0, a1);
sym_thunk!(sym_thunk_3, a0, a1, a2);
sym_thunk!(sym_thunk_4, a0, a1, a2, a3);
sym_thunk!(sym_thunk_5, a0, a1, a2, a3, a4);
sym_thunk!(sym_thunk_6, a0, a1, a2, a3, a4, a5);
sym_thunk!(sym_thunk_7, a0, a1, a2, a3, a4, a5, a6);
sym_thunk!(sym_thunk_8, a0, a1, a2, a3, a4, a5, a6, a7);
sym_thunk!(sym_thunk_9, a0, a1, a2, a3, a4, a5, a6, a7, a8);
sym_thunk!(sym_thunk_10, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9);
sym_thunk!(sym_thunk_11, a0, a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
sym_thunk!(
    sym_thunk_12,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11
);
sym_thunk!(
    sym_thunk_13,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12
);
sym_thunk!(
    sym_thunk_14,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13
);
sym_thunk!(
    sym_thunk_15,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13,
    a14
);
sym_thunk!(
    sym_thunk_16,
    a0,
    a1,
    a2,
    a3,
    a4,
    a5,
    a6,
    a7,
    a8,
    a9,
    a10,
    a11,
    a12,
    a13,
    a14,
    a15
);

fn sym_thunk_for(arity: usize) -> *const u8 {
    match arity {
        0 => sym_thunk_0 as *const u8,
        1 => sym_thunk_1 as *const u8,
        2 => sym_thunk_2 as *const u8,
        3 => sym_thunk_3 as *const u8,
        4 => sym_thunk_4 as *const u8,
        5 => sym_thunk_5 as *const u8,
        6 => sym_thunk_6 as *const u8,
        7 => sym_thunk_7 as *const u8,
        8 => sym_thunk_8 as *const u8,
        9 => sym_thunk_9 as *const u8,
        10 => sym_thunk_10 as *const u8,
        11 => sym_thunk_11 as *const u8,
        12 => sym_thunk_12 as *const u8,
        13 => sym_thunk_13 as *const u8,
        14 => sym_thunk_14 as *const u8,
        15 => sym_thunk_15 as *const u8,
        _ => sym_thunk_16 as *const u8,
    }
}

extern "C" fn close_thunk(closure: *const ClosureHeader) -> f64 {
    let lib_index = crate::closure::js_closure_get_capture_bits(closure, 0) as usize;
    let mut libs = LIBS.lock().unwrap();
    if let Some(rec) = libs.get_mut(lib_index) {
        if !rec.closed {
            rec.closed = true;
            let handle = rec.handle;
            drop(libs);
            unsafe { close_library(handle) };
        }
    }
    super::undefined()
}

/// Allocate a call-stub closure whose capture 0 is a plain (non-pointer)
/// u64 index. The capture is written as raw bits — a small integer never
/// classifies as pointer-bearing in the GC layout, so the closure stays
/// pointer-free.
fn index_closure(func: *const u8, index: usize, arity: u32, name: &str) -> f64 {
    crate::closure::js_register_closure_arity(func, arity);
    crate::closure::js_register_closure_length(func, arity);
    let closure = crate::closure::js_closure_alloc(func, 1);
    crate::closure::js_closure_set_capture_bits(closure, 0, index as u64);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    crate::value::js_nanbox_pointer(closure as i64)
}

// ── dlopen ──────────────────────────────────────────────────────────────────

fn throw_dlopen_failed(name: &str, detail: &str) -> ! {
    crate::fs::validate::throw_error_with_code(
        &format!("Failed to open library \"{name}\": {detail}"),
        "ERR_DLOPEN_FAILED",
    )
}

/// Validate one symbol signature at dlopen time so a bad signature can
/// never reach the trampoline. Returns (int_class_count, float_class_count).
fn validate_signature(sym: &str, args: &[u8], ret: u8) -> (usize, usize) {
    let reject = |what: &str| -> ! {
        crate::fs::validate::throw_type_error_with_code(
            &format!("bun:ffi: symbol \"{sym}\": {what}"),
            "ERR_INVALID_ARG_TYPE",
        )
    };
    let mut ints = 0usize;
    let mut floats = 0usize;
    for &t in args {
        match t {
            T_FUNCTION => reject(
                "FFIType.function / JSCallback arguments are not yet supported \
                 in perry (bun:ffi stage 1, #6562)",
            ),
            T_NAPI_ENV | T_NAPI_VALUE => reject("napi types are not supported"),
            T_BUFFER => reject("FFIType.buffer is not yet supported (use ptr)"),
            T_VOID => reject("void is not a valid argument type"),
            t if types::is_float_class(t) => floats += 1,
            _ => ints += 1,
        }
    }
    match ret {
        T_FUNCTION => reject(
            "FFIType.function / JSCallback returns are not yet supported \
             in perry (bun:ffi stage 1, #6562)",
        ),
        T_NAPI_ENV | T_NAPI_VALUE => reject("napi types are not supported"),
        T_BUFFER => reject("FFIType.buffer is not yet supported (use ptr / toArrayBuffer)"),
        _ => {}
    }
    if args.len() > MAX_ARGS {
        reject(&format!("more than {MAX_ARGS} arguments are not supported"));
    }
    if ints > MAX_INT_ARGS {
        reject(&format!(
            "more than {MAX_INT_ARGS} integer/pointer arguments are not supported \
             by perry's stage-1 call stubs"
        ));
    }
    if floats > MAX_FLOAT_ARGS {
        reject(&format!(
            "more than {MAX_FLOAT_ARGS} float arguments are not supported \
             by perry's stage-1 call stubs"
        ));
    }
    (ints, floats)
}

/// `dlopen(path, symbolTable)` → `{ symbols: { <name>: fn }, close(): void }`.
pub(crate) unsafe fn dlopen_value(path_arg: f64, table_arg: f64) -> f64 {
    if !call::platform_supported() {
        crate::fs::validate::throw_error_with_code(
            "bun:ffi is not supported on this platform yet (stage 1 targets \
             unix x86_64 / aarch64, #6562)",
            "ERR_NOT_IMPLEMENTED",
        );
    }
    let Some(path) = value_to_owned_string(path_arg) else {
        crate::fs::validate::throw_type_error_with_code(
            "dlopen(path, symbols) expects a string path",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    let Some(table) = object_ptr_of(table_arg) else {
        crate::fs::validate::throw_type_error_with_code(
            "dlopen(path, symbols) expects a symbols object",
            "ERR_INVALID_ARG_TYPE",
        );
    };

    let handle = match open_library(&path) {
        Ok(h) => h,
        Err(msg) => throw_dlopen_failed(&path, &msg),
    };
    let lib_index = {
        let mut libs = LIBS.lock().unwrap();
        libs.push(LibRecord {
            handle,
            path: path.clone(),
            closed: false,
        });
        libs.len() - 1
    };

    // Walk the symbol table BEFORE building any JS result, so an invalid
    // entry throws without leaking half-built objects (the lib handle stays
    // registered; dlopen handles are refcounted by the loader and this
    // mirrors Bun, which also leaves the library mapped on validation
    // throws).
    let keys = crate::object::js_object_keys(table);
    let key_count = crate::array::js_array_length(keys);
    if key_count == 0 {
        throw_dlopen_failed(&path, "Expected at least 1 symbol");
    }

    struct Prepared {
        name: String,
        sym_index: usize,
        argc: u32,
    }
    let mut prepared: Vec<Prepared> = Vec::with_capacity(key_count as usize);

    for i in 0..key_count {
        let key_value = crate::array::js_array_get(keys, i);
        let name = match value_to_owned_string(f64::from_bits(key_value.bits())) {
            Some(n) => n,
            None => continue,
        };
        let Some(entry) = object_ptr_of(get_field(table, &name)) else {
            crate::fs::validate::throw_type_error_with_code(
                &format!("bun:ffi: symbol \"{name}\": expected {{ args, returns }}"),
                "ERR_INVALID_ARG_TYPE",
            );
        };

        // args: optional array of FFIType values; returns: optional FFIType
        // (missing → void), both exactly as Bun accepts them.
        let mut args = [0u8; MAX_ARGS];
        let mut argc = 0usize;
        let args_value = get_field(entry, "args");
        let args_jv = JSValue::from_bits(args_value.to_bits());
        if !args_jv.is_undefined() && !args_jv.is_null() {
            let arr_addr = crate::value::js_nanbox_get_pointer(args_value);
            if arr_addr == 0 {
                crate::fs::validate::throw_type_error_with_code(
                    &format!("bun:ffi: symbol \"{name}\": args must be an array"),
                    "ERR_INVALID_ARG_TYPE",
                );
            }
            let arr = arr_addr as usize as *const crate::array::ArrayHeader;
            let len = crate::array::js_array_length(arr);
            if len as usize > MAX_ARGS {
                crate::fs::validate::throw_type_error_with_code(
                    &format!(
                        "bun:ffi: symbol \"{name}\": more than {MAX_ARGS} arguments \
                         are not supported"
                    ),
                    "ERR_INVALID_ARG_TYPE",
                );
            }
            for j in 0..len {
                let t = crate::array::js_array_get(arr, j);
                args[argc] = types::parse_ffi_type(f64::from_bits(t.bits()));
                argc += 1;
            }
        }
        let returns_value = get_field(entry, "returns");
        let returns_jv = JSValue::from_bits(returns_value.to_bits());
        let ret = if returns_jv.is_undefined() || returns_jv.is_null() {
            T_VOID
        } else {
            types::parse_ffi_type(returns_value)
        };
        validate_signature(&name, &args[..argc], ret);

        let Some(fn_ptr) = find_symbol(handle, &name) else {
            crate::fs::validate::throw_type_error_with_code(
                &format!("Symbol \"{name}\" not found in \"{path}\""),
                "ERR_INVALID_ARG_TYPE",
            );
        };

        let leaked_name: &'static str = name.clone().leak();
        let sym_index = {
            let mut syms = SYMS.lock().unwrap();
            syms.push(SymRecord {
                fn_ptr,
                lib: lib_index,
                ret,
                argc: argc as u8,
                args,
                name: leaked_name,
            });
            syms.len() - 1
        };
        prepared.push(Prepared {
            name,
            sym_index,
            argc: argc as u32,
        });
    }

    // Build `{ symbols, close }` with every intermediate rooted across the
    // remaining allocations.
    let scope = crate::gc::RuntimeHandleScope::new();
    let symbols_obj = crate::object::js_object_alloc(0, prepared.len() as u32);
    let symbols_handle = scope.root_raw_mut_ptr(symbols_obj);
    for p in &prepared {
        let value = index_closure(sym_thunk_for(p.argc as usize), p.sym_index, p.argc, &p.name);
        let value_handle = scope.root_nanbox_f64(value);
        set_field(
            symbols_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
            &p.name,
            value_handle.get_nanbox_f64(),
        );
    }

    let close_value = index_closure(close_thunk as *const u8, lib_index, 0, "close");
    let close_handle = scope.root_nanbox_f64(close_value);

    let result = crate::object::js_object_alloc(0, 2);
    let result_handle = scope.root_raw_mut_ptr(result);
    let symbols_value =
        f64::from_bits(JSValue::object_ptr(symbols_handle.get_raw_mut_ptr::<u8>()).bits());
    set_field(
        result_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        "symbols",
        symbols_value,
    );
    set_field(
        result_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        "close",
        close_handle.get_nanbox_f64(),
    );
    f64::from_bits(JSValue::object_ptr(result_handle.get_raw_mut_ptr::<u8>()).bits())
}

// ── ptr / CString ───────────────────────────────────────────────────────────

/// `ptr(view[, byteOffset])` → number address of the view's bytes.
///
/// Lifetime contract (see the module doc in `mod.rs`): the address is the
/// buffer's non-moving inline storage (resolved through the view registry
/// to the ultimate backing); it stays valid while the JS object is alive
/// and un-detached. `ptr()` does not root the object.
pub(crate) unsafe fn ptr_value(view_arg: f64, offset_arg: f64) -> f64 {
    let Some((data, len)) = call::value_buffer_span(view_arg) else {
        let jv = JSValue::from_bits(view_arg.to_bits());
        if jv.is_any_string() {
            crate::fs::validate::throw_type_error_with_code(
                "To convert a string to a pointer, encode it as a buffer",
                "ERR_INVALID_ARG_TYPE",
            );
        }
        crate::fs::validate::throw_type_error_with_code(
            "ptr(view) expects a TypedArray, Buffer, ArrayBuffer or DataView",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    let offset_jv = JSValue::from_bits(offset_arg.to_bits());
    let offset = if offset_jv.is_int32() {
        offset_jv.as_int32() as i64
    } else if offset_jv.is_number() {
        offset_jv.as_number() as i64
    } else {
        0
    };
    if offset < 0 || offset as usize > len {
        crate::fs::validate::throw_range_error_named(
            &format!("ptr(view, byteOffset): byteOffset {offset} is out of range (0..={len})"),
            "ERR_OUT_OF_RANGE",
        );
    }
    super::number_value(data as usize as f64 + offset as f64)
}

/// `CString(ptr[, byteOffset[, byteLength]])` → JS string.
///
/// Stage-1 divergence from Bun (documented): returns a primitive string
/// rather than a `String` subclass carrying `.ptr` — the decoded text is
/// identical. NULL pointers return `null` like Bun's `cstring` return
/// conversion.
pub(crate) unsafe fn cstring_value(ptr_arg: f64, offset_arg: f64, length_arg: f64) -> f64 {
    let jv = JSValue::from_bits(ptr_arg.to_bits());
    let base = if jv.is_undefined() || jv.is_null() {
        0usize
    } else if jv.is_int32() {
        jv.as_int32() as i64 as usize
    } else if jv.is_number() {
        jv.as_number() as i64 as usize
    } else if jv.is_bigint() {
        let b = crate::value::js_nanbox_get_bigint(ptr_arg);
        if b == 0 {
            0
        } else {
            (*(b as usize as *const crate::bigint::BigIntHeader)).limbs[0] as usize
        }
    } else if let Some((data, _)) = call::value_buffer_span(ptr_arg) {
        data as usize
    } else {
        crate::fs::validate::throw_type_error_with_code(
            "CString(ptr) expects a pointer",
            "ERR_INVALID_ARG_TYPE",
        );
    };
    if base == 0 {
        return super::null();
    }
    let offset_jv = JSValue::from_bits(offset_arg.to_bits());
    let offset = if offset_jv.is_int32() {
        offset_jv.as_int32() as i64
    } else if offset_jv.is_number() {
        offset_jv.as_number() as i64
    } else {
        0
    };
    let start = (base as i64 + offset.max(0)) as usize;
    let length_jv = JSValue::from_bits(length_arg.to_bits());
    let explicit_len = if length_jv.is_int32() {
        Some(length_jv.as_int32() as i64)
    } else if length_jv.is_number() {
        Some(length_jv.as_number() as i64)
    } else {
        None
    };
    match explicit_len {
        Some(n) if n >= 0 => {
            let bytes = std::slice::from_raw_parts(start as *const u8, n as usize);
            match std::str::from_utf8(bytes) {
                Ok(s) => super::string_value(s),
                Err(_) => super::string_value(&String::from_utf8_lossy(bytes)),
            }
        }
        _ => call::read_cstring_value(start),
    }
}
