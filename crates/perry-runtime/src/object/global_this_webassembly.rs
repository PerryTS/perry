//! WebAssembly pieces of the `globalThis` namespace installer.
//!
//! ## #6558 baseline: spec-shaped surface, graceful failure
//!
//! Perry ships no WebAssembly engine in the default build. This namespace is
//! nevertheless **spec-shaped**: every standard member exists with the right
//! type, `.name` and `.length`, so feature-detection code
//! (`typeof WebAssembly !== "undefined" && WebAssembly.validate(...)`) and
//! lazy wasm-bindgen loaders (photon-node, @jsquash/webp, undici's llhttp
//! probe) run their own catch/fallback paths instead of crashing on an
//! `undefined` read. The failure contract:
//!
//! - `compile` / `compileStreaming` / `instantiate` / `instantiateStreaming`
//!   return a Promise **rejected** with a `WebAssembly.CompileError` whose
//!   message points at issue #6558 — never a crash, never a hang, never a
//!   synchronous throw where the spec says reject.
//! - `validate(bytes)` returns `false` (spec-legal for any module, and the
//!   honest answer for "can I run this here").
//! - `new WebAssembly.Module(...)` throws `CompileError` synchronously (the
//!   spec's shape for an unsupported/invalid module); `Instance` throws
//!   `LinkError`; `Table` / `Global` throw `RuntimeError`.
//! - `new WebAssembly.Memory({ initial })` genuinely works: it backs the
//!   instance with a real zero-filled `ArrayBuffer` (`initial` 64KiB pages)
//!   and supports `grow(delta)`. Feature probes that allocate one page
//!   succeed.
//! - `CompileError` / `LinkError` / `RuntimeError` construct real error
//!   objects (`instanceof Error` is true) and `instanceof` against the
//!   namespace constructors brand-checks by error `.name` (see
//!   `webassembly_error_ctor_instanceof`, consulted from
//!   `js_instanceof_dynamic`).
//!
//! When the runtime is built with the `wasm-host` cargo feature (the opt-in
//! `--enable-wasm-runtime` wasmi path, issue #76), `validate` / `compile` /
//! `instantiate` and the `Module` constructor + metadata statics route to the
//! real host shims in `crate::webassembly` instead. The streaming entry
//! points still reject (no `Response`-driven compile in the host MVP).
//!
//! NOTE: the statically-recognized spellings (`WebAssembly.compile(bytes)`
//! etc. written literally against the global) lower to dedicated HIR
//! intrinsics in `perry-hir` and never reach this namespace object — they
//! auto-link the wasmi host archive. This file is the surface every aliased /
//! dynamic access hits (`const WA = globalThis.WebAssembly; WA.compile(...)`,
//! which is also how minified bundles usually spell it).

use super::*;

/// One-line story every graceful-failure message carries. Keep the issue URL
/// in here — it is the actionable breadcrumb when this surfaces in an app log.
pub(crate) const WASM_UNSUPPORTED_HINT: &str = "perry does not support WebAssembly yet; \
     WASM-dependent features degrade gracefully (tracked in \
     https://github.com/PerryTS/perry/issues/6558)";

const WASM_PAGE_BYTES: u32 = 65536;
/// Largest page count whose byte size still fits the `i32` ArrayBuffer
/// allocator (`js_array_buffer_new`): 32767 pages = 2GiB - 64KiB.
const WASM_MAX_PAGES: u32 = (i32::MAX as u32) / WASM_PAGE_BYTES;

#[inline]
fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn named_key(bytes: &[u8]) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

/// Accept both the NaN-boxed pointer encoding (top16 == 0x7FFD) and the
/// raw-i64 heap-pointer form module-level object variables are stored as
/// (mirrors the Temporal-subclass handling in `instanceof.rs`).
fn value_heap_ptr(value: f64) -> Option<*mut u8> {
    let bits = value.to_bits();
    let top16 = bits >> 48;
    let raw = if top16 == 0x7FFD {
        (bits & crate::value::POINTER_MASK) as usize
    } else if top16 == 0 && crate::value::addr_class::is_above_handle_band(bits as usize) {
        bits as usize
    } else {
        0
    };
    if raw == 0 {
        return None;
    }
    Some(raw as *mut u8)
}

fn value_gc_type(value: f64) -> Option<u8> {
    let ptr = value_heap_ptr(value)?;
    // Magnitude-classify (reject handle band / implausible / small-buf slab)
    // before dereferencing `ptr - GC_HEADER_SIZE` (#6279).
    let header = unsafe { crate::value::addr_class::try_read_gc_header(ptr as usize)? };
    Some(header.obj_type)
}

fn value_object_ptr(value: f64) -> Option<*mut ObjectHeader> {
    if value_gc_type(value) == Some(crate::gc::GC_TYPE_OBJECT) {
        Some(value_heap_ptr(value)? as *mut ObjectHeader)
    } else {
        None
    }
}

// ────────────────────────────────────────────────────────────────────────
// Graceful-failure error factory (#6558)
// ────────────────────────────────────────────────────────────────────────

/// Build a `WebAssembly.<name>Error`-shaped error object: an ordinary
/// `ErrorHeader` (so `instanceof Error`, `.message`, `.stack` all work)
/// whose `.name` is `CompileError` / `LinkError` / `RuntimeError`.
fn wasm_error_with_message(name: &'static [u8], message: &str) -> f64 {
    let message_ptr = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_error_new_with_name_message_bytes(name, message_ptr);
    crate::value::js_nanbox_pointer(err as i64)
}

/// The user-facing constructor path: coerce an arbitrary message VALUE the
/// way `new Error(v)` does (`undefined` → empty message).
fn wasm_error_from_message_value(name: &'static [u8], message: f64) -> f64 {
    let jv = crate::value::JSValue::from_bits(message.to_bits());
    let message_ptr = if jv.is_undefined() {
        crate::string::js_string_from_bytes(b"".as_ptr(), 0)
    } else {
        crate::builtins::js_string_coerce(message)
    };
    let err = crate::error::js_error_new_with_name_message_bytes(name, message_ptr);
    crate::value::js_nanbox_pointer(err as i64)
}

fn wasm_unsupported_error(name: &'static [u8], api: &str) -> f64 {
    wasm_error_with_message(name, &format!("{api}: {WASM_UNSUPPORTED_HINT}"))
}

/// A Promise rejected with a `WebAssembly.CompileError` carrying the #6558
/// unsupported message. This is the graceful-failure result of every async
/// entry point below.
fn wasm_unsupported_rejection(api: &str) -> f64 {
    let error = wasm_unsupported_error(b"CompileError", api);
    let promise = crate::promise::js_promise_rejected(error);
    crate::value::js_nanbox_pointer(promise as i64)
}

// ────────────────────────────────────────────────────────────────────────
// instanceof support for the namespace error constructors
// ────────────────────────────────────────────────────────────────────────

/// Resolve a candidate `instanceof` RHS back to the wasm error constructor
/// it is (if any), identified by the constructor thunk `func_ptr` — stable
/// across GC moves, and not forgeable by a user function that merely shares
/// the name. Returns the error `.name` the constructor brands.
fn webassembly_error_ctor_expected_name(type_ref: f64) -> Option<&'static [u8]> {
    let jv = crate::value::JSValue::from_bits(type_ref.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>() as *const crate::closure::ClosureHeader;
    if ptr.is_null()
        || !(ptr as usize).is_multiple_of(std::mem::align_of::<crate::closure::ClosureHeader>())
    {
        return None;
    }
    // `is_valid_obj_ptr` alone does not reject the small-handle band on
    // Linux/Windows/Android/iOS — use the canonical band+heap-floor pairing
    // (#6279).
    if !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return None;
    }
    unsafe {
        if (*ptr).type_tag != crate::closure::CLOSURE_MAGIC {
            return None;
        }
        let func_ptr = (*ptr).func_ptr as usize;
        if func_ptr == webassembly_compile_error_ctor_thunk as *const u8 as usize {
            Some(b"CompileError")
        } else if func_ptr == webassembly_link_error_ctor_thunk as *const u8 as usize {
            Some(b"LinkError")
        } else if func_ptr == webassembly_runtime_error_ctor_thunk as *const u8 as usize {
            Some(b"RuntimeError")
        } else {
            None
        }
    }
}

fn error_value_name_matches(value: f64, expected: &[u8]) -> bool {
    if value_gc_type(value) != Some(crate::gc::GC_TYPE_ERROR) {
        return false;
    }
    let Some(ptr) = value_heap_ptr(value) else {
        return false;
    };
    let err = ptr as *const crate::error::ErrorHeader;
    unsafe {
        let name = (*err).name;
        if name.is_null() {
            return false;
        }
        let len = (*name).byte_len as usize;
        if len != expected.len() {
            return false;
        }
        let data = (name as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
        std::slice::from_raw_parts(data, len) == expected
    }
}

/// `e instanceof WebAssembly.CompileError` (and LinkError / RuntimeError).
/// The wasm error constructors live on the WebAssembly NAMESPACE — not on
/// `globalThis` — and their instances are `ErrorHeader`-backed values with
/// no prototype chain reaching the namespace constructor's `.prototype`, so
/// neither the builtin-name path nor the ordinary prototype walk in
/// `js_instanceof_dynamic` can brand them. `Some(matches)` when `type_ref`
/// IS one of the three constructors; `None` for every other RHS.
pub(crate) fn webassembly_error_ctor_instanceof(value: f64, type_ref: f64) -> Option<bool> {
    let expected = webassembly_error_ctor_expected_name(type_ref)?;
    Some(error_value_name_matches(value, expected))
}

// ────────────────────────────────────────────────────────────────────────
// Constructor-call plumbing
// ────────────────────────────────────────────────────────────────────────

/// Whether the current dispatch is a `new <this constructor>` construction.
/// The dynamic construct path (`js_new_function_construct`) binds
/// `new.target` to the constructor value around the body call; a plain call
/// leaves it as whatever enclosing construction (if any) set — never OUR
/// closure. (A dynamic `class X extends WebAssembly.Memory` construction
/// passes X as new.target and is reported as a plain call here; subclassing
/// the wasm builtins is out of the #6558 baseline.)
fn invoked_as_constructor(closure: *const crate::closure::ClosureHeader) -> bool {
    if closure.is_null() {
        return false;
    }
    let bits = js_new_target_get().to_bits();
    if (bits >> 48) != 0x7FFD {
        return false;
    }
    ((bits & crate::value::POINTER_MASK) as usize) == closure as usize
}

fn throw_requires_new(what: &str) -> ! {
    super::super::object_ops::throw_object_type_error(
        format!("Constructor {what} requires 'new'").as_bytes(),
    )
}

// ────────────────────────────────────────────────────────────────────────
// Async entry points: compile / instantiate / *Streaming
// ────────────────────────────────────────────────────────────────────────

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_compile_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bytes: f64,
) -> f64 {
    crate::webassembly::js_webassembly_compile(bytes)
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_compile_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _bytes: f64,
) -> f64 {
    wasm_unsupported_rejection("WebAssembly.compile")
}

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_instantiate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bytes: f64,
) -> f64 {
    crate::webassembly::js_webassembly_instantiate(bytes)
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_instantiate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _bytes: f64,
) -> f64 {
    wasm_unsupported_rejection("WebAssembly.instantiate")
}

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_validate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    bytes: f64,
) -> f64 {
    crate::webassembly::js_webassembly_validate(bytes)
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_validate_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _bytes: f64,
) -> f64 {
    f64::from_bits(crate::value::JSValue::bool(false).bits())
}

/// Streaming compiles need a `Response`-driven source; the wasmi host MVP
/// has no streaming path either, so these reject under both cfgs.
extern "C" fn webassembly_compile_streaming_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _source: f64,
) -> f64 {
    wasm_unsupported_rejection("WebAssembly.compileStreaming")
}

extern "C" fn webassembly_instantiate_streaming_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _source: f64,
) -> f64 {
    wasm_unsupported_rejection("WebAssembly.instantiateStreaming")
}

/// `WebAssembly.promising` (JSPI) — kept as an existing-but-inert function.
extern "C" fn webassembly_unsupported_static_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _arg: f64,
) -> f64 {
    undefined()
}

// ────────────────────────────────────────────────────────────────────────
// Constructors
// ────────────────────────────────────────────────────────────────────────

extern "C" fn webassembly_module_ctor_thunk(
    closure: *const crate::closure::ClosureHeader,
    bytes: f64,
) -> f64 {
    if !invoked_as_constructor(closure) {
        throw_requires_new("WebAssembly.Module");
    }
    #[cfg(feature = "wasm-host")]
    {
        // Real wasmi compile; throws CompileError itself on invalid bytes.
        crate::webassembly::js_webassembly_module_new(bytes)
    }
    #[cfg(not(feature = "wasm-host"))]
    {
        let _ = bytes;
        crate::exception::js_throw(wasm_unsupported_error(
            b"CompileError",
            "WebAssembly.Module",
        ));
    }
}

extern "C" fn webassembly_instance_ctor_thunk(
    closure: *const crate::closure::ClosureHeader,
    _module: f64,
) -> f64 {
    if !invoked_as_constructor(closure) {
        throw_requires_new("WebAssembly.Instance");
    }
    crate::exception::js_throw(wasm_unsupported_error(b"LinkError", "WebAssembly.Instance"));
}

extern "C" fn webassembly_table_ctor_thunk(
    closure: *const crate::closure::ClosureHeader,
    _descriptor: f64,
) -> f64 {
    if !invoked_as_constructor(closure) {
        throw_requires_new("WebAssembly.Table");
    }
    crate::exception::js_throw(wasm_unsupported_error(b"RuntimeError", "WebAssembly.Table"));
}

extern "C" fn webassembly_global_ctor_thunk(
    closure: *const crate::closure::ClosureHeader,
    _descriptor: f64,
) -> f64 {
    if !invoked_as_constructor(closure) {
        throw_requires_new("WebAssembly.Global");
    }
    crate::exception::js_throw(wasm_unsupported_error(
        b"RuntimeError",
        "WebAssembly.Global",
    ));
}

// ── Memory: minimally functional (real ArrayBuffer backing) ─────────────

enum MemoryCtorError {
    /// Descriptor missing / not an object / `initial` absent or not a
    /// non-negative number → TypeError per spec.
    Type(&'static str),
    /// `initial` (or `maximum`) out of the representable page range →
    /// RangeError per spec.
    Range(&'static str),
}

/// Validate a `MemoryDescriptor` and return the requested `initial` page
/// count. Split from the thunk so the unit tests can exercise every arm
/// without triggering the `js_throw` unwind path.
fn wasm_memory_descriptor_pages(descriptor: f64) -> Result<u32, MemoryCtorError> {
    let Some(obj) = value_object_ptr(descriptor) else {
        return Err(MemoryCtorError::Type(
            "WebAssembly.Memory(): argument must be a memory descriptor object",
        ));
    };
    let initial_raw = js_object_get_field_by_name_f64(obj, named_key(b"initial"));
    let initial = crate::value::JSValue::from_bits(initial_raw.to_bits()).to_number();
    if !initial.is_finite() || initial < 0.0 {
        return Err(MemoryCtorError::Type(
            "WebAssembly.Memory(): descriptor property 'initial' must be a non-negative number",
        ));
    }
    let pages = initial.trunc();
    if pages > WASM_MAX_PAGES as f64 {
        return Err(MemoryCtorError::Range(
            "WebAssembly.Memory(): could not allocate the requested initial pages",
        ));
    }
    let pages = pages as u32;
    let maximum_raw = js_object_get_field_by_name_f64(obj, named_key(b"maximum"));
    let maximum_jv = crate::value::JSValue::from_bits(maximum_raw.to_bits());
    if !maximum_jv.is_undefined() {
        let maximum = maximum_jv.to_number();
        if maximum.is_finite() && maximum.trunc() < pages as f64 {
            return Err(MemoryCtorError::Range(
                "WebAssembly.Memory(): 'maximum' must be at least 'initial'",
            ));
        }
    }
    Ok(pages)
}

fn wasm_memory_new_buffer(pages: u32) -> f64 {
    let buf = crate::buffer::js_array_buffer_new((pages * WASM_PAGE_BYTES) as i32);
    crate::value::js_nanbox_pointer(buf as i64)
}

extern "C" fn webassembly_memory_ctor_thunk(
    closure: *const crate::closure::ClosureHeader,
    descriptor: f64,
) -> f64 {
    if !invoked_as_constructor(closure) {
        throw_requires_new("WebAssembly.Memory");
    }
    let pages = match wasm_memory_descriptor_pages(descriptor) {
        Ok(pages) => pages,
        Err(MemoryCtorError::Type(msg)) => {
            super::super::object_ops::throw_object_type_error(msg.as_bytes())
        }
        Err(MemoryCtorError::Range(msg)) => {
            let message_ptr = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err = crate::error::js_rangeerror_new(message_ptr);
            crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
        }
    };
    let buffer = wasm_memory_new_buffer(pages);
    // The dynamic construct path pre-allocated the receiver with
    // `Memory.prototype` linked (so `instanceof` works); fill it in place.
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    if let Some(this_obj) = value_object_ptr(this) {
        js_object_set_field_by_name(this_obj, named_key(b"buffer"), buffer);
        undefined()
    } else {
        // Reached only from a non-construct dispatch that faked new.target;
        // still return a usable standalone instance rather than crashing.
        let obj = js_object_alloc(0, 1);
        if obj.is_null() {
            return undefined();
        }
        js_object_set_field_by_name(obj, named_key(b"buffer"), buffer);
        crate::value::js_nanbox_pointer(obj as i64)
    }
}

fn memory_buffer_ptr(value: f64) -> Option<*mut crate::buffer::BufferHeader> {
    let ptr = value_heap_ptr(value)?;
    if crate::buffer::is_array_buffer(ptr as usize) {
        Some(ptr as *mut crate::buffer::BufferHeader)
    } else {
        None
    }
}

/// Grow logic split from the thunk for unit-testability: returns the OLD
/// page count on success, replacing the receiver's `buffer` with a larger
/// copy (the spec detaches the old buffer; perry's baseline leaves the old
/// buffer intact — stale aliases keep reading the pre-grow bytes).
fn wasm_memory_grow_on(this: f64, delta: f64) -> Result<u32, MemoryCtorError> {
    let Some(this_obj) = value_object_ptr(this) else {
        return Err(MemoryCtorError::Type(
            "WebAssembly.Memory.prototype.grow called on an incompatible receiver",
        ));
    };
    let buffer_val = js_object_get_field_by_name_f64(this_obj, named_key(b"buffer"));
    let Some(buf) = memory_buffer_ptr(buffer_val) else {
        return Err(MemoryCtorError::Type(
            "WebAssembly.Memory.prototype.grow called on an incompatible receiver",
        ));
    };
    if !delta.is_finite() || delta < 0.0 {
        return Err(MemoryCtorError::Type(
            "WebAssembly.Memory.grow(): argument must be a non-negative number",
        ));
    }
    let delta_pages = delta.trunc();
    let old_bytes = unsafe { (*buf).length } as usize;
    let old_pages = (old_bytes / WASM_PAGE_BYTES as usize) as u32;
    if delta_pages > (WASM_MAX_PAGES - old_pages) as f64 {
        return Err(MemoryCtorError::Range(
            "WebAssembly.Memory.grow(): could not grow memory",
        ));
    }
    let new_pages = old_pages + delta_pages as u32;
    let new_buf = crate::buffer::js_array_buffer_new((new_pages * WASM_PAGE_BYTES) as i32);
    if new_buf.is_null() {
        return Err(MemoryCtorError::Range(
            "WebAssembly.Memory.grow(): could not grow memory",
        ));
    }
    if old_bytes > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                crate::buffer::buffer_data_mut(buf),
                crate::buffer::buffer_data_mut(new_buf),
                old_bytes,
            );
        }
    }
    js_object_set_field_by_name(
        this_obj,
        named_key(b"buffer"),
        crate::value::js_nanbox_pointer(new_buf as i64),
    );
    Ok(old_pages)
}

extern "C" fn webassembly_memory_grow_thunk(
    _closure: *const crate::closure::ClosureHeader,
    delta: f64,
) -> f64 {
    let this = f64::from_bits(IMPLICIT_THIS.with(|c| c.get()));
    match wasm_memory_grow_on(this, delta) {
        Ok(old_pages) => old_pages as f64,
        Err(MemoryCtorError::Type(msg)) => {
            super::super::object_ops::throw_object_type_error(msg.as_bytes())
        }
        Err(MemoryCtorError::Range(msg)) => {
            let message_ptr = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
            let err = crate::error::js_rangeerror_new(message_ptr);
            crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64));
        }
    }
}

// ── Error constructors ──────────────────────────────────────────────────

extern "C" fn webassembly_compile_error_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    wasm_error_from_message_value(b"CompileError", message)
}

extern "C" fn webassembly_link_error_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    wasm_error_from_message_value(b"LinkError", message)
}

extern "C" fn webassembly_runtime_error_ctor_thunk(
    _closure: *const crate::closure::ClosureHeader,
    message: f64,
) -> f64 {
    wasm_error_from_message_value(b"RuntimeError", message)
}

// ── Module metadata statics ─────────────────────────────────────────────

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_module_exports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    module: f64,
) -> f64 {
    crate::webassembly::js_webassembly_module_exports(module)
}

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_module_imports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    module: f64,
) -> f64 {
    crate::webassembly::js_webassembly_module_imports(module)
}

#[cfg(feature = "wasm-host")]
extern "C" fn webassembly_module_custom_sections_thunk(
    _closure: *const crate::closure::ClosureHeader,
    module: f64,
    name: f64,
) -> f64 {
    crate::webassembly::js_webassembly_module_custom_sections(module, name)
}

/// Without an engine no `WebAssembly.Module` instance can exist, so any
/// argument fails the spec's brand check → synchronous TypeError.
#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_module_exports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _module: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(
        b"WebAssembly.Module.exports(): argument must be a WebAssembly.Module",
    )
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_module_imports_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _module: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(
        b"WebAssembly.Module.imports(): argument must be a WebAssembly.Module",
    )
}

#[cfg(not(feature = "wasm-host"))]
extern "C" fn webassembly_module_custom_sections_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _module: f64,
    _name: f64,
) -> f64 {
    super::super::object_ops::throw_object_type_error(
        b"WebAssembly.Module.customSections(): argument must be a WebAssembly.Module",
    )
}

// ────────────────────────────────────────────────────────────────────────
// Namespace assembly
// ────────────────────────────────────────────────────────────────────────

pub(super) fn create_webassembly_namespace() -> f64 {
    let ns_obj = js_object_alloc(0, 0);
    if ns_obj.is_null() {
        return undefined();
    }

    let module_ctor = install_webassembly_constructor(
        ns_obj,
        "Module",
        webassembly_module_ctor_thunk as *const u8,
    );
    if !module_ctor.is_null() {
        install_webassembly_static_fn(
            module_ctor as *mut ObjectHeader,
            "exports",
            webassembly_module_exports_thunk as *const u8,
            1,
            true,
        );
        install_webassembly_static_fn(
            module_ctor as *mut ObjectHeader,
            "imports",
            webassembly_module_imports_thunk as *const u8,
            1,
            true,
        );
        install_webassembly_static_fn(
            module_ctor as *mut ObjectHeader,
            "customSections",
            webassembly_module_custom_sections_thunk as *const u8,
            2,
            true,
        );
    }

    let instance_ctor = install_webassembly_constructor(
        ns_obj,
        "Instance",
        webassembly_instance_ctor_thunk as *const u8,
    );
    install_webassembly_proto_data(instance_ctor, "exports", undefined());

    let memory_ctor = install_webassembly_constructor(
        ns_obj,
        "Memory",
        webassembly_memory_ctor_thunk as *const u8,
    );
    install_webassembly_proto_data(memory_ctor, "buffer", undefined());
    install_webassembly_proto_fn(
        memory_ctor,
        "grow",
        webassembly_memory_grow_thunk as *const u8,
        1,
    );

    let table_ctor =
        install_webassembly_constructor(ns_obj, "Table", webassembly_table_ctor_thunk as *const u8);
    install_webassembly_proto_method(table_ctor, "get", 1);
    install_webassembly_proto_method(table_ctor, "grow", 1);
    install_webassembly_proto_data(table_ctor, "length", undefined());
    install_webassembly_proto_method(table_ctor, "set", 2);

    let global_ctor = install_webassembly_constructor(
        ns_obj,
        "Global",
        webassembly_global_ctor_thunk as *const u8,
    );
    install_webassembly_proto_data(global_ctor, "value", undefined());
    install_webassembly_proto_method(global_ctor, "valueOf", 0);

    for (name, func_ptr) in [
        (
            "CompileError",
            webassembly_compile_error_ctor_thunk as *const u8,
        ),
        ("LinkError", webassembly_link_error_ctor_thunk as *const u8),
        (
            "RuntimeError",
            webassembly_runtime_error_ctor_thunk as *const u8,
        ),
        ("Exception", global_this_builtin_noop_thunk as *const u8),
        ("Tag", global_this_builtin_noop_thunk as *const u8),
    ] {
        let ctor = install_webassembly_constructor(ns_obj, name, func_ptr);
        if matches!(name, "CompileError" | "LinkError" | "RuntimeError") {
            install_webassembly_error_proto_data(ctor, name);
        }
    }

    install_webassembly_object_property(
        ns_obj,
        "JSTag",
        super::super::PropertyAttrs::new(false, false, true),
    );

    install_webassembly_static_fn(
        ns_obj,
        "compile",
        webassembly_compile_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "validate",
        webassembly_validate_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "instantiate",
        webassembly_instantiate_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "compileStreaming",
        webassembly_compile_streaming_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "instantiateStreaming",
        webassembly_instantiate_streaming_thunk as *const u8,
        1,
        true,
    );
    install_webassembly_static_fn(
        ns_obj,
        "promising",
        webassembly_unsupported_static_thunk as *const u8,
        1,
        true,
    );

    crate::value::js_nanbox_pointer(ns_obj as i64)
}

fn install_webassembly_constructor(
    ns_obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
) -> *mut crate::closure::ClosureHeader {
    if ns_obj.is_null() {
        return std::ptr::null_mut();
    }
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return std::ptr::null_mut();
    }
    crate::closure::js_register_closure_arity(func_ptr, 1);
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, 1);
    super::super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );

    let proto_obj = js_object_alloc(0, 0);
    if !proto_obj.is_null() {
        let proto_key = crate::string::js_string_from_bytes(b"prototype".as_ptr(), 9);
        let proto_value = crate::value::js_nanbox_pointer(proto_obj as i64);
        js_object_set_field_by_name(closure as *mut ObjectHeader, proto_key, proto_value);
        super::super::set_builtin_property_attrs(
            closure as usize,
            "prototype".to_string(),
            super::super::PropertyAttrs::new(false, false, false),
        );

        let ctor_key = crate::string::js_string_from_bytes(b"constructor".as_ptr(), 11);
        let ctor_value = crate::value::js_nanbox_pointer(closure as i64);
        js_object_set_field_by_name(proto_obj, ctor_key, ctor_value);
        super::super::set_builtin_property_attrs(
            proto_obj as usize,
            "constructor".to_string(),
            super::super::PropertyAttrs::new(true, false, true),
        );
    }

    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(ns_obj, key, value);
    super::super::set_builtin_property_attrs(
        ns_obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
    closure
}

fn install_webassembly_static_fn(
    obj: *mut ObjectHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
    enumerable: bool,
) {
    if obj.is_null() {
        return;
    }
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    super::super::native_module::set_bound_native_closure_name(closure, name);
    super::super::native_module::set_builtin_closure_length(closure as usize, arity);
    super::super::set_builtin_property_attrs(
        closure as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    super::super::set_builtin_property_attrs(
        closure as usize,
        "length".to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    js_object_set_field_by_name(obj, key, value);
    // Node (v26) descriptor for the namespace FUNCTION members and the
    // `Module.*` metadata statics: { writable: true, enumerable: true,
    // configurable: true } — while the CONSTRUCTOR members are installed
    // non-enumerable (see `install_webassembly_constructor`). Verified
    // against the webassembly-namespace.ts node-suite fixture.
    super::super::set_builtin_property_attrs(
        obj as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(true, enumerable, true),
    );
}

fn webassembly_constructor_proto(ctor: *mut crate::closure::ClosureHeader) -> *mut ObjectHeader {
    if ctor.is_null() {
        return std::ptr::null_mut();
    }
    let value = crate::closure::closure_get_dynamic_prop(ctor as usize, "prototype");
    let jsv = crate::value::JSValue::from_bits(value.to_bits());
    if jsv.is_pointer() {
        jsv.as_pointer::<ObjectHeader>() as *mut ObjectHeader
    } else {
        std::ptr::null_mut()
    }
}

fn install_webassembly_proto_method(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    arity: u32,
) {
    install_webassembly_proto_fn(
        ctor,
        name,
        global_this_builtin_noop_thunk as *const u8,
        arity,
    );
}

fn install_webassembly_proto_fn(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    func_ptr: *const u8,
    arity: u32,
) {
    let proto = webassembly_constructor_proto(ctor);
    if proto.is_null() {
        return;
    }
    install_proto_method(proto, name, func_ptr, arity);
}

fn install_webassembly_proto_data(
    ctor: *mut crate::closure::ClosureHeader,
    name: &str,
    value: f64,
) {
    let proto = webassembly_constructor_proto(ctor);
    if proto.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(proto, key, value);
    super::super::set_builtin_property_attrs(
        proto as usize,
        name.to_string(),
        super::super::PropertyAttrs::new(false, false, true),
    );
}

fn install_webassembly_error_proto_data(ctor: *mut crate::closure::ClosureHeader, name: &str) {
    let proto = webassembly_constructor_proto(ctor);
    if proto.is_null() {
        return;
    }
    let name_key = crate::string::js_string_from_bytes(b"name".as_ptr(), 4);
    let name_string = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    js_object_set_field_by_name(
        proto,
        name_key,
        crate::value::js_nanbox_string(name_string as i64),
    );
    super::super::set_builtin_property_attrs(
        proto as usize,
        "name".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );

    let message_key = crate::string::js_string_from_bytes(b"message".as_ptr(), 7);
    let message_string = crate::string::js_string_from_bytes(b"".as_ptr(), 0);
    js_object_set_field_by_name(
        proto,
        message_key,
        crate::value::js_nanbox_string(message_string as i64),
    );
    super::super::set_builtin_property_attrs(
        proto as usize,
        "message".to_string(),
        super::super::PropertyAttrs::new(true, false, true),
    );
}

fn install_webassembly_object_property(
    ns_obj: *mut ObjectHeader,
    name: &str,
    attrs: super::super::PropertyAttrs,
) {
    if ns_obj.is_null() {
        return;
    }
    let obj = js_object_alloc(0, 0);
    if obj.is_null() {
        return;
    }
    let key = crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    js_object_set_field_by_name(ns_obj, key, value);
    super::super::set_builtin_property_attrs(ns_obj as usize, name.to_string(), attrs);
}

// ────────────────────────────────────────────────────────────────────────
// Unit tests (#6558). Only the non-throwing arms are exercised here — the
// `js_throw` paths longjmp and are covered by the e2e/parity fixtures
// (`test-parity/node-suite/globals/webassembly-*.ts`,
// `tests/test_webassembly_graceful_fail.sh`).
// ────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn string_value(text: &str) -> f64 {
        let ptr = crate::string::js_string_from_bytes(text.as_ptr(), text.len() as u32);
        crate::value::js_nanbox_string(ptr as i64)
    }

    fn ns_field(ns: f64, name: &[u8]) -> f64 {
        let obj = value_heap_ptr(ns).expect("namespace pointer") as *mut ObjectHeader;
        js_object_get_field_by_name_f64(obj, named_key(name))
    }

    fn closure_ptr(value: f64) -> *const crate::closure::ClosureHeader {
        let jv = crate::value::JSValue::from_bits(value.to_bits());
        assert!(jv.is_pointer(), "expected closure pointer value");
        jv.as_pointer::<u8>() as *const crate::closure::ClosureHeader
    }

    fn error_name_bytes(value: f64) -> Vec<u8> {
        let ptr = value_heap_ptr(value).expect("error pointer");
        let err = ptr as *const crate::error::ErrorHeader;
        unsafe {
            let name = (*err).name;
            assert!(!name.is_null());
            let len = (*name).byte_len as usize;
            let data = (name as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            std::slice::from_raw_parts(data, len).to_vec()
        }
    }

    fn error_message_string(value: f64) -> String {
        let ptr = value_heap_ptr(value).expect("error pointer");
        let err = ptr as *const crate::error::ErrorHeader;
        unsafe {
            let message = (*err).message;
            if message.is_null() {
                return String::new();
            }
            let len = (*message).byte_len as usize;
            let data =
                (message as *const u8).add(std::mem::size_of::<crate::string::StringHeader>());
            String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
        }
    }

    fn assert_rejected_with_compile_error(promise_value: f64, api_fragment: &str) {
        let ptr = value_heap_ptr(promise_value).expect("promise pointer");
        let promise = ptr as *const crate::promise::Promise;
        let (state, reason) = unsafe { ((*promise).state, (*promise).reason) };
        assert!(
            matches!(state, crate::promise::PromiseState::Rejected),
            "{api_fragment}: expected a rejected promise"
        );
        assert_eq!(
            error_name_bytes(reason),
            b"CompileError".to_vec(),
            "{api_fragment}: rejection reason must be a WebAssembly.CompileError"
        );
        let message = error_message_string(reason);
        assert!(
            message.contains(api_fragment),
            "rejection message must name the API: {message}"
        );
        assert!(
            message.contains("6558"),
            "rejection message must reference issue #6558: {message}"
        );
        assert!(
            value_gc_type(reason) == Some(crate::gc::GC_TYPE_ERROR),
            "rejection reason must be a real error object"
        );
    }

    #[test]
    fn namespace_members_exist_with_expected_shapes() {
        let ns = create_webassembly_namespace();
        assert!(value_heap_ptr(ns).is_some(), "namespace must be an object");

        for name in [
            &b"compile"[..],
            b"validate",
            b"instantiate",
            b"compileStreaming",
            b"instantiateStreaming",
            b"promising",
            b"Module",
            b"Instance",
            b"Memory",
            b"Table",
            b"Global",
            b"CompileError",
            b"LinkError",
            b"RuntimeError",
            b"Exception",
            b"Tag",
        ] {
            let member = ns_field(ns, name);
            let jv = crate::value::JSValue::from_bits(member.to_bits());
            assert!(
                jv.is_pointer(),
                "WebAssembly.{} must exist as a function",
                String::from_utf8_lossy(name)
            );
            let closure = closure_ptr(member);
            unsafe {
                assert_eq!(
                    (*closure).type_tag,
                    crate::closure::CLOSURE_MAGIC,
                    "WebAssembly.{} must be a closure",
                    String::from_utf8_lossy(name)
                );
            }
        }

        // Constructors expose a prototype with a constructor backref.
        let module_ctor = closure_ptr(ns_field(ns, b"Module"));
        let proto = webassembly_constructor_proto(module_ctor as *mut _);
        assert!(!proto.is_null(), "Module.prototype must exist");

        // Memory.prototype carries a callable grow.
        let memory_ctor = closure_ptr(ns_field(ns, b"Memory"));
        let memory_proto = webassembly_constructor_proto(memory_ctor as *mut _);
        assert!(!memory_proto.is_null());
        let grow = js_object_get_field_by_name_f64(memory_proto, named_key(b"grow"));
        let grow_jv = crate::value::JSValue::from_bits(grow.to_bits());
        assert!(grow_jv.is_pointer(), "Memory.prototype.grow must exist");
    }

    #[cfg(not(feature = "wasm-host"))]
    #[test]
    fn async_members_reject_with_compile_error() {
        let closure = std::ptr::null();
        assert_rejected_with_compile_error(
            webassembly_compile_thunk(closure, undefined()),
            "WebAssembly.compile",
        );
        assert_rejected_with_compile_error(
            webassembly_instantiate_thunk(closure, undefined()),
            "WebAssembly.instantiate",
        );
        assert_rejected_with_compile_error(
            webassembly_compile_streaming_thunk(closure, undefined()),
            "WebAssembly.compileStreaming",
        );
        assert_rejected_with_compile_error(
            webassembly_instantiate_streaming_thunk(closure, undefined()),
            "WebAssembly.instantiateStreaming",
        );
    }

    #[cfg(not(feature = "wasm-host"))]
    #[test]
    fn validate_reports_false() {
        let result = webassembly_validate_thunk(std::ptr::null(), undefined());
        assert_eq!(
            result.to_bits(),
            crate::value::JSValue::bool(false).bits(),
            "validate must answer false without an engine"
        );
    }

    #[test]
    fn error_constructors_build_branded_error_objects() {
        let message = string_value("boom");
        let compile_err = webassembly_compile_error_ctor_thunk(std::ptr::null(), message);
        assert_eq!(error_name_bytes(compile_err), b"CompileError".to_vec());
        assert_eq!(error_message_string(compile_err), "boom");
        assert_eq!(value_gc_type(compile_err), Some(crate::gc::GC_TYPE_ERROR));

        // Message coercion mirrors `new Error(v)`.
        let numbered = webassembly_link_error_ctor_thunk(std::ptr::null(), 42.0);
        assert_eq!(error_name_bytes(numbered), b"LinkError".to_vec());
        assert_eq!(error_message_string(numbered), "42");

        let bare = webassembly_runtime_error_ctor_thunk(std::ptr::null(), undefined());
        assert_eq!(error_name_bytes(bare), b"RuntimeError".to_vec());
        assert_eq!(error_message_string(bare), "");
    }

    #[test]
    fn error_ctor_instanceof_brands_by_ctor_identity_and_error_name() {
        let ns = create_webassembly_namespace();
        let compile_ctor = ns_field(ns, b"CompileError");
        let link_ctor = ns_field(ns, b"LinkError");
        let runtime_ctor = ns_field(ns, b"RuntimeError");

        let compile_err = webassembly_compile_error_ctor_thunk(std::ptr::null(), string_value("x"));
        let link_err = webassembly_link_error_ctor_thunk(std::ptr::null(), string_value("x"));

        assert_eq!(
            webassembly_error_ctor_instanceof(compile_err, compile_ctor),
            Some(true)
        );
        assert_eq!(
            webassembly_error_ctor_instanceof(link_err, link_ctor),
            Some(true)
        );
        // Cross-brand must not match.
        assert_eq!(
            webassembly_error_ctor_instanceof(compile_err, link_ctor),
            Some(false)
        );
        assert_eq!(
            webassembly_error_ctor_instanceof(link_err, runtime_ctor),
            Some(false)
        );
        // Non-error LHS values never match.
        assert_eq!(
            webassembly_error_ctor_instanceof(undefined(), compile_ctor),
            Some(false)
        );
        assert_eq!(
            webassembly_error_ctor_instanceof(1.5, compile_ctor),
            Some(false)
        );
        // A non-wasm-error RHS is not ours to answer.
        assert_eq!(
            webassembly_error_ctor_instanceof(compile_err, ns_field(ns, b"validate")),
            None
        );
        assert_eq!(webassembly_error_ctor_instanceof(compile_err, 2.0), None);
        // An ordinary Error whose name merely coincides IS accepted by the
        // name brand (documented looseness of the baseline): verify the
        // negative case that matters — a plain Error does NOT match.
        let plain = crate::error::js_error_new_with_message(named_key(b"plain"));
        let plain_value = crate::value::js_nanbox_pointer(plain as i64);
        assert_eq!(
            webassembly_error_ctor_instanceof(plain_value, compile_ctor),
            Some(false)
        );
    }

    #[test]
    fn memory_descriptor_validation_and_buffer_backing() {
        // Valid: 1 page → 65536-byte zero-filled ArrayBuffer.
        let descriptor = js_object_alloc(0, 1);
        js_object_set_field_by_name(descriptor, named_key(b"initial"), 1.0);
        let pages =
            wasm_memory_descriptor_pages(crate::value::js_nanbox_pointer(descriptor as i64))
                .unwrap_or_else(|_| panic!("descriptor {{initial: 1}} must validate"));
        assert_eq!(pages, 1);
        let buffer = wasm_memory_new_buffer(pages);
        let buf = memory_buffer_ptr(buffer).expect("buffer must be a registered ArrayBuffer");
        unsafe {
            assert_eq!((*buf).length, WASM_PAGE_BYTES);
        }

        // Zero pages are legal.
        let zero_desc = js_object_alloc(0, 1);
        js_object_set_field_by_name(zero_desc, named_key(b"initial"), 0.0);
        assert!(matches!(
            wasm_memory_descriptor_pages(crate::value::js_nanbox_pointer(zero_desc as i64)),
            Ok(0)
        ));

        // Missing initial → TypeError arm.
        let empty_desc = js_object_alloc(0, 0);
        assert!(matches!(
            wasm_memory_descriptor_pages(crate::value::js_nanbox_pointer(empty_desc as i64)),
            Err(MemoryCtorError::Type(_))
        ));

        // Non-object descriptor → TypeError arm.
        assert!(matches!(
            wasm_memory_descriptor_pages(undefined()),
            Err(MemoryCtorError::Type(_))
        ));

        // Oversized initial → RangeError arm.
        let big_desc = js_object_alloc(0, 1);
        js_object_set_field_by_name(big_desc, named_key(b"initial"), 1e9);
        assert!(matches!(
            wasm_memory_descriptor_pages(crate::value::js_nanbox_pointer(big_desc as i64)),
            Err(MemoryCtorError::Range(_))
        ));

        // maximum < initial → RangeError arm.
        let shrunk_desc = js_object_alloc(0, 2);
        js_object_set_field_by_name(shrunk_desc, named_key(b"initial"), 2.0);
        js_object_set_field_by_name(shrunk_desc, named_key(b"maximum"), 1.0);
        assert!(matches!(
            wasm_memory_descriptor_pages(crate::value::js_nanbox_pointer(shrunk_desc as i64)),
            Err(MemoryCtorError::Range(_))
        ));
    }

    #[test]
    fn memory_grow_replaces_buffer_and_returns_old_page_count() {
        let instance = js_object_alloc(0, 1);
        js_object_set_field_by_name(instance, named_key(b"buffer"), wasm_memory_new_buffer(1));
        let instance_value = crate::value::js_nanbox_pointer(instance as i64);

        // Write a marker byte and confirm grow copies it over.
        let before = js_object_get_field_by_name_f64(instance, named_key(b"buffer"));
        let before_buf = memory_buffer_ptr(before).unwrap();
        unsafe {
            *crate::buffer::buffer_data_mut(before_buf) = 0xAB;
        }

        let old_pages = match wasm_memory_grow_on(instance_value, 2.0) {
            Ok(pages) => pages,
            Err(_) => panic!("grow(2) on a 1-page memory must succeed"),
        };
        assert_eq!(old_pages, 1);

        let after = js_object_get_field_by_name_f64(instance, named_key(b"buffer"));
        let after_buf = memory_buffer_ptr(after).unwrap();
        unsafe {
            assert_eq!((*after_buf).length, 3 * WASM_PAGE_BYTES);
            assert_eq!(*crate::buffer::buffer_data_mut(after_buf), 0xAB);
        }

        // Negative delta → TypeError arm; absurd delta → RangeError arm.
        assert!(matches!(
            wasm_memory_grow_on(instance_value, -1.0),
            Err(MemoryCtorError::Type(_))
        ));
        assert!(matches!(
            wasm_memory_grow_on(instance_value, 1e9),
            Err(MemoryCtorError::Range(_))
        ));
        // Non-memory receiver → TypeError arm.
        assert!(matches!(
            wasm_memory_grow_on(undefined(), 1.0),
            Err(MemoryCtorError::Type(_))
        ));
    }
}
