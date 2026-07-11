//! Perry WebAssembly host runtime — wasmi wrapper.
//!
//! Isolated crate so the default Perry build does not pull `wasmi` as a
//! transitive dependency. Linked into the final binary only when the user
//! passes `--enable-wasm-runtime` to `perry compile/run`.
//!
//! Issue: <https://github.com/PerryTS/perry/issues/76>
//!
//! API is intentionally narrow and uses owned, opaque handles so callers
//! (`perry-runtime::webassembly`) never touch a `wasmi::*` type directly.
//! That keeps the wasmi version surface small and lets us swap engines
//! (wasmtime, etc.) behind the same shape later.

use std::sync::Arc;

use wasmi::{Engine, ExternType, Linker, Module, Store, Val};

/// Numeric WebAssembly value. MVP supports only the four core numeric types;
/// `externref` / `funcref` / `v128` are out of scope (see issue #76, "Open
/// questions").
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WasmVal {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
}

impl WasmVal {
    fn from_wasmi(v: &Val) -> Option<Self> {
        match v {
            Val::I32(x) => Some(WasmVal::I32(*x)),
            Val::I64(x) => Some(WasmVal::I64(*x)),
            Val::F32(x) => Some(WasmVal::F32(f32::from_bits(x.to_bits()))),
            Val::F64(x) => Some(WasmVal::F64(f64::from_bits(x.to_bits()))),
            _ => None,
        }
    }

    fn to_wasmi(self) -> Val {
        match self {
            WasmVal::I32(x) => Val::I32(x),
            WasmVal::I64(x) => Val::I64(x),
            // wasmi 1.x re-exports F32/F64 at the crate root; `wasmi::core`
            // is private there (0.x accepted `wasmi::core::F32/F64`).
            WasmVal::F32(x) => Val::F32(wasmi::F32::from_bits(x.to_bits())),
            WasmVal::F64(x) => Val::F64(wasmi::F64::from_bits(x.to_bits())),
        }
    }
}

/// Opaque compiled module. Cheap to clone (Arc).
#[derive(Clone)]
pub struct WasmModuleHandle(Arc<ModuleInner>);

struct ModuleInner {
    engine: Engine,
    module: Module,
}

/// Opaque instance. Owns its own `Store` so each instance has independent
/// memory / globals — matches JS `WebAssembly.Instance` semantics.
pub struct WasmInstanceHandle {
    inner: Box<InstanceInner>,
}

struct InstanceInner {
    store: Store<()>,
    instance: wasmi::Instance,
    /// Keep the module alive for the lifetime of the instance so `engine` /
    /// `module` references stay valid.
    _module: WasmModuleHandle,
}

#[derive(Debug)]
pub enum WasmHostError {
    Compile(String),
    Link(String),
    Runtime(String),
    InvalidExport(String),
    UnsupportedSignature(String),
}

impl std::fmt::Display for WasmHostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmHostError::Compile(m) => write!(f, "WebAssembly.CompileError: {m}"),
            WasmHostError::Link(m) => write!(f, "WebAssembly.LinkError: {m}"),
            WasmHostError::Runtime(m) => write!(f, "WebAssembly.RuntimeError: {m}"),
            WasmHostError::InvalidExport(m) => write!(f, "Export not found: {m}"),
            WasmHostError::UnsupportedSignature(m) => {
                write!(f, "Unsupported export signature: {m}")
            }
        }
    }
}

impl std::error::Error for WasmHostError {}

/// Cheap byte-level magic check (`\0asm\01\0\0\0`). Mirrors `WebAssembly.validate`
/// — for the MVP we delegate to wasmi's full module decode.
pub fn validate(bytes: &[u8]) -> bool {
    let engine = Engine::default();
    Module::new(&engine, bytes).is_ok()
}

/// Compile bytes to a module. No imports resolved at this stage.
pub fn compile(bytes: &[u8]) -> Result<WasmModuleHandle, WasmHostError> {
    let engine = Engine::default();
    let module = Module::new(&engine, bytes).map_err(|e| WasmHostError::Compile(e.to_string()))?;
    Ok(WasmModuleHandle(Arc::new(ModuleInner { engine, module })))
}

/// Instantiate without imports. The MVP host-imports bridge is wired
/// separately via [`instantiate_with_imports`] once we have a JS closure to
/// trampoline.
pub fn instantiate(module: &WasmModuleHandle) -> Result<WasmInstanceHandle, WasmHostError> {
    let mut store = Store::new(&module.0.engine, ());
    let linker = <Linker<()>>::new(&module.0.engine);
    let instance = linker
        .instantiate_and_start(&mut store, &module.0.module)
        .map_err(|e| WasmHostError::Link(e.to_string()))?;
    Ok(WasmInstanceHandle {
        inner: Box::new(InstanceInner {
            store,
            instance,
            _module: module.clone(),
        }),
    })
}

/// Call an exported function by name. Returns the first return value (MVP
/// assumes 0-or-1 result, matching the numeric-only subset).
pub fn call_export(
    inst: &mut WasmInstanceHandle,
    name: &str,
    args: &[WasmVal],
) -> Result<Option<WasmVal>, WasmHostError> {
    let func = inst
        .inner
        .instance
        .get_func(&inst.inner.store, name)
        .ok_or_else(|| WasmHostError::InvalidExport(name.to_string()))?;

    let ty = func.ty(&inst.inner.store);
    let params = ty.params();
    if params.len() != args.len() {
        return Err(WasmHostError::Runtime(format!(
            "{name}: arity mismatch (export expects {}, got {})",
            params.len(),
            args.len()
        )));
    }

    let wasmi_args: Vec<Val> = args.iter().map(|v| v.to_wasmi()).collect();
    let results_len = ty.results().len();
    if results_len > 1 {
        return Err(WasmHostError::UnsupportedSignature(format!(
            "{name}: multi-value return not supported in MVP"
        )));
    }
    let mut results: Vec<Val> = vec![Val::I32(0); results_len];
    func.call(&mut inst.inner.store, &wasmi_args, &mut results)
        .map_err(|e| WasmHostError::Runtime(e.to_string()))?;

    Ok(results.first().and_then(WasmVal::from_wasmi))
}

// ────────────────────────────────────────────────────────────────────────
// C ABI surface — these are the `extern "C"` symbols that `perry-runtime`'s
// `js_webassembly_*` shims call into via forward declarations. Keeping them
// in this isolated crate is the whole point of the design (see issue #76):
// the default Perry build never links wasmi, so the binary stays slim.
//
// Lifecycle: the runtime owns the opaque pointers and is responsible for
// calling `perry_wasm_host_module_drop` / `..._instance_drop` when its
// wrapping JSValue is GC'd. None of these functions panic — errors flow
// back through `*mut c_char` out-params (caller frees with
// `perry_wasm_host_string_free`).
// ────────────────────────────────────────────────────────────────────────

use std::ffi::{c_char, CString};
use std::slice;

fn capture_err(out_err: *mut *mut c_char, e: WasmHostError) {
    if out_err.is_null() {
        return;
    }
    let cs =
        CString::new(e.to_string()).unwrap_or_else(|_| CString::new("wasm host error").unwrap());
    unsafe { *out_err = cs.into_raw() };
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_validate(bytes: *const u8, len: usize) -> i32 {
    if bytes.is_null() {
        return 0;
    }
    let slice = unsafe { slice::from_raw_parts(bytes, len) };
    if validate(slice) {
        1
    } else {
        0
    }
}

/// Compile bytes to an opaque module handle. Returns NULL on error and writes
/// a heap-allocated error message into `*out_err`. Caller frees the message
/// via `perry_wasm_host_string_free`.
#[no_mangle]
pub extern "C" fn perry_wasm_host_module_new(
    bytes: *const u8,
    len: usize,
    out_err: *mut *mut c_char,
) -> *mut WasmModuleHandle {
    if bytes.is_null() {
        capture_err(out_err, WasmHostError::Compile("null buffer".into()));
        return std::ptr::null_mut();
    }
    let slice = unsafe { slice::from_raw_parts(bytes, len) };
    match compile(slice) {
        Ok(m) => Box::into_raw(Box::new(m)),
        Err(e) => {
            capture_err(out_err, e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_drop(module: *mut WasmModuleHandle) {
    if !module.is_null() {
        unsafe { drop(Box::from_raw(module)) };
    }
}

/// WebAssembly external kind tags for module metadata. Mirrors the standard
/// `WebAssembly.Module.exports/imports` descriptor `kind` strings:
/// function/table/memory/global.
pub const WASM_EXTERN_KIND_FUNCTION: u8 = 0;
pub const WASM_EXTERN_KIND_TABLE: u8 = 1;
pub const WASM_EXTERN_KIND_MEMORY: u8 = 2;
pub const WASM_EXTERN_KIND_GLOBAL: u8 = 3;

fn extern_type_kind(ty: &ExternType) -> u8 {
    match ty {
        ExternType::Func(_) => WASM_EXTERN_KIND_FUNCTION,
        ExternType::Table(_) => WASM_EXTERN_KIND_TABLE,
        ExternType::Memory(_) => WASM_EXTERN_KIND_MEMORY,
        ExternType::Global(_) => WASM_EXTERN_KIND_GLOBAL,
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_exports_len(module: *mut WasmModuleHandle) -> usize {
    if module.is_null() {
        return 0;
    }
    let module = unsafe { &*module };
    module.0.module.exports().count()
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_export_at(
    module: *mut WasmModuleHandle,
    index: usize,
    out_name: *mut *const c_char,
    out_name_len: *mut usize,
    out_kind: *mut u8,
) -> i32 {
    if module.is_null() || out_name.is_null() || out_name_len.is_null() || out_kind.is_null() {
        return 0;
    }
    let module = unsafe { &*module };
    let Some(export) = module.0.module.exports().nth(index) else {
        return 0;
    };
    let name = export.name();
    unsafe {
        *out_name = name.as_ptr() as *const c_char;
        *out_name_len = name.len();
        *out_kind = extern_type_kind(export.ty());
    }
    1
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_imports_len(module: *mut WasmModuleHandle) -> usize {
    if module.is_null() {
        return 0;
    }
    let module = unsafe { &*module };
    module.0.module.imports().len()
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_import_at(
    module: *mut WasmModuleHandle,
    index: usize,
    out_module: *mut *const c_char,
    out_module_len: *mut usize,
    out_name: *mut *const c_char,
    out_name_len: *mut usize,
    out_kind: *mut u8,
) -> i32 {
    if module.is_null()
        || out_module.is_null()
        || out_module_len.is_null()
        || out_name.is_null()
        || out_name_len.is_null()
        || out_kind.is_null()
    {
        return 0;
    }
    let module = unsafe { &*module };
    let Some(import) = module.0.module.imports().nth(index) else {
        return 0;
    };
    let module_name = import.module();
    let name = import.name();
    unsafe {
        *out_module = module_name.as_ptr() as *const c_char;
        *out_module_len = module_name.len();
        *out_name = name.as_ptr() as *const c_char;
        *out_name_len = name.len();
        *out_kind = extern_type_kind(import.ty());
    }
    1
}

fn utf8_arg<'a>(ptr: *const c_char, len: usize) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    let bytes = unsafe { slice::from_raw_parts(ptr as *const u8, len) };
    std::str::from_utf8(bytes).ok()
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_custom_sections_len(
    module: *mut WasmModuleHandle,
    name: *const c_char,
    name_len: usize,
) -> usize {
    if module.is_null() {
        return 0;
    }
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    let module = unsafe { &*module };
    module
        .0
        .module
        .custom_sections()
        .filter(|section| section.name() == name)
        .count()
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_module_custom_section_at(
    module: *mut WasmModuleHandle,
    name: *const c_char,
    name_len: usize,
    nth: usize,
    out_data: *mut *const u8,
    out_data_len: *mut usize,
) -> i32 {
    if module.is_null() || out_data.is_null() || out_data_len.is_null() {
        return 0;
    }
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    let module = unsafe { &*module };
    let Some(section) = module
        .0
        .module
        .custom_sections()
        .filter(|section| section.name() == name)
        .nth(nth)
    else {
        return 0;
    };
    let data = section.data();
    unsafe {
        *out_data = data.as_ptr();
        *out_data_len = data.len();
    }
    1
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_new(
    module: *mut WasmModuleHandle,
    out_err: *mut *mut c_char,
) -> *mut WasmInstanceHandle {
    if module.is_null() {
        capture_err(out_err, WasmHostError::Link("null module".into()));
        return std::ptr::null_mut();
    }
    let module = unsafe { &*module };
    match instantiate(module) {
        Ok(i) => Box::into_raw(Box::new(i)),
        Err(e) => {
            capture_err(out_err, e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_drop(inst: *mut WasmInstanceHandle) {
    if !inst.is_null() {
        unsafe { drop(Box::from_raw(inst)) };
    }
}

/// Numeric value type tags for the C ABI — must match
/// `perry_wasm_host_call_export`'s `arg_kinds` / `ret_kind` encoding.
pub const WASM_VAL_KIND_I32: u8 = 0;
pub const WASM_VAL_KIND_I64: u8 = 1;
pub const WASM_VAL_KIND_F32: u8 = 2;
pub const WASM_VAL_KIND_F64: u8 = 3;
pub const WASM_VAL_KIND_NONE: u8 = 0xFF;

/// Call an export by name. Args are encoded as parallel arrays:
/// `arg_kinds[i]` is the type tag, `arg_bits[i]` is the raw 64-bit payload
/// (i32/f32 widened, i64/f64 as-is). On success writes `*out_kind` /
/// `*out_bits` (or `WASM_VAL_KIND_NONE` for void exports). On error returns
/// 0 and writes `*out_err`.
#[no_mangle]
pub extern "C" fn perry_wasm_host_call_export(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    arg_kinds: *const u8,
    arg_bits: *const u64,
    arg_count: usize,
    out_kind: *mut u8,
    out_bits: *mut u64,
    out_err: *mut *mut c_char,
) -> i32 {
    if inst.is_null() || name.is_null() || out_kind.is_null() || out_bits.is_null() {
        capture_err(out_err, WasmHostError::Runtime("null arg".into()));
        return 0;
    }
    let inst = unsafe { &mut *inst };
    let name_bytes = unsafe { slice::from_raw_parts(name as *const u8, name_len) };
    let name_str = match std::str::from_utf8(name_bytes) {
        Ok(s) => s,
        Err(_) => {
            capture_err(
                out_err,
                WasmHostError::InvalidExport("non-utf8 export name".into()),
            );
            return 0;
        }
    };
    let kinds = unsafe { slice::from_raw_parts(arg_kinds, arg_count) };
    let bits = unsafe { slice::from_raw_parts(arg_bits, arg_count) };
    let mut args: Vec<WasmVal> = Vec::with_capacity(arg_count);
    for i in 0..arg_count {
        let v = match kinds[i] {
            WASM_VAL_KIND_I32 => WasmVal::I32(bits[i] as i32),
            WASM_VAL_KIND_I64 => WasmVal::I64(bits[i] as i64),
            WASM_VAL_KIND_F32 => WasmVal::F32(f32::from_bits(bits[i] as u32)),
            WASM_VAL_KIND_F64 => WasmVal::F64(f64::from_bits(bits[i])),
            other => {
                capture_err(
                    out_err,
                    WasmHostError::UnsupportedSignature(format!("arg kind {other}")),
                );
                return 0;
            }
        };
        args.push(v);
    }
    match call_export(inst, name_str, &args) {
        Ok(None) => {
            unsafe {
                *out_kind = WASM_VAL_KIND_NONE;
                *out_bits = 0;
            }
            1
        }
        Ok(Some(v)) => {
            let (k, b) = match v {
                WasmVal::I32(x) => (WASM_VAL_KIND_I32, x as u32 as u64),
                WasmVal::I64(x) => (WASM_VAL_KIND_I64, x as u64),
                WasmVal::F32(x) => (WASM_VAL_KIND_F32, x.to_bits() as u64),
                WasmVal::F64(x) => (WASM_VAL_KIND_F64, x.to_bits()),
            };
            unsafe {
                *out_kind = k;
                *out_bits = b;
            }
            1
        }
        Err(e) => {
            capture_err(out_err, e);
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal `(module (func (export "add") (param i32 i32) (result i32)
    ///                local.get 0 local.get 1 i32.add))`.
    const ADD_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f,
        0x01, 0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x07, 0x01, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00,
        0x0a, 0x09, 0x01, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b,
    ];
    /// `(module (import "env" "f" (func (param i32) (result i32))))`.
    const IMPORT_FUNC_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01,
        0x7f, 0x02, 0x09, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x01, 0x66, 0x00, 0x00,
    ];
    /// `(module (@custom "meta" "\01\02\03"))`.
    const CUSTOM_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x00, 0x08, 0x04, 0x6d, 0x65, 0x74, 0x61,
        0x01, 0x02, 0x03,
    ];

    #[test]
    fn validate_add_wasm() {
        assert!(validate(ADD_WASM));
        assert!(!validate(&[0x00, 0x00, 0x00, 0x00]));
    }

    #[test]
    fn instantiate_and_call_add() {
        let module = compile(ADD_WASM).expect("compile");
        let mut inst = instantiate(&module).expect("instantiate");
        let result =
            call_export(&mut inst, "add", &[WasmVal::I32(2), WasmVal::I32(3)]).expect("call");
        assert_eq!(result, Some(WasmVal::I32(5)));
    }

    #[test]
    fn c_abi_reports_module_exports_imports_and_custom_sections() {
        let mut err = std::ptr::null_mut();
        let add = perry_wasm_host_module_new(ADD_WASM.as_ptr(), ADD_WASM.len(), &mut err);
        assert!(!add.is_null(), "compile add module: {err:p}");
        assert_eq!(perry_wasm_host_module_exports_len(add), 1);

        let mut name = std::ptr::null();
        let mut name_len = 0usize;
        let mut kind = u8::MAX;
        assert_eq!(
            perry_wasm_host_module_export_at(add, 0, &mut name, &mut name_len, &mut kind),
            1
        );
        let export_name =
            unsafe { std::str::from_utf8(std::slice::from_raw_parts(name as *const u8, name_len)) }
                .unwrap();
        assert_eq!(export_name, "add");
        assert_eq!(kind, WASM_EXTERN_KIND_FUNCTION);
        perry_wasm_host_module_drop(add);

        let imports =
            perry_wasm_host_module_new(IMPORT_FUNC_WASM.as_ptr(), IMPORT_FUNC_WASM.len(), &mut err);
        assert!(!imports.is_null(), "compile import module: {err:p}");
        assert_eq!(perry_wasm_host_module_imports_len(imports), 1);

        let mut module_name = std::ptr::null();
        let mut module_name_len = 0usize;
        name = std::ptr::null();
        name_len = 0;
        kind = u8::MAX;
        assert_eq!(
            perry_wasm_host_module_import_at(
                imports,
                0,
                &mut module_name,
                &mut module_name_len,
                &mut name,
                &mut name_len,
                &mut kind,
            ),
            1
        );
        let import_module = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(
                module_name as *const u8,
                module_name_len,
            ))
        }
        .unwrap();
        let import_name =
            unsafe { std::str::from_utf8(std::slice::from_raw_parts(name as *const u8, name_len)) }
                .unwrap();
        assert_eq!(
            (import_module, import_name, kind),
            ("env", "f", WASM_EXTERN_KIND_FUNCTION)
        );
        perry_wasm_host_module_drop(imports);

        let custom = perry_wasm_host_module_new(CUSTOM_WASM.as_ptr(), CUSTOM_WASM.len(), &mut err);
        assert!(!custom.is_null(), "compile custom module: {err:p}");
        let section_name = b"meta";
        assert_eq!(
            perry_wasm_host_module_custom_sections_len(
                custom,
                section_name.as_ptr() as *const c_char,
                section_name.len(),
            ),
            1
        );
        let mut data = std::ptr::null();
        let mut data_len = 0usize;
        assert_eq!(
            perry_wasm_host_module_custom_section_at(
                custom,
                section_name.as_ptr() as *const c_char,
                section_name.len(),
                0,
                &mut data,
                &mut data_len,
            ),
            1
        );
        let bytes = unsafe { std::slice::from_raw_parts(data, data_len) };
        assert_eq!(bytes, &[1, 2, 3]);
        perry_wasm_host_module_drop(custom);
    }
}
