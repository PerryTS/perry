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
//! Numeric args only (i32/i64/f32/f64). Standard surface tracked as
//! follow-up work in the issue thread.

use std::ffi::{c_char, c_void};

use crate::value::{JSValue, TAG_UNDEFINED};

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
const WASM_EXTERN_KIND_FUNCTION: u8 = 0;
const WASM_EXTERN_KIND_TABLE: u8 = 1;
const WASM_EXTERN_KIND_MEMORY: u8 = 2;
const WASM_EXTERN_KIND_GLOBAL: u8 = 3;

type WasmImportCallback = unsafe extern "C" fn(
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
) -> i32;

extern "C" {
    fn perry_wasm_host_string_free(s: *mut c_char);
    fn perry_wasm_host_validate(bytes: *const u8, len: usize) -> i32;
    fn perry_wasm_host_module_new(
        bytes: *const u8,
        len: usize,
        out_err: *mut *mut c_char,
    ) -> *mut c_void;
    fn perry_wasm_host_module_drop(module: *mut c_void);
    fn perry_wasm_host_module_exports_len(module: *mut c_void) -> usize;
    fn perry_wasm_host_module_export_at(
        module: *mut c_void,
        index: usize,
        out_name: *mut *const c_char,
        out_name_len: *mut usize,
        out_kind: *mut u8,
    ) -> i32;
    fn perry_wasm_host_module_export_func_arity(module: *mut c_void, index: usize) -> usize;
    fn perry_wasm_host_module_imports_len(module: *mut c_void) -> usize;
    fn perry_wasm_host_module_import_at(
        module: *mut c_void,
        index: usize,
        out_module: *mut *const c_char,
        out_module_len: *mut usize,
        out_name: *mut *const c_char,
        out_name_len: *mut usize,
        out_kind: *mut u8,
    ) -> i32;
    fn perry_wasm_host_module_custom_sections_len(
        module: *mut c_void,
        name: *const c_char,
        name_len: usize,
    ) -> usize;
    fn perry_wasm_host_module_custom_section_at(
        module: *mut c_void,
        name: *const c_char,
        name_len: usize,
        nth: usize,
        out_data: *mut *const u8,
        out_data_len: *mut usize,
    ) -> i32;
    fn perry_wasm_host_instance_new(
        module: *mut c_void,
        import_callback: Option<WasmImportCallback>,
        import_context: u64,
        out_err: *mut *mut c_char,
    ) -> *mut c_void;
    fn perry_wasm_host_instance_set_import_context(inst: *mut c_void, import_context: u64);
    #[allow(dead_code)]
    fn perry_wasm_host_instance_drop(inst: *mut c_void);
    fn perry_wasm_host_instance_memory_len(inst: *mut c_void) -> usize;
    fn perry_wasm_host_instance_memory_copy(inst: *mut c_void, out: *mut u8, len: usize) -> usize;
    fn perry_wasm_host_instance_memory_write(
        inst: *mut c_void,
        data: *const u8,
        len: usize,
    ) -> usize;
    fn perry_wasm_host_instance_table_len(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
    ) -> usize;
    fn perry_wasm_host_instance_table_set(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        index: usize,
        bits: u64,
        is_null: i32,
    ) -> i32;
    fn perry_wasm_host_instance_table_grow(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        delta: usize,
        bits: u64,
        is_null: i32,
        out_old_len: *mut usize,
    ) -> i32;
    fn perry_wasm_host_instance_take_exit_code(inst: *mut c_void, out_code: *mut i32) -> i32;
    fn perry_wasm_host_call_export(
        inst: *mut c_void,
        name: *const c_char,
        name_len: usize,
        arg_kinds: *const u8,
        arg_bits: *const u64,
        arg_count: usize,
        out_kinds: *mut u8,
        out_bits: *mut u64,
        out_capacity: usize,
        out_count: *mut usize,
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
    crate::object::js_object_set_field_by_name(
        obj.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        key.get_raw_const_ptr::<crate::string::StringHeader>(),
        value.get_nanbox_f64(),
    );
    obj.get_raw_mut_ptr::<crate::object::ObjectHeader>()
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

fn copy_instance_memory(inst: *mut c_void, buffer: f64) {
    let ptr = unbox_pointer(buffer) as *mut crate::buffer::BufferHeader;
    if ptr.is_null() || !crate::buffer::is_array_buffer(ptr as usize) {
        return;
    }
    let len = unsafe { (*ptr).length.max(0) as usize };
    unsafe {
        perry_wasm_host_instance_memory_copy(inst, crate::buffer::buffer_data_mut(ptr), len);
    }
}

fn write_instance_memory(inst: *mut c_void, buffer: f64) {
    let ptr = unbox_pointer(buffer) as *mut crate::buffer::BufferHeader;
    if ptr.is_null() || !crate::buffer::is_array_buffer(ptr as usize) {
        return;
    }
    let len = unsafe { (*ptr).length.max(0) as usize };
    unsafe {
        perry_wasm_host_instance_memory_write(inst, crate::buffer::buffer_data_mut(ptr), len);
    }
}

fn memory_buffer_value(memory: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let memory = scope.root_nanbox_f64(memory);
    let value = JSValue::from_bits(memory.get_nanbox_f64().to_bits());
    if !value.is_pointer() {
        return nanbox_undefined();
    }
    let key = scope.root_string_ptr(named_key(b"buffer"));
    key.with_const_ptr(|key: *const crate::string::StringHeader| {
        crate::object::js_object_get_field_by_name_f64(
            JSValue::from_bits(memory.get_nanbox_f64().to_bits())
                .as_pointer::<crate::object::ObjectHeader>(),
            key,
        )
    })
}

fn sync_memory_to_wasm(inst: *mut c_void, memory: f64) {
    write_instance_memory(inst, memory_buffer_value(memory));
}

fn sync_memory_from_wasm(inst: *mut c_void, memory: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let memory = scope.root_nanbox_f64(memory);
    let memory_value = JSValue::from_bits(memory.get_nanbox_f64().to_bits());
    if !memory_value.is_pointer() {
        return;
    }
    let host_len = unsafe { perry_wasm_host_instance_memory_len(inst) };
    if host_len == 0 || host_len > i32::MAX as usize {
        return;
    }
    let old_buffer = scope.root_nanbox_f64(memory_buffer_value(memory.get_nanbox_f64()));
    let old_ptr = unbox_pointer(old_buffer.get_nanbox_f64()) as *mut crate::buffer::BufferHeader;
    let old_len = if !old_ptr.is_null() && crate::buffer::is_array_buffer(old_ptr as usize) {
        unsafe { (*old_ptr).length.max(0) as usize }
    } else {
        0
    };
    let buffer = if old_len == host_len {
        old_buffer.get_nanbox_f64()
    } else {
        let new_buffer = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::buffer::js_array_buffer_new(host_len as i32) as i64,
        ));
        let memory_ptr = JSValue::from_bits(memory.get_nanbox_f64().to_bits())
            .as_pointer::<crate::object::ObjectHeader>()
            as *mut crate::object::ObjectHeader;
        let _ = object_set(memory_ptr, b"buffer", new_buffer.get_nanbox_f64());
        new_buffer.get_nanbox_f64()
    };
    copy_instance_memory(inst, buffer);
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

    let scope = crate::gc::RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(f64::from_bits(context));
    let imports_value = JSValue::from_bits(imports.get_nanbox_f64().to_bits());
    if !imports_value.is_pointer() {
        return 0;
    }

    let module_bytes = std::slice::from_raw_parts(module, module_len);
    let module_key = scope.root_string_ptr(named_key(module_bytes));
    let module_value = scope.root_nanbox_f64(crate::object::js_object_get_field_by_name_f64(
        imports_value.as_pointer::<crate::object::ObjectHeader>(),
        module_key.get_raw_const_ptr::<crate::string::StringHeader>(),
    ));
    let module_object = JSValue::from_bits(module_value.get_nanbox_f64().to_bits());
    if !module_object.is_pointer() {
        return 0;
    }

    let name_bytes = std::slice::from_raw_parts(name, name_len);
    let name_key = scope.root_string_ptr(named_key(name_bytes));
    let callback = scope.root_nanbox_f64(crate::object::js_object_get_field_by_name_f64(
        module_object.as_pointer::<crate::object::ObjectHeader>(),
        name_key.get_raw_const_ptr::<crate::string::StringHeader>(),
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
            WASM_VAL_KIND_I64 => (*bits as i64) as f64,
            WASM_VAL_KIND_F32 => f32::from_bits(*bits as u32) as f64,
            WASM_VAL_KIND_F64 => f64::from_bits(*bits),
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
            WASM_VAL_KIND_I64 => result as i64 as u64,
            WASM_VAL_KIND_F32 => (result as f32).to_bits() as u64,
            WASM_VAL_KIND_F64 => result.to_bits(),
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
    unsafe {
        perry_wasm_host_instance_set_import_context(inst, imports.get_nanbox_f64().to_bits())
    };
    sync_memory_to_wasm(inst, memory.get_nanbox_f64());
    let result = call_export_n(nanbox_pointer_raw(inst), name.get_nanbox_f64(), args);
    sync_memory_from_wasm(inst, memory.get_nanbox_f64());
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

extern "C" fn js_wasm_export_call_0(closure: *const crate::closure::ClosureHeader) -> f64 {
    call_captured_wasm_export(closure, &[])
}

extern "C" fn js_wasm_export_call_1(closure: *const crate::closure::ClosureHeader, a: f64) -> f64 {
    call_captured_wasm_export(closure, &[a])
}

extern "C" fn js_wasm_export_call_2(
    closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
) -> f64 {
    call_captured_wasm_export(closure, &[a, b])
}

extern "C" fn js_wasm_export_call_3(
    closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
    c: f64,
) -> f64 {
    call_captured_wasm_export(closure, &[a, b, c])
}

extern "C" fn js_wasm_export_call_4(
    closure: *const crate::closure::ClosureHeader,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
) -> f64 {
    call_captured_wasm_export(closure, &[a, b, c, d])
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
    let (func_ptr, declared_arity) = match arity {
        0 => (js_wasm_export_call_0 as *const u8, 0),
        1 => (js_wasm_export_call_1 as *const u8, 1),
        2 => (js_wasm_export_call_2 as *const u8, 2),
        3 => (js_wasm_export_call_3 as *const u8, 3),
        _ => (js_wasm_export_call_4 as *const u8, 4),
    };
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 5));
    if closure
        .get_raw_mut_ptr::<crate::closure::ClosureHeader>()
        .is_null()
    {
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
    crate::object::set_bound_native_closure_name(
        closure_ptr,
        std::str::from_utf8(name).unwrap_or("wasm"),
    );
    crate::value::js_nanbox_pointer(closure_ptr as i64)
}

fn table_method_context<'scope>(
    scope: &'scope crate::gc::RuntimeHandleScope,
    closure: *const crate::closure::ClosureHeader,
) -> (
    *mut c_void,
    crate::gc::RuntimeHandle<'scope>,
    crate::gc::RuntimeHandle<'scope>,
) {
    let inst = crate::closure::js_closure_get_capture_f64(closure, 0) as usize as *mut c_void;
    let name = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 1));
    let table = scope.root_nanbox_f64(crate::closure::js_closure_get_capture_f64(closure, 2));
    (inst, name, table)
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

extern "C" fn js_wasm_table_get(closure: *const crate::closure::ClosureHeader, index: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (_inst, _name, table) = table_method_context(&scope, closure);
    let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
    let values = JSValue::from_bits(values.get_nanbox_f64().to_bits());
    if !values.is_pointer() || !index.is_finite() || index < 0.0 {
        return nanbox_undefined();
    }
    crate::array::js_array_get_f64(
        values.as_pointer::<crate::array::ArrayHeader>(),
        index as u32,
    )
}

extern "C" fn js_wasm_table_set(
    closure: *const crate::closure::ClosureHeader,
    index: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (inst, name, table) = table_method_context(&scope, closure);
    let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) else {
        return nanbox_undefined();
    };
    let is_null = (value.to_bits() == crate::value::TAG_NULL) as i32;
    let ok = unsafe {
        perry_wasm_host_instance_table_set(
            inst,
            name_ptr as *const c_char,
            name_len,
            index.max(0.0) as usize,
            value.to_bits(),
            is_null,
        )
    };
    if ok != 0 {
        let values = scope.root_nanbox_f64(table_values(table.get_nanbox_f64()));
        let values_value = JSValue::from_bits(values.get_nanbox_f64().to_bits());
        if values_value.is_pointer() {
            let values_ptr = values_value.as_pointer::<crate::array::ArrayHeader>()
                as *mut crate::array::ArrayHeader;
            crate::array::js_array_set_f64(values_ptr, index.max(0.0) as u32, value);
        }
    }
    nanbox_undefined()
}

extern "C" fn js_wasm_table_grow(
    closure: *const crate::closure::ClosureHeader,
    delta: f64,
    value: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let (inst, name, table) = table_method_context(&scope, closure);
    let Some((name_ptr, name_len)) = extract_string_bytes(name.get_nanbox_f64()) else {
        return nanbox_undefined();
    };
    let value_bits = value.to_bits();
    let is_null = matches!(
        value_bits,
        crate::value::TAG_NULL | crate::value::TAG_UNDEFINED
    ) as i32;
    let mut old_len = 0usize;
    let ok = unsafe {
        perry_wasm_host_instance_table_grow(
            inst,
            name_ptr as *const c_char,
            name_len,
            delta.max(0.0) as usize,
            value_bits,
            is_null,
            &mut old_len,
        )
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
    for _ in 0..delta.max(0.0) as usize {
        values_ptr = crate::array::js_array_push_f64(values_ptr, fill);
        values.set_nanbox_f64(array_value(values_ptr));
    }
    let table_value = JSValue::from_bits(table.get_nanbox_f64().to_bits());
    if table_value.is_pointer() {
        let _ = object_set(
            table_value.as_pointer::<crate::object::ObjectHeader>()
                as *mut crate::object::ObjectHeader,
            b"length",
            old_len.saturating_add(delta.max(0.0) as usize) as f64,
        );
    }
    old_len as f64
}

fn make_table_method(
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
    let closure = scope.root_raw_mut_ptr(crate::closure::js_closure_alloc(func_ptr, 3));
    if closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| closure.is_null()) {
        return nanbox_undefined();
    }
    crate::closure::js_register_closure_arity(func_ptr, arity);
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 0, inst as usize as f64)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 1, name.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::closure::js_closure_set_capture_f64(closure, 2, table.get_nanbox_f64())
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::object::set_bound_native_closure_name(closure, display_name)
    });
    closure.with_mut_ptr(|closure: *mut crate::closure::ClosureHeader| {
        crate::value::js_nanbox_pointer(closure as i64)
    })
}

fn make_export_table(inst: *mut c_void, name: &[u8]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let table = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
    let name_value = scope.root_nanbox_f64(string_value(name));
    let Some((name_ptr, name_len)) = extract_string_bytes(name_value.get_nanbox_f64()) else {
        return nanbox_undefined();
    };
    let len =
        unsafe { perry_wasm_host_instance_table_len(inst, name_ptr as *const c_char, name_len) };
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
            inst,
            name_value.get_nanbox_f64(),
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
    table.get_nanbox_f64()
}

fn make_instance_value(module: *mut c_void, inst: *mut c_void, imports: f64, receiver: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(imports);
    let memory_len = unsafe { perry_wasm_host_instance_memory_len(inst) };
    let memory = if memory_len == 0 {
        scope.root_nanbox_f64(nanbox_undefined())
    } else {
        let buffer = scope.root_nanbox_f64(crate::value::js_nanbox_pointer(
            crate::buffer::js_array_buffer_new(memory_len.min(i32::MAX as usize) as i32) as i64,
        ));
        copy_instance_memory(inst, buffer.get_nanbox_f64());
        let object = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
        let object = object.with_mut_ptr(|object: *mut crate::object::ObjectHeader| {
            object_set(object, b"buffer", buffer.get_nanbox_f64())
        });
        scope.root_nanbox_f64(object_value(object))
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

fn make_instance_result(module: *mut c_void, inst: *mut c_void, imports: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let instance = scope.root_nanbox_f64(make_instance_value(
        module,
        inst,
        imports,
        nanbox_undefined(),
    ));

    let result = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    let module_value = scope.root_nanbox_f64(make_module_object(module));
    let result_ptr = result.with_mut_ptr(|r: *mut crate::object::ObjectHeader| {
        object_set(r, b"module", module_value.get_nanbox_f64())
    });
    result.set_raw_mut_ptr(result_ptr);
    let result_ptr = object_set(
        result.get_raw_mut_ptr::<crate::object::ObjectHeader>(),
        b"instance",
        instance.get_nanbox_f64(),
    );
    result.set_raw_mut_ptr(result_ptr);
    result.with_mut_ptr(|r: *mut crate::object::ObjectHeader| object_value(r))
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
            imports.get_nanbox_f64().to_bits(),
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

/// `WebAssembly.instantiate(bytes, imports?)` returns the standard instance
/// result shape. Imported numeric functions are resolved from the JS imports
/// object by module/name and called synchronously by the wasmi host.
#[no_mangle]
pub extern "C" fn js_webassembly_instantiate(bytes_jsval: f64, imports_jsval: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let imports = scope.root_nanbox_f64(imports_jsval);
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
            imports.get_nanbox_f64().to_bits(),
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

    const MAX_RESULTS: usize = 16;
    let mut out_kinds = [WASM_VAL_KIND_NONE; MAX_RESULTS];
    let mut out_bits = [0u64; MAX_RESULTS];
    let mut out_count = 0usize;
    let mut err: *mut c_char = std::ptr::null_mut();
    let ok = unsafe {
        perry_wasm_host_call_export(
            inst,
            name_ptr as *const c_char,
            name_len,
            kinds.as_ptr(),
            bits.as_ptr(),
            kinds.len(),
            out_kinds.as_mut_ptr(),
            out_bits.as_mut_ptr(),
            MAX_RESULTS,
            &mut out_count,
            &mut err,
        )
    };
    if ok == 0 {
        emit_error_to_stderr("WebAssembly.RuntimeError", err);
        return nanbox_undefined();
    }
    let decode = |kind: u8, bits: u64| match kind {
        WASM_VAL_KIND_I32 => (bits as u32 as i32) as f64,
        WASM_VAL_KIND_I64 => (bits as i64) as f64,
        WASM_VAL_KIND_F32 => f32::from_bits(bits as u32) as f64,
        WASM_VAL_KIND_F64 => f64::from_bits(bits),
        _ => nanbox_undefined(),
    };
    let result = match out_count {
        0 => nanbox_undefined(),
        1 => decode(out_kinds[0], out_bits[0]),
        count => {
            let scope = crate::gc::RuntimeHandleScope::new();
            let array = scope.root_nanbox_f64(array_value(crate::array::js_array_alloc(
                count.min(MAX_RESULTS) as u32,
            )));
            for index in 0..count.min(MAX_RESULTS) {
                let array_ptr = JSValue::from_bits(array.get_nanbox_f64().to_bits())
                    .as_pointer::<crate::array::ArrayHeader>()
                    as *mut crate::array::ArrayHeader;
                let array_ptr = crate::array::js_array_push_f64(
                    array_ptr,
                    decode(out_kinds[index], out_bits[index]),
                );
                array.set_nanbox_f64(array_value(array_ptr));
            }
            array.get_nanbox_f64()
        }
    };
    // Avoid leaking the unused err buffer on success.
    if !err.is_null() {
        unsafe { perry_wasm_host_string_free(err) };
    }
    result
}
