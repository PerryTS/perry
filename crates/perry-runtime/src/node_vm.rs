//! Narrow `node:vm` execution and experimental module lifecycle support.
//!
//! Perry is V8-free, so this is not a full JavaScript interpreter. It models the
//! Node-observable VM shape plus deterministic local subsets used by the VM
//! parity fixtures: context markers, object-backed sandbox reads/writes,
//! repeated `Script` execution, `runIn*Context`, `compileFunction`, and gated
//! `SourceTextModule`/`SyntheticModule` lifecycle behavior.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::array::ArrayHeader;
use crate::buffer::BufferHeader;
use crate::closure::ClosureHeader;
use crate::object::{ObjectHeader, PropertyAttrs};
use crate::string::StringHeader;
use crate::value::JSValue;

const STATUS_UNLINKED: &str = "unlinked";
const STATUS_LINKING: &str = "linking";
const STATUS_LINKED: &str = "linked";
const STATUS_EVALUATING: &str = "evaluating";
const STATUS_EVALUATED: &str = "evaluated";
const STATUS_ERRORED: &str = "errored";

const KIND_SOURCE: &str = "source";
const KIND_SYNTHETIC: &str = "synthetic";

const FIELD_KIND: &str = "__vm_kind";
const FIELD_STATUS: &str = "__vm_status";
const FIELD_IDENTIFIER: &str = "__vm_identifier";
const FIELD_ERROR: &str = "__vm_error";
const FIELD_NAMESPACE: &str = "__vm_namespace";
const FIELD_SOURCE: &str = "__vm_source";
const FIELD_REQUESTS: &str = "__vm_requests";
const FIELD_IMPORTS: &str = "__vm_imports";
const FIELD_EXPORTS: &str = "__vm_exports";
const FIELD_LINKED_MODULES: &str = "__vm_linked_modules";
const FIELD_EVALUATE_CALLBACK: &str = "__vm_evaluate_callback";
const FIELD_CONTEXT: &str = "__vm_context";

static MODULE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
const CACHE_PREFIX: &[u8] = b"PERRY_VM_CACHE\0";
const CACHE_KIND_SCRIPT: u8 = 1;
const CACHE_KIND_FUNCTION: u8 = 2;
const CACHE_KIND_MODULE: u8 = 3;

#[derive(Clone)]
struct ScriptMetadata {
    source: String,
    filename: String,
    line_offset: i32,
    column_offset: i32,
}

static VM_COMPILED_FUNCTION_SOURCES: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();

thread_local! {
    static VM_INTRINSIC_GLOBAL: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    static VM_CONTEXTS: std::cell::RefCell<HashMap<usize, ContextState>> =
        std::cell::RefCell::new(HashMap::new());
    static MAIN_CONTEXT: std::cell::RefCell<Option<ContextState>> =
        const { std::cell::RefCell::new(None) };
}

#[derive(Clone)]
struct ContextState {
    returned_bits: u64,
    sandbox_bits: u64,
    global_this_bits: u64,
    intrinsics_bits: u64,
    lexical_env_bits: u64,
    strings_allowed: bool,
    wasm_allowed: bool,
    microtask_after_evaluate: bool,
}

#[derive(Clone, Copy)]
struct ContextOptions {
    strings_allowed: bool,
    wasm_allowed: bool,
    microtask_after_evaluate: bool,
}

impl Default for ContextOptions {
    fn default() -> Self {
        Self {
            strings_allowed: true,
            wasm_allowed: true,
            microtask_after_evaluate: false,
        }
    }
}

static VM_SCRIPTS: OnceLock<Mutex<HashMap<usize, ScriptMetadata>>> = OnceLock::new();

fn scripts() -> &'static Mutex<HashMap<usize, ScriptMetadata>> {
    VM_SCRIPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn compiled_function_sources() -> &'static Mutex<HashMap<usize, String>> {
    VM_COMPILED_FUNCTION_SOURCES.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn compiled_function_source_for_closure(closure: usize) -> Option<String> {
    VM_COMPILED_FUNCTION_SOURCES
        .get()
        .and_then(|sources| sources.lock().ok()?.get(&closure).cloned())
}

pub(crate) fn function_source_for_closure(closure: usize) -> String {
    compiled_function_source_for_closure(closure).unwrap_or_else(|| {
        let func_ptr = unsafe { (*(closure as *const ClosureHeader)).func_ptr as usize };
        crate::builtins::function_source_for_func_ptr(func_ptr)
    })
}

#[derive(Clone, Debug)]
struct ImportBinding {
    specifier: String,
    imported: String,
    local: String,
}

#[derive(Clone, Debug)]
struct ExportBinding {
    name: String,
    expr: String,
}

#[derive(Clone, Debug)]
struct ParsedSource {
    requests: Vec<String>,
    imports: Vec<ImportBinding>,
    exports: Vec<ExportBinding>,
    has_top_level_await: bool,
}

pub fn vm_modules_enabled() -> bool {
    std::env::var_os("PERRY_EXPERIMENTAL_VM_MODULES").is_some()
}

fn undefined_value() -> f64 {
    f64::from_bits(JSValue::undefined().bits())
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn string_ptr(value: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32)
}

fn string_value(value: &str) -> f64 {
    f64::from_bits(JSValue::string_ptr(string_ptr(value)).bits())
}

fn object_value(obj: *mut ObjectHeader) -> f64 {
    crate::value::js_nanbox_pointer(obj as i64)
}

fn array_value(arr: *mut ArrayHeader) -> f64 {
    crate::value::js_nanbox_pointer(arr as i64)
}

fn buffer_value(buf: *mut BufferHeader) -> f64 {
    crate::value::js_nanbox_pointer(buf as i64)
}

fn raw_addr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jv = JSValue::from_bits(bits);
    if jv.is_pointer() || jv.is_string() {
        (bits & 0x0000_FFFF_FFFF_FFFF) as usize
    } else if !value.is_nan() && (0x1000..0x0001_0000_0000_0000).contains(&bits) {
        bits as usize
    } else {
        0
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null()
        || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000
        || unsafe { crate::symbol::js_is_symbol(value) != 0 }
        || crate::closure::is_closure_ptr(ptr as usize)
    {
        return None;
    }
    unsafe {
        let gc = ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
        if (*gc).obj_type != crate::gc::GC_TYPE_OBJECT {
            return None;
        }
    }
    Some(ptr as *mut ObjectHeader)
}

fn array_ptr_from_value(value: f64) -> Option<*mut ArrayHeader> {
    if crate::array::js_array_is_array(value).to_bits() != JSValue::bool(true).bits() {
        return None;
    }
    let raw = crate::value::js_nanbox_get_pointer(value);
    if raw == 0 {
        None
    } else {
        Some(raw as *mut ArrayHeader)
    }
}

fn field_key(name: &str) -> *mut StringHeader {
    string_ptr(name)
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let value = scope.root_nanbox_f64(value);
    let key = scope.root_string_ptr(field_key(name));
    crate::object::js_object_set_field_by_name(
        obj.get_raw_mut_ptr::<ObjectHeader>(),
        key.get_raw_mut_ptr::<StringHeader>(),
        value.get_nanbox_f64(),
    );
}

fn get_field(obj: *mut ObjectHeader, name: &str) -> f64 {
    crate::object::js_object_get_field_by_name_f64(obj, field_key(name))
}

fn set_value_field(value: f64, name: &str, field_value: f64) {
    if let Some(ptr) = object_ptr_from_value(value) {
        set_field(ptr, name, field_value);
        return;
    }
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<ObjectHeader>() as *mut ObjectHeader;
        crate::object::js_object_set_field_by_name(ptr, field_key(name), field_value);
    }
}

fn set_object_field(object: f64, name: &str, value: f64) {
    if let Some(ptr) = object_ptr_from_value(object) {
        set_field(ptr, name, value);
    }
}

fn get_string_field(obj: *mut ObjectHeader, name: &str) -> Option<String> {
    string_from_value(get_field(obj, name))
}

fn module_options(options: f64) -> (f64, String) {
    let options = options_object_or_default(options);
    let identifier = options
        .map(|obj| get_field(obj, "identifier"))
        .filter(|value| !JSValue::from_bits(value.to_bits()).is_undefined())
        .map(|value| {
            string_from_value(value).unwrap_or_else(|| {
                let message = format!(
                    "The \"options.identifier\" property must be of type string. Received {}",
                    crate::fs::validate::describe_received(value)
                );
                throw_invalid_arg(&message);
            })
        })
        .unwrap_or_else(default_identifier);
    let context = options
        .map(|obj| get_field(obj, "context"))
        .filter(|value| !JSValue::from_bits(value.to_bits()).is_undefined())
        .map(|value| require_context(value, "options.context"))
        .unwrap_or_else(|| create_context(undefined_value(), undefined_value()));
    (context, identifier)
}

fn default_identifier() -> String {
    let id = MODULE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("vm:module({id})")
}

fn throw_vm_unimplemented(api: &str, issue: &str) -> f64 {
    let message = format!("node:vm {api} is not implemented in Perry (tracked by #{issue}).");
    crate::fs::validate::throw_error_with_code(&message, "ERR_PERRY_VM_UNIMPLEMENTED")
}

fn throw_invalid_arg(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn throw_invalid_arg_value(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_VALUE")
}

fn throw_vm_status(message: &str) -> f64 {
    crate::fs::validate::throw_error_with_code(message, "ERR_VM_MODULE_STATUS")
}

fn throw_vm_type(message: &str) -> f64 {
    crate::fs::validate::throw_error_with_code(message, "ERR_INVALID_ARG_TYPE")
}

fn throw_type_error_no_code(message: &str) -> f64 {
    let msg = string_ptr(message);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_reference_error_no_code(message: &str) -> f64 {
    let msg = string_ptr(message);
    let err = crate::error::js_referenceerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_vm_module_cached_data_rejected() -> f64 {
    crate::fs::validate::throw_error_with_code(
        "cachedData buffer was rejected",
        "ERR_VM_MODULE_CACHED_DATA_REJECTED",
    )
}

fn throw_vm_module_cannot_create_cached_data() -> f64 {
    crate::fs::validate::throw_error_with_code(
        "Cached data cannot be created for a module which has been evaluated",
        "ERR_VM_MODULE_CANNOT_CREATE_CACHED_DATA",
    )
}

fn option_field(options: f64, name: &str) -> f64 {
    object_ptr_from_value(options)
        .map(|obj| get_field(obj, name))
        .unwrap_or_else(undefined_value)
}

fn options_object_or_default(options: f64) -> Option<*mut ObjectHeader> {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() {
        return None;
    }
    object_ptr_from_value(options).or_else(|| {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(options)
        );
        throw_invalid_arg(&message);
    })
}

fn validate_produce_cached_data(options: f64) -> bool {
    let value = option_field(options, "produceCachedData");
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return false;
    }
    if jv.is_bool() {
        return jv.as_bool();
    }
    let message = format!(
        "The \"options.produceCachedData\" property must be of type boolean. Received {}",
        crate::fs::validate::describe_received(value)
    );
    throw_invalid_arg(&message);
}

fn typed_array_or_buffer_bytes(value: f64) -> Option<Vec<u8>> {
    let mut len = 0_u32;
    let ptr = unsafe { crate::buffer::js_value_buffer_or_typedarray_data(value, &mut len) };
    if !ptr.is_null() {
        return Some(unsafe { std::slice::from_raw_parts(ptr, len as usize).to_vec() });
    }
    let addr = raw_addr_from_value(value);
    if addr != 0 && crate::buffer::is_data_view(addr) {
        return Some(Vec::new());
    }
    None
}

fn validate_cached_data_option(options: f64) -> Option<Vec<u8>> {
    let value = option_field(options, "cachedData");
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return None;
    }
    if let Some(bytes) = typed_array_or_buffer_bytes(value) {
        return Some(bytes);
    }
    let message = format!(
        "The \"options.cachedData\" property must be an instance of Buffer, TypedArray, or DataView. Received {}",
        crate::fs::validate::describe_received(value)
    );
    throw_invalid_arg(&message);
}

fn validate_one_of_string(
    value: f64,
    property: &str,
    allowed: &[&str],
    default_value: &str,
) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return default_value.to_string();
    }
    if let Some(value) = string_from_value(value) {
        if allowed.iter().any(|allowed| value == *allowed) {
            return value;
        }
    }
    let expected = allowed
        .iter()
        .map(|value| format!("'{value}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let message = format!(
        "The property '{property}' must be one of: {expected}. Received {}",
        crate::fs::validate::describe_received(value)
    );
    throw_invalid_arg_value(&message);
}

fn source_hash(kind: u8, source: &str, params: &[String]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in [kind]
        .iter()
        .copied()
        .chain(source.as_bytes().iter().copied())
    {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    for param in params {
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        for byte in param.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    hash
}

fn cached_data_bytes(kind: u8, hash: u64) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(CACHE_PREFIX.len() + 9);
    bytes.extend_from_slice(CACHE_PREFIX);
    bytes.push(kind);
    bytes.extend_from_slice(&hash.to_le_bytes());
    bytes
}

fn cache_bytes_accepted(bytes: &[u8], kind: u8, hash: u64) -> bool {
    bytes == cached_data_bytes(kind, hash).as_slice()
}

fn cached_data_buffer(kind: u8, hash: u64) -> f64 {
    let bytes = cached_data_bytes(kind, hash);
    let buf = crate::buffer::buffer_alloc(bytes.len() as u32);
    unsafe {
        (*buf).length = bytes.len() as u32;
        std::ptr::copy_nonoverlapping(
            bytes.as_ptr(),
            crate::buffer::buffer_data_mut(buf),
            bytes.len(),
        );
    }
    buffer_value(buf)
}

fn extract_source_map_url(source: &str) -> Option<String> {
    for line in source.lines().rev() {
        let trimmed = line.trim();
        let marker = if let Some(idx) = trimmed.find("sourceMappingURL=") {
            idx + "sourceMappingURL=".len()
        } else {
            continue;
        };
        let tail = trimmed[marker..].trim();
        let tail = tail.strip_suffix("*/").unwrap_or(tail).trim();
        if !tail.is_empty() {
            return Some(tail.to_string());
        }
    }
    None
}

fn split_source_statements(source: &str) -> Vec<String> {
    source
        .split(';')
        .flat_map(|part| {
            let trimmed = part.trim();
            if trimmed.contains('\n') {
                trimmed
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else if trimmed.is_empty() {
                Vec::new()
            } else {
                vec![trimmed.to_string()]
            }
        })
        .collect()
}

fn extract_quoted(input: &str) -> Option<String> {
    let mut quote_start = None;
    let mut quote_byte = b'\0';
    for (idx, byte) in input.as_bytes().iter().copied().enumerate() {
        if byte == b'\'' || byte == b'"' {
            quote_start = Some(idx + 1);
            quote_byte = byte;
            break;
        }
    }
    let start = quote_start?;
    let rest = &input[start..];
    let end_rel = rest.as_bytes().iter().position(|b| *b == quote_byte)?;
    Some(rest[..end_rel].to_string())
}

fn parse_import_clause(stmt: &str, specifier: &str) -> Vec<ImportBinding> {
    let Some(open) = stmt.find('{') else {
        return Vec::new();
    };
    let Some(close_rel) = stmt[open + 1..].find('}') else {
        return Vec::new();
    };
    let close = open + 1 + close_rel;
    stmt[open + 1..close]
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return None;
            }
            let (imported, local) = if let Some(as_idx) = part.find(" as ") {
                (
                    part[..as_idx].trim().to_string(),
                    part[as_idx + 4..].trim().to_string(),
                )
            } else {
                (part.to_string(), part.to_string())
            };
            Some(ImportBinding {
                specifier: specifier.to_string(),
                imported,
                local,
            })
        })
        .collect()
}

fn parse_export_const(stmt: &str) -> Option<ExportBinding> {
    let prefixes = ["export const ", "export let ", "export var "];
    let body = prefixes
        .iter()
        .find_map(|prefix| stmt.strip_prefix(prefix))?;
    let eq = body.find('=')?;
    let name = body[..eq].trim();
    if name.is_empty() {
        return None;
    }
    Some(ExportBinding {
        name: name.to_string(),
        expr: body[eq + 1..].trim().to_string(),
    })
}

fn parse_source(source: &str) -> ParsedSource {
    let mut requests = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();

    for stmt in split_source_statements(source) {
        if stmt.starts_with("import ") {
            if let Some(specifier) = stmt
                .find(" from ")
                .and_then(|idx| extract_quoted(&stmt[idx..]))
            {
                if !requests.iter().any(|existing| existing == &specifier) {
                    requests.push(specifier.clone());
                }
                imports.extend(parse_import_clause(&stmt, &specifier));
            } else if let Some(specifier) = extract_quoted(&stmt) {
                if !requests.iter().any(|existing| existing == &specifier) {
                    requests.push(specifier);
                }
            }
        } else if let Some(export) = parse_export_const(&stmt) {
            exports.push(export);
        }
    }

    ParsedSource {
        requests,
        imports,
        exports,
        has_top_level_await: source.contains("await "),
    }
}

fn validate_module_source(source: &str) {
    if perry_parser::parse_typescript(source, "perry-vm-module.mjs").is_err() {
        throw_syntax("Invalid module source");
    }
}

fn strings_array(strings: &[String]) -> f64 {
    let mut arr = crate::array::js_array_alloc(strings.len() as u32);
    for value in strings {
        arr = crate::array::js_array_push_f64(arr, string_value(value));
    }
    array_value(arr)
}

fn requests_array(requests: &[String]) -> f64 {
    let mut arr = crate::array::js_array_alloc(requests.len() as u32);
    for specifier in requests {
        let obj = crate::object::js_object_alloc_null_proto(0, 3);
        set_field(obj, "specifier", string_value(specifier));
        set_field(
            obj,
            "attributes",
            object_value(crate::object::js_object_alloc(0, 0)),
        );
        set_field(obj, "phase", string_value("evaluation"));
        arr = crate::array::js_array_push_f64(arr, object_value(obj));
    }
    array_value(arr)
}

fn imports_array(imports: &[ImportBinding]) -> f64 {
    let mut arr = crate::array::js_array_alloc(imports.len() as u32);
    for import in imports {
        let obj = crate::object::js_object_alloc(0, 3);
        set_field(obj, "specifier", string_value(&import.specifier));
        set_field(obj, "imported", string_value(&import.imported));
        set_field(obj, "local", string_value(&import.local));
        arr = crate::array::js_array_push_f64(arr, object_value(obj));
    }
    array_value(arr)
}

fn exports_array(exports: &[ExportBinding]) -> f64 {
    let mut arr = crate::array::js_array_alloc(exports.len() as u32);
    for export in exports {
        let obj = crate::object::js_object_alloc(0, 2);
        set_field(obj, "name", string_value(&export.name));
        set_field(obj, "expr", string_value(&export.expr));
        arr = crate::array::js_array_push_f64(arr, object_value(obj));
    }
    array_value(arr)
}

fn read_imports(module: *mut ObjectHeader) -> Vec<ImportBinding> {
    let Some(arr) = array_ptr_from_value(get_field(module, FIELD_IMPORTS)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let len = crate::array::js_array_length(arr);
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(arr, idx);
        let Some(obj) = object_ptr_from_value(value) else {
            continue;
        };
        let Some(specifier) = get_string_field(obj, "specifier") else {
            continue;
        };
        let Some(imported) = get_string_field(obj, "imported") else {
            continue;
        };
        let Some(local) = get_string_field(obj, "local") else {
            continue;
        };
        out.push(ImportBinding {
            specifier,
            imported,
            local,
        });
    }
    out
}

fn read_exports(module: *mut ObjectHeader) -> Vec<ExportBinding> {
    let Some(arr) = array_ptr_from_value(get_field(module, FIELD_EXPORTS)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let len = crate::array::js_array_length(arr);
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(arr, idx);
        let Some(obj) = object_ptr_from_value(value) else {
            continue;
        };
        let Some(name) = get_string_field(obj, "name") else {
            continue;
        };
        let Some(expr) = get_string_field(obj, "expr") else {
            continue;
        };
        out.push(ExportBinding { name, expr });
    }
    out
}

fn read_requests(module: *mut ObjectHeader) -> Vec<String> {
    let Some(arr) = array_ptr_from_value(get_field(module, FIELD_REQUESTS)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let len = crate::array::js_array_length(arr);
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(arr, idx);
        let Some(obj) = object_ptr_from_value(value) else {
            continue;
        };
        if let Some(specifier) = get_string_field(obj, "specifier") {
            out.push(specifier);
        }
    }
    out
}

fn namespace_for_module(module: *mut ObjectHeader) -> Option<*mut ObjectHeader> {
    object_ptr_from_value(get_field(module, FIELD_NAMESPACE))
}

fn module_status(module: *mut ObjectHeader) -> String {
    get_string_field(module, FIELD_STATUS).unwrap_or_else(|| STATUS_UNLINKED.to_string())
}

fn module_kind(module: *mut ObjectHeader) -> String {
    get_string_field(module, FIELD_KIND).unwrap_or_default()
}

fn set_status(module: *mut ObjectHeader, status: &str) {
    set_field(module, FIELD_STATUS, string_value(status));
    set_field(module, "status", string_value(status));
}

fn module_linked_modules(module: *mut ObjectHeader) -> Option<*mut ArrayHeader> {
    array_ptr_from_value(get_field(module, FIELD_LINKED_MODULES))
}

fn module_for_specifier(module: *mut ObjectHeader, specifier: &str) -> Option<*mut ObjectHeader> {
    let requests = read_requests(module);
    let index = requests.iter().position(|request| request == specifier)?;
    let linked = module_linked_modules(module)?;
    let value = crate::array::js_array_get_f64(linked, index as u32);
    object_ptr_from_value(value)
}

fn module_request_extra() -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    set_field(
        obj,
        "attributes",
        object_value(crate::object::js_object_alloc(0, 0)),
    );
    set_field(
        obj,
        "assert",
        object_value(crate::object::js_object_alloc(0, 0)),
    );
    object_value(obj)
}

fn build_import_env(module: *mut ObjectHeader) -> HashMap<String, f64> {
    let mut env = HashMap::new();
    for import in read_imports(module) {
        let Some(dep) = module_for_specifier(module, &import.specifier) else {
            continue;
        };
        let Some(ns) = namespace_for_module(dep) else {
            continue;
        };
        env.insert(import.local, get_field(ns, &import.imported));
    }
    env
}

fn evaluate_source_module(module: *mut ObjectHeader) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_raw_mut_ptr(module);
    let status = module_status(module.get_raw_mut_ptr::<ObjectHeader>());
    if status != STATUS_LINKED && status != STATUS_EVALUATED {
        return throw_vm_status("Module status must be linked");
    }
    if status == STATUS_EVALUATED {
        return undefined_value();
    }

    set_status(module.get_raw_mut_ptr::<ObjectHeader>(), STATUS_EVALUATING);
    let Some(namespace) = namespace_for_module(module.get_raw_mut_ptr::<ObjectHeader>()) else {
        set_status(module.get_raw_mut_ptr::<ObjectHeader>(), STATUS_ERRORED);
        return throw_vm_status("Module namespace is unavailable");
    };
    let namespace = scope.root_raw_mut_ptr(namespace);

    let source = get_string_field(module.get_raw_mut_ptr::<ObjectHeader>(), FIELD_SOURCE)
        .unwrap_or_default();
    let context = scope.root_nanbox_f64(get_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_CONTEXT,
    ));
    for (name, value) in build_import_env(module.get_raw_mut_ptr::<ObjectHeader>()) {
        set_object_field(context.get_nanbox_f64(), &name, value);
    }
    let executable = split_source_statements(&source)
        .into_iter()
        .filter(|stmt| !stmt.starts_with("import "))
        .map(|stmt| stmt.strip_prefix("export ").unwrap_or(&stmt).to_string())
        .collect::<Vec<_>>()
        .join(";");
    let lexical = scope.root_nanbox_f64(crate::dyn_eval::script_environment(
        context.get_nanbox_f64(),
        &[],
    ));
    if let Err(error) = crate::exception::js_call_catching(|| {
        crate::dyn_eval::eval_script_in(
            &executable,
            context.get_nanbox_f64(),
            context.get_nanbox_f64(),
            lexical.get_nanbox_f64(),
        )
    }) {
        set_field(module.get_raw_mut_ptr::<ObjectHeader>(), FIELD_ERROR, error);
        set_status(module.get_raw_mut_ptr::<ObjectHeader>(), STATUS_ERRORED);
        return error;
    }
    for export in read_exports(module.get_raw_mut_ptr::<ObjectHeader>()) {
        set_field(
            namespace.get_raw_mut_ptr::<ObjectHeader>(),
            &export.name,
            crate::dyn_eval::script_binding(lexical.get_nanbox_f64(), &export.name),
        );
    }
    set_status(module.get_raw_mut_ptr::<ObjectHeader>(), STATUS_EVALUATED);
    undefined_value()
}

fn evaluate_synthetic_module(module: *mut ObjectHeader) -> f64 {
    let status = module_status(module);
    if status == STATUS_EVALUATED {
        return undefined_value();
    }
    if status != STATUS_LINKED {
        return throw_vm_status("Module status must be linked");
    }

    set_status(module, STATUS_EVALUATING);
    let callback = get_field(module, FIELD_EVALUATE_CALLBACK);
    let js = JSValue::from_bits(callback.to_bits());
    if !js.is_undefined() && !js.is_null() {
        let prev = crate::object::js_implicit_this_set(object_value(module));
        let _ = unsafe { crate::closure::js_native_call_value(callback, std::ptr::null(), 0) };
        crate::object::js_implicit_this_set(prev);
    }
    set_status(module, STATUS_EVALUATED);
    undefined_value()
}

fn module_has_tla(module: *mut ObjectHeader) -> bool {
    let Some(source) = get_string_field(module, FIELD_SOURCE) else {
        return false;
    };
    parse_source(&source).has_top_level_await
}

fn module_has_async_graph(module: *mut ObjectHeader) -> bool {
    if module_has_tla(module) {
        return true;
    }
    let Some(linked) = module_linked_modules(module) else {
        return false;
    };
    let len = crate::array::js_array_length(linked);
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(linked, idx);
        if let Some(dep) = object_ptr_from_value(value) {
            if module_has_async_graph(dep) {
                return true;
            }
        }
    }
    false
}

fn new_module_base(kind: &str, status: &str, identifier: String) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = crate::object::js_object_alloc(0, 16);
    let module = scope.root_raw_mut_ptr(module);
    let value = string_value(kind);
    set_field(module.get_raw_mut_ptr::<ObjectHeader>(), FIELD_KIND, value);
    let value = string_value(status);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_STATUS,
        value,
    );
    let value = string_value(status);
    set_field(module.get_raw_mut_ptr::<ObjectHeader>(), "status", value);
    let value = string_value(&identifier);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_IDENTIFIER,
        value,
    );
    let value = string_value(&identifier);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "identifier",
        value,
    );
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_ERROR,
        undefined_value(),
    );
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "error",
        undefined_value(),
    );
    let value = array_value(crate::array::js_array_alloc(0));
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_LINKED_MODULES,
        value,
    );
    module.get_raw_mut_ptr::<ObjectHeader>()
}

extern "C" fn module_namespace_getter(closure: *const ClosureHeader) -> f64 {
    js_vm_module_namespace(crate::closure::js_closure_get_capture_f64(closure, 0))
}

extern "C" fn module_error_getter(closure: *const ClosureHeader) -> f64 {
    js_vm_module_error(crate::closure::js_closure_get_capture_f64(closure, 0))
}

fn install_module_accessor(
    module: *mut ObjectHeader,
    name: &str,
    getter: extern "C" fn(*const ClosureHeader) -> f64,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let module = scope.root_raw_mut_ptr(module);
    let closure = crate::closure::js_closure_alloc(getter as *const u8, 1);
    let closure = scope.root_raw_mut_ptr(closure);
    crate::closure::js_register_closure_arity(getter as *const u8, 0);
    crate::closure::js_closure_set_capture_f64(
        closure.get_raw_mut_ptr::<ClosureHeader>(),
        0,
        object_value(module.get_raw_mut_ptr::<ObjectHeader>()),
    );
    unsafe {
        crate::closure::rebuild_closure_layout_and_barriers(
            closure.get_raw_mut_ptr::<ClosureHeader>(),
            1,
        );
    }
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        name,
        undefined_value(),
    );
    crate::object::set_builtin_accessor_descriptor(
        module.get_raw_mut_ptr::<ObjectHeader>() as usize,
        name.to_string(),
        crate::object::AccessorDescriptor {
            get: crate::value::js_nanbox_pointer(closure.get_raw_mut_ptr::<ClosureHeader>() as i64)
                .to_bits(),
            set: 0,
        },
        PropertyAttrs::new(false, false, false),
    );
}

fn set_module_namespace_tag(namespace: *mut ObjectHeader) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let namespace = scope.root_raw_mut_ptr(namespace);
    let symbol = crate::symbol::well_known_symbol("toStringTag");
    if symbol.is_null() {
        return;
    }
    let symbol = scope.root_raw_mut_ptr(symbol);
    let tag = scope.root_nanbox_f64(string_value("Module"));
    unsafe {
        crate::symbol::js_object_set_symbol_property(
            object_value(namespace.get_raw_mut_ptr::<ObjectHeader>()),
            crate::value::js_nanbox_pointer(
                symbol.get_raw_mut_ptr::<crate::symbol::SymbolHeader>() as i64,
            ),
            tag.get_nanbox_f64(),
        );
    }
}

fn throw_type_error(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_typeerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn throw_syntax(message: &str) -> ! {
    let msg = crate::string::js_string_from_bytes(message.as_ptr(), message.len() as u32);
    let err = crate::error::js_syntaxerror_new(msg);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(err as i64))
}

fn rust_string_from_header(ptr: *const StringHeader) -> Option<String> {
    if ptr.is_null() || (ptr as usize) < 0x1000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn string_from_value(value: f64) -> Option<String> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    rust_string_from_header(ptr)
}

fn code_string_required(value: f64, name: &str) -> String {
    string_from_value(value).unwrap_or_else(|| {
        let message = format!(
            "The \"{name}\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(value)
        );
        throw_invalid_arg(&message);
    })
}

fn code_string_for_script(value: f64) -> String {
    if let Some(code) = string_from_value(value) {
        return code;
    }
    let ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    rust_string_from_header(ptr).unwrap_or_default()
}

#[derive(Clone)]
struct SourceOptions {
    filename: String,
    line_offset: i32,
    column_offset: i32,
}

impl Default for SourceOptions {
    fn default() -> Self {
        Self {
            filename: "evalmachine.<anonymous>".to_string(),
            line_offset: 0,
            column_offset: 0,
        }
    }
}

fn source_options(options: f64, allow_string_filename: bool) -> SourceOptions {
    if allow_string_filename {
        if let Some(filename) = string_from_value(options) {
            return SourceOptions {
                filename,
                ..SourceOptions::default()
            };
        }
    }
    let Some(options) = options_object_or_default(options) else {
        return SourceOptions::default();
    };
    let mut result = SourceOptions::default();
    let filename = get_field(options, "filename");
    if !JSValue::from_bits(filename.to_bits()).is_undefined() {
        crate::validators::validate_string(filename, "options.filename");
        result.filename = string_from_value(filename).unwrap_or_default();
    }
    let line_offset = get_field(options, "lineOffset");
    if !JSValue::from_bits(line_offset.to_bits()).is_undefined() {
        result.line_offset = crate::validators::validate_int32(
            line_offset,
            "options.lineOffset",
            i32::MIN,
            i32::MAX,
        );
    }
    let column_offset = get_field(options, "columnOffset");
    if !JSValue::from_bits(column_offset.to_bits()).is_undefined() {
        result.column_offset = crate::validators::validate_int32(
            column_offset,
            "options.columnOffset",
            i32::MIN,
            i32::MAX,
        );
    }
    result
}

fn validate_run_options(options: f64) {
    let Some(options) = options_object_or_default(options) else {
        return;
    };
    let timeout = get_field(options, "timeout");
    if !JSValue::from_bits(timeout.to_bits()).is_undefined() {
        crate::validators::validate_integer(timeout, "options.timeout", 1.0, u32::MAX as f64);
    }
    for name in ["displayErrors", "breakOnSigint"] {
        let value = get_field(options, name);
        if !JSValue::from_bits(value.to_bits()).is_undefined() {
            crate::validators::validate_boolean(value, &format!("options.{name}"));
        }
    }
}

fn with_source_location(options: &SourceOptions, f: impl FnOnce() -> f64) -> f64 {
    let old = crate::error::replace_runtime_source_location(Some((
        options.filename.clone(),
        (options.line_offset as i64 + 1).max(1) as u32,
        (options.column_offset as i64 + 1).max(1) as u32,
    )));
    let result = crate::exception::js_call_catching(f);
    crate::error::replace_runtime_source_location(old);
    match result {
        Ok(value) => value,
        Err(error) => crate::exception::js_throw(error),
    }
}

fn symbol_key(value: f64) -> Option<String> {
    if unsafe { crate::symbol::js_is_symbol(value) == 0 } {
        return None;
    }
    let key = unsafe { crate::symbol::js_symbol_key_for(value) };
    string_from_value(key)
}

fn is_dont_contextify(value: f64) -> bool {
    symbol_key(value).as_deref() == Some("vm_context_no_contextify")
}

fn value_is_object_like(value: f64) -> bool {
    object_ptr_from_value(value).is_some()
        || array_ptr_from_value(value).is_some()
        || crate::proxy::js_proxy_is_proxy(value) != 0
}

fn context_key(value: f64) -> usize {
    raw_addr_from_value(value)
}

fn fresh_intrinsic_global() -> f64 {
    // ponytail: share one VM intrinsic realm until Perry has a cheap realm-graph
    // clone; rebuilding the 1.15 MB bootstrap per context exceeds the runner timeout.
    let cached = VM_INTRINSIC_GLOBAL.with(|slot| slot.get());
    if cached != 0 {
        return f64::from_bits(cached);
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let intrinsics = crate::object::js_object_alloc(0, 0);
    let intrinsics = scope.root_raw_mut_ptr(intrinsics);
    crate::object::populate_global_this_builtins(intrinsics.get_raw_mut_ptr::<ObjectHeader>());
    crate::object::js_object_delete_field(
        intrinsics.get_raw_mut_ptr::<ObjectHeader>(),
        string_ptr("process"),
    );
    let intrinsics = intrinsics.get_raw_mut_ptr::<ObjectHeader>();
    let value = object_value(intrinsics);
    VM_INTRINSIC_GLOBAL.with(|slot| {
        slot.set(value.to_bits());
        crate::gc::runtime_write_barrier_root_heap_word(intrinsics as u64);
        crate::gc::js_gc_register_global_root(slot.as_ptr() as i64);
    });
    value
}

fn new_context_state(sandbox: f64, dont_contextify: bool, options: ContextOptions) -> ContextState {
    let scope = crate::gc::RuntimeHandleScope::new();
    let sandbox = scope.root_nanbox_f64(sandbox);
    let global_this = if dont_contextify {
        sandbox.get_nanbox_f64()
    } else {
        let handler = scope.root_nanbox_f64(object_value(crate::object::js_object_alloc(0, 0)));
        crate::proxy::js_proxy_new(sandbox.get_nanbox_f64(), handler.get_nanbox_f64())
    };
    let global_this = scope.root_nanbox_f64(global_this);
    let intrinsics = scope.root_nanbox_f64(fresh_intrinsic_global());
    let lexical_env = scope.root_nanbox_f64(crate::dyn_eval::script_environment(
        sandbox.get_nanbox_f64(),
        &[],
    ));
    let returned = sandbox.get_nanbox_f64();
    ContextState {
        returned_bits: returned.to_bits(),
        sandbox_bits: sandbox.get_nanbox_f64().to_bits(),
        global_this_bits: global_this.get_nanbox_f64().to_bits(),
        intrinsics_bits: intrinsics.get_nanbox_f64().to_bits(),
        lexical_env_bits: lexical_env.get_nanbox_f64().to_bits(),
        strings_allowed: options.strings_allowed,
        wasm_allowed: options.wasm_allowed,
        microtask_after_evaluate: options.microtask_after_evaluate,
    }
}

fn validate_optional_bool(object: *mut ObjectHeader, name: &str, default: bool) -> bool {
    let value = get_field(object, name);
    let js = JSValue::from_bits(value.to_bits());
    if js.is_undefined() {
        return default;
    }
    if !js.is_bool() {
        let message = format!(
            "The \"{name}\" property must be of type boolean. Received {}",
            crate::fs::validate::describe_received(value)
        );
        throw_invalid_arg(&message);
    }
    js.as_bool()
}

fn context_options(options: f64, code_generation_name: &str) -> ContextOptions {
    let js = JSValue::from_bits(options.to_bits());
    if js.is_undefined() {
        return ContextOptions::default();
    }
    let Some(options) = object_ptr_from_value(options) else {
        let message = format!(
            "The \"options\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(f64::from_bits(js.bits()))
        );
        throw_invalid_arg(&message);
    };
    for name in ["name", "origin"] {
        let value = get_field(options, name);
        if !JSValue::from_bits(value.to_bits()).is_undefined() && string_from_value(value).is_none()
        {
            let message = format!(
                "The \"options.{name}\" property must be of type string. Received {}",
                crate::fs::validate::describe_received(value)
            );
            throw_invalid_arg(&message);
        }
    }
    let mut result = ContextOptions::default();
    let code_generation = get_field(options, code_generation_name);
    if !JSValue::from_bits(code_generation.to_bits()).is_undefined() {
        let Some(code_generation) = object_ptr_from_value(code_generation) else {
            let message = format!(
                "The \"options.{code_generation_name}\" property must be of type object. Received {}",
                crate::fs::validate::describe_received(code_generation)
            );
            throw_invalid_arg(&message);
        };
        result.strings_allowed = validate_optional_bool(code_generation, "strings", true);
        result.wasm_allowed = validate_optional_bool(code_generation, "wasm", true);
    }
    let microtask = get_field(options, "microtaskMode");
    if !JSValue::from_bits(microtask.to_bits()).is_undefined() {
        if string_from_value(microtask).as_deref() != Some("afterEvaluate") {
            let message = "The \"options.microtaskMode\" property must be 'afterEvaluate'";
            throw_invalid_arg_value(message);
        }
        result.microtask_after_evaluate = true;
    }
    result
}

pub(crate) fn create_context(value: f64, options: f64) -> f64 {
    context_from_arg(value, "object", context_options(options, "codeGeneration"))
}

fn context_from_arg(value: f64, arg_name: &str, options: ContextOptions) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    let dont_contextify = is_dont_contextify(value);
    let sandbox = if jv.is_undefined() || dont_contextify {
        object_value(crate::object::js_object_alloc(0, 0))
    } else {
        value
    };
    if !value_is_object_like(sandbox) {
        let message = format!(
            "The \"{arg_name}\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(value)
        );
        throw_invalid_arg(&message);
    }
    let key = context_key(sandbox);
    if !dont_contextify {
        if let Some(existing) = VM_CONTEXTS.with(|contexts| contexts.borrow().get(&key).cloned()) {
            return f64::from_bits(existing.returned_bits);
        }
    }
    let state = new_context_state(sandbox, dont_contextify, options);
    let returned = f64::from_bits(state.returned_bits);
    VM_CONTEXTS.with(|contexts| {
        contexts.borrow_mut().insert(context_key(returned), state);
    });
    returned
}

fn is_context(value: f64) -> bool {
    let key = context_key(value);
    key != 0 && VM_CONTEXTS.with(|contexts| contexts.borrow().contains_key(&key))
}

fn require_context_state(value: f64, arg_name: &str) -> ContextState {
    if let Some(state) =
        VM_CONTEXTS.with(|contexts| contexts.borrow().get(&context_key(value)).cloned())
    {
        state
    } else {
        let message = format!(
            "The \"{arg_name}\" argument must be an vm.Context. Received {}",
            crate::fs::validate::describe_received(value)
        );
        throw_invalid_arg(&message);
    }
}

fn require_context(value: f64, arg_name: &str) -> f64 {
    f64::from_bits(require_context_state(value, arg_name).returned_bits)
}

fn main_context_state() -> ContextState {
    if let Some(state) = MAIN_CONTEXT.with(|main| main.borrow().clone()) {
        return state;
    }
    let global = crate::object::js_get_global_this();
    let state = ContextState {
        returned_bits: global.to_bits(),
        sandbox_bits: global.to_bits(),
        global_this_bits: global.to_bits(),
        intrinsics_bits: global.to_bits(),
        lexical_env_bits: crate::dyn_eval::script_environment(global, &[]).to_bits(),
        strings_allowed: true,
        wasm_allowed: true,
        microtask_after_evaluate: false,
    };
    MAIN_CONTEXT.with(|main| *main.borrow_mut() = Some(state.clone()));
    state
}

fn execute_in_state(source: &str, state: &ContextState) -> f64 {
    let result = crate::dyn_eval::eval_script_in_with_codegen(
        source,
        f64::from_bits(state.global_this_bits),
        f64::from_bits(state.intrinsics_bits),
        f64::from_bits(state.lexical_env_bits),
        state.strings_allowed,
        state.wasm_allowed,
    );
    if state.microtask_after_evaluate {
        crate::promise::js_promise_run_microtasks();
    }
    result
}

fn script_metadata(script_value: f64) -> Option<ScriptMetadata> {
    object_ptr_from_value(script_value)
        .and_then(|ptr| scripts().lock().unwrap().get(&(ptr as usize)).cloned())
}

fn install_script_method(
    obj: *mut ObjectHeader,
    name: &str,
    func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
    arity: u32,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(obj);
    let key = scope.root_string_ptr(field_key(name));
    let func_ptr = func as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    let closure = scope.root_raw_mut_ptr(closure);
    crate::object::set_builtin_closure_length(
        closure.get_raw_mut_ptr::<ClosureHeader>() as usize,
        arity,
    );
    let value = crate::value::js_nanbox_pointer(closure.get_raw_mut_ptr::<ClosureHeader>() as i64);
    crate::object::js_object_set_field_by_name(
        obj.get_raw_mut_ptr::<ObjectHeader>(),
        key.get_raw_mut_ptr::<StringHeader>(),
        value,
    );
    crate::object::set_builtin_property_attrs(
        obj.get_raw_mut_ptr::<ObjectHeader>() as usize,
        name.to_string(),
        PropertyAttrs::new(true, false, true),
    );
}

fn script_receiver() -> f64 {
    crate::object::js_implicit_this_get()
}

pub(crate) fn install_script_prototypes(constructor: f64) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_nanbox_f64(constructor);
    let Some(proto_value) =
        crate::object::ordinary_function_prototype_value_for_read(constructor.get_nanbox_f64())
    else {
        return;
    };
    let Some(proto_ptr) = object_ptr_from_value(proto_value) else {
        return;
    };
    let proto = scope.root_raw_mut_ptr(proto_ptr);
    let key = scope.root_nanbox_f64(string_value("runInThisContext"));
    if JSValue::from_bits(
        crate::object::js_object_has_own(
            object_value(proto.get_raw_mut_ptr::<ObjectHeader>()),
            key.get_nanbox_f64(),
        )
        .to_bits(),
    )
    .as_bool()
    {
        return;
    }
    let old_parent = scope.root_nanbox_u64(
        crate::object::prototype_chain::object_static_prototype(
            proto.get_raw_mut_ptr::<ObjectHeader>() as usize,
        )
        .unwrap_or(JSValue::null().bits()),
    );
    let base = crate::object::js_object_alloc(0, 0);
    let base = scope.root_raw_mut_ptr(base);
    crate::object::prototype_chain::object_set_static_prototype(
        base.get_raw_mut_ptr::<ObjectHeader>() as usize,
        old_parent.get_nanbox_u64(),
    );
    crate::object::prototype_chain::object_set_static_prototype(
        proto.get_raw_mut_ptr::<ObjectHeader>() as usize,
        object_value(base.get_raw_mut_ptr::<ObjectHeader>()).to_bits(),
    );
    set_field(
        base.get_raw_mut_ptr::<ObjectHeader>(),
        "constructor",
        constructor.get_nanbox_f64(),
    );
    crate::object::set_builtin_property_attrs(
        base.get_raw_mut_ptr::<ObjectHeader>() as usize,
        "constructor".to_string(),
        PropertyAttrs::new(true, false, true),
    );
    install_script_method(
        proto.get_raw_mut_ptr::<ObjectHeader>(),
        "runInThisContext",
        vm_script_run_in_this_context_method,
        1,
    );
    install_script_method(
        proto.get_raw_mut_ptr::<ObjectHeader>(),
        "runInContext",
        vm_script_run_in_context_method,
        2,
    );
    install_script_method(
        proto.get_raw_mut_ptr::<ObjectHeader>(),
        "runInNewContext",
        vm_script_run_in_new_context_method,
        2,
    );
    install_script_method(
        base.get_raw_mut_ptr::<ObjectHeader>(),
        "runInContext",
        vm_script_run_in_context_method,
        2,
    );
    install_script_method(
        base.get_raw_mut_ptr::<ObjectHeader>(),
        "createCachedData",
        vm_script_create_cached_data_method,
        0,
    );
    crate::object::set_builtin_property_attrs(
        base.get_raw_mut_ptr::<ObjectHeader>() as usize,
        "createCachedData".to_string(),
        PropertyAttrs::new(true, true, true),
    );
}

fn make_script(code: String, options: f64) -> f64 {
    let source_options = source_options(options, true);
    with_source_location(&source_options, || {
        crate::dyn_eval::validate_script_source(&code)
    });
    let hash = source_hash(CACHE_KIND_SCRIPT, &code, &[]);
    let cached_data = validate_cached_data_option(options);
    let produce_cached_data = validate_produce_cached_data(options);
    let source_map_url = extract_source_map_url(&code);
    let scope = crate::gc::RuntimeHandleScope::new();
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 0));
    scripts().lock().unwrap().insert(
        obj.get_raw_mut_ptr::<ObjectHeader>() as usize,
        ScriptMetadata {
            source: code,
            filename: source_options.filename,
            line_offset: source_options.line_offset,
            column_offset: source_options.column_offset,
        },
    );
    if let Some(url) = source_map_url {
        set_field(
            obj.get_raw_mut_ptr::<ObjectHeader>(),
            "sourceMapURL",
            string_value(&url),
        );
    }
    if let Some(bytes) = cached_data {
        set_field(
            obj.get_raw_mut_ptr::<ObjectHeader>(),
            "cachedDataRejected",
            bool_value(!cache_bytes_accepted(&bytes, CACHE_KIND_SCRIPT, hash)),
        );
    } else if produce_cached_data {
        set_field(
            obj.get_raw_mut_ptr::<ObjectHeader>(),
            "cachedData",
            cached_data_buffer(CACHE_KIND_SCRIPT, hash),
        );
        set_field(
            obj.get_raw_mut_ptr::<ObjectHeader>(),
            "cachedDataProduced",
            bool_value(true),
        );
    }
    object_value(obj.get_raw_mut_ptr::<ObjectHeader>())
}

extern "C" fn vm_script_create_cached_data_method(
    _closure: *const ClosureHeader,
    _unused1: f64,
    _unused2: f64,
) -> f64 {
    let script = script_receiver();
    let Some(metadata) = script_metadata(script) else {
        return cached_data_buffer(CACHE_KIND_SCRIPT, 0);
    };
    cached_data_buffer(
        CACHE_KIND_SCRIPT,
        source_hash(CACHE_KIND_SCRIPT, &metadata.source, &[]),
    )
}

extern "C" fn vm_script_run_in_this_context_method(
    _closure: *const ClosureHeader,
    _options: f64,
    _unused: f64,
) -> f64 {
    let script = script_receiver();
    let Some(metadata) = script_metadata(script) else {
        return undefined_value();
    };
    validate_run_options(_options);
    let options = SourceOptions {
        filename: metadata.filename,
        line_offset: metadata.line_offset,
        column_offset: metadata.column_offset,
    };
    with_source_location(&options, || {
        execute_in_state(&metadata.source, &main_context_state())
    })
}

extern "C" fn vm_script_run_in_context_method(
    _closure: *const ClosureHeader,
    contextified_object: f64,
    _options: f64,
) -> f64 {
    let script = script_receiver();
    let Some(metadata) = script_metadata(script) else {
        return undefined_value();
    };
    validate_run_options(_options);
    let context = require_context_state(contextified_object, "contextifiedObject");
    let options = SourceOptions {
        filename: metadata.filename,
        line_offset: metadata.line_offset,
        column_offset: metadata.column_offset,
    };
    with_source_location(&options, || execute_in_state(&metadata.source, &context))
}

extern "C" fn vm_script_run_in_new_context_method(
    _closure: *const ClosureHeader,
    context_object: f64,
    options: f64,
) -> f64 {
    let script = script_receiver();
    let Some(metadata) = script_metadata(script) else {
        return undefined_value();
    };
    validate_run_options(options);
    let context = context_from_arg(
        context_object,
        "object",
        context_options(options, "contextCodeGeneration"),
    );
    let context = require_context_state(context, "contextifiedObject");
    let source_options = SourceOptions {
        filename: metadata.filename,
        line_offset: metadata.line_offset,
        column_offset: metadata.column_offset,
    };
    with_source_location(&source_options, || {
        execute_in_state(&metadata.source, &context)
    })
}

pub extern "C" fn js_vm_create_script(code: f64, options: f64) -> f64 {
    make_script(code_string_for_script(code), options)
}

pub extern "C" fn js_vm_run_in_context(code: f64, contextified_object: f64, options: f64) -> f64 {
    let code = code_string_for_script(code);
    let source_options = source_options(options, true);
    let context = require_context_state(contextified_object, "contextifiedObject");
    with_source_location(&source_options, || execute_in_state(&code, &context))
}

pub extern "C" fn js_vm_run_in_new_context(code: f64, context_object: f64, options: f64) -> f64 {
    let code = code_string_for_script(code);
    let source_options = source_options(options, true);
    let context_options = if string_from_value(options).is_some() {
        ContextOptions::default()
    } else {
        context_options(options, "contextCodeGeneration")
    };
    let context = context_from_arg(context_object, "object", context_options);
    let context = require_context_state(context, "contextifiedObject");
    with_source_location(&source_options, || execute_in_state(&code, &context))
}

pub extern "C" fn js_vm_run_in_this_context(code: f64, options: f64) -> f64 {
    let code = code_string_for_script(code);
    let source_options = source_options(options, true);
    with_source_location(&source_options, || {
        execute_in_state(&code, &main_context_state())
    })
}

pub extern "C" fn js_vm_is_context(object: f64) -> f64 {
    if !value_is_object_like(object) {
        let message = format!(
            "The \"object\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(object)
        );
        throw_invalid_arg(&message);
    }
    bool_value(is_context(object))
}

fn compile_params(params: f64) -> Vec<String> {
    let jv = JSValue::from_bits(params.to_bits());
    if jv.is_undefined() {
        return Vec::new();
    }
    let Some(arr) = array_ptr_from_value(params) else {
        let message = format!(
            "The \"params\" argument must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(params)
        );
        throw_invalid_arg(&message);
    };
    let len = crate::array::js_array_length(arr) as usize;
    let mut out = Vec::with_capacity(len);
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(arr, idx as u32);
        let Some(name) = string_from_value(value) else {
            let message = format!(
                "The \"params[{}]\" argument must be of type string. Received {}",
                idx,
                crate::fs::validate::describe_received(value)
            );
            throw_invalid_arg(&message);
        };
        if !name.chars().enumerate().all(|(i, c)| {
            c == '_' || c == '$' || (c.is_ascii_alphanumeric() && (i > 0 || !c.is_ascii_digit()))
        }) {
            throw_syntax("Arg string terminates parameters early");
        }
        out.push(name);
    }
    out
}

fn compile_options(options: f64) -> (ContextState, Vec<f64>, SourceOptions) {
    let options = options_object_or_default(options);
    let Some(options) = options else {
        return (main_context_state(), Vec::new(), SourceOptions::default());
    };
    let source_options = source_options(object_value(options), false);

    let parsing = get_field(options, "parsingContext");
    let pv = JSValue::from_bits(parsing.to_bits());
    let context = if pv.is_undefined() {
        main_context_state()
    } else {
        require_context_state(parsing, "options.parsingContext")
    };

    let extensions = get_field(options, "contextExtensions");
    if JSValue::from_bits(extensions.to_bits()).is_undefined() {
        return (context, Vec::new(), source_options);
    }
    let Some(extensions) = array_ptr_from_value(extensions) else {
        let message = format!(
            "The \"options.contextExtensions\" property must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(extensions)
        );
        throw_invalid_arg(&message);
    };
    let mut values = Vec::with_capacity(crate::array::js_array_length(extensions) as usize);
    for index in 0..crate::array::js_array_length(extensions) {
        let extension = crate::array::js_array_get_f64(extensions, index);
        crate::validators::validate_object(
            extension,
            &format!("options.contextExtensions[{index}]"),
        );
        values.push(extension);
    }
    (context, values, source_options)
}

pub extern "C" fn js_vm_compile_function(code: f64, params: f64, options: f64) -> f64 {
    let body = code_string_required(code, "code");
    let params = compile_params(params);
    let hash = source_hash(CACHE_KIND_FUNCTION, &body, &params);
    let cached_data = validate_cached_data_option(options);
    let produce_cached_data = validate_produce_cached_data(options);
    let (context, extensions, source_options) = compile_options(options);
    let mut source = params.clone();
    source.push(body.clone());
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(with_source_location(&source_options, || {
        crate::dyn_eval::function_from_strings_in_with_codegen(
            &source,
            f64::from_bits(context.global_this_bits),
            f64::from_bits(context.intrinsics_bits),
            &extensions,
            context.strings_allowed,
            context.wasm_allowed,
        )
    }));
    let closure = crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize;
    crate::object::set_builtin_closure_length(closure, params.len() as u32);
    compiled_function_sources().lock().unwrap().insert(
        crate::value::js_nanbox_get_pointer(value.get_nanbox_f64()) as usize,
        format!("function ({}) {{\n{}\n}}", params.join(", "), body),
    );
    if let Some(bytes) = cached_data {
        set_value_field(
            value.get_nanbox_f64(),
            "cachedDataRejected",
            bool_value(!cache_bytes_accepted(&bytes, CACHE_KIND_FUNCTION, hash)),
        );
    } else if produce_cached_data {
        set_value_field(
            value.get_nanbox_f64(),
            "cachedData",
            cached_data_buffer(CACHE_KIND_FUNCTION, hash),
        );
        set_value_field(
            value.get_nanbox_f64(),
            "cachedDataProduced",
            bool_value(true),
        );
    }
    value.get_nanbox_f64()
}

fn memory_range_value(estimate: f64) -> f64 {
    let mut range = crate::array::js_array_alloc(2);
    range = crate::array::js_array_push_f64(range, estimate);
    range = crate::array::js_array_push_f64(range, estimate);
    array_value(range)
}

fn memory_entry_value(estimate: f64) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    set_field(obj, "jsMemoryEstimate", estimate);
    set_field(obj, "jsMemoryRange", memory_range_value(estimate));
    object_value(obj)
}

fn webassembly_memory_value() -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    set_field(obj, "code", 0.0);
    set_field(obj, "metadata", 0.0);
    object_value(obj)
}

fn measure_memory_result(detailed: bool) -> f64 {
    let mut heap_used = 0_u64;
    let mut heap_total = 0_u64;
    crate::arena::js_arena_stats(&mut heap_used, &mut heap_total);
    let estimate = heap_used.max(heap_total) as f64;
    let obj = crate::object::js_object_alloc(0, if detailed { 4 } else { 2 });
    set_field(obj, "total", memory_entry_value(estimate));
    set_field(obj, "WebAssembly", webassembly_memory_value());
    if detailed {
        set_field(obj, "current", memory_entry_value(estimate));
        set_field(obj, "other", array_value(crate::array::js_array_alloc(0)));
    }
    object_value(obj)
}

fn validate_measure_memory_options(options: f64) -> bool {
    let options = options_object_or_default(options);
    let mode_value = options
        .map(|options| get_field(options, "mode"))
        .unwrap_or_else(undefined_value);
    let execution_value = options
        .map(|options| get_field(options, "execution"))
        .unwrap_or_else(undefined_value);
    let mode = validate_one_of_string(
        mode_value,
        "options.mode",
        &["summary", "detailed"],
        "summary",
    );
    let _execution = validate_one_of_string(
        execution_value,
        "options.execution",
        &["default", "eager"],
        "default",
    );
    mode == "detailed"
}

pub extern "C" fn js_vm_measure_memory(options: f64) -> f64 {
    let detailed = validate_measure_memory_options(options);
    let scope = crate::gc::RuntimeHandleScope::new();
    let result = scope.root_nanbox_f64(measure_memory_result(detailed));
    let promise = crate::promise::js_promise_resolved(result.get_nanbox_f64());
    crate::value::js_nanbox_pointer(promise as i64)
}

pub extern "C" fn js_vm_script_new(code: f64, options: f64) -> f64 {
    js_vm_create_script(code, options)
}

pub extern "C" fn js_vm_script_call(_code: f64, _options: f64) -> f64 {
    throw_type_error("Class constructor Script cannot be invoked without 'new'")
}

pub fn scan_vm_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    VM_CONTEXTS.with(|contexts| {
        let mut contexts = contexts.borrow_mut();
        let mut rebuilt = HashMap::with_capacity(contexts.len());
        for (_, mut state) in contexts.drain() {
            visitor.visit_nanbox_u64_slot(&mut state.returned_bits);
            visitor.visit_nanbox_u64_slot(&mut state.sandbox_bits);
            visitor.visit_nanbox_u64_slot(&mut state.global_this_bits);
            visitor.visit_nanbox_u64_slot(&mut state.intrinsics_bits);
            visitor.visit_nanbox_u64_slot(&mut state.lexical_env_bits);
            rebuilt.insert(context_key(f64::from_bits(state.returned_bits)), state);
        }
        *contexts = rebuilt;
    });
    MAIN_CONTEXT.with(|main| {
        if let Some(state) = main.borrow_mut().as_mut() {
            visitor.visit_nanbox_u64_slot(&mut state.returned_bits);
            visitor.visit_nanbox_u64_slot(&mut state.sandbox_bits);
            visitor.visit_nanbox_u64_slot(&mut state.global_this_bits);
            visitor.visit_nanbox_u64_slot(&mut state.intrinsics_bits);
            visitor.visit_nanbox_u64_slot(&mut state.lexical_env_bits);
        }
    });
    if let Some(scripts) = VM_SCRIPTS.get() {
        let mut guard = scripts.lock().unwrap();
        let mut rewrites = Vec::new();
        for old in guard.keys().copied().collect::<Vec<_>>() {
            let mut new = old;
            if visitor.visit_metadata_usize_slot(&mut new) && new != old {
                rewrites.push((old, new));
            }
        }
        for (old, new) in rewrites {
            if let Some(source) = guard.remove(&old) {
                if new != 0 {
                    guard.insert(new, source);
                }
            }
        }
    }
    if let Some(sources) = VM_COMPILED_FUNCTION_SOURCES.get() {
        let mut guard = sources.lock().unwrap();
        let mut rewrites = Vec::new();
        for old in guard.keys().copied().collect::<Vec<_>>() {
            let mut new = old;
            if visitor.visit_metadata_usize_slot(&mut new) && new != old {
                rewrites.push((old, new));
            }
        }
        for (old, new) in rewrites {
            if let Some(source) = guard.remove(&old) {
                if new != 0 {
                    guard.insert(new, source);
                }
            }
        }
    }
}

/// Prune VM metadata whose owner is provably dead. `VM_CONTEXTS` is local to
/// the calling thread; process-global script/source entries owned by another
/// thread are retained because their deadness cannot be attributed here.
pub(crate) fn prune_dead_vm_owner_entries(is_dead_owner: &dyn Fn(usize) -> bool) {
    VM_CONTEXTS.with(|contexts| {
        contexts
            .borrow_mut()
            .retain(|owner, _| !is_dead_owner(*owner));
    });
    if let Some(scripts) = VM_SCRIPTS.get() {
        if let Ok(mut guard) = scripts.lock() {
            guard.retain(|owner, _| !is_dead_owner(*owner));
        }
    }
    if let Some(sources) = VM_COMPILED_FUNCTION_SOURCES.get() {
        if let Ok(mut guard) = sources.lock() {
            guard.retain(|owner, _| !is_dead_owner(*owner));
        }
    }
}

#[cfg(test)]
pub(crate) fn test_seed_vm_script_entry(owner: usize, source: &str) {
    scripts().lock().unwrap().insert(
        owner,
        ScriptMetadata {
            source: source.to_string(),
            filename: "evalmachine.<anonymous>".to_string(),
            line_offset: 0,
            column_offset: 0,
        },
    );
}

#[cfg(test)]
pub(crate) fn test_vm_script_entry_exists(owner: usize) -> bool {
    scripts().lock().unwrap().contains_key(&owner)
}

pub extern "C" fn js_vm_module_call() -> f64 {
    throw_type_error_no_code("Class constructor Module cannot be invoked without 'new'")
}

#[no_mangle]
pub extern "C" fn js_vm_module_constructor_error() -> f64 {
    throw_type_error_no_code("Module is not a constructor")
}

pub extern "C" fn js_vm_source_text_module_new(code: f64, options: f64) -> f64 {
    if !vm_modules_enabled() {
        return throw_vm_unimplemented("SourceTextModule experimental gate", "3132");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let options = scope.root_nanbox_f64(options);
    let source = code_string_required(code, "code");
    validate_module_source(&source);
    let (context, identifier) = module_options(options.get_nanbox_f64());
    let context = scope.root_nanbox_f64(context);
    let hash = source_hash(CACHE_KIND_MODULE, &source, &[]);
    if let Some(bytes) = validate_cached_data_option(options.get_nanbox_f64()) {
        if !cache_bytes_accepted(&bytes, CACHE_KIND_MODULE, hash) {
            return throw_vm_module_cached_data_rejected();
        }
    }
    let parsed = parse_source(&source);
    let module = new_module_base(KIND_SOURCE, STATUS_UNLINKED, identifier);
    let module = scope.root_raw_mut_ptr(module);
    let namespace = crate::object::js_object_alloc_null_proto(0, parsed.exports.len() as u32);
    let namespace = scope.root_raw_mut_ptr(namespace);
    set_module_namespace_tag(namespace.get_raw_mut_ptr::<ObjectHeader>());
    for export in &parsed.exports {
        set_field(
            namespace.get_raw_mut_ptr::<ObjectHeader>(),
            &export.name,
            undefined_value(),
        );
    }
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_NAMESPACE,
        object_value(namespace.get_raw_mut_ptr::<ObjectHeader>()),
    );
    install_module_accessor(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "namespace",
        module_namespace_getter,
    );
    install_module_accessor(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "error",
        module_error_getter,
    );
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_CONTEXT,
        context.get_nanbox_f64(),
    );
    let value = string_value(&source);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_SOURCE,
        value,
    );
    let value = requests_array(&parsed.requests);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_REQUESTS,
        value,
    );
    let value = strings_array(&parsed.requests);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "dependencySpecifiers",
        value,
    );
    let value = requests_array(&parsed.requests);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "moduleRequests",
        value,
    );
    let value = imports_array(&parsed.imports);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_IMPORTS,
        value,
    );
    let value = exports_array(&parsed.exports);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_EXPORTS,
        value,
    );
    object_value(module.get_raw_mut_ptr::<ObjectHeader>())
}

pub extern "C" fn js_vm_synthetic_module_new(
    export_names_value: f64,
    evaluate_callback: f64,
    options: f64,
) -> f64 {
    if !vm_modules_enabled() {
        return throw_vm_unimplemented("SyntheticModule experimental gate", "3133");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let export_names_value = scope.root_nanbox_f64(export_names_value);
    let evaluate_callback = scope.root_nanbox_f64(evaluate_callback);
    let options = scope.root_nanbox_f64(options);
    let Some(export_names) = array_ptr_from_value(export_names_value.get_nanbox_f64()) else {
        let message = format!(
            "The \"exportNames\" argument must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(export_names_value.get_nanbox_f64())
        );
        throw_invalid_arg(&message);
    };
    let export_names = scope.root_raw_mut_ptr(export_names);
    if !crate::object::value_is_callable(evaluate_callback.get_nanbox_f64()) {
        let message = format!(
            "The \"evaluateCallback\" argument must be of type function. Received {}",
            crate::fs::validate::describe_received(evaluate_callback.get_nanbox_f64())
        );
        throw_invalid_arg(&message);
    }
    let (context, identifier) = module_options(options.get_nanbox_f64());
    let context = scope.root_nanbox_f64(context);
    let module = new_module_base(KIND_SYNTHETIC, STATUS_LINKED, identifier);
    let module = scope.root_raw_mut_ptr(module);
    let namespace = crate::object::js_object_alloc_null_proto(0, 0);
    let namespace = scope.root_raw_mut_ptr(namespace);
    set_module_namespace_tag(namespace.get_raw_mut_ptr::<ObjectHeader>());
    let len = crate::array::js_array_length(export_names.get_raw_mut_ptr::<ArrayHeader>());
    let mut exports = Vec::new();
    for idx in 0..len {
        let value =
            crate::array::js_array_get_f64(export_names.get_raw_mut_ptr::<ArrayHeader>(), idx);
        let Some(name) = string_from_value(value) else {
            let message = format!(
                "The \"exportNames[{idx}]\" argument must be of type string. Received {}",
                crate::fs::validate::describe_received(value)
            );
            throw_invalid_arg(&message);
        };
        exports.push(ExportBinding {
            name: name.clone(),
            expr: String::new(),
        });
        set_field(
            namespace.get_raw_mut_ptr::<ObjectHeader>(),
            &name,
            undefined_value(),
        );
    }
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_NAMESPACE,
        object_value(namespace.get_raw_mut_ptr::<ObjectHeader>()),
    );
    install_module_accessor(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "namespace",
        module_namespace_getter,
    );
    install_module_accessor(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        "error",
        module_error_getter,
    );
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_CONTEXT,
        context.get_nanbox_f64(),
    );
    let value = requests_array(&[]);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_REQUESTS,
        value,
    );
    let value = imports_array(&[]);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_IMPORTS,
        value,
    );
    let value = exports_array(&exports);
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_EXPORTS,
        value,
    );
    set_field(
        module.get_raw_mut_ptr::<ObjectHeader>(),
        FIELD_EVALUATE_CALLBACK,
        evaluate_callback.get_nanbox_f64(),
    );
    object_value(module.get_raw_mut_ptr::<ObjectHeader>())
}

pub extern "C" fn js_vm_module_status(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    string_value(&module_status(module))
}

pub extern "C" fn js_vm_module_identifier(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    get_field(module, FIELD_IDENTIFIER)
}

pub extern "C" fn js_vm_module_error(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_status(module) != STATUS_ERRORED {
        return throw_vm_status("Module status must be errored");
    }
    get_field(module, FIELD_ERROR)
}

pub extern "C" fn js_vm_module_namespace(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_kind(module) == KIND_SOURCE && module_status(module) == STATUS_UNLINKED {
        return throw_vm_status("Module status must be linked");
    }
    get_field(module, FIELD_NAMESPACE)
}

pub extern "C" fn js_vm_module_link(module_value: f64, linker: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_kind(module) == KIND_SYNTHETIC {
        set_status(module, STATUS_LINKED);
        return undefined_value();
    }
    if module_status(module) != STATUS_UNLINKED {
        return undefined_value();
    }

    set_status(module, STATUS_LINKING);
    let requests = read_requests(module);
    let mut linked = crate::array::js_array_alloc(requests.len() as u32);
    for specifier in &requests {
        let args = [
            string_value(specifier),
            module_value,
            module_request_extra(),
        ];
        let dep =
            unsafe { crate::closure::js_native_call_value(linker, args.as_ptr(), args.len()) };
        linked = crate::array::js_array_push_f64(linked, dep);
    }
    set_field(module, FIELD_LINKED_MODULES, array_value(linked));
    set_status(module, STATUS_LINKED);
    undefined_value()
}

pub extern "C" fn js_vm_module_evaluate(module_value: f64, _options: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let result = match module_kind(module).as_str() {
        KIND_SOURCE => evaluate_source_module(module),
        KIND_SYNTHETIC => evaluate_synthetic_module(module),
        _ => undefined_value(),
    };
    let promise = if module_status(module) == STATUS_ERRORED {
        crate::promise::js_promise_rejected(result)
    } else {
        crate::promise::js_promise_resolved(result)
    };
    crate::value::js_nanbox_pointer(promise as i64)
}

pub extern "C" fn js_vm_source_text_module_dependency_specifiers(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return array_value(crate::array::js_array_alloc(0));
    };
    strings_array(&read_requests(module))
}

pub extern "C" fn js_vm_source_text_module_module_requests(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return array_value(crate::array::js_array_alloc(0));
    };
    let requests = read_requests(module);
    requests_array(&requests)
}

pub extern "C" fn js_vm_source_text_module_create_cached_data(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return cached_data_buffer(CACHE_KIND_MODULE, 0);
    };
    if module_status(module) == STATUS_EVALUATED {
        return throw_vm_module_cannot_create_cached_data();
    }
    let source = get_string_field(module, FIELD_SOURCE).unwrap_or_default();
    cached_data_buffer(
        CACHE_KIND_MODULE,
        source_hash(CACHE_KIND_MODULE, &source, &[]),
    )
}

pub extern "C" fn js_vm_source_text_module_link_requests(
    module_value: f64,
    modules_value: f64,
) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let Some(modules) = array_ptr_from_value(modules_value) else {
        return throw_vm_type("linkRequests modules must be an array");
    };
    set_field(module, FIELD_LINKED_MODULES, array_value(modules));
    undefined_value()
}

pub extern "C" fn js_vm_source_text_module_instantiate(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    if module_status(module) == STATUS_UNLINKED {
        set_status(module, STATUS_LINKED);
    }
    undefined_value()
}

pub extern "C" fn js_vm_source_text_module_has_top_level_await(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return bool_value(false);
    };
    bool_value(module_has_tla(module))
}

pub extern "C" fn js_vm_source_text_module_has_async_graph(module_value: f64) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return bool_value(false);
    };
    if module_status(module) == STATUS_UNLINKED {
        return throw_vm_status("Module status must be instantiated");
    }
    bool_value(module_has_async_graph(module))
}

pub extern "C" fn js_vm_synthetic_module_set_export(
    module_value: f64,
    name_value: f64,
    value: f64,
) -> f64 {
    let Some(module) = object_ptr_from_value(module_value) else {
        return undefined_value();
    };
    let Some(name) = string_from_value(name_value) else {
        return throw_vm_type("SyntheticModule export name must be a string");
    };
    let exports = read_exports(module);
    if !exports.iter().any(|export| export.name == name) {
        return throw_reference_error_no_code(&format!("Export '{name}' is not defined in module"));
    }
    let Some(namespace) = namespace_for_module(module) else {
        return throw_vm_status("SyntheticModule namespace is unavailable");
    };
    set_field(namespace, &name, value);
    undefined_value()
}

/// Dispatch a `node:vm` module method reached as a value/namespace call.
/// `createContext` routes to the working #4050 contextification helper; the
/// remaining entries live in this VM scaffold/lifecycle module.
pub fn dispatch_vm_method(method: &str, arg0: f64, arg1: f64, arg2: f64) -> f64 {
    match method {
        "Script" => js_vm_script_call(arg0, arg1),
        "Module" => js_vm_module_call(),
        "SourceTextModule" => throw_type_error_no_code(
            "Class constructor SourceTextModule cannot be invoked without 'new'",
        ),
        "SyntheticModule" => throw_type_error_no_code(
            "Class constructor SyntheticModule cannot be invoked without 'new'",
        ),
        "createContext" => create_context(arg0, arg1),
        "createScript" => crate::object::brand_vm_script_instance(js_vm_create_script(arg0, arg1)),
        "runInContext" => js_vm_run_in_context(arg0, arg1, arg2),
        "runInNewContext" => js_vm_run_in_new_context(arg0, arg1, arg2),
        "runInThisContext" => js_vm_run_in_this_context(arg0, arg1),
        "isContext" => js_vm_is_context(arg0),
        "compileFunction" => js_vm_compile_function(arg0, arg1, arg2),
        "measureMemory" => js_vm_measure_memory(arg0),
        "status" => js_vm_module_status(arg0),
        "identifier" => js_vm_module_identifier(arg0),
        "error" => js_vm_module_error(arg0),
        "namespace" => js_vm_module_namespace(arg0),
        "link" => js_vm_module_link(arg0, arg1),
        "evaluate" => js_vm_module_evaluate(arg0, arg1),
        "dependencySpecifiers" => js_vm_source_text_module_dependency_specifiers(arg0),
        "moduleRequests" => js_vm_source_text_module_module_requests(arg0),
        "createCachedData" => js_vm_source_text_module_create_cached_data(arg0),
        "linkRequests" => js_vm_source_text_module_link_requests(arg0, arg1),
        "instantiate" => js_vm_source_text_module_instantiate(arg0),
        "hasTopLevelAwait" => js_vm_source_text_module_has_top_level_await(arg0),
        "hasAsyncGraph" => js_vm_source_text_module_has_async_graph(arg0),
        "setExport" => js_vm_synthetic_module_set_export(arg0, arg1, arg2),
        _ => undefined_value(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_values_keep_vm_realm_prototypes() {
        let _ = crate::object::js_get_global_this();
        let outer_prototype = crate::object::builtin_prototype_value("Object");
        let sandbox = object_value(crate::object::js_object_alloc(0, 0));
        let state = new_context_state(sandbox, false, ContextOptions::default());
        let value = execute_in_state("({ value: 1 })", &state);
        assert_ne!(
            crate::object::js_object_get_prototype_of(value).to_bits(),
            outer_prototype.to_bits(),
        );
        let raw = f64::from_bits(crate::value::js_nanbox_get_pointer(value) as u64);
        assert_ne!(
            crate::object::js_object_get_prototype_of(raw).to_bits(),
            outer_prototype.to_bits(),
        );
    }
}
