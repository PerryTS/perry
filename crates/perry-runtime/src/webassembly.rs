//! WebAssembly host shims — bridge between the JS-facing FFI surface and
//! `perry-wasm-host`'s C ABI. Issue: <https://github.com/PerryTS/perry/issues/76>.
//!
//! ## Design
//!
//! `perry-runtime` always declares the `js_webassembly_*` FFIs and forward-
//! declares the `perry_wasm_host_*` symbols they call into. The
//! `perry-wasm-host` archive (wasmi-backed) is linked **only** when the
//! user passed `--enable-wasm-runtime`. Programs that never reference
//! `WebAssembly.*` never trigger an undefined-symbol error because the
//! linker dead-strips the unreferenced `js_webassembly_*` functions.
//!
//! ## MVP API (Perry-specific, not standard)
//!
//! The standard `WebAssembly.instantiate(bytes).then(({instance}) =>
//! instance.exports.add(2, 3))` shape needs (a) Promise wrapping and
//! (b) dynamic property access proxying — both substantial codegen work.
//! For the PoC we expose three top-level builtins on the `WebAssembly`
//! namespace instead, all synchronous:
//!
//! ```ts
//! WebAssembly.validate(bytes: Uint8Array): boolean;
//! WebAssembly.instantiate(bytes: Uint8Array): number; // opaque handle
//! WebAssembly.callExport(handle: number, name: string, ...args: number[]): number;
//! ```
//!
//! Numeric args only (i32/i64/f32/f64). Standard surface tracked as
//! follow-up work in the issue thread.

use std::ffi::{c_char, c_void};

use crate::value::TAG_UNDEFINED;

const TAG_FALSE: u64 = 0x7FFC_0000_0000_0003;
const TAG_TRUE: u64 = 0x7FFC_0000_0000_0004;
const POINTER_TAG: u64 = 0x7FFD_0000_0000_0000;
const POINTER_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

#[inline]
fn nanbox_bool(b: bool) -> f64 {
    f64::from_bits(if b { TAG_TRUE } else { TAG_FALSE })
}

#[inline]
fn nanbox_undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

#[inline]
fn nanbox_pointer_raw(ptr: *const c_void) -> f64 {
    if ptr.is_null() {
        return nanbox_undefined();
    }
    f64::from_bits(POINTER_TAG | ((ptr as u64) & POINTER_MASK))
}

#[inline]
fn unbox_pointer(v: f64) -> *mut c_void {
    let bits = v.to_bits();
    let upper = bits >> 48;
    let raw = if upper >= 0x7FF8 {
        bits & POINTER_MASK
    } else {
        bits
    };
    raw as *mut c_void
}

/// Extract `(ptr, len)` for a JSValue that the user passed as the wasm bytes
/// source. Accepts both `Uint8Array` (TypedArrayHeader, kind=KIND_UINT8) and
/// raw ArrayBuffer-style `BufferHeader`. Returns `None` if the JSValue isn't
/// a recognised byte buffer.
fn extract_bytes(jsval: f64) -> Option<(*const u8, usize)> {
    let ptr = unbox_pointer(jsval);
    if ptr.is_null() {
        return None;
    }
    let addr = ptr as usize;

    if let Some(kind) = crate::typedarray::lookup_typed_array_kind(addr) {
        // KIND_UINT8 = 0 per typedarray.rs (Int8=0,Uint8=1 — verify via
        // elem_size_for_kind which returns 1 for both byte kinds anyway).
        // We accept any single-byte kind for bytes input — wasmi treats it
        // as raw u8.
        if crate::typedarray::elem_size_for_kind(kind) == 1 {
            let header = addr as *const crate::typedarray::TypedArrayHeader;
            let len = unsafe { (*header).length as usize };
            let data = unsafe {
                (header as *const u8)
                    .add(std::mem::size_of::<crate::typedarray::TypedArrayHeader>())
            };
            return Some((data, len));
        }
    }

    if crate::buffer::is_registered_buffer(addr)
        || crate::buffer::is_array_buffer(addr)
        || crate::buffer::is_uint8array_buffer(addr)
    {
        let header = addr as *const crate::buffer::BufferHeader;
        let len = unsafe { (*header).length as usize };
        let data = unsafe {
            (header as *const u8).add(std::mem::size_of::<crate::buffer::BufferHeader>())
        };
        return Some((data, len));
    }

    None
}

/// Extract a UTF-8 byte view of a JS string. Accepts StringHeader-backed
/// heap strings only (the short-string SSO path is unlikely to carry an
/// export name longer than 5 chars, so SSO support can come later).
fn extract_string_bytes(jsval: f64) -> Option<(*const u8, usize)> {
    let ptr =
        crate::value::js_get_string_pointer_unified(jsval) as *const crate::string::StringHeader;
    if ptr.is_null() {
        return None;
    }
    let byte_len = unsafe { (*ptr).byte_len } as usize;
    let data =
        unsafe { (ptr as *const u8).add(std::mem::size_of::<crate::string::StringHeader>()) };
    Some((data, byte_len))
}

// ────────────────────────────────────────────────────────────────────────
// Forward declarations of the C ABI from perry-wasm-host. These symbols
// only need to resolve at link time when the user's program actually calls
// a `js_webassembly_*` function — otherwise the linker strips this whole
// translation unit.
// ────────────────────────────────────────────────────────────────────────

const WASM_VAL_KIND_I32: u8 = 0;
const WASM_VAL_KIND_I64: u8 = 1;
const WASM_VAL_KIND_F32: u8 = 2;
const WASM_VAL_KIND_F64: u8 = 3;
const WASM_VAL_KIND_NONE: u8 = 0xFF;

extern "C" {
    fn perry_wasm_host_string_free(s: *mut c_char);
    fn perry_wasm_host_validate(bytes: *const u8, len: usize) -> i32;
    fn perry_wasm_host_module_new(
        bytes: *const u8,
        len: usize,
        out_err: *mut *mut c_char,
    ) -> *mut c_void;
    fn perry_wasm_host_module_drop(module: *mut c_void);
    fn perry_wasm_host_instance_new(module: *mut c_void, out_err: *mut *mut c_char) -> *mut c_void;
    #[allow(dead_code)]
    fn perry_wasm_host_instance_drop(inst: *mut c_void);
    fn perry_wasm_host_call_export(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        arg_kinds: *const u8,
        arg_bits: *const u64,
        arg_count: usize,
        out_kind: *mut u8,
        out_bits: *mut u64,
        out_err: *mut *mut c_char,
    ) -> i32;
}

fn emit_error_to_stderr(prefix: &str, err: *mut c_char) {
    if !err.is_null() {
        let cs = unsafe { std::ffi::CStr::from_ptr(err) };
        eprintln!("{prefix}: {}", cs.to_string_lossy());
        unsafe { perry_wasm_host_string_free(err) };
    } else {
        eprintln!("{prefix}: <unknown>");
    }
}

// ────────────────────────────────────────────────────────────────────────
// FFI surface called from codegen.
// ────────────────────────────────────────────────────────────────────────

/// `WebAssembly.validate(bytes)` — returns boolean.
#[no_mangle]
pub extern "C" fn js_webassembly_validate(bytes_jsval: f64) -> f64 {
    let Some((ptr, len)) = extract_bytes(bytes_jsval) else {
        return nanbox_bool(false);
    };
    let ok = unsafe { perry_wasm_host_validate(ptr, len) } != 0;
    nanbox_bool(ok)
}

/// `WebAssembly.instantiate(bytes)` — synchronous, returns an opaque handle
/// (NaN-boxed pointer) suitable for `callExport`. On error logs to stderr
/// and returns `undefined`.
///
/// Note: this is the Perry MVP shape, **not** the standard
/// `Promise<{module,instance}>`. The standard async surface is tracked as
/// follow-up work (see issue #76).
#[no_mangle]
pub extern "C" fn js_webassembly_instantiate(bytes_jsval: f64) -> f64 {
    let Some((ptr, len)) = extract_bytes(bytes_jsval) else {
        eprintln!("WebAssembly.instantiate: argument is not a Uint8Array or ArrayBuffer");
        return nanbox_undefined();
    };
    let mut err: *mut c_char = std::ptr::null_mut();
    let module = unsafe { perry_wasm_host_module_new(ptr, len, &mut err) };
    if module.is_null() {
        emit_error_to_stderr("WebAssembly.CompileError", err);
        return nanbox_undefined();
    }
    let mut err2: *mut c_char = std::ptr::null_mut();
    let inst = unsafe { perry_wasm_host_instance_new(module, &mut err2) };
    // Drop the module: the instance holds its own reference internally via
    // wasmi's Arc. Leaks the wrapper but not the wasm module data.
    unsafe { perry_wasm_host_module_drop(module) };
    if inst.is_null() {
        emit_error_to_stderr("WebAssembly.LinkError", err2);
        return nanbox_undefined();
    }
    nanbox_pointer_raw(inst as *const c_void)
}

/// `WebAssembly.callExport(handle, name, ...args)` — invoke an exported
/// function by name with numeric arguments. Currently supports up to 4
/// numeric args, mirroring the closure-call ABI in `closure.rs`. All
/// arguments and the return value are passed as f64; the runtime infers
/// the wasm signature from the export type and widens/narrows as needed.
///
/// Args > 4 are silently truncated in this MVP — the codegen-side wiring
/// only routes 0-4 args anyway.
#[no_mangle]
pub extern "C" fn js_webassembly_call_export_0(inst_jsval: f64, name_jsval: f64) -> f64 {
    call_export_n(inst_jsval, name_jsval, &[])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_1(inst_jsval: f64, name_jsval: f64, a: f64) -> f64 {
    call_export_n(inst_jsval, name_jsval, &[a])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_2(
    inst_jsval: f64,
    name_jsval: f64,
    a: f64,
    b: f64,
) -> f64 {
    call_export_n(inst_jsval, name_jsval, &[a, b])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_3(
    inst_jsval: f64,
    name_jsval: f64,
    a: f64,
    b: f64,
    c: f64,
) -> f64 {
    call_export_n(inst_jsval, name_jsval, &[a, b, c])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_4(
    inst_jsval: f64,
    name_jsval: f64,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
) -> f64 {
    call_export_n(inst_jsval, name_jsval, &[a, b, c, d])
}

fn call_export_n(inst_jsval: f64, name_jsval: f64, args: &[f64]) -> f64 {
    let inst = unbox_pointer(inst_jsval);
    if inst.is_null() {
        eprintln!("WebAssembly.callExport: instance handle is null/undefined");
        return nanbox_undefined();
    }
    let Some((name_ptr, name_len)) = extract_string_bytes(name_jsval) else {
        eprintln!("WebAssembly.callExport: export name must be a string");
        return nanbox_undefined();
    };

    // MVP: every input arg is treated as f64. wasmi's `call` will
    // coerce/typecheck against the actual signature on the wasm side —
    // we re-marshal to the right kind here based on the export type.
    // For simplicity we send everything as F64 and let the host translate.
    // (Pragmatic for the PoC: most numeric wasm exports are i32/f64; an
    // f64-encoded i32 round-trips losslessly.)
    let mut kinds: Vec<u8> = Vec::with_capacity(args.len());
    let mut bits: Vec<u64> = Vec::with_capacity(args.len());
    for v in args {
        // Encode as i32 if the f64 round-trips through i32 exactly, else
        // as f64. Covers `add(2,3)` (i32 add) without forcing the user to
        // think about wasm signatures, while still passing real f64s
        // through faithfully.
        let as_i32 = *v as i32;
        if (as_i32 as f64) == *v && v.is_finite() {
            kinds.push(WASM_VAL_KIND_I32);
            bits.push(as_i32 as u32 as u64);
        } else {
            kinds.push(WASM_VAL_KIND_F64);
            bits.push(v.to_bits());
        }
    }

    let mut out_kind: u8 = WASM_VAL_KIND_NONE;
    let mut out_bits: u64 = 0;
    let mut err: *mut c_char = std::ptr::null_mut();
    let ok = unsafe {
        perry_wasm_host_call_export(
            inst,
            name_ptr as *const c_char,
            name_len,
            kinds.as_ptr(),
            bits.as_ptr(),
            kinds.len(),
            &mut out_kind,
            &mut out_bits,
            &mut err,
        )
    };
    if ok == 0 {
        emit_error_to_stderr("WebAssembly.RuntimeError", err);
        return nanbox_undefined();
    }
    let result = match out_kind {
        WASM_VAL_KIND_I32 => (out_bits as u32 as i32) as f64,
        WASM_VAL_KIND_I64 => (out_bits as i64) as f64,
        WASM_VAL_KIND_F32 => f32::from_bits(out_bits as u32) as f64,
        WASM_VAL_KIND_F64 => f64::from_bits(out_bits),
        WASM_VAL_KIND_NONE => nanbox_undefined(),
        _ => nanbox_undefined(),
    };
    // Avoid leaking the unused err buffer on success.
    if !err.is_null() {
        unsafe { perry_wasm_host_string_free(err) };
    }
    result
}
