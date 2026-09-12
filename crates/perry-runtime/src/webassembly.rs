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
//! ## API shape
//!
//! The standard `WebAssembly.instantiate(bytes).then(({instance}) =>
//! instance.exports.add(2, 3))` shape needs (a) Promise wrapping and
//! (b) dynamic property access proxying. The first wasm-host pass exposed
//! a Perry-specific synchronous helper:
//!
//! ```ts
//! WebAssembly.validate(bytes: Uint8Array): boolean;
//! WebAssembly.instantiate(bytes: Uint8Array): number; // opaque handle
//! WebAssembly.callExport(handle: number, name: string, ...args: number[]): number;
//! ```
//!
//! This file also carries the low-risk standard module metadata slice:
//! `new WebAssembly.Module(bytes)`, `WebAssembly.compile(bytes)`, and
//! `WebAssembly.Module.{exports,imports,customSections}`.
//!
//! Export calls support the four numeric types, while imported callbacks also
//! preserve `externref` JavaScript values and shared `funcref` table entries.

use std::ffi::{c_char, c_void};

use crate::value::{JSValue, TAG_UNDEFINED};

#[path = "webassembly_host.rs"]
mod host;
use host::*;
#[path = "webassembly_calls.rs"]
mod calls;
use calls::*;

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
            if let Some(bytes) = unsafe { crate::typedarray::typed_array_bytes(header) } {
                return Some((bytes.as_ptr(), bytes.len()));
            }
        }
    }

    if crate::buffer::is_registered_buffer(addr)
        || crate::buffer::is_array_buffer(addr)
        || crate::buffer::is_uint8array_buffer(addr)
    {
        let header = addr as *const crate::buffer::BufferHeader;
        let len = unsafe { (*header).length as usize };
        let data = crate::buffer::buffer_data(header as *const crate::buffer::BufferHeader);
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

fn emit_error_to_stderr(prefix: &str, err: *mut c_char) {
    if !err.is_null() {
        let cs = unsafe { std::ffi::CStr::from_ptr(err) };
        eprintln!("{prefix}: {}", cs.to_string_lossy());
        unsafe { perry_wasm_host_string_free(err) };
    } else {
        eprintln!("{prefix}: <unknown>");
    }
}

pub(crate) fn drop_host_extern_handle(handle: usize) {
    if handle != 0 {
        unsafe { perry_wasm_host_extern_drop(handle as *mut c_void) };
    }
}

/// Consume (and free) a host error C-string into a `WebAssembly.<name>`-
/// shaped error value: an ordinary `ErrorHeader` whose `.name` is
/// `CompileError` / `LinkError` — the same shape the graceful-fail
/// namespace produces (#6558), so `err instanceof WebAssembly.CompileError`
/// and `.catch` handlers see one consistent brand in both modes.
fn wasm_error_value_from_host(name: &'static [u8], err: *mut c_char, fallback: &str) -> f64 {
    let message = if err.is_null() {
        fallback.to_string()
    } else {
        let cs = unsafe { std::ffi::CStr::from_ptr(err) };
        let text = cs.to_string_lossy().into_owned();
        unsafe { perry_wasm_host_string_free(err) };
        text
    };
    let message_ptr = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_error_new_with_name_message_bytes(name, message_ptr);
    crate::value::js_nanbox_pointer(error as i64)
}

fn wasm_type_error_value(message: &str) -> f64 {
    let message_ptr = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let error = crate::error::js_typeerror_new(message_ptr);
    crate::value::js_nanbox_pointer(error as i64)
}

fn rejected_promise_value(reason: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let reason = scope.root_nanbox_f64(reason);
    let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
    crate::promise::js_promise_reject(
        promise.get_raw_mut_ptr::<crate::promise::Promise>(),
        reason.get_nanbox_f64(),
    );
    crate::value::js_nanbox_pointer(promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64)
}

/// Compile `bytes_jsval` into a module wrapper. `Err` carries a ready-to-
/// throw/reject JS error VALUE (TypeError for a non-buffer argument,
/// CompileError for invalid bytes) so each caller can pick the spec-mandated
/// delivery: `new WebAssembly.Module` throws synchronously, `compile` /
/// `instantiate` reject their promise.
fn module_new_value(bytes_jsval: f64) -> Result<f64, f64> {
    let Some((ptr, len)) = extract_bytes(bytes_jsval) else {
        return Err(wasm_type_error_value(
            "WebAssembly.Module: argument must be a Uint8Array or ArrayBuffer",
        ));
    };
    let mut err: *mut c_char = std::ptr::null_mut();
    let module = unsafe { perry_wasm_host_module_new(ptr, len, &mut err) };
    if module.is_null() {
        return Err(wasm_error_value_from_host(
            b"CompileError",
            err,
            "WebAssembly.Module(): compile failed",
        ));
    }
    Ok(make_module_object(module))
}

fn string_value(bytes: &[u8]) -> f64 {
    let ptr = crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn named_key(bytes: &[u8]) -> *mut crate::string::StringHeader {
    crate::string::js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn object_set(
    obj: *mut crate::object::ObjectHeader,
    key: &[u8],
    value: f64,
) -> *mut crate::object::ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(named_key(key));
    obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| {
        key.with_const_ptr(|k: *const crate::string::StringHeader| {
            crate::object::js_object_set_field_by_name(o, k, value.get_nanbox_f64())
        })
    });
    obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| o)
}

fn object_set_string(
    obj: *mut crate::object::ObjectHeader,
    key: &[u8],
    value: &[u8],
) -> *mut crate::object::ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(string_value(value));
    object_set(
        obj.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        key,
        value.get_nanbox_f64(),
    )
}

fn object_value(obj: *mut crate::object::ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn array_value(arr: *mut crate::array::ArrayHeader) -> f64 {
    crate::value::js_nanbox_pointer(arr as i64)
}

fn array_buffer_from_bytes(data: *const u8, len: usize) -> f64 {
    let len_i32 = len.min(i32::MAX as usize) as i32;
    let buf = crate::buffer::js_array_buffer_new(len_i32);
    if !buf.is_null() && !data.is_null() && len_i32 > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(
                data,
                crate::buffer::buffer_data_mut(buf),
                len_i32 as usize,
            );
        }
    }
    crate::value::js_nanbox_pointer(buf as i64)
}

fn make_module_object(module: *mut c_void) -> f64 {
    if module.is_null() {
        return nanbox_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj_handle = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 2));

    // String/key creation can collect and evacuate the fresh wrapper. Root
    // each value first, then reload the object before entering the generic
    // setter (which roots its arguments internally).
    let kind_key = scope.root_string_ptr(named_key(b"__wasmKind"));
    let kind_value = scope.root_nanbox_f64(string_value(b"module"));
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        kind_key.get_raw_const_ptr::<crate::string::StringHeader>(),
        kind_value.get_nanbox_f64(),
    );

    let ptr_key = scope.root_string_ptr(named_key(b"__wasmModulePtr"));
    crate::object::js_object_set_field_by_name(
        obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        ptr_key.get_raw_const_ptr::<crate::string::StringHeader>(),
        module as usize as f64,
    );

    let obj = obj_handle.get_raw_mut_ptr::<crate::object::ObjectHeader>();
    // Wrapper identity, not either public property, is the unforgeable brand
    // and the only route back to the trusted host handle.
    crate::object::register_wasm_module_wrapper(obj as usize, module as usize);
    object_value(obj)
}

fn extract_module_handle(module_jsval: f64) -> Option<*mut c_void> {
    let value = JSValue::from_bits(module_jsval.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let wrapper = value.as_pointer::<crate::object::ObjectHeader>() as usize;
    crate::object::registered_module_handle(wrapper).map(|handle| handle as *mut c_void)
}

fn extern_kind_name(kind: u8) -> &'static [u8] {
    match kind {
        WASM_EXTERN_KIND_FUNCTION => b"function",
        WASM_EXTERN_KIND_TABLE => b"table",
        WASM_EXTERN_KIND_MEMORY => b"memory",
        WASM_EXTERN_KIND_GLOBAL => b"global",
        _ => b"unknown",
    }
}

fn make_export_descriptor(name: *const c_char, name_len: usize, kind: u8) -> f64 {
    let mut obj = crate::object::js_object_alloc(0, 2);
    let name_bytes = if name.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(name as *const u8, name_len) }
    };
    obj = object_set_string(obj, b"name", name_bytes);
    obj = object_set_string(obj, b"kind", extern_kind_name(kind));
    object_value(obj)
}

fn make_import_descriptor(
    module: *const c_char,
    module_len: usize,
    name: *const c_char,
    name_len: usize,
    kind: u8,
) -> f64 {
    let mut obj = crate::object::js_object_alloc(0, 3);
    let module_bytes = if module.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(module as *const u8, module_len) }
    };
    let name_bytes = if name.is_null() {
        &[][..]
    } else {
        unsafe { std::slice::from_raw_parts(name as *const u8, name_len) }
    };
    obj = object_set_string(obj, b"module", module_bytes);
    obj = object_set_string(obj, b"name", name_bytes);
    obj = object_set_string(obj, b"kind", extern_kind_name(kind));
    object_value(obj)
}

fn empty_array_value() -> f64 {
    array_value(crate::array::js_array_alloc(0))
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

/// `new WebAssembly.Module(bytes)` — compile bytes and return a JS wrapper
/// around the host module handle. Per spec this constructor THROWS
/// synchronously: TypeError for a non-buffer argument, CompileError for
/// invalid bytes (#6558 — previously logged to stderr and returned
/// `undefined`, which crashed callers later at the first property read).
#[no_mangle]
pub extern "C" fn js_webassembly_module_new(bytes_jsval: f64) -> f64 {
    match module_new_value(bytes_jsval) {
        Ok(module) => module,
        Err(error) => crate::exception::js_throw(error),
    }
}

/// `WebAssembly.compile(bytes)` — async-standard shape, implemented as a
/// pre-resolved Promise over the same module wrapper used by the
/// constructor. Failures REJECT (never throw) with a `CompileError`-named
/// error carrying the host's message, per spec.
#[no_mangle]
pub extern "C" fn js_webassembly_compile(bytes_jsval: f64) -> f64 {
    match module_new_value(bytes_jsval) {
        Ok(module) => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let module = scope.root_nanbox_f64(module);
            let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
            crate::promise::js_promise_resolve(
                promise.get_raw_mut_ptr::<crate::promise::Promise>(),
                module.get_nanbox_f64(),
            );
            crate::value::js_nanbox_pointer(
                promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64
            )
        }
        Err(error) => rejected_promise_value(error),
    }
}

#[no_mangle]
pub extern "C" fn js_webassembly_module_exports(module_jsval: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module_jsval = scope.root_nanbox_f64(module_jsval);
    let Some(module) = extract_module_handle(module_jsval.get_nanbox_f64()) else {
        return empty_array_value();
    };
    let len = unsafe { perry_wasm_host_module_exports_len(module) };
    let arr = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(len as u32)));
    for i in 0..len {
        let mut name: *const c_char = std::ptr::null();
        let mut name_len = 0usize;
        let mut kind = 0u8;
        let ok = unsafe {
            perry_wasm_host_module_export_at(module, i, &mut name, &mut name_len, &mut kind)
        };
        if ok != 0 {
            let descriptor = scope.root_nanbox_f64(make_export_descriptor(name, name_len, kind));
            let arr_ptr = JSValue::from_bits(arr.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            let arr_ptr = crate::array::js_array_push_f64(arr_ptr, descriptor.get_nanbox_f64());
            arr.set_nanbox_f64(array_value(arr_ptr));
        }
    }
    arr.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_webassembly_module_imports(module_jsval: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module_jsval = scope.root_nanbox_f64(module_jsval);
    let Some(module) = extract_module_handle(module_jsval.get_nanbox_f64()) else {
        return empty_array_value();
    };
    let len = unsafe { perry_wasm_host_module_imports_len(module) };
    let arr = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(len as u32)));
    for i in 0..len {
        let mut module_name: *const c_char = std::ptr::null();
        let mut module_name_len = 0usize;
        let mut name: *const c_char = std::ptr::null();
        let mut name_len = 0usize;
        let mut kind = 0u8;
        let ok = unsafe {
            perry_wasm_host_module_import_at(
                module,
                i,
                &mut module_name,
                &mut module_name_len,
                &mut name,
                &mut name_len,
                &mut kind,
            )
        };
        if ok != 0 {
            let descriptor = scope.root_nanbox_f64(make_import_descriptor(
                module_name,
                module_name_len,
                name,
                name_len,
                kind,
            ));
            let arr_ptr = JSValue::from_bits(arr.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            let arr_ptr = crate::array::js_array_push_f64(arr_ptr, descriptor.get_nanbox_f64());
            arr.set_nanbox_f64(array_value(arr_ptr));
        }
    }
    arr.get_nanbox_f64()
}

#[no_mangle]
pub extern "C" fn js_webassembly_module_custom_sections(module_jsval: f64, name_jsval: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module_jsval = scope.root_nanbox_f64(module_jsval);
    let name_jsval = scope.root_nanbox_f64(name_jsval);
    let Some(module) = extract_module_handle(module_jsval.get_nanbox_f64()) else {
        return empty_array_value();
    };
    let Some((name_ptr, name_len)) = extract_string_bytes(name_jsval.get_nanbox_f64()) else {
        return empty_array_value();
    };
    // The result array and each section buffer may trigger a moving GC. Keep
    // the lookup bytes outside the GC heap instead of retaining a raw string
    // interior pointer across those allocations.
    let name = unsafe { std::slice::from_raw_parts(name_ptr, name_len) }.to_vec();
    let len = unsafe {
        perry_wasm_host_module_custom_sections_len(
            module,
            name.as_ptr() as *const c_char,
            name.len(),
        )
    };
    let arr = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(len as u32)));
    for i in 0..len {
        let mut data: *const u8 = std::ptr::null();
        let mut data_len = 0usize;
        let ok = unsafe {
            perry_wasm_host_module_custom_section_at(
                module,
                name.as_ptr() as *const c_char,
                name.len(),
                i,
                &mut data,
                &mut data_len,
            )
        };
        if ok != 0 {
            let section = scope.root_nanbox_f64(array_buffer_from_bytes(data, data_len));
            let arr_ptr = JSValue::from_bits(arr.get_nanbox_f64().to_bits())
                .as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            let arr_ptr = crate::array::js_array_push_f64(arr_ptr, section.get_nanbox_f64());
            arr.set_nanbox_f64(array_value(arr_ptr));
        }
    }
    arr.get_nanbox_f64()
}

// ── Zero-copy linear memory (#9611) ──────────────────────────────────────
//
// `WebAssembly.Memory.prototype.buffer` is a foreign-backed `ArrayBuffer`
// wrapper pointed straight at wasmi's linear memory, so neither direction
// copies: a JS store through `new Uint8Array(memory.buffer)` lands in wasm
// memory, and a wasm store is visible to JS with no synchronisation at all.
// Before this, every exported call memcpy'd the WHOLE linear memory in and
// then back out, which made one call cost 0.11 ns per byte of memory — 44,000x
// node once llhttp's memory had grown to a few MiB.
//
// The one thing that can invalidate the published span is `memory.grow`, which
// reallocates wasmi's backing `Vec` and can move it. Growth can only happen
// while wasm is executing, so the span is re-read (a pointer and a length
// compare, no copying) at the two boundaries where JS can observe it again:
// when an exported call returns, and when wasm calls back into a JS import.
struct MemoryBinding {
    /// `BufferHeader` currently published as `memory.buffer`.
    buffer: usize,
    /// The wasmi span that buffer is pointed at.
    data: usize,
    len: usize,
    /// The span moved mid-call and the published buffer was re-pointed at it
    /// in place. The buffer still has to be REPLACED when the call unwinds —
    /// node hands out a fresh `ArrayBuffer` after a grow and detaches the old
    /// one, which is the signal glue code (wasm-bindgen) uses to rebuild its
    /// cached views.
    rebound: bool,
}

crate::perry_thread_local! {
    /// `instance handle -> the memory span published to JS`. An entry lives as
    /// long as its instance, which today is the process: the runtime never
    /// calls `perry_wasm_host_instance_drop`, so an instance address is never
    /// recycled under a live binding.
    static WASM_MEMORY_BINDINGS: std::cell::RefCell<crate::fast_hash::PtrHashMap<usize, MemoryBinding>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// Instances with an exported call on the stack, innermost last. Only
    /// these can have grown their memory since JS last looked.
    static ACTIVE_WASM_INSTANCES: std::cell::RefCell<Vec<usize>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Pushes an instance onto [`ACTIVE_WASM_INSTANCES`] for the duration of one
/// exported call, popping it however the call leaves.
struct ActiveInstanceGuard;

impl ActiveInstanceGuard {
    fn enter(inst: *mut c_void) -> Self {
        ACTIVE_WASM_INSTANCES.with(|stack| stack.borrow_mut().push(inst as usize));
        ActiveInstanceGuard
    }
}

impl Drop for ActiveInstanceGuard {
    fn drop(&mut self) {
        ACTIVE_WASM_INSTANCES.with(|stack| {
            stack.borrow_mut().pop();
        });
    }
}

crate::perry_thread_local! {
    /// `import token -> the imports object the instance was instantiated with`.
    ///
    /// The token is a monotonic counter, NOT a heap address. That is the whole
    /// point: it is what the wasm host stores and hands back to
    /// [`call_wasm_import`], and the host has no way to learn that a JS object
    /// moved. Before this table the host stored the imports object's NaN-boxed
    /// bits directly, so a copying collection triggered inside one import
    /// callback left every later import in the same call reading a relocated
    /// object — the lookup failed, `call_wasm_import` returned 0, and the host
    /// substituted the import's default result, so wasm ran on with no error
    /// at all. Through undici's llhttp that silently dropped
    /// `on_message_complete`: a truncated HTTP response reported as a clean
    /// parse (found by the #9611 llhttp differential).
    static WASM_IMPORT_OBJECTS: std::cell::RefCell<crate::fast_hash::PtrHashMap<u64, f64>> =
        std::cell::RefCell::new(crate::fast_hash::new_ptr_hash_map());
    /// Source of import tokens. Starts at 1 so 0 is never a live token.
    static WASM_NEXT_IMPORT_TOKEN: std::cell::Cell<u64> = const { std::cell::Cell::new(1) };
}

/// Reserve a token for `imports` and record it. The token is what crosses the
/// C ABI into the host; the object itself stays on this side, where the
/// collector can see it.
fn register_instance_imports(imports: f64) -> u64 {
    let token = WASM_NEXT_IMPORT_TOKEN.with(|next| {
        let token = next.get();
        next.set(token.wrapping_add(1).max(1));
        token
    });
    WASM_IMPORT_OBJECTS.with(|objects| {
        objects.borrow_mut().insert(token, imports);
    });
    token
}

/// The imports object for `token`, or `undefined` when the token is unknown.
fn instance_imports(token: u64) -> f64 {
    WASM_IMPORT_OBJECTS.with(|objects| {
        objects
            .borrow()
            .get(&token)
            .copied()
            .unwrap_or_else(nanbox_undefined)
    })
}

/// Rewrite the imports objects in [`WASM_IMPORT_OBJECTS`] when a collection
/// relocates them. Registered in `gc_init`.
///
/// Rewrite-only, like [`scan_wasm_memory_binding_roots_mut`]: every path that
/// can reach an import already roots the imports object on the stack —
/// `call_captured_wasm_export` roots the export closure's capture for the
/// whole call, and instantiation roots it across the start function — so this
/// table is a lookup side table, not the reference that keeps the object
/// alive. Marking from here would instead pin the imports object of every
/// instance ever created, since entries live as long as their instance.
pub(crate) fn scan_wasm_import_object_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    WASM_IMPORT_OBJECTS.with(|objects| {
        for slot in objects.borrow_mut().values_mut() {
            visitor.visit_metadata_nanbox_f64_slot(slot);
        }
    });
}

/// Rewrite the published-buffer address in [`WASM_MEMORY_BINDINGS`] if the
/// collector ever relocates a `BufferHeader`. Registered in `gc_init`.
///
/// The address is a metadata KEY, not an ownership root, so this visits it
/// with `visit_metadata_usize_slot` — rewritten when forwarded, never marked.
/// Marking from here would be wrong twice over: the buffer is already strongly
/// held by the `WebAssembly.Memory` object's own `buffer` property for as long
/// as any export closure (and therefore any caller that can reach this table)
/// is alive, and an entry lives as long as its instance, so marking would pin
/// a buffer whose JS owner has died.
///
/// It rewrites nothing today. `GC_TYPE_BUFFER` is declared `movable: false`
/// (`gc/types.rs`), both old-page evacuation paths bail on
/// `!gc_type_is_movable` (`gc/oldgen.rs`), and `buffer_alloc_foreign` allocates
/// straight into the old arena as `TENURED`, so no copying minor sees one
/// either — the same guarantee `bun_ffi`'s pointer-lifetime contract already
/// rests on. It is registered anyway so that this table is not what breaks if
/// that flag is ever flipped, and so the holder is covered by a scanner rather
/// than by a verdict that would silently rot on the day it changed.
/// Drop bindings whose published `BufferHeader` did not survive the cycle.
///
/// The KEY is a native wasmi instance pointer, not a GC address, so it is the
/// VALUE that can die: a `WebAssembly.Memory` whose JS owner became unreachable
/// is swept while its instance entry lives on. Leaving the address behind is
/// the #8174 hazard — the slot is rewritten by `visit_metadata_usize_slot`, so
/// once that address is recycled by a movable object and forwarded, the stale
/// key would be silently re-pointed at an unrelated allocation.
///
/// A pruned instance simply has no published buffer until its next
/// `publish_memory_buffer`, which is the same state it holds before its first.
pub(crate) fn prune_dead_wasm_memory_bindings(is_dead_owner: &dyn Fn(usize) -> bool) {
    WASM_MEMORY_BINDINGS.with(|bindings| {
        bindings
            .borrow_mut()
            .retain(|_, b| !is_dead_owner(b.buffer));
    });
}

pub(crate) fn scan_wasm_memory_binding_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // Safe to take unconditionally: every allocating call in this module sits
    // OUTSIDE the `with` block that borrows this map, so a collection can
    // never land while the borrow is held.
    WASM_MEMORY_BINDINGS.with(|bindings| {
        for binding in bindings.borrow_mut().values_mut() {
            visitor.visit_metadata_usize_slot(&mut binding.buffer);
        }
    });
}

fn instance_memory_span(inst: *mut c_void) -> (*mut u8, usize) {
    let mut len = 0usize;
    let data = unsafe { perry_wasm_host_instance_memory_span(inst, &mut len) };
    if data.is_null() {
        return (std::ptr::null_mut(), 0);
    }
    // A `BufferHeader` length is an i32-domain byte count; a wasm32 memory can
    // in principle exceed that, and the excess simply stays invisible to JS
    // rather than wrapping the header's length.
    (data, len.min(i32::MAX as usize))
}

/// Allocate the `ArrayBuffer` that exposes `data[..len]` — wasmi's own linear
/// memory — and record it as the instance's published span.
fn publish_memory_buffer(inst: *mut c_void, data: *mut u8, len: usize) -> f64 {
    let buffer = crate::buffer::buffer_alloc_foreign(data, len as u32);
    if buffer.is_null() {
        return nanbox_undefined();
    }
    crate::buffer::mark_as_array_buffer(buffer as usize);
    WASM_MEMORY_BINDINGS.with(|bindings| {
        bindings.borrow_mut().insert(
            inst as usize,
            MemoryBinding {
                buffer: buffer as usize,
                data: data as usize,
                len,
                rebound: false,
            },
        );
    });
    crate::value::js_nanbox_pointer(buffer as i64)
}

/// Re-point every active instance's published buffer at its current linear
/// memory. Called before wasm re-enters JS through an imported function: wasm
/// may have grown the memory since the buffer was published, and the JS import
/// handler is about to read and write `memory.buffer` (this is exactly what
/// wasm-bindgen glue does), which without this would dereference the `Vec`
/// allocation the grow freed.
///
/// The published buffer keeps its identity here — swapping in a fresh
/// `ArrayBuffer` needs the `WebAssembly.Memory` object, which this path does
/// not hold — and the `rebound` flag defers that to the end of the call.
fn rebind_active_wasm_memories() {
    ACTIVE_WASM_INSTANCES.with(|stack| {
        let active = stack.borrow();
        for &inst in active.iter() {
            let (data, len) = instance_memory_span(inst as *mut c_void);
            if data.is_null() {
                continue;
            }
            WASM_MEMORY_BINDINGS.with(|bindings| {
                let mut bindings = bindings.borrow_mut();
                let Some(binding) = bindings.get_mut(&inst) else {
                    return;
                };
                if binding.data == data as usize && binding.len == len {
                    return;
                }
                if crate::buffer::rebind_foreign_buffer(binding.buffer, data, len as u32) {
                    binding.data = data as usize;
                    binding.len = len;
                    binding.rebound = true;
                }
            });
        }
    });
}

/// Bring `memory.buffer` back in sync with the instance's linear memory after
/// an exported call. In the overwhelmingly common case — the call did not grow
/// the memory — this is one pointer and one length compare and nothing else.
///
/// After a grow it does what node does: publish a FRESH `ArrayBuffer` over the
/// new span and detach the old one, so a JS view captured before the grow
/// reports `byteLength === 0` instead of aliasing a freed allocation.
fn resync_memory_after_call(inst: *mut c_void, memory: f64) {
    let (data, len) = instance_memory_span(inst);
    if data.is_null() {
        return;
    }
    let stale =
        WASM_MEMORY_BINDINGS.with(|bindings| match bindings.borrow().get(&(inst as usize)) {
            Some(binding) => binding.rebound || binding.data != data as usize || binding.len != len,
            None => true,
        });
    if !stale {
        return;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let memory = scope.root_nanbox_f64(memory);
    let memory_value = JSValue::from_bits(memory.get_nanbox_f64().to_bits());
    if !memory_value.is_pointer() {
        return;
    }
    let previous = WASM_MEMORY_BINDINGS
        .with(|bindings| bindings.borrow().get(&(inst as usize)).map(|b| b.buffer))
        .unwrap_or(0);
    let buffer = scope.root_nanbox_f64(publish_memory_buffer(inst, data, len));
    let memory_ptr = JSValue::from_bits(memory.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let _ = object_set(memory_ptr, b"buffer", buffer.get_nanbox_f64());
    let replacement = unbox_pointer(buffer.get_nanbox_f64()) as usize;
    if previous != 0 && previous != replacement {
        crate::buffer::detach_array_buffer(previous);
    }
}

fn wasm_import_value(
    context: u64,
    module: *const u8,
    module_len: usize,
    name: *const u8,
    name_len: usize,
) -> f64 {
    if module.is_null() || name.is_null() {
        return nanbox_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(instance_imports(context));
    if !JSValue::from_bits(imports.get_nanbox_f64().to_bits()).is_pointer() {
        return nanbox_undefined();
    }
    let module_bytes = unsafe { std::slice::from_raw_parts(module, module_len) };
    let module_key = scope.root_string_ptr(named_key(module_bytes));
    let module_value = scope.root_nanbox_f64(module_key.with_const_ptr(
        |k: *const crate::string::StringHeader| {
            let imports_value = JSValue::from_bits(imports.get_nanbox_f64().to_bits());
            crate::object::js_object_get_field_by_name_f64(
                imports_value.as_pointer::<crate::object::ObjectHeader>(),
                k,
            )
        },
    ));
    if !JSValue::from_bits(module_value.get_nanbox_f64().to_bits()).is_pointer() {
        return nanbox_undefined();
    }
    let name_bytes = unsafe { std::slice::from_raw_parts(name, name_len) };
    let name_key = scope.root_string_ptr(named_key(name_bytes));
    name_key.with_const_ptr(|k: *const crate::string::StringHeader| {
        let module_object = JSValue::from_bits(module_value.get_nanbox_f64().to_bits());
        crate::object::js_object_get_field_by_name_f64(
            module_object.as_pointer::<crate::object::ObjectHeader>(),
            k,
        )
    })
}

unsafe extern "C" fn resolve_wasm_import(
    context: u64,
    module: *const u8,
    module_len: usize,
    name: *const u8,
    name_len: usize,
    kind: u8,
) -> *mut c_void {
    let value = wasm_import_value(context, module, module_len, name, name_len);
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_pointer() {
        return std::ptr::null_mut();
    }
    let pointer = js.as_pointer::<u8>() as usize;
    if kind == WASM_EXTERN_KIND_FUNCTION {
        let Some(header) = crate::value::addr_class::try_read_gc_header(pointer) else {
            return std::ptr::null_mut();
        };
        if header.obj_type != crate::gc::GC_TYPE_CLOSURE {
            return std::ptr::null_mut();
        }
        let closure = pointer as *const crate::closure::ClosureHeader;
        let fp = (*closure).func_ptr;
        if is_wasm_export_call_shim(fp) {
            return crate::closure::js_closure_get_capture_f64(closure, 6) as usize as *mut c_void;
        }
        return std::ptr::null_mut();
    }
    let expected_kind: &[u8] = match kind {
        WASM_EXTERN_KIND_TABLE => b"table",
        WASM_EXTERN_KIND_MEMORY => b"memory",
        WASM_EXTERN_KIND_GLOBAL => b"global",
        _ => return std::ptr::null_mut(),
    };
    crate::object::registered_extern_handle(pointer, expected_kind)
        .map(|handle| handle as *mut c_void)
        .unwrap_or(std::ptr::null_mut())
}

unsafe extern "C" fn call_wasm_import(
    context: u64,
    module: *const u8,
    module_len: usize,
    name: *const u8,
    name_len: usize,
    arg_kinds: *const u8,
    arg_bits: *const u64,
    arg_count: usize,
    result_kinds: *const u8,
    result_bits: *mut u64,
    result_count: usize,
) -> i32 {
    if module.is_null()
        || name.is_null()
        || (arg_count != 0 && (arg_kinds.is_null() || arg_bits.is_null()))
        || (result_count != 0 && (result_kinds.is_null() || result_bits.is_null()))
    {
        return 0;
    }

    // wasm may have grown its memory before reaching this import, which
    // reallocates wasmi's backing store; the JS handler is about to read and
    // write `memory.buffer`, so re-point it at the live span first (#9611).
    rebind_active_wasm_memories();

    // `context` is an import TOKEN, not an object address: the host cannot be
    // told that a collection moved the imports object, so it never holds one.
    let scope = crate::gc::RuntimeHandleScope::new();
    let callback = scope.root_nanbox_f64(wasm_import_value(
        context, module, module_len, name, name_len,
    ));

    let kinds = if arg_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(arg_kinds, arg_count)
    };
    let bits = if arg_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(arg_bits, arg_count)
    };
    let args: Vec<f64> = kinds
        .iter()
        .zip(bits.iter())
        .map(|(kind, bits)| match *kind {
            WASM_VAL_KIND_I32 => (*bits as u32 as i32) as f64,
            WASM_VAL_KIND_I64 => wasm_i64_to_js(*bits),
            WASM_VAL_KIND_F32 => f32::from_bits(*bits as u32) as f64,
            WASM_VAL_KIND_F64 => f64::from_bits(*bits),
            WASM_VAL_KIND_EXTERNREF => f64::from_bits(*bits),
            _ => f64::from_bits(TAG_UNDEFINED),
        })
        .collect();
    let result =
        crate::closure::js_native_call_value(callback.get_nanbox_f64(), args.as_ptr(), args.len());

    let result_kinds = if result_count == 0 {
        &[]
    } else {
        std::slice::from_raw_parts(result_kinds, result_count)
    };
    let result_bits = if result_count == 0 {
        &mut []
    } else {
        std::slice::from_raw_parts_mut(result_bits, result_count)
    };
    for (kind, bits) in result_kinds.iter().zip(result_bits.iter_mut()) {
        *bits = match *kind {
            WASM_VAL_KIND_I32 => result as i32 as u32 as u64,
            WASM_VAL_KIND_I64 => js_to_wasm_i64_bits(result).unwrap_or(result as i64 as u64),
            WASM_VAL_KIND_F32 => (result as f32).to_bits() as u64,
            WASM_VAL_KIND_F64 => result.to_bits(),
            WASM_VAL_KIND_EXTERNREF => result.to_bits(),
            _ => 0,
        };
    }
    1
}

fn call_captured_wasm_export(closure: *const crate::closure::ClosureHeader, args: &[f64]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let inst = crate::closure::js_closure_get_capture_f64(closure, 0) as usize as *mut c_void;
    let name = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let memory = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
    let instance = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 3));
    let imports = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 4));
    let handle = crate::closure::js_closure_get_capture_f64(closure, 5) as usize;
    let external = crate::closure::js_closure_get_capture_f64(closure, 6) as usize as *mut c_void;
    // No per-call import-context store: the token the host holds was fixed at
    // instantiation and cannot go stale, which is the bug this replaced.
    let _ = &imports;
    let result = if inst.is_null() && !external.is_null() {
        call_external_function(external, args)
    } else {
        let _active = ActiveInstanceGuard::enter(inst);
        if handle != 0 {
            call_export_by_handle(inst, handle, args)
        } else {
            call_export_n(nanbox_pointer_raw(inst), name.get_nanbox_f64(), args)
        }
    };
    // A module with no exported memory has nothing to resync, and asking the
    // host for a span it does not have would put an FFI call back on a hot
    // path that no longer needs one.
    if JSValue::from_bits(memory.get_nanbox_f64().to_bits()).is_pointer() {
        resync_memory_after_call(inst, memory.get_nanbox_f64());
    }
    let mut exit_code = 0;
    if unsafe { perry_wasm_host_instance_take_exit_code(inst, &mut exit_code) } != 0 {
        let instance = unbox_pointer(instance.get_nanbox_f64()) as *mut crate::object::ObjectHeader;
        if !instance.is_null() {
            let _ = object_set(instance, b"__wasiProcExitCode", exit_code as f64);
        }
        exit_code as f64
    } else {
        result
    }
}

fn make_export_function(
    inst: *mut c_void,
    name: &[u8],
    arity: usize,
    memory: f64,
    instance: f64,
    imports: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let memory = scope.root_nanbox_f64(memory);
    let instance = scope.root_nanbox_f64(instance);
    let imports = scope.root_nanbox_f64(imports);
    let (func_ptr, declared_arity) = wasm_export_call_shim_for_arity(arity);
    // Resolve the export ONCE here rather than by name on every call: the
    // per-call `get_func` probe plus `FuncType` clone was the sub-microsecond
    // floor left under the linear-memory copy this binding removed (#9611).
    let handle = unsafe {
        perry_wasm_host_instance_export_handle(inst, name.as_ptr() as *const c_char, name.len())
    };
    let external = unsafe {
        perry_wasm_host_instance_export_extern(inst, name.as_ptr() as *const c_char, name.len())
    };
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 7));
    if closure
        .get_raw_mut_ptr::<crate::closure::ClosureHeader>()
        .is_null()
    {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    crate::closure::js_register_closure_arity(func_ptr, declared_arity);
    let name_value = scope.root_nanbox_f64(string_value(name));
    let closure_ptr = closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>();
    crate::closure::js_closure_set_capture_f64(closure_ptr, 0, inst as usize as f64);
    crate::closure::js_closure_set_capture_f64(closure_ptr, 1, name_value.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure_ptr, 2, memory.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure_ptr, 3, instance.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure_ptr, 4, imports.get_nanbox_f64());
    crate::closure::js_closure_set_capture_f64(closure_ptr, 5, handle as f64);
    crate::closure::js_closure_set_capture_f64(closure_ptr, 6, external as usize as f64);
    crate::object::set_bound_native_closure_name(
        closure_ptr,
        std::str::from_utf8(name).unwrap_or("wasm"),
    );
    let closure_ptr = closure.get_raw_mut_ptr::<crate::closure::ClosureHeader>();
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}

fn table_method_context<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    closure: *const crate::closure::ClosureHeader,
) -> (
    *mut c_void,
    *mut c_void,
    crate::gc::RuntimeHandle<'scope>,
    crate::gc::RuntimeHandle<'scope>,
) {
    let external = crate::closure::js_closure_get_capture_f64(closure, 0) as usize as *mut c_void;
    let inst = crate::closure::js_closure_get_capture_f64(closure, 1) as usize as *mut c_void;
    let name = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
    let table = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 3));
    (external, inst, name, table)
}

fn table_values(table: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let table = scope.root_nanbox_f64(table);
    let table_value = JSValue::from_bits(table.get_nanbox_f64().to_bits());
    if !table_value.is_pointer() {
        return nanbox_undefined();
    }
    let key = scope.root_string_ptr(named_key(b"__wasmValues"));
    key.with_const_ptr(|key: *const crate::string::StringHeader| {
        crate::object::js_object_get_field_by_name_f64(
            JSValue::from_bits(table.get_nanbox_f64().to_bits())
                .as_pointer::<crate::object::ObjectHeader>(),
            key,
        )
    })
}

fn wasm_function_external(value: f64) -> *mut c_void {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return std::ptr::null_mut();
    }
    let closure = value.as_pointer::<crate::closure::ClosureHeader>();
    let Some(header) = (unsafe { crate::value::addr_class::try_read_gc_header(closure as usize) })
    else {
        return std::ptr::null_mut();
    };
    if header.obj_type != crate::gc::GC_TYPE_CLOSURE {
        return std::ptr::null_mut();
    }
    let fp = unsafe { (*closure).func_ptr };
    if !is_wasm_export_call_shim(fp) {
        return std::ptr::null_mut();
    }
    crate::closure::js_closure_get_capture_f64(closure, 6) as usize as *mut c_void
}

extern "C" fn js_wasm_table_get(closure: *const crate::closure::ClosureHeader, index: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (external, inst, name, table) = table_method_context(&scope, closure);
    let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
    let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
    if !values_value.is_pointer() || !index.is_finite() || index < 0.0 {
        return nanbox_undefined();
    }
    let index = index as usize;
    let cached = crate::array::js_array_get_f64(
        values_value.as_pointer::<crate::array::ArrayHeader>(),
        index as u32,
    );
    if cached.to_bits() != crate::value::TAG_NULL {
        return cached;
    }
    let mut bits = 0u64;
    let mut is_null = 0i32;
    let mut function_external = std::ptr::null_mut();
    let ok = if inst.is_null() {
        unsafe {
            perry_wasm_host_table_get(
                external,
                index,
                &mut bits,
                &mut is_null,
                &mut function_external,
            )
        }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe {
            perry_wasm_host_instance_table_get(
                inst,
                name_ptr.cast(),
                name_len,
                index,
                &mut bits,
                &mut is_null,
                &mut function_external,
            )
        }
    } else {
        0
    };
    if ok == 0 {
        return nanbox_undefined();
    }
    let value = if is_null != 0 {
        f64::from_bits(crate::value::TAG_NULL)
    } else if !function_external.is_null() {
        make_table_function(function_external)
    } else {
        f64::from_bits(bits)
    };
    let values_ptr = JSValue::from_bits(values.get_nanbox_f64().to_bits())
        .as_pointer::<crate::array::ArrayHeader>()
        as *mut crate::array::ArrayHeader;
    crate::array::js_array_set_f64(values_ptr, index as u32, value);
    value
}

extern "C" fn js_wasm_table_set(
    closure: *const crate::closure::ClosureHeader,
    index: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (external, inst, name, table) = table_method_context(&scope, closure);
    let is_null = (value.to_bits() == crate::value::TAG_NULL) as i32;
    let index = index.max(0.0) as usize;
    let function = wasm_function_external(value);
    let ok = if inst.is_null() {
        unsafe { perry_wasm_host_table_set(external, index, value.to_bits(), is_null, function) }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe {
            perry_wasm_host_instance_table_set(
                inst,
                name_ptr.cast(),
                name_len,
                index,
                value.to_bits(),
                is_null,
                function,
            )
        }
    } else {
        0
    };
    if ok != 0 {
        let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
        let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
        if values_value.is_pointer() {
            let values_ptr = values_value.as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            crate::array::js_array_set_f64(values_ptr, index as u32, value);
        }
    } else {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Table.set(): value is not a WebAssembly function",
        ));
    }
    nanbox_undefined()
}

extern "C" fn js_wasm_table_grow(
    closure: *const crate::closure::ClosureHeader,
    delta: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (external, inst, name, table) = table_method_context(&scope, closure);
    let value_bits = value.to_bits();
    let is_null = matches!(
        value_bits,
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) as i32;
    let mut old_len = 0usize;
    let delta = delta.max(0.0) as usize;
    let function = wasm_function_external(value);
    let ok = if inst.is_null() {
        unsafe {
            perry_wasm_host_table_grow(external, delta, value_bits, is_null, function, &mut old_len)
        }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe {
            perry_wasm_host_instance_table_grow(
                inst,
                name_ptr.cast(),
                name_len,
                delta,
                value_bits,
                is_null,
                function,
                &mut old_len,
            )
        }
    } else {
        0
    };
    if ok == 0 {
        return nanbox_undefined();
    }
    let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
    let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
    if !values_value.is_pointer() {
        return nanbox_undefined();
    }
    let mut values_ptr =
        values_value.as_pointer::<crate::array::ArrayHeader>() as *mut crate::array::ArrayHeader;
    let fill = if is_null != 0 {
        f64::from_bits(crate::value::TAG_NULL)
    } else {
        value
    };
    for _ in 0..delta {
        values_ptr = crate::array::js_array_push_f64(values_ptr, fill);
        values.set_nanbox_f64(array_value(values_ptr));
    }
    let table_value = JSValue::from_bits(table.get_nanbox_f64().to_bits());
    if table_value.is_pointer() {
        let _ = object_set(
            table_value.as_pointer::<crate::object::ObjectHeader>()
                as *mut crate::object::ObjectHeader,
            b"length",
            old_len.saturating_add(delta) as f64,
        );
    }
    old_len as f64
}

fn make_table_method(
    external: *mut c_void,
    inst: *mut c_void,
    name: f64,
    table: f64,
    func_ptr: *const u8,
    arity: u32,
    display_name: &str,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    let table = scope.root_nanbox_f64(table);
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 4));
    if closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| closure.is_null()) {
        return nanbox_undefined();
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 0, external as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 1, inst as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 2, name.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 3, table.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, display_name)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

fn make_table_function(external: *mut c_void) -> f64 {
    let arity = unsafe { perry_wasm_host_func_arity(external) };
    if arity == usize::MAX {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    let (func_ptr, declared_arity) = wasm_export_call_shim_for_arity(arity);
    let scope = crate::gc::RuntimeHandleScope::new();
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 7));
    if closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| closure.is_null()) {
        drop_host_extern_handle(external as usize);
        return nanbox_undefined();
    }
    crate::closure::js_register_closure_arity(func_ptr, declared_arity);
    for index in 0..6 {
        closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
            crate::closure::js_closure_set_capture_f64(closure, index, nanbox_undefined())
        });
    }
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 0, 0.0)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 5, 0.0)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 6, external as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, "wasm-table-function")
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::register_wasm_funcref_external(closure as usize, external as usize);
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

fn make_table_object(external: *mut c_void, inst: *mut c_void, name: f64, receiver: f64) -> f64 {
    if external.is_null() {
        return nanbox_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    let receiver = JSValue::from_bits(receiver.to_bits());
    let table_ptr = if receiver.is_pointer() {
        receiver.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader
    } else {
        crate::object::js_object_alloc(0, 0)
    };
    let table = scope.root_nanbox_f64(object_value(table_ptr));
    let len = if inst.is_null() {
        unsafe { perry_wasm_host_table_len(external) }
    } else if let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) {
        unsafe { perry_wasm_host_instance_table_len(inst, name_ptr.cast(), name_len) }
    } else {
        usize::MAX
    };
    if len == usize::MAX {
        return nanbox_undefined();
    }
    let values = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(len as u32)));
    for _ in 0..len {
        let values_ptr = JSValue::from_bits(values.get_nanbox_f64().to_bits())
            .as_pointer::<crate::array::ArrayHeader>()
            as *mut crate::array::ArrayHeader;
        let values_ptr =
            crate::array::js_array_push_f64(values_ptr, f64::from_bits(crate::value::TAG_NULL));
        values.set_nanbox_f64(array_value(values_ptr));
    }
    let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let _ = object_set(table_ptr, b"__wasmValues", values.get_nanbox_f64());
    let methods = [
        ("get", js_wasm_table_get as *const u8, 1u32),
        ("grow", js_wasm_table_grow as *const u8, 2u32),
        ("set", js_wasm_table_set as *const u8, 2u32),
    ];
    for (method_name, func_ptr, arity) in methods {
        let method = scope.root_nanbox_f64(make_table_method(
            external,
            inst,
            name.get_nanbox_f64(),
            table.get_nanbox_f64(),
            func_ptr,
            arity,
            method_name,
        ));
        let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
            .as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader;
        let _ = object_set(table_ptr, method_name.as_bytes(), method.get_nanbox_f64());
    }
    let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let _ = object_set(table_ptr, b"length", len as f64);
    let table_ptr = JSValue::from_bits(table.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>();
    crate::object::register_wasm_extern_wrapper(table_ptr as usize, b"table", external as usize);
    table.get_nanbox_f64()
}

fn make_export_table(inst: *mut c_void, name: &[u8]) -> f64 {
    let name_value = string_value(name);
    let external =
        unsafe { perry_wasm_host_instance_export_extern(inst, name.as_ptr().cast(), name.len()) };
    make_table_object(external, inst, name_value, nanbox_undefined())
}

fn global_handle_from_receiver() -> Option<*mut c_void> {
    let receiver = JSValue::from_bits(crate::object::js_implicit_this_get().to_bits());
    if !receiver.is_pointer() {
        return None;
    }
    crate::object::registered_extern_handle(
        receiver.as_pointer::<crate::object::ObjectHeader>() as usize,
        b"global",
    )
    .map(|handle| handle as *mut c_void)
}

extern "C" fn js_wasm_global_get(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let Some(handle) = global_handle_from_receiver() else {
        return nanbox_undefined();
    };
    let mut kind = WASM_VAL_KIND_NONE;
    let mut bits = 0u64;
    if unsafe { perry_wasm_host_global_get(handle, &mut kind, &mut bits) } == 0 {
        return nanbox_undefined();
    }
    decode_wasm_value(kind, bits)
}

extern "C" fn js_wasm_global_set(
    _closure: *const crate::closure::ClosureHeader,
    value: f64,
) -> f64 {
    let Some(handle) = global_handle_from_receiver() else {
        return nanbox_undefined();
    };
    let mut kind = WASM_VAL_KIND_NONE;
    let mut previous = 0u64;
    if unsafe { perry_wasm_host_global_get(handle, &mut kind, &mut previous) } == 0 {
        return nanbox_undefined();
    }
    let bits = match kind {
        WASM_VAL_KIND_I32 => value as i32 as u32 as u64,
        WASM_VAL_KIND_I64 => js_to_wasm_i64_bits(value).unwrap_or(value as i64 as u64),
        WASM_VAL_KIND_F32 => (value as f32).to_bits() as u64,
        WASM_VAL_KIND_F64 => value.to_bits(),
        _ => return nanbox_undefined(),
    };
    let _ = unsafe { perry_wasm_host_global_set(handle, kind, bits) };
    nanbox_undefined()
}

fn make_global_object(external: *mut c_void, receiver: f64) -> f64 {
    if external.is_null() {
        return nanbox_undefined();
    }
    let receiver_value = JSValue::from_bits(receiver.to_bits());
    let object = if receiver_value.is_pointer() {
        receiver_value.as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader
    } else {
        crate::object::js_object_alloc(0, 0)
    };
    if object.is_null() {
        return nanbox_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let object = scope.root_raw_mut_ptr(object);
    let placeholder = scope.root_string_ptr(named_key(b"value"));
    crate::object::js_object_set_field_by_name(
        object.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        placeholder.get_raw_const_ptr::<crate::string::StringHeader>(),
        nanbox_undefined(),
    );
    let getter_fp = js_wasm_global_get as *const u8;
    let setter_fp = js_wasm_global_set as *const u8;
    crate::closure::js_register_closure_arity(getter_fp, 0);
    crate::closure::js_register_closure_arity(setter_fp, 1);
    let getter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(getter_fp, 0));
    let setter = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(setter_fp, 0));
    let object_ptr = object.get_raw_mut_ptr::<crate::object::ObjectHeader>();
    crate::object::set_builtin_accessor_descriptor(
        object_ptr as usize,
        "value".to_string(),
        crate::object::AccessorDescriptor {
            get: crate::value::js_nanbox_pointer(
                getter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            )
            .to_bits(),
            set: crate::value::js_nanbox_pointer(
                setter.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
            )
            .to_bits(),
        },
        crate::object::PropertyAttrs::new(true, false, false),
    );
    crate::object::register_wasm_extern_wrapper(object_ptr as usize, b"global", external as usize);
    object_value(object_ptr)
}

fn make_export_global(inst: *mut c_void, name: &[u8]) -> f64 {
    let external =
        unsafe { perry_wasm_host_instance_export_extern(inst, name.as_ptr().cast(), name.len()) };
    make_global_object(external, nanbox_undefined())
}

fn descriptor_object(value: f64) -> Option<*mut crate::object::ObjectHeader> {
    let value = JSValue::from_bits(value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let object =
        value.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    let header = unsafe { crate::value::addr_class::try_read_gc_header(object as usize)? };
    (header.obj_type == crate::gc::GC_TYPE_OBJECT).then_some(object)
}

fn descriptor_field(object: *mut crate::object::ObjectHeader, name: &[u8]) -> f64 {
    crate::object::js_object_get_field_by_name_f64(object, named_key(name))
}

fn descriptor_string(object: *mut crate::object::ObjectHeader, name: &[u8]) -> Option<Vec<u8>> {
    let value = descriptor_field(object, name);
    let (ptr, len) = extract_string_bytes(value)?;
    Some(unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec())
}

fn wasm_numeric_kind(name: &[u8]) -> Option<u8> {
    match name {
        b"i32" => Some(WASM_VAL_KIND_I32),
        b"i64" => Some(WASM_VAL_KIND_I64),
        b"f32" => Some(WASM_VAL_KIND_F32),
        b"f64" => Some(WASM_VAL_KIND_F64),
        _ => None,
    }
}

#[no_mangle]
pub extern "C" fn js_webassembly_table_new(descriptor: f64, receiver: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let descriptor = scope.root_nanbox_f64(descriptor);
    let receiver = scope.root_nanbox_f64(receiver);
    let Some(object) = descriptor_object(descriptor.get_nanbox_f64()) else {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Table(): argument must be a table descriptor object",
        ));
    };
    let element = match descriptor_string(object, b"element").as_deref() {
        Some(b"anyfunc" | b"funcref") => 0,
        Some(b"externref") => 1,
        _ => crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Table(): descriptor property 'element' must be anyfunc, funcref, or externref",
        )),
    };
    let initial = JSValue::from_bits(descriptor_field(object, b"initial").to_bits()).to_number();
    if !initial.is_finite() || initial < 0.0 || initial > u32::MAX as f64 {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Table(): descriptor property 'initial' must be a non-negative number",
        ));
    }
    let maximum_value = JSValue::from_bits(descriptor_field(object, b"maximum").to_bits());
    let maximum = if maximum_value.is_undefined() {
        u32::MAX
    } else {
        let maximum = maximum_value.to_number();
        if !maximum.is_finite()
            || maximum < initial.trunc()
            || maximum < 0.0
            || maximum > u32::MAX as f64
        {
            crate::exception::js_throw(wasm_type_error_value(
                "WebAssembly.Table(): 'maximum' must be at least 'initial'",
            ));
        }
        maximum.trunc() as u32
    };
    let external = unsafe { perry_wasm_host_table_new(element, initial.trunc() as u32, maximum) };
    if external.is_null() {
        crate::exception::js_throw(wasm_error_value_from_host(
            b"RuntimeError",
            std::ptr::null_mut(),
            "WebAssembly.Table(): allocation failed",
        ));
    }
    make_table_object(
        external,
        std::ptr::null_mut(),
        nanbox_undefined(),
        receiver.get_nanbox_f64(),
    )
}

#[no_mangle]
pub extern "C" fn js_webassembly_global_new(descriptor: f64, initial: f64, receiver: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let descriptor = scope.root_nanbox_f64(descriptor);
    let receiver = scope.root_nanbox_f64(receiver);
    let Some(object) = descriptor_object(descriptor.get_nanbox_f64()) else {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Global(): argument must be a global descriptor object",
        ));
    };
    let Some(kind) = descriptor_string(object, b"value").and_then(|name| wasm_numeric_kind(&name))
    else {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Global(): descriptor property 'value' must be a numeric value type",
        ));
    };
    let mutable = crate::value::js_is_truthy(descriptor_field(object, b"mutable")) != 0;
    let bits = match kind {
        WASM_VAL_KIND_I32 => initial as i32 as u32 as u64,
        WASM_VAL_KIND_I64 => js_to_wasm_i64_bits(initial).unwrap_or(initial as i64 as u64),
        WASM_VAL_KIND_F32 => (initial as f32).to_bits() as u64,
        WASM_VAL_KIND_F64 => initial.to_bits(),
        _ => 0,
    };
    let external = unsafe { perry_wasm_host_global_new(kind, mutable as i32, bits) };
    if external.is_null() {
        crate::exception::js_throw(wasm_error_value_from_host(
            b"RuntimeError",
            std::ptr::null_mut(),
            "WebAssembly.Global(): allocation failed",
        ));
    }
    make_global_object(external, receiver.get_nanbox_f64())
}

#[no_mangle]
pub extern "C" fn js_webassembly_memory_new(initial: u32, maximum: u32, receiver: f64) -> f64 {
    let external = unsafe { perry_wasm_host_memory_new(initial, maximum) };
    if external.is_null() {
        crate::exception::js_throw(wasm_error_value_from_host(
            b"RuntimeError",
            std::ptr::null_mut(),
            "WebAssembly.Memory(): allocation failed",
        ));
    }
    let mut len = 0usize;
    let data = unsafe { perry_wasm_host_memory_span(external, &mut len) };
    let buffer = crate::buffer::buffer_alloc_foreign(data, len.min(u32::MAX as usize) as u32);
    if buffer.is_null() {
        return nanbox_undefined();
    }
    crate::buffer::mark_as_array_buffer(buffer as usize);
    let receiver_value = JSValue::from_bits(receiver.to_bits());
    let object = if receiver_value.is_pointer() {
        receiver_value.as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader
    } else {
        crate::object::js_object_alloc(0, 0)
    };
    let object = object_set(
        object,
        b"buffer",
        crate::value::js_nanbox_pointer(buffer as i64),
    );
    crate::object::register_wasm_extern_wrapper(object as usize, b"memory", external as usize);
    object_value(object)
}

#[no_mangle]
pub extern "C" fn js_webassembly_memory_grow(memory_value: f64, delta: u32) -> f64 {
    let value = JSValue::from_bits(memory_value.to_bits());
    if !value.is_pointer() {
        return -1.0;
    }
    let object =
        value.as_pointer::<crate::object::ObjectHeader>() as *mut crate::object::ObjectHeader;
    let Some(handle) = crate::object::registered_extern_handle(object as usize, b"memory") else {
        return -1.0;
    };
    let old_pages = unsafe { perry_wasm_host_memory_grow(handle as *mut c_void, delta) };
    if old_pages < 0 {
        return -1.0;
    }
    let previous = crate::object::js_object_get_field_by_name_f64(object, named_key(b"buffer"));
    let previous = unbox_pointer(previous) as usize;
    let mut len = 0usize;
    let data = unsafe { perry_wasm_host_memory_span(handle as *mut c_void, &mut len) };
    let buffer = crate::buffer::buffer_alloc_foreign(data, len.min(u32::MAX as usize) as u32);
    if buffer.is_null() {
        return -1.0;
    }
    crate::buffer::mark_as_array_buffer(buffer as usize);
    let _ = object_set(
        object,
        b"buffer",
        crate::value::js_nanbox_pointer(buffer as i64),
    );
    if previous != 0 && previous != buffer as usize {
        crate::buffer::detach_array_buffer(previous);
    }
    old_pages as f64
}

fn make_instance_value(module: *mut c_void, inst: *mut c_void, imports: f64, receiver: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(imports);
    let (memory_data, memory_len) = instance_memory_span(inst);
    let memory = if memory_data.is_null() {
        scope.root_nanbox_f64(nanbox_undefined())
    } else {
        let buffer = scope.root_nanbox_f64(publish_memory_buffer(inst, memory_data, memory_len));
        let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let object = object.with_mut_ptr(|object: *mut crate::object::ObjectHeader| {
            object_set(object, b"buffer", buffer.get_nanbox_f64())
        });
        let object_value = scope.root_nanbox_f64(object_value(object));
        let external =
            unsafe { perry_wasm_host_instance_export_extern(inst, b"memory".as_ptr().cast(), 6) };
        crate::object::register_wasm_extern_wrapper(object as usize, b"memory", external as usize);
        object_value
    };
    // `new WebAssembly.Instance(...)` arrives with a receiver whose
    // [[Prototype]] was already linked to `WebAssembly.Instance.prototype` by
    // the generic construct path. Populate that object in place so
    // `instanceof WebAssembly.Instance` keeps working. The static
    // `WebAssembly.instantiate(...)` path passes `undefined` and gets a fresh
    // ordinary wrapper instead.
    let receiver_value = JSValue::from_bits(receiver.to_bits());
    let instance = scope.root_nanbox_f64(if receiver_value.is_pointer() {
        receiver
    } else {
        object_value(crate::object::js_object_alloc(0, 0))
    });
    let exports = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let exports_len = unsafe { perry_wasm_host_module_exports_len(module) };
    for index in 0..exports_len {
        let mut name: *const c_char = std::ptr::null();
        let mut name_len = 0usize;
        let mut kind = 0u8;
        if unsafe {
            perry_wasm_host_module_export_at(module, index, &mut name, &mut name_len, &mut kind)
        } == 0
            || name.is_null()
        {
            continue;
        }
        let name = unsafe { std::slice::from_raw_parts(name as *const u8, name_len) };
        let value = scope.root_nanbox_f64(match kind {
            WASM_EXTERN_KIND_FUNCTION => make_export_function(
                inst,
                name,
                unsafe { perry_wasm_host_module_export_func_arity(module, index) },
                memory.get_nanbox_f64(),
                instance.get_nanbox_f64(),
                imports.get_nanbox_f64(),
            ),
            WASM_EXTERN_KIND_MEMORY => memory.get_nanbox_f64(),
            WASM_EXTERN_KIND_TABLE => make_export_table(inst, name),
            WASM_EXTERN_KIND_GLOBAL => make_export_global(inst, name),
            _ => nanbox_undefined(),
        });
        let exports_ptr = object_set(
            exports.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
            name,
            value.get_nanbox_f64(),
        );
        exports.set_raw_mut_ptr(exports_ptr);
    }
    let instance_ptr = JSValue::from_bits(instance.get_nanbox_f64().to_bits())
        .as_pointer::<crate::object::ObjectHeader>()
        as *mut crate::object::ObjectHeader;
    let instance_ptr = object_set(
        instance_ptr,
        b"exports",
        object_value(exports.get_raw_mut_ptr::<crate::object::ObjectHeader>()),
    );
    instance.set_nanbox_f64(object_value(instance_ptr));

    instance.get_nanbox_f64()
}

/// `new WebAssembly.Instance(module, imports?)` — synchronously instantiate a
/// previously compiled module. This is the shape emitted by wasm-bindgen's
/// Node glue (including `@silvia-odwyer/photon-node`). Unlike the async
/// `WebAssembly.instantiate(bytes, imports)` API, constructor failures throw a
/// `TypeError`/`LinkError` directly.
#[no_mangle]
pub extern "C" fn js_webassembly_instance_new(
    module_jsval: f64,
    imports_jsval: f64,
    receiver_jsval: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module_value = scope.root_nanbox_f64(module_jsval);
    let imports = scope.root_nanbox_f64(imports_jsval);
    let receiver = scope.root_nanbox_f64(receiver_jsval);
    let Some(module) = extract_module_handle(module_value.get_nanbox_f64()) else {
        crate::exception::js_throw(wasm_type_error_value(
            "WebAssembly.Instance(): first argument must be a WebAssembly.Module",
        ));
    };

    let mut err: *mut c_char = std::ptr::null_mut();
    let inst = unsafe {
        perry_wasm_host_instance_new(
            module,
            Some(call_wasm_import),
            Some(resolve_wasm_import),
            register_instance_imports(imports.get_nanbox_f64()),
            &mut err,
        )
    };
    if inst.is_null() {
        crate::exception::js_throw(wasm_error_value_from_host(
            b"LinkError",
            err,
            "WebAssembly.Instance(): instantiation failed",
        ));
    }

    make_instance_value(
        module,
        inst,
        imports.get_nanbox_f64(),
        receiver.get_nanbox_f64(),
    )
}

/// `WebAssembly.instantiate(source, imports?)` resolves to an `Instance` for a
/// compiled `Module`, or the standard `{ module, instance }` result for bytes.
/// Imported numeric functions are resolved from the JS imports object by
/// module/name and called synchronously by the wasmi host.
#[no_mangle]
pub extern "C" fn js_webassembly_instantiate(bytes_jsval: f64, imports_jsval: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(imports_jsval);
    if let Some(module) = extract_module_handle(bytes_jsval) {
        let mut err: *mut c_char = std::ptr::null_mut();
        let inst = unsafe {
            perry_wasm_host_instance_new(
                module,
                Some(call_wasm_import),
                Some(resolve_wasm_import),
                register_instance_imports(imports.get_nanbox_f64()),
                &mut err,
            )
        };
        if inst.is_null() {
            return rejected_promise_value(wasm_error_value_from_host(
                b"LinkError",
                err,
                "WebAssembly.instantiate(): instantiation failed",
            ));
        }
        let instance = scope.root_nanbox_f64(make_instance_value(
            module,
            inst,
            imports.get_nanbox_f64(),
            nanbox_undefined(),
        ));
        let promise = scope.root_raw_mut_ptr(crate::promise::js_promise_new());
        crate::promise::js_promise_resolve(
            promise.get_raw_mut_ptr::<crate::promise::Promise>(),
            instance.get_nanbox_f64(),
        );
        return crate::value::js_nanbox_pointer(
            promise.get_raw_mut_ptr::<crate::promise::Promise>() as i64,
        );
    }
    let Some((ptr, len)) = extract_bytes(bytes_jsval) else {
        return rejected_promise_value(wasm_type_error_value(
            "WebAssembly.instantiate: argument must be a Uint8Array or ArrayBuffer",
        ));
    };
    let mut err: *mut c_char = std::ptr::null_mut();
    let module = unsafe { perry_wasm_host_module_new(ptr, len, &mut err) };
    if module.is_null() {
        return rejected_promise_value(wasm_error_value_from_host(
            b"CompileError",
            err,
            "WebAssembly.instantiate(): compile failed",
        ));
    }
    let mut err2: *mut c_char = std::ptr::null_mut();
    let inst = unsafe {
        perry_wasm_host_instance_new(
            module,
            Some(call_wasm_import),
            Some(resolve_wasm_import),
            register_instance_imports(imports.get_nanbox_f64()),
            &mut err2,
        )
    };
    if inst.is_null() {
        unsafe { perry_wasm_host_module_drop(module) };
        return rejected_promise_value(wasm_error_value_from_host(
            b"LinkError",
            err2,
            "WebAssembly.instantiate(): instantiation failed",
        ));
    }
    make_instance_result(module, inst, imports.get_nanbox_f64())
}

/// `WebAssembly.callExport(handle, name, ...args)` — invoke an exported
/// function by name with numeric arguments. Currently supports up to 4
/// numeric args, mirroring the closure-call ABI in `closure.rs`. All
/// arguments and the return value are passed as f64; the runtime infers
/// the wasm signature from the export type and widens/narrows as needed.
///
/// Args > 4 are silently truncated in this MVP — the codegen-side wiring
/// only routes 0-4 args anyway.
/// `WebAssembly.callExport(...)` — the ahead-of-time lowering
/// (`Expr::WebAssemblyCallExport`), which reaches an export without the
/// closure that owns the instance's `WebAssembly.Memory` object. The call can
/// still grow the memory, so the published buffer is re-pointed at the live
/// span in place; only the closure path can additionally hand out a fresh
/// `ArrayBuffer` (it is the one that holds the memory object).
fn call_export_named_rebinding(inst_jsval: f64, name_jsval: f64, args: &[f64]) -> f64 {
    let _active = ActiveInstanceGuard::enter(unbox_pointer(inst_jsval));
    let result = call_export_n(inst_jsval, name_jsval, args);
    rebind_active_wasm_memories();
    result
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_0(inst_jsval: f64, name_jsval: f64) -> f64 {
    call_export_named_rebinding(inst_jsval, name_jsval, &[])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_1(inst_jsval: f64, name_jsval: f64, a: f64) -> f64 {
    call_export_named_rebinding(inst_jsval, name_jsval, &[a])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_2(
    inst_jsval: f64,
    name_jsval: f64,
    a: f64,
    b: f64,
) -> f64 {
    call_export_named_rebinding(inst_jsval, name_jsval, &[a, b])
}

#[no_mangle]
pub extern "C" fn js_webassembly_call_export_3(
    inst_jsval: f64,
    name_jsval: f64,
    a: f64,
    b: f64,
    c: f64,
) -> f64 {
    call_export_named_rebinding(inst_jsval, name_jsval, &[a, b, c])
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
    call_export_named_rebinding(inst_jsval, name_jsval, &[a, b, c, d])
}
