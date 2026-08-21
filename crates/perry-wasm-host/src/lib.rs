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

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use wasmi::{Engine, ExternRef, ExternType, Linker, Module, Ref, Store, Table, Val, ValType};

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
    store: Store<WasmHostState>,
    instance: wasmi::Instance,
    /// Keep the module alive for the lifetime of the instance so `engine` /
    /// `module` references stay valid.
    _module: WasmModuleHandle,
}

struct WasmHostState {
    exit_code: Option<i32>,
    import_callback: Option<WasmImportCallback>,
    import_context: u64,
}

#[derive(Clone, Copy)]
struct PendingTableValue {
    bits: u64,
    is_null: bool,
}

enum PendingTableOp {
    Set {
        name: String,
        index: usize,
        value: PendingTableValue,
    },
    Grow {
        name: String,
        delta: usize,
        value: PendingTableValue,
    },
}

struct ActiveInstanceTables {
    instance: usize,
    lengths: HashMap<String, usize>,
    overrides: HashMap<(String, usize), PendingTableValue>,
    ops: Vec<PendingTableOp>,
}

thread_local! {
    /// JS imports may call methods on an exported table while wasmi already
    /// has the instance Store mutably borrowed. Queue those mutations until
    /// the Wasm call unwinds instead of re-entering the same Store through the
    /// C ABI (which would create aliased mutable Rust references).
    static ACTIVE_INSTANCE_TABLES: RefCell<Vec<ActiveInstanceTables>> = const { RefCell::new(Vec::new()) };
}

fn begin_instance_call(inst: &mut WasmInstanceHandle) {
    let instance_id = inst as *mut WasmInstanceHandle as usize;
    let mut lengths = HashMap::new();
    for export in inst.inner._module.0.module.exports() {
        let ExternType::Table(table_type) = export.ty() else {
            continue;
        };
        if table_type.element() != ValType::ExternRef {
            continue;
        }
        let Some(table) = inst
            .inner
            .instance
            .get_table(&inst.inner.store, export.name())
        else {
            continue;
        };
        if let Ok(len) = usize::try_from(table.size(&inst.inner.store)) {
            lengths.insert(export.name().to_string(), len);
        }
    }
    ACTIVE_INSTANCE_TABLES.with(|active| {
        active.borrow_mut().push(ActiveInstanceTables {
            instance: instance_id,
            lengths,
            overrides: HashMap::new(),
            ops: Vec::new(),
        });
    });
}

fn finish_instance_call(inst: &mut WasmInstanceHandle) -> Result<(), WasmHostError> {
    let instance_id = inst as *mut WasmInstanceHandle as usize;
    let active = ACTIVE_INSTANCE_TABLES.with(|active| active.borrow_mut().pop());
    let Some(active) = active else {
        return Ok(());
    };
    debug_assert_eq!(active.instance, instance_id);
    for op in active.ops {
        match op {
            PendingTableOp::Set { name, index, value } => {
                let Some(table) = instance_table(inst, &name) else {
                    return Err(WasmHostError::Runtime(format!(
                        "table export {name:?} disappeared during imported callback"
                    )));
                };
                let value = table_value(inst, value.bits, value.is_null as i32);
                table
                    .set(&mut inst.inner.store, index as u64, value)
                    .map_err(|error| WasmHostError::Runtime(error.to_string()))?;
            }
            PendingTableOp::Grow { name, delta, value } => {
                let Some(table) = instance_table(inst, &name) else {
                    return Err(WasmHostError::Runtime(format!(
                        "table export {name:?} disappeared during imported callback"
                    )));
                };
                let value = table_value(inst, value.bits, value.is_null as i32);
                table
                    .grow(&mut inst.inner.store, delta as u64, value)
                    .map_err(|error| WasmHostError::Runtime(error.to_string()))?;
            }
        }
    }
    Ok(())
}

fn with_active_instance_tables<R>(
    instance: usize,
    f: impl FnOnce(&mut ActiveInstanceTables) -> R,
) -> Option<R> {
    ACTIVE_INSTANCE_TABLES.with(|active| {
        let mut active = active.borrow_mut();
        let state = active
            .iter_mut()
            .rev()
            .find(|state| state.instance == instance)?;
        Some(f(state))
    })
}

pub type WasmImportCallback = unsafe extern "C" fn(
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

/// Instantiate with the module's imported numeric functions routed through an
/// optional embedding callback. Missing callbacks keep the historical typed
/// zero-result fallback. `proc_exit` records its status so the JS WASI wrapper
/// can return it after `_start` completes.
pub fn instantiate(module: &WasmModuleHandle) -> Result<WasmInstanceHandle, WasmHostError> {
    instantiate_with_import_callback(module, None, 0)
}

fn instantiate_with_import_callback(
    module: &WasmModuleHandle,
    import_callback: Option<WasmImportCallback>,
    import_context: u64,
) -> Result<WasmInstanceHandle, WasmHostError> {
    let mut store = Store::new(
        &module.0.engine,
        WasmHostState {
            exit_code: None,
            import_callback,
            import_context,
        },
    );
    let mut linker = <Linker<WasmHostState>>::new(&module.0.engine);
    for import in module.0.module.imports() {
        let ExternType::Func(ty) = import.ty() else {
            return Err(WasmHostError::Link(format!(
                "unsupported import {}.{}",
                import.module(),
                import.name()
            )));
        };
        let module_name = import.module().to_owned();
        let import_name = import.name().to_owned();
        let callback_module = module_name.clone();
        let callback_name = import_name.clone();
        linker
            .func_new(
                &module_name,
                &import_name,
                ty.clone(),
                move |mut caller, params, results| {
                    if matches!(
                        callback_module.as_str(),
                        "wasi_snapshot_preview1" | "wasi_unstable"
                    ) && callback_name == "proc_exit"
                    {
                        if let Some(Val::I32(code)) = params.first() {
                            caller.data_mut().exit_code = Some(*code);
                        }
                    }
                    if let Some(callback) = caller.data().import_callback {
                        let mut arg_kinds = Vec::with_capacity(params.len());
                        let mut arg_bits = Vec::with_capacity(params.len());
                        let mut numeric = true;
                        for param in params {
                            if let Some((kind, bits)) = val_kind_bits(param) {
                                arg_kinds.push(kind);
                                arg_bits.push(bits);
                            } else {
                                numeric = false;
                                break;
                            }
                        }
                        let mut result_kinds = Vec::with_capacity(results.len());
                        let mut result_bits = vec![0u64; results.len()];
                        for result in results.iter() {
                            if let Some((kind, _)) = val_kind_bits(result) {
                                result_kinds.push(kind);
                            } else {
                                numeric = false;
                                break;
                            }
                        }
                        if numeric {
                            let called = unsafe {
                                callback(
                                    caller.data().import_context,
                                    callback_module.as_ptr(),
                                    callback_module.len(),
                                    callback_name.as_ptr(),
                                    callback_name.len(),
                                    arg_kinds.as_ptr(),
                                    arg_bits.as_ptr(),
                                    arg_bits.len(),
                                    result_kinds.as_ptr(),
                                    result_bits.as_mut_ptr(),
                                    result_bits.len(),
                                )
                            };
                            if called != 0 {
                                for ((result, kind), bits) in results
                                    .iter_mut()
                                    .zip(result_kinds.iter())
                                    .zip(result_bits.iter())
                                {
                                    *result = val_from_kind_bits(*kind, *bits);
                                }
                                return Ok(());
                            }
                        }
                    }
                    for result in results {
                        *result = Val::default(result.ty());
                    }
                    Ok(())
                },
            )
            .map_err(|e| WasmHostError::Link(e.to_string()))?;
    }
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

fn coerce_numeric_value(value: WasmVal, expected: ValType) -> Option<Val> {
    let number = match value {
        WasmVal::I32(value) => value as f64,
        WasmVal::I64(value) => value as f64,
        WasmVal::F32(value) => value as f64,
        WasmVal::F64(value) => value,
    };
    match expected {
        ValType::I32 => Some(Val::I32(number as i32)),
        ValType::I64 => Some(Val::I64(number as i64)),
        ValType::F32 => Some(Val::F32(wasmi::F32::from_bits((number as f32).to_bits()))),
        ValType::F64 => Some(Val::F64(wasmi::F64::from_bits(number.to_bits()))),
        ValType::V128 | ValType::FuncRef | ValType::ExternRef => None,
    }
}

/// Call an exported function by name. Numeric JavaScript inputs are coerced
/// against the declared Wasm parameter types, and all numeric results are
/// returned in declaration order. wasm-bindgen uses multi-value returns for
/// pointer/length pairs, so preserving only the first result is not enough for
/// real generated glue.
pub fn call_export(
    inst: &mut WasmInstanceHandle,
    name: &str,
    args: &[WasmVal],
) -> Result<Vec<WasmVal>, WasmHostError> {
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

    let wasmi_args: Vec<Val> = args
        .iter()
        .copied()
        .zip(ty.params().iter().copied())
        .map(|(value, expected)| {
            coerce_numeric_value(value, expected).ok_or_else(|| {
                WasmHostError::UnsupportedSignature(format!(
                    "{name}: unsupported parameter type {expected:?}"
                ))
            })
        })
        .collect::<Result<_, _>>()?;
    let mut results: Vec<Val> = ty.results().iter().copied().map(Val::default).collect();
    begin_instance_call(inst);
    let call_result = func.call(&mut inst.inner.store, &wasmi_args, &mut results);
    let table_result = finish_instance_call(inst);
    call_result.map_err(|e| WasmHostError::Runtime(e.to_string()))?;
    table_result?;

    results
        .iter()
        .map(|value| {
            WasmVal::from_wasmi(value).ok_or_else(|| {
                WasmHostError::UnsupportedSignature(format!(
                    "{name}: unsupported result type {:?}",
                    value.ty()
                ))
            })
        })
        .collect()
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

fn val_kind_bits(value: &Val) -> Option<(u8, u64)> {
    match value {
        Val::I32(value) => Some((WASM_VAL_KIND_I32, *value as u32 as u64)),
        Val::I64(value) => Some((WASM_VAL_KIND_I64, *value as u64)),
        Val::F32(value) => Some((WASM_VAL_KIND_F32, value.to_bits() as u64)),
        Val::F64(value) => Some((WASM_VAL_KIND_F64, value.to_bits())),
        _ => None,
    }
}

fn val_from_kind_bits(kind: u8, bits: u64) -> Val {
    match kind {
        WASM_VAL_KIND_I32 => Val::I32(bits as u32 as i32),
        WASM_VAL_KIND_I64 => Val::I64(bits as i64),
        WASM_VAL_KIND_F32 => Val::F32(wasmi::F32::from_bits(bits as u32)),
        WASM_VAL_KIND_F64 => Val::F64(wasmi::F64::from_bits(bits)),
        _ => Val::I32(0),
    }
}

/// Return the declared parameter count for a function export. `usize::MAX`
/// denotes either a non-function export or an invalid module/index.
#[no_mangle]
pub extern "C" fn perry_wasm_host_module_export_func_arity(
    module: *mut WasmModuleHandle,
    index: usize,
) -> usize {
    let Some(module) = (unsafe { module.as_ref() }) else {
        return usize::MAX;
    };
    let Some(export) = module.0.module.exports().nth(index) else {
        return usize::MAX;
    };
    match export.ty() {
        ExternType::Func(ty) => ty.params().len(),
        _ => usize::MAX,
    }
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
    import_callback: Option<WasmImportCallback>,
    import_context: u64,
    out_err: *mut *mut c_char,
) -> *mut WasmInstanceHandle {
    if module.is_null() {
        capture_err(out_err, WasmHostError::Link("null module".into()));
        return std::ptr::null_mut();
    }
    let module = unsafe { &*module };
    match instantiate_with_import_callback(module, import_callback, import_context) {
        Ok(i) => Box::into_raw(Box::new(i)),
        Err(e) => {
            capture_err(out_err, e);
            std::ptr::null_mut()
        }
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_set_import_context(
    inst: *mut WasmInstanceHandle,
    import_context: u64,
) {
    if let Some(inst) = unsafe { inst.as_mut() } {
        inst.inner.store.data_mut().import_context = import_context;
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_drop(inst: *mut WasmInstanceHandle) {
    if !inst.is_null() {
        unsafe { drop(Box::from_raw(inst)) };
    }
}

/// Return the byte length of the exported `memory`, or zero when absent.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_memory_len(inst: *mut WasmInstanceHandle) -> usize {
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return 0;
    };
    inst.inner
        .instance
        .get_memory(&inst.inner.store, "memory")
        .map(|memory| memory.data_size(&inst.inner.store))
        .unwrap_or(0)
}

/// Copy the exported `memory` into caller-provided storage.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_memory_copy(
    inst: *mut WasmInstanceHandle,
    out: *mut u8,
    len: usize,
) -> usize {
    if out.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return 0;
    };
    let Some(memory) = inst.inner.instance.get_memory(&inst.inner.store, "memory") else {
        return 0;
    };
    let data = memory.data(&inst.inner.store);
    let copied = data.len().min(len);
    unsafe { std::ptr::copy_nonoverlapping(data.as_ptr(), out, copied) };
    copied
}

/// Copy caller-provided bytes into the exported `memory`. This keeps the
/// wasmi allocation coherent with writes made through the JavaScript-facing
/// `memory.buffer` between exported function calls.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_memory_write(
    inst: *mut WasmInstanceHandle,
    data: *const u8,
    len: usize,
) -> usize {
    if data.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let Some(memory) = inst.inner.instance.get_memory(&inst.inner.store, "memory") else {
        return 0;
    };
    let target = memory.data_mut(&mut inst.inner.store);
    let copied = target.len().min(len);
    unsafe { std::ptr::copy_nonoverlapping(data, target.as_mut_ptr(), copied) };
    copied
}

fn instance_table(inst: &WasmInstanceHandle, name: &str) -> Option<Table> {
    inst.inner.instance.get_table(&inst.inner.store, name)
}

/// Return the current length of an exported table, or `usize::MAX` when the
/// export does not exist or cannot be represented on this platform.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_len(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
) -> usize {
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return usize::MAX;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return usize::MAX;
    };
    if let Some(len) = with_active_instance_tables(inst as *const _ as usize, |active| {
        active.lengths.get(name).copied()
    }) {
        return len.unwrap_or(usize::MAX);
    }
    instance_table(inst, name)
        .and_then(|table| usize::try_from(table.size(&inst.inner.store)).ok())
        .unwrap_or(usize::MAX)
}

/// Read an `externref` table entry. Perry stores the nan-boxed JavaScript
/// value bits inside wasmi's opaque `ExternRef`; null remains a real null ref.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_get(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    index: usize,
    out_bits: *mut u64,
    out_is_null: *mut i32,
) -> i32 {
    if out_bits.is_null() || out_is_null.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_ref() }) else {
        return 0;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    if let Some(value) = with_active_instance_tables(inst as *const _ as usize, |active| {
        active.overrides.get(&(name.to_string(), index)).copied()
    }) {
        let Some(value) = value else {
            // The instance Store is already borrowed by wasmi. Existing
            // entries that JS has not overwritten cannot be inspected until
            // the call unwinds.
            return 0;
        };
        unsafe {
            *out_bits = value.bits;
            *out_is_null = value.is_null as i32;
        }
        return 1;
    }
    let Some(table) = instance_table(inst, name) else {
        return 0;
    };
    let Some(Val::ExternRef(value)) = table.get(&inst.inner.store, index as u64) else {
        return 0;
    };
    match value {
        Ref::Null => unsafe {
            *out_bits = 0;
            *out_is_null = 1;
        },
        Ref::Val(value) => {
            let Some(bits) = value.data(&inst.inner.store).downcast_ref::<u64>() else {
                return 0;
            };
            unsafe {
                *out_bits = *bits;
                *out_is_null = 0;
            }
        }
    }
    1
}

fn table_value(inst: &mut WasmInstanceHandle, bits: u64, is_null: i32) -> Val {
    if is_null != 0 {
        Val::ExternRef(Ref::Null)
    } else {
        Val::from(ExternRef::new(&mut inst.inner.store, bits))
    }
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_set(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    index: usize,
    bits: u64,
    is_null: i32,
) -> i32 {
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    let pending_value = PendingTableValue {
        bits,
        is_null: is_null != 0,
    };
    if let Some(queued) = with_active_instance_tables(inst as *mut _ as usize, |active| {
        let Some(len) = active.lengths.get(name).copied() else {
            return false;
        };
        if index >= len {
            return false;
        }
        active
            .overrides
            .insert((name.to_string(), index), pending_value);
        active.ops.push(PendingTableOp::Set {
            name: name.to_string(),
            index,
            value: pending_value,
        });
        true
    }) {
        return queued as i32;
    }
    let Some(table) = instance_table(inst, name) else {
        return 0;
    };
    if table.ty(&inst.inner.store).element() != ValType::ExternRef {
        return 0;
    }
    let value = table_value(inst, bits, is_null);
    table
        .set(&mut inst.inner.store, index as u64, value)
        .is_ok() as i32
}

#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_table_grow(
    inst: *mut WasmInstanceHandle,
    name: *const c_char,
    name_len: usize,
    delta: usize,
    bits: u64,
    is_null: i32,
    out_old_len: *mut usize,
) -> i32 {
    if out_old_len.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let Some(name) = utf8_arg(name, name_len) else {
        return 0;
    };
    let pending_value = PendingTableValue {
        bits,
        is_null: is_null != 0,
    };
    if let Some(old_len) = with_active_instance_tables(inst as *mut _ as usize, |active| {
        let old_len = *active.lengths.get(name)?;
        let new_len = old_len.checked_add(delta)?;
        active.lengths.insert(name.to_string(), new_len);
        for index in old_len..new_len {
            active
                .overrides
                .insert((name.to_string(), index), pending_value);
        }
        active.ops.push(PendingTableOp::Grow {
            name: name.to_string(),
            delta,
            value: pending_value,
        });
        Some(old_len)
    }) {
        let Some(old_len) = old_len else {
            return 0;
        };
        unsafe { *out_old_len = old_len };
        return 1;
    }
    let Some(table) = instance_table(inst, name) else {
        return 0;
    };
    if table.ty(&inst.inner.store).element() != ValType::ExternRef {
        return 0;
    }
    let value = table_value(inst, bits, is_null);
    let Ok(old_len) = table.grow(&mut inst.inner.store, delta as u64, value) else {
        return 0;
    };
    let Ok(old_len) = usize::try_from(old_len) else {
        return 0;
    };
    unsafe { *out_old_len = old_len };
    1
}

/// Consume the status captured by WASI `proc_exit`, if that import ran.
#[no_mangle]
pub extern "C" fn perry_wasm_host_instance_take_exit_code(
    inst: *mut WasmInstanceHandle,
    out_code: *mut i32,
) -> i32 {
    if out_code.is_null() {
        return 0;
    }
    let Some(inst) = (unsafe { inst.as_mut() }) else {
        return 0;
    };
    let Some(code) = inst.inner.store.data_mut().exit_code.take() else {
        return 0;
    };
    unsafe { *out_code = code };
    1
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
/// (i32/f32 widened, i64/f64 as-is). On success writes every result into the
/// parallel output arrays and sets `*out_count`. On error returns 0 and writes
/// `*out_err`.
#[no_mangle]
pub extern "C" fn perry_wasm_host_call_export(
    inst: *mut WasmInstanceHandle,
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
) -> i32 {
    if inst.is_null()
        || name.is_null()
        || out_count.is_null()
        || (out_capacity != 0 && (out_kinds.is_null() || out_bits.is_null()))
    {
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
        Ok(values) if values.len() <= out_capacity => {
            for (index, value) in values.iter().copied().enumerate() {
                let (kind, bits) = match value {
                    WasmVal::I32(value) => (WASM_VAL_KIND_I32, value as u32 as u64),
                    WasmVal::I64(value) => (WASM_VAL_KIND_I64, value as u64),
                    WasmVal::F32(value) => (WASM_VAL_KIND_F32, value.to_bits() as u64),
                    WasmVal::F64(value) => (WASM_VAL_KIND_F64, value.to_bits()),
                };
                unsafe {
                    *out_kinds.add(index) = kind;
                    *out_bits.add(index) = bits;
                }
            }
            unsafe { *out_count = values.len() };
            1
        }
        Ok(values) => {
            capture_err(
                out_err,
                WasmHostError::UnsupportedSignature(format!(
                    "{name_str}: {} results exceed host capacity {out_capacity}",
                    values.len()
                )),
            );
            0
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
    /// Module with an exported memory, byte load/store helpers, and a
    /// two-result function whose input is declared as f64.
    const LIVE_MEMORY_MULTI_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x0f, 0x03, 0x60, 0x00, 0x01, 0x7f,
        0x60, 0x01, 0x7f, 0x00, 0x60, 0x01, 0x7c, 0x02, 0x7f, 0x7c, 0x03, 0x04, 0x03, 0x00, 0x01,
        0x02, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x20, 0x04, 0x06, 0x6d, 0x65, 0x6d, 0x6f, 0x72,
        0x79, 0x02, 0x00, 0x04, 0x6c, 0x6f, 0x61, 0x64, 0x00, 0x00, 0x05, 0x73, 0x74, 0x6f, 0x72,
        0x65, 0x00, 0x01, 0x04, 0x70, 0x61, 0x69, 0x72, 0x00, 0x02, 0x0a, 0x1a, 0x03, 0x07, 0x00,
        0x41, 0x00, 0x2d, 0x00, 0x00, 0x0b, 0x09, 0x00, 0x41, 0x00, 0x20, 0x00, 0x3a, 0x00, 0x00,
        0x0b, 0x06, 0x00, 0x41, 0x07, 0x20, 0x00, 0x0b,
    ];
    const EXTERNREF_TABLE_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x04, 0x04, 0x01, 0x6f, 0x00, 0x02, 0x07,
        0x08, 0x01, 0x04, 0x72, 0x65, 0x66, 0x73, 0x01, 0x00,
    ];
    const EXTERNREF_TABLE_CYCLE_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x04, 0x01, 0x60, 0x00, 0x00, 0x02,
        0x15, 0x01, 0x0c, 0x2e, 0x2f, 0x74, 0x61, 0x62, 0x6c, 0x65, 0x2d, 0x67, 0x6c, 0x75, 0x65,
        0x04, 0x69, 0x6e, 0x69, 0x74, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00, 0x04, 0x04, 0x01, 0x6f,
        0x00, 0x02, 0x07, 0x10, 0x02, 0x04, 0x72, 0x65, 0x66, 0x73, 0x01, 0x00, 0x05, 0x73, 0x74,
        0x61, 0x72, 0x74, 0x00, 0x01, 0x0a, 0x06, 0x01, 0x04, 0x00, 0x10, 0x00, 0x0b,
    ];
    /// `(module (import "env" "f" (func (param i32) (result i32))))`.
    const IMPORT_FUNC_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x06, 0x01, 0x60, 0x01, 0x7f, 0x01,
        0x7f, 0x02, 0x09, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x01, 0x66, 0x00, 0x00,
    ];
    /// `(module (import "env" "f" (func $f (result f64)))
    ///          (func (export "call") (result f64) call $f))`.
    const IMPORT_F64_RESULT_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7c,
        0x02, 0x09, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x01, 0x66, 0x00, 0x00, 0x03, 0x02, 0x01, 0x00,
        0x07, 0x08, 0x01, 0x04, 0x63, 0x61, 0x6c, 0x6c, 0x00, 0x01, 0x0a, 0x06, 0x01, 0x04, 0x00,
        0x10, 0x00, 0x0b,
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
        assert_eq!(result, vec![WasmVal::I32(5)]);
    }

    #[test]
    fn live_memory_round_trips_and_multi_results_are_preserved() {
        let module = compile(LIVE_MEMORY_MULTI_WASM).expect("compile");
        let mut inst = instantiate(&module).expect("instantiate");

        let input = [42u8];
        assert_eq!(
            perry_wasm_host_instance_memory_write(&mut inst, input.as_ptr(), input.len()),
            1
        );
        assert_eq!(
            call_export(&mut inst, "load", &[]).expect("load"),
            vec![WasmVal::I32(42)]
        );

        call_export(&mut inst, "store", &[WasmVal::I32(99)]).expect("store");
        let mut output = [0u8];
        assert_eq!(
            perry_wasm_host_instance_memory_copy(&mut inst, output.as_mut_ptr(), output.len()),
            1
        );
        assert_eq!(output, [99]);

        assert_eq!(
            call_export(&mut inst, "pair", &[WasmVal::I32(3)]).expect("pair"),
            vec![WasmVal::I32(7), WasmVal::F64(3.0)]
        );
    }

    #[test]
    fn exported_externref_table_round_trips_host_values() {
        let module = compile(EXTERNREF_TABLE_WASM).expect("compile");
        let mut inst = instantiate(&module).expect("instantiate");
        let name = b"refs";
        assert_eq!(
            perry_wasm_host_instance_table_len(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len()
            ),
            2
        );
        assert_eq!(
            perry_wasm_host_instance_table_set(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len(),
                1,
                0x1234,
                0,
            ),
            1
        );
        let mut bits = 0;
        let mut is_null = 1;
        assert_eq!(
            perry_wasm_host_instance_table_get(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len(),
                1,
                &mut bits,
                &mut is_null,
            ),
            1
        );
        assert_eq!((bits, is_null), (0x1234, 0));

        let mut old_len = 0;
        assert_eq!(
            perry_wasm_host_instance_table_grow(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len(),
                3,
                0,
                1,
                &mut old_len,
            ),
            1
        );
        assert_eq!(old_len, 2);
        assert_eq!(
            perry_wasm_host_instance_table_len(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len()
            ),
            5
        );
    }

    unsafe extern "C" fn reentrant_table_import_callback(
        context: u64,
        _module: *const u8,
        _module_len: usize,
        _name: *const u8,
        _name_len: usize,
        _arg_kinds: *const u8,
        _arg_bits: *const u64,
        _arg_count: usize,
        _result_kinds: *const u8,
        _result_bits: *mut u64,
        _result_count: usize,
    ) -> i32 {
        let inst = context as *mut WasmInstanceHandle;
        let name = b"refs";
        let mut old_len = 0;
        assert_eq!(
            perry_wasm_host_instance_table_grow(
                inst,
                name.as_ptr() as *const c_char,
                name.len(),
                2,
                0,
                1,
                &mut old_len,
            ),
            1
        );
        assert_eq!(old_len, 2);
        assert_eq!(
            perry_wasm_host_instance_table_set(
                inst,
                name.as_ptr() as *const c_char,
                name.len(),
                3,
                0x5678,
                0,
            ),
            1
        );
        1
    }

    #[test]
    fn table_mutations_from_import_callbacks_are_deferred_safely() {
        let module = compile(EXTERNREF_TABLE_CYCLE_WASM).expect("compile");
        let mut inst =
            instantiate_with_import_callback(&module, Some(reentrant_table_import_callback), 0)
                .expect("instantiate");
        inst.inner.store.data_mut().import_context =
            &mut inst as *mut WasmInstanceHandle as usize as u64;
        call_export(&mut inst, "start", &[]).expect("start");

        let name = b"refs";
        assert_eq!(
            perry_wasm_host_instance_table_len(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len(),
            ),
            4
        );
        let mut bits = 0;
        let mut is_null = 1;
        assert_eq!(
            perry_wasm_host_instance_table_get(
                &mut inst,
                name.as_ptr() as *const c_char,
                name.len(),
                3,
                &mut bits,
                &mut is_null,
            ),
            1
        );
        assert_eq!((bits, is_null), (0x5678, 0));
    }

    #[test]
    fn wasi_proc_exit_import_records_status() {
        let bytes =
            include_bytes!("../../../test-parity/node-suite/wasi/fixtures/exit-7-command.wasm");
        let module = compile(bytes).expect("compile");
        let mut inst = instantiate(&module).expect("instantiate with proc_exit");
        call_export(&mut inst, "_start", &[]).expect("call _start");
        assert_eq!(inst.inner.store.data_mut().exit_code.take(), Some(7));
    }

    #[test]
    fn placeholder_import_results_preserve_the_declared_wasm_type() {
        let module = compile(IMPORT_F64_RESULT_WASM).expect("compile");
        let mut inst = instantiate(&module).expect("instantiate with f64 import");
        let result = call_export(&mut inst, "call", &[]).expect("call import");
        assert_eq!(result, vec![WasmVal::F64(0.0)]);
    }

    unsafe extern "C" fn f64_import_callback(
        context: u64,
        module: *const u8,
        module_len: usize,
        name: *const u8,
        name_len: usize,
        _arg_kinds: *const u8,
        _arg_bits: *const u64,
        arg_count: usize,
        result_kinds: *const u8,
        result_bits: *mut u64,
        result_count: usize,
    ) -> i32 {
        assert_eq!(std::slice::from_raw_parts(module, module_len), b"env");
        assert_eq!(std::slice::from_raw_parts(name, name_len), b"f");
        assert_eq!(arg_count, 0);
        assert_eq!(result_count, 1);
        assert_eq!(*result_kinds, WASM_VAL_KIND_F64);
        *result_bits = f64::from_bits(context).to_bits();
        1
    }

    #[test]
    fn imported_functions_route_through_embedding_callback() {
        let module = compile(IMPORT_F64_RESULT_WASM).expect("compile");
        let mut inst =
            instantiate_with_import_callback(&module, Some(f64_import_callback), 6.5f64.to_bits())
                .expect("instantiate with callback");
        let result = call_export(&mut inst, "call", &[]).expect("call import");
        assert_eq!(result, vec![WasmVal::F64(6.5)]);
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
        assert_eq!(perry_wasm_host_module_export_func_arity(add, 0), 2);
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
