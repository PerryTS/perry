//! Stable FFI surface for native bindings packages.
//!
//! # Why this crate exists
//!
//! Wrappers for Rust crates (mysql2, ioredis, dotenv, sharp, …) need
//! to allocate JS strings, read string contents back, hand objects
//! and arrays in and out, and call closures. Today they reach into
//! `perry-runtime` directly — `StringHeader` field offsets,
//! `js_string_from_bytes` argument types, NaN-boxing tag values.
//! That makes every `perry-runtime` refactor a breaking change for
//! every wrapper, including the third-party ones (Bloom Engine ships
//! ~230 FFI functions externally already).
//!
//! `perry-ffi` is the **API-stable** surface those wrappers are
//! supposed to depend on. It does not isolate the runtime — it's the
//! same process, same arena, same GC — but it pins a semver-versioned
//! API so refactors below the line don't ripple out.
//!
//! # Versioning
//!
//! - This crate ships its own semver, currently tracking Perry's minor
//!   (`0.5.x`). Wrappers depend on `perry-ffi = "0.5"`.
//! - A wrapper's `package.json` declares
//!   `perry.nativeLibrary.abiVersion: "0.5"`. The compiler refuses to
//!   load a wrapper whose declared `abiVersion` doesn't satisfy the
//!   bundled `perry-ffi`'s semver range (#466 Phase 2 — not yet
//!   enforced as of this crate's introduction).
//! - Any backwards-incompatible change to a function in this module
//!   bumps the perry-ffi major version, regardless of what
//!   `perry-runtime` does internally.
//!
//! # Today's surface (v0.5.x)
//!
//! Just enough to port the smallest stdlib wrappers (`dotenv`,
//! `nanoid`, `uuid`, `slugify`) — read a string, allocate a string.
//! The minimal set is intentional: every helper added is a forever
//! commitment, and we'd rather grow it as real wrappers demand than
//! over-design up front.
//!
//! Followups will add: array read/alloc, object read/alloc, closure
//! call helpers, NaN-box constants, async-runtime sharing
//! (`spawn_async` / `block_on`). Tracked in #466 Phase 1's "Open
//! questions" section.

#![deny(missing_docs)]

mod async_runtime;
pub use async_runtime::{
    nanbox_string_bits, run_pending, spawn_async, spawn_blocking, spawn_blocking_with_reactor,
    JsNativeAsyncCompletion, JsPromise, PERRY_NATIVE_ASYNC_ALREADY_COMPLETED,
    PERRY_NATIVE_ASYNC_CLEANUP_ON_CANCEL, PERRY_NATIVE_ASYNC_CLEANUP_ON_REJECT,
    PERRY_NATIVE_ASYNC_CLEANUP_ON_SUCCESS, PERRY_NATIVE_ASYNC_INVALID, PERRY_NATIVE_ASYNC_OK,
    PERRY_NATIVE_ASYNC_THREAD_MAIN, PERRY_NATIVE_ASYNC_WRONG_THREAD,
};

mod types;
pub use types::{
    ArrayHeader, BigIntHeader, BufferHeader, ClosureHeader, NativeAsyncCompletion, ObjectHeader,
    Promise, StringHeader, BIGINT_LIMBS,
};

mod handle;
#[allow(deprecated)]
pub use handle::gc_register_root_scanner;
pub use handle::{
    drain_quarantined_handles, drop_handle, drop_handle_until, gc_register_mutable_root_scanner,
    gc_register_mutable_root_scanner_named, get_handle, get_handle_mut, handle_exists,
    iter_handle_ids_of, iter_handles_of, iter_handles_of_mut, register_handle, reserve_handle_id,
    take_handle, with_handle, with_handle_mut, GcMutableRootScanner, GcRootVisitor, Handle,
    INVALID_HANDLE,
};

mod jsvalue;
pub use jsvalue::{
    build_object_shape, js_array_alloc, js_array_get, js_array_length, js_array_push, js_array_set,
    js_object_alloc_with_shape, js_object_get_field, js_object_set_field, JsValue,
};

mod closure;
pub use closure::{JsClosure, RawClosureHeader};

mod bigint;
pub use bigint::{alloc_bigint_from_str, read_bigint_limbs};

mod buffer;
pub use buffer::{alloc_buffer, read_buffer_bytes};

mod json;
pub use json::json_stringify;

mod error;
pub use error::{
    error_value_with_code, system_error_value, throw_with_code, value_byte_slice, ErrorKind,
};

mod event_pump;
pub use event_pump::notify_main_thread;

mod raw_net;
pub use raw_net::{raw_net, register_raw_net, RawNetVtable};

// `runtime-link` gates this `extern crate` so external npm-packaged
// wrappers (which lack perry-runtime in their Cargo graph) compile
// without it. In-tree extension crates that need the runtime
// symbols at test time enable the feature in their
// `[dev-dependencies]` (`perry-ffi = { ..., features =
// ["runtime-link"] }`), which pulls perry-runtime in here and makes
// `js_string_from_bytes` etc. resolvable. For non-test builds,
// Perry's compiler driver (`crates/perry/src/commands/compile/
// library_search.rs`) appends `libperry_runtime.a` to the linker
// invocation, so the runtime symbols are also present without the
// Cargo dep edge.
#[cfg(feature = "runtime-link")]
extern crate perry_runtime as _perry_runtime_link;

extern "C" {
    fn js_string_from_bytes(data: *const u8, len: u32) -> *mut StringHeader;
}

/// Opaque handle to a JS string allocated in the Perry arena.
///
/// Internally this is a `*mut StringHeader` — the field layout is an
/// implementation detail that may change between minor versions of
/// `perry-runtime`. Treat it as opaque: pass it around, return it
/// from your FFI function, hand it to other `perry-ffi` helpers,
/// don't peek inside.
///
/// Constructed via [`alloc_string`]; consumed by [`read_string`].
#[repr(transparent)]
#[derive(Copy, Clone)]
pub struct JsString(*mut StringHeader);

impl JsString {
    /// Wrap a raw pointer obtained from `perry-runtime` (e.g., as a
    /// function argument). Use sparingly — prefer
    /// [`read_string`] / [`alloc_string`] when possible.
    ///
    /// # Safety
    ///
    /// `ptr` must either be null or point to a valid `StringHeader`
    /// allocated by Perry's runtime. Borrowing rules: the pointee is
    /// valid for the lifetime of the calling FFI invocation.
    pub unsafe fn from_raw(ptr: *mut StringHeader) -> Self {
        Self(ptr)
    }

    /// Unwrap to the raw pointer, for callers that need to forward
    /// the value back through `perry-runtime`'s public ABI directly.
    /// Most wrappers should not need this.
    pub fn as_raw(self) -> *mut StringHeader {
        self.0
    }

    /// True if the handle is null. Null is what the runtime returns
    /// in error paths (allocation failure, invalid input). FFI
    /// callers usually want to check this and return undefined or
    /// propagate.
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }
}

/// Allocate a new JS string in Perry's arena from a Rust `&str`.
///
/// The returned [`JsString`] is owned by the runtime — Perry's GC
/// will reclaim it when no live references remain. Wrappers
/// typically return this directly to TypeScript callers via their
/// `extern "C"` boundary.
///
/// ```ignore
/// // Inside an FFI function:
/// #[no_mangle]
/// pub extern "C" fn js_my_module_greet() -> *mut perry_ffi::StringHeader {
///     perry_ffi::alloc_string("hello").as_raw()
/// }
/// ```
pub fn alloc_string(s: &str) -> JsString {
    // SAFETY: `js_string_from_bytes` accepts any `*const u8` + length pair,
    // copies the bytes into a freshly allocated arena slot, and returns the
    // header pointer. The input slice is borrowed only for the duration of
    // the call.
    let ptr = unsafe { js_string_from_bytes(s.as_ptr(), s.len() as u32) };
    JsString(ptr)
}

/// Read a `JsString` as a borrowed `&str`.
///
/// Returns `None` on a null handle or invalid UTF-8. The borrow lives
/// as long as the runtime guarantees the string remains alive — for
/// the simple call-and-copy pattern in most FFI functions, that's the
/// duration of the function call.
///
/// ```ignore
/// #[no_mangle]
/// pub extern "C" fn js_my_module_echo(input: *const perry_ffi::StringHeader)
///     -> *mut perry_ffi::StringHeader {
///     // SAFETY: input is either null or a valid runtime-allocated header.
///     let handle = unsafe { perry_ffi::JsString::from_raw(input as *mut _) };
///     match perry_ffi::read_string(handle) {
///         Some(s) => perry_ffi::alloc_string(&format!("got: {}", s)).as_raw(),
///         None => std::ptr::null_mut(),
///     }
/// }
/// ```
pub fn read_string(handle: JsString) -> Option<&'static str> {
    let bytes = read_bytes(handle)?;
    std::str::from_utf8(bytes).ok()
}

/// Read a `JsString` as a borrowed `&[u8]` — does not require
/// valid UTF-8. Used by binary-data wrappers (zlib, crypto buffer
/// ops, …) that store arbitrary bytes inside a `StringHeader` but
/// can't go through [`read_string`]'s UTF-8 validation.
///
/// Returns `None` on a null handle. The borrow lives as long as
/// the runtime guarantees the string remains alive — same lifetime
/// rules as [`read_string`].
pub fn read_bytes(handle: JsString) -> Option<&'static [u8]> {
    if handle.is_null() {
        return None;
    }
    // SAFETY: `from_raw`'s contract requires `handle.0` to point
    // to a valid `StringHeader`. We bound the slice length by the
    // stored `byte_len` so we never read past the allocation.
    unsafe {
        let header = &*handle.0;
        let len = header.byte_len as usize;
        let data_ptr = (handle.0 as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(std::slice::from_raw_parts(data_ptr, len))
    }
}

/// Allocate a runtime string from raw bytes — bypasses the UTF-8
/// validation [`alloc_string`] does implicitly. Use for compressed
/// payloads, crypto digests, and other binary-as-string outputs
/// that perry-stdlib's existing wrappers carry as `*mut StringHeader`.
pub fn alloc_bytes(bytes: &[u8]) -> JsString {
    let ptr = unsafe { js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32) };
    JsString(ptr)
}

// `StringHeader` is re-exported at the top of this module. External
// wrappers declare their FFI return types as `*mut StringHeader`
// from this crate, not from `perry-runtime` directly — that way a
// layout change in the runtime doesn't immediately become a breaking
// change for wrapper authors. The type itself is the same; this is
// just a stable-named import path.

#[cfg(all(test, feature = "runtime-link"))]
mod tests {
    use super::*;

    #[test]
    fn round_trip_string() {
        let handle = alloc_string("hello, perry-ffi");
        assert!(!handle.is_null());
        let read = read_string(handle).expect("readable");
        assert_eq!(read, "hello, perry-ffi");
    }

    #[test]
    fn empty_string_round_trips() {
        let handle = alloc_string("");
        assert!(!handle.is_null());
        let read = read_string(handle).expect("empty is still readable");
        assert_eq!(read, "");
    }

    #[test]
    fn null_handle_reads_none() {
        // SAFETY: explicitly constructing a null handle is the
        // documented escape; we only read it to verify the None path.
        let null_handle = unsafe { JsString::from_raw(std::ptr::null_mut()) };
        assert!(null_handle.is_null());
        assert_eq!(read_string(null_handle), None);
    }
}
