//! `bun:ffi` — C-ABI foreign-function interface, stage 1 (#6562).
//!
//! Implements the Bun FFI API shape for perry-compiled programs:
//!
//! - `dlopen(path, symbolTable)` → `{ symbols, close() }` with typed call
//!   stubs generated per symbol signature.
//! - `FFIType` — the runtime enum object (numeric values + string aliases
//!   mirror Bun's `src/js/bun/ffi.ts` object literal exactly).
//! - `ptr(view[, byteOffset])` — raw native address of a Buffer /
//!   TypedArray / ArrayBuffer / DataView's bytes.
//! - `CString(ptr[, byteOffset[, byteLength]])` — read a NUL-terminated
//!   (or length-bounded) UTF-8 string from a native pointer.
//! - `suffix` — platform dylib suffix ("dylib" / "so" / "dll").
//!
//! Stage-1 scope: `toArrayBuffer` (external backing stores), `JSCallback` /
//! `FFIType.function` (native→JS trampolines), `linkSymbols`, `CFunction`,
//! `viewSource` and `read` are declared but throw a clear "not yet
//! supported" error. The dispatch/type plumbing here is shaped so those can
//! be added without reworking stage 1 (see `types::` for the reserved
//! numeric slots and `dlopen::` for the per-symbol signature records).
//!
//! ## Pointer lifetime / pinning contract (the part that must not be wrong)
//!
//! perry's GC relocates nursery objects, but **Buffer / TypedArray /
//! ArrayBuffer / DataView byte storage never moves**: every such object is
//! allocated directly in the non-moving old arena, born `TENURED`, with
//! `movable: false` in `GC_TYPE_INFO_BY_ID` and its bytes stored inline
//! after the header (`buffer/header.rs:468-494`, `typedarray/mod.rs:700-724`
//! — the 2026-07-09 audit made this unconditional precisely because raw
//! data pointers are handed to FFI/tokio). There is also no in-place growth
//! path for buffers (unlike arrays, which reallocate through forwarding
//! stubs — #6228): every buffer-producing operation allocates a fresh
//! header. Consequently:
//!
//! 1. The address returned by `ptr(view)` is stable for the **lifetime of
//!    the JS object**. It is invalidated by (a) the object becoming
//!    unreachable and being swept (old-arena blocks are recycled — the
//!    #6080 ABA class), or (b) `ArrayBuffer.prototype.transfer` /
//!    structured-clone detach. The caller must keep a live reference to
//!    the buffer for as long as native code holds the pointer — the same
//!    contract Bun documents ("keep a reference to the TypedArray while
//!    native code uses it"). `ptr()` itself does NOT root the buffer.
//! 2. For **views** (`buffer.subarray`, `new Uint8Array(ab, off, len)`),
//!    perry keeps a local byte copy plus a view registry whose backing is
//!    the source of truth (#1205/#6515). `ptr()` resolves through
//!    `buffer::view::resolve_data_ptr`, so native code always sees the
//!    true backing bytes — but a native **write** through such a pointer
//!    is not propagated into the view's local copy, so subsequent JS reads
//!    through the view's codegen fast path can be stale. Pass base
//!    (non-view) Buffers/TypedArrays to native code that writes — which is
//!    what real `bun:ffi` consumers (bun-pty, opentui) do.
//! 3. During a synchronous FFI call no GC can run on the calling thread
//!    (stage 1 has no native→JS callbacks), and argument marshalling that
//!    allocates (temporary NUL-terminated copies of string args) cannot
//!    invalidate buffer arguments because buffers are non-moving and are
//!    kept alive by the caller's frame.
//!
//! ## Call-stub mechanism
//!
//! Hand-generated register-image thunks rather than libffi (no new native
//! deps, no linker-driver changes): all FFI types are scalars, so on the
//! two supported ABIs (SysV x86-64, AAPCS64 incl. Apple arm64) integer-class
//! args fill the integer register file in order and float-class args fill
//! the vector register file in order, independently. Calling through a
//! 16-slot `extern "C"` signature with the marshalled values packed in
//! class order therefore produces exactly the register (and, on x86-64,
//! stack) image the callee's real prototype expects. See `call.rs` for the
//! per-ABI limits (≤ 8 integer-class + ≤ 8 float-class args) and the f32
//! bit-image trick. Signatures beyond those limits, and non-unix or
//! non-{x86_64, aarch64} targets, throw a descriptive error at `dlopen`
//! time rather than corrupting registers at call time.

pub mod call;
pub mod dlopen;
pub mod types;

use crate::value::JSValue;

pub(crate) fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

pub(crate) fn null() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

pub(crate) fn string_value(s: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

pub(crate) fn number_value(n: f64) -> f64 {
    f64::from_bits(JSValue::number(n).bits())
}

/// Platform shared-library suffix, matching Bun's `suffix` export
/// (WITHOUT the leading dot, e.g. `"dylib"`).
pub(crate) fn suffix_str() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "dll"
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "so"
    }
}

/// GC root scanner for the module's cached JS objects (the `FFIType`
/// enum object). Registered from `gc_init` alongside the other runtime
/// side-table scanners.
pub fn scan_bun_ffi_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    types::scan_ffi_type_cache_mut(visitor);
}

/// Stage-1 boundary: named exports that exist in `bun:ffi` but are not yet
/// implemented in perry. Kept callable so real-world feature probes fail
/// with an actionable message instead of `undefined is not a function`.
fn throw_stage1_unsupported(what: &str) -> ! {
    crate::fs::validate::throw_error_with_code(
        &format!(
            "bun:ffi: {what} is not supported yet in perry (stage 1, #6562). \
             Available: dlopen, FFIType, ptr, CString, suffix."
        ),
        "ERR_NOT_IMPLEMENTED",
    )
}

/// Method dispatch for the `bun:ffi` namespace — the single entry the
/// `nm_dispatch_bun_ffi` bucket routes through. `args` are NaN-boxed
/// JSValues.
///
/// # Safety
/// `args_ptr` must point at `args_len` valid NaN-boxed f64 slots (or be
/// null when `args_len == 0`), per the NmCtx contract.
pub(crate) unsafe fn dispatch(
    method_name: &str,
    args_ptr: *const f64,
    args_len: usize,
) -> Option<f64> {
    let arg = |n: usize| -> f64 {
        if n < args_len && !args_ptr.is_null() {
            *args_ptr.add(n)
        } else {
            undefined()
        }
    };
    match method_name {
        "dlopen" => Some(dlopen::dlopen_value(arg(0), arg(1))),
        "ptr" => Some(dlopen::ptr_value(arg(0), arg(1))),
        "CString" => Some(dlopen::cstring_value(arg(0), arg(1), arg(2))),
        // Constants are normally served by `get_native_module_constant`,
        // but destructured/dynamic reads can land here too.
        "FFIType" => Some(types::ffi_type_object_value()),
        "suffix" => Some(string_value(suffix_str())),
        "toArrayBuffer" => throw_stage1_unsupported("toArrayBuffer (external backing stores)"),
        "JSCallback" => throw_stage1_unsupported("JSCallback (native-to-JS callbacks)"),
        "CFunction" => throw_stage1_unsupported("CFunction"),
        "linkSymbols" => throw_stage1_unsupported("linkSymbols"),
        "viewSource" => throw_stage1_unsupported("viewSource"),
        "read" => throw_stage1_unsupported("the read namespace"),
        "toBuffer" => throw_stage1_unsupported("toBuffer"),
        _ => None,
    }
}
