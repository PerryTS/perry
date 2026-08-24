//! `bun:ffi` — C-ABI foreign-function interface (#6562).
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
//! - `toArrayBuffer` / `toBuffer` — zero-copy JS views over native-owned
//!   memory. The native allocation remains caller-owned.
//! - `JSCallback` / `FFIType.function` — native→JS callbacks, including a
//!   synchronous main-thread handoff for Bun's `threadsafe` callbacks.
//! - `node:ffi` — Node's `{ lib, functions }` adapter, raw-pointer helpers,
//!   and library-owned callback registration used by OpenTUI/Yoga.
//! - `suffix` — platform dylib suffix ("dylib" / "so" / "dll").
//!
//! `read` exposes direct native-endian scalar loads used by struct wrappers.
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
//! 3. A synchronous native call may invoke a same-thread `JSCallback` and
//!    trigger GC. Buffer addresses remain valid because their storage is
//!    non-moving, while the caller's frame keeps buffer arguments alive.
//!    Callback functions are held in a runtime root registry until closed.
//!
//! ## Call-stub mechanism
//!
//! Hand-generated register-image thunks rather than libffi (no new native
//! deps, no linker-driver changes): all FFI types are scalars, so on the
//! two supported ABIs (SysV x86-64, AAPCS64 incl. Apple arm64) integer-class
//! args fill the integer register file in order and float-class args fill
//! the vector register file in order, independently. Calling through a
//! one assembly thunk with the marshalled values packed in class order
//! therefore produces exactly the register and stack image the callee's real
//! prototype expects. See `call.rs` for the 16-scalar public
//! limit and the f32 bit-image trick. Signatures beyond that limit, and non-unix or
//! non-{x86_64, aarch64} targets, throw a descriptive error at `dlopen`
//! time rather than corrupting registers at call time.

pub mod call;
pub mod callback;
pub mod dlopen;
pub mod memory;
pub mod read;
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
    read::scan_read_cache_mut(visitor);
    callback::scan_callback_roots_mut(visitor);
}

/// Method dispatch for the `bun:ffi` / `node:ffi` namespaces — the single entry the
/// `nm_dispatch_bun_ffi` bucket routes through. `args` are NaN-boxed
/// JSValues.
///
/// # Safety
/// `args_ptr` must point at `args_len` valid NaN-boxed f64 slots (or be
/// null when `args_len == 0`), per the NmCtx contract.
pub(crate) unsafe fn dispatch(
    module_name: &str,
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
    if matches!(module_name, "ffi" | "ffi.default") {
        return match method_name {
            "dlopen" => Some(dlopen::node_dlopen_value(arg(0), arg(1))),
            "getRawPointer" => Some(memory::node_get_raw_pointer_value(arg(0))),
            "toArrayBuffer" => Some(memory::node_view_value(arg(0), arg(1), arg(2), true)),
            "toBuffer" => Some(memory::node_view_value(arg(0), arg(1), arg(2), false)),
            "toString" => Some(memory::node_to_string_value(arg(0))),
            "suffix" => Some(string_value(suffix_str())),
            _ => None,
        };
    }
    match method_name {
        "dlopen" => Some(dlopen::dlopen_value(arg(0), arg(1))),
        "ptr" => Some(dlopen::ptr_value(arg(0), arg(1))),
        "CString" => Some(dlopen::cstring_value(arg(0), arg(1), arg(2))),
        // Constants are normally served by `get_native_module_constant`,
        // but destructured/dynamic reads can land here too.
        "FFIType" => Some(types::ffi_type_object_value()),
        "suffix" => Some(string_value(suffix_str())),
        "toArrayBuffer" => Some(memory::view_value(arg(0), arg(1), arg(2), true)),
        "JSCallback" => Some(callback::js_callback_value(arg(0), arg(1))),
        "CFunction" => Some(dlopen::c_function_value(arg(0))),
        "linkSymbols" => Some(dlopen::link_symbols_value(arg(0))),
        "viewSource" => Some(dlopen::view_source_value(arg(0))),
        "read" => Some(read::read_object_value()),
        "toBuffer" => Some(memory::view_value(arg(0), arg(1), arg(2), false)),
        _ => None,
    }
}
