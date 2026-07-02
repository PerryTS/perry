//! Narrow `node:vm` execution and experimental module lifecycle support.
//!
//! Perry is V8-free, so this is not a full JavaScript interpreter. It models the
//! Node-observable VM shape plus deterministic local subsets used by the VM
//! parity fixtures: context markers, object-backed sandbox reads/writes,
//! repeated `Script` execution, `runIn*Context`, `compileFunction`, and gated
//! `SourceTextModule`/`SyntheticModule` lifecycle behavior.

use std::collections::{HashMap, HashSet};
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

static MODULE_ID_COUNTER: AtomicU64 = AtomicU64::new(0);
const CACHE_PREFIX: &[u8] = b"PERRY_VM_CACHE\0";
const CACHE_KIND_SCRIPT: u8 = 1;
const CACHE_KIND_FUNCTION: u8 = 2;
const CACHE_KIND_MODULE: u8 = 3;

#[derive(Clone)]
struct CompiledFunction {
    body: String,
    params: Vec<String>,
    context_bits: u64,
}

#[derive(Clone)]
struct ScriptMetadata {
    source: String,
}

struct EvalEnv {
    target: f64,
    params: HashMap<String, f64>,
}

static VM_CONTEXTS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
static VM_SCRIPTS: OnceLock<Mutex<HashMap<usize, ScriptMetadata>>> = OnceLock::new();
static VM_FUNCTIONS: OnceLock<Mutex<HashMap<usize, CompiledFunction>>> = OnceLock::new();

fn contexts() -> &'static Mutex<HashSet<usize>> {
    VM_CONTEXTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn scripts() -> &'static Mutex<HashMap<usize, ScriptMetadata>> {
    VM_SCRIPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn functions() -> &'static Mutex<HashMap<usize, CompiledFunction>> {
    VM_FUNCTIONS.get_or_init(|| Mutex::new(HashMap::new()))
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

fn number_value(value: f64) -> f64 {
    f64::from_bits(JSValue::number(value).bits())
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
    crate::object::js_object_set_field_by_name(obj, field_key(name), value);
}

fn get_field(obj: *mut ObjectHeader, name: &str) -> f64 {
    crate::object::js_object_get_field_by_name_f64(obj, field_key(name))
}

fn get_object_field(object: f64, name: &str) -> f64 {
    let Some(ptr) = object_ptr_from_value(object) else {
        return undefined_value();
    };
    get_field(ptr, name)
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

fn options_identifier(options: f64) -> Option<String> {
    object_ptr_from_value(options)
        .map(|obj| get_field(obj, "identifier"))
        .and_then(string_from_value)
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

fn module_term_value(term: &str, env: &HashMap<String, f64>) -> f64 {
    let term = term.trim();
    if term.is_empty() {
        return undefined_value();
    }
    if (term.starts_with('"') && term.ends_with('"'))
        || (term.starts_with('\'') && term.ends_with('\''))
    {
        return string_value(&term[1..term.len() - 1]);
    }
    if term == "true" {
        return bool_value(true);
    }
    if term == "false" {
        return bool_value(false);
    }
    if let Ok(number) = term.parse::<f64>() {
        return number;
    }
    env.get(term).copied().unwrap_or_else(undefined_value)
}

fn concat_string_for_value(value: f64) -> String {
    if let Some(s) = string_from_value(value) {
        return s;
    }
    let js = JSValue::from_bits(value.to_bits());
    if js.is_int32() {
        return js.as_int32().to_string();
    }
    if js.is_number() {
        let n = js.as_number();
        if n.is_finite() && n.fract() == 0.0 {
            return (n as i64).to_string();
        }
        return n.to_string();
    }
    if js.is_bool() {
        return js.as_bool().to_string();
    }
    if js.is_undefined() {
        return "undefined".to_string();
    }
    if js.is_null() {
        return "null".to_string();
    }
    "[object Object]".to_string()
}

fn module_add(a: f64, b: f64) -> f64 {
    let a_js = JSValue::from_bits(a.to_bits());
    let b_js = JSValue::from_bits(b.to_bits());
    if a_js.is_any_string() || b_js.is_any_string() {
        return string_value(&format!(
            "{}{}",
            concat_string_for_value(a),
            concat_string_for_value(b)
        ));
    }
    unsafe { crate::value::js_dynamic_add(a, b) }
}

fn eval_module_expr(expr: &str, env: &HashMap<String, f64>) -> f64 {
    let mut parts = expr.split('+').map(str::trim);
    let Some(first) = parts.next() else {
        return undefined_value();
    };
    let mut acc = module_term_value(first, env);
    for part in parts {
        let rhs = module_term_value(part, env);
        acc = module_add(acc, rhs);
    }
    acc
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
    let status = module_status(module);
    if status != STATUS_LINKED && status != STATUS_EVALUATED {
        return throw_vm_status("Module status must be linked");
    }
    if status == STATUS_EVALUATED {
        return undefined_value();
    }

    set_status(module, STATUS_EVALUATING);
    let Some(namespace) = namespace_for_module(module) else {
        set_status(module, STATUS_ERRORED);
        return throw_vm_status("Module namespace is unavailable");
    };

    let mut env = build_import_env(module);
    for export in read_exports(module) {
        let value = eval_module_expr(&export.expr, &env);
        env.insert(export.name.clone(), value);
        set_field(namespace, &export.name, value);
    }
    set_status(module, STATUS_EVALUATED);
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
    let module = crate::object::js_object_alloc(0, 10);
    set_field(module, FIELD_KIND, string_value(kind));
    set_field(module, FIELD_STATUS, string_value(status));
    set_field(module, "status", string_value(status));
    set_field(module, FIELD_IDENTIFIER, string_value(&identifier));
    set_field(module, "identifier", string_value(&identifier));
    set_field(module, FIELD_ERROR, undefined_value());
    set_field(module, "error", undefined_value());
    set_field(
        module,
        FIELD_LINKED_MODULES,
        array_value(crate::array::js_array_alloc(0)),
    );
    module
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

fn mark_context(value: f64) {
    if let Some(ptr) = object_ptr_from_value(value) {
        contexts().lock().unwrap().insert(ptr as usize);
    }
}

fn is_context(value: f64) -> bool {
    object_ptr_from_value(value)
        .map(|ptr| contexts().lock().unwrap().contains(&(ptr as usize)))
        .unwrap_or(false)
}

fn new_plain_context() -> f64 {
    let obj = crate::object::js_object_alloc(0, 0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    mark_context(value);
    value
}

pub(crate) fn create_context(value: f64) -> f64 {
    context_from_arg(value, "object")
}

fn context_from_arg(value: f64, arg_name: &str) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || is_dont_contextify(value) {
        return new_plain_context();
    }
    if object_ptr_from_value(value).is_none() {
        let message = format!(
            "The \"{arg_name}\" argument must be of type object. Received {}",
            crate::fs::validate::describe_received(value)
        );
        throw_invalid_arg(&message);
    }
    mark_context(value);
    value
}

fn require_context(value: f64, arg_name: &str) -> f64 {
    if is_context(value) {
        value
    } else {
        let message = format!(
            "The \"{arg_name}\" argument must be an vm.Context. Received {}",
            crate::fs::validate::describe_received(value)
        );
        throw_invalid_arg(&message);
    }
}

fn script_source(script_value: f64) -> Option<String> {
    object_ptr_from_value(script_value)
        .and_then(|ptr| scripts().lock().unwrap().get(&(ptr as usize)).cloned())
        .map(|metadata| metadata.source)
}

fn split_top_level(input: &str, delimiter: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0_i32;
    let mut quote = None::<char>;
    let mut escape = false;
    for (idx, ch) in input.char_indices() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if ch == delimiter && depth == 0 => {
                out.push(input[start..idx].trim());
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }
    out.push(input[start..].trim());
    out
}

fn strip_wrapping_parens(mut s: &str) -> &str {
    loop {
        let t = s.trim();
        if !(t.starts_with('(') && t.ends_with(')')) {
            return t;
        }
        let mut depth = 0_i32;
        let mut quote = None::<char>;
        let mut escape = false;
        let mut wraps = true;
        for (idx, ch) in t.char_indices() {
            if let Some(q) = quote {
                if escape {
                    escape = false;
                } else if ch == '\\' {
                    escape = true;
                } else if ch == q {
                    quote = None;
                }
                continue;
            }
            match ch {
                '\'' | '"' | '`' => quote = Some(ch),
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 && idx != t.len() - 1 {
                        wraps = false;
                        break;
                    }
                }
                _ => {}
            }
        }
        if !wraps {
            return t;
        }
        s = &t[1..t.len() - 1];
    }
}

fn find_top_level_operator(input: &str, op: &str) -> Option<usize> {
    let mut depth = 0_i32;
    let mut quote = None::<char>;
    let mut escape = false;
    let mut found = None;
    for (idx, ch) in input.char_indices() {
        if let Some(q) = quote {
            if escape {
                escape = false;
            } else if ch == '\\' {
                escape = true;
            } else if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '\'' | '"' | '`' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ if depth == 0 && input[idx..].starts_with(op) => found = Some(idx),
            _ => {}
        }
    }
    found
}

fn unquote(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let q = bytes[0] as char;
    if !matches!(q, '\'' | '"' | '`') || bytes[bytes.len() - 1] as char != q {
        return None;
    }
    let inner = &s[1..s.len() - 1];
    Some(
        inner
            .replace("\\n", "\n")
            .replace("\\t", "\t")
            .replace("\\\"", "\"")
            .replace("\\'", "'")
            .replace("\\\\", "\\"),
    )
}

fn value_to_number(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_int32() {
        jv.as_int32() as f64
    } else if jv.is_number() {
        jv.as_number()
    } else if jv.is_bool() {
        if jv.as_bool() {
            1.0
        } else {
            0.0
        }
    } else if jv.is_null() {
        0.0
    } else {
        f64::NAN
    }
}

fn coerce_to_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    rust_string_from_header(ptr).unwrap_or_default()
}

fn add_values(a: f64, b: f64) -> f64 {
    let aj = JSValue::from_bits(a.to_bits());
    let bj = JSValue::from_bits(b.to_bits());
    if aj.is_any_string() || bj.is_any_string() {
        return string_value(&format!("{}{}", coerce_to_string(a), coerce_to_string(b)));
    }
    number_value(value_to_number(a) + value_to_number(b))
}

fn value_same(a: f64, b: f64) -> bool {
    crate::value::js_jsvalue_equals(a, b) != 0
}

fn get_reference(name: &str, env: &EvalEnv) -> f64 {
    match name {
        "undefined" => undefined_value(),
        "null" => f64::from_bits(JSValue::null().bits()),
        "true" => bool_value(true),
        "false" => bool_value(false),
        "globalThis" | "this" => env.target,
        _ => env
            .params
            .get(name)
            .copied()
            .unwrap_or_else(|| get_object_field(env.target, name)),
    }
}

fn eval_property_path(expr: &str, env: &EvalEnv) -> Option<f64> {
    let expr = expr.trim();
    // Peel a trailing computed accessor (`a.b["k"]`) and read through it after
    // resolving the receiver expression. Recurses so chained accessors work.
    if let Some((object_expr, accessor)) = split_trailing_computed(expr) {
        let object = eval_property_path(object_expr, env)?;
        let key = computed_key_name(accessor, env)?;
        return Some(get_object_field(object, &key));
    }
    let mut parts = expr.split('.');
    let first = parts.next()?.trim();
    if first.is_empty() {
        return None;
    }
    let mut value = get_reference(first, env);
    for part in parts {
        let name = part.trim();
        if name.is_empty() {
            return None;
        }
        value = get_object_field(value, name);
    }
    Some(value)
}

/// Split a trailing computed-member accessor off a member-access expression,
/// e.g. `globalThis.M["/a/b"]` -> (`globalThis.M`, `["/a/b"]`). Returns `None`
/// when the expression does not end in a top-level `[...]` accessor.
fn split_trailing_computed(expr: &str) -> Option<(&str, &str)> {
    let trimmed = expr.trim_end();
    if !trimmed.ends_with(']') {
        return None;
    }
    let bytes = trimmed.as_bytes();
    let mut depth = 0_i32;
    let mut quote = None::<u8>;
    let mut open = None;
    for idx in (0..bytes.len()).rev() {
        let ch = bytes[idx];
        if let Some(q) = quote {
            // Walking backwards through a quoted span: a quote char that is not
            // backslash-escaped closes (opens, in reverse) the span.
            if ch == q && (idx == 0 || bytes[idx - 1] != b'\\') {
                quote = None;
            }
            continue;
        }
        match ch {
            b'\'' | b'"' | b'`' => quote = Some(ch),
            b']' => depth += 1,
            b'[' => {
                depth -= 1;
                if depth == 0 {
                    open = Some(idx);
                    break;
                }
            }
            _ => {}
        }
    }
    let open = open?;
    if open == 0 {
        return None;
    }
    Some((trimmed[..open].trim(), &trimmed[open..]))
}

/// Evaluate the key inside a `[...]` accessor to a property name string.
fn computed_key_name(accessor: &str, env: &EvalEnv) -> Option<String> {
    let inner = accessor.trim();
    let inner = inner.strip_prefix('[')?.strip_suffix(']')?.trim();
    let value = eval_expr(inner, env);
    Some(coerce_to_string(value))
}

fn set_reference(lhs: &str, value: f64, env: &mut EvalEnv) {
    let lhs = lhs.trim();
    if let Some((object_expr, accessor)) = split_trailing_computed(lhs) {
        if let (Some(object), Some(key)) = (
            eval_property_path(object_expr, env),
            computed_key_name(accessor, env),
        ) {
            set_object_field(object, &key, value);
        }
        return;
    }
    if let Some((head, tail)) = lhs.rsplit_once('.') {
        if let Some(object) = eval_property_path(head, env) {
            set_object_field(object, tail.trim(), value);
        }
        return;
    }
    if env.params.contains_key(lhs) {
        env.params.insert(lhs.to_string(), value);
    } else {
        set_object_field(env.target, lhs, value);
    }
}

/// Rewrite a JS object/array literal into strict JSON so the runtime JSON
/// parser can build it. Next.js serializes the RSC manifest payload with
/// `JSON.stringify` (already strict JSON), but the broader contract accepts
/// plain object literals too, so we quote bare identifier keys (`{a:1}` ->
/// `{"a":1}`) and normalize single-quoted strings to double-quoted. Returns
/// `None` if the text is not a well-formed literal we can normalize.
fn normalize_literal_to_json(expr: &str) -> Option<String> {
    let bytes = expr.as_bytes();
    let mut out = String::with_capacity(expr.len() + 8);
    let mut i = 0;
    // Tracks whether the next bare-identifier run is in a key position (right
    // after `{` or a `,` while inside an object). The brace stack records the
    // kind of each open container: `true` = object, `false` = array.
    let mut object_stack: Vec<bool> = Vec::new();
    let mut expect_key = false;
    while i < bytes.len() {
        let ch = bytes[i];
        match ch {
            b' ' | b'\t' | b'\n' | b'\r' => {
                out.push(ch as char);
                i += 1;
            }
            b'"' | b'\'' => {
                // Copy a quoted string, re-emitting as a double-quoted JSON
                // string. Track escapes so a quote inside the string doesn't end
                // it early.
                let quote = ch;
                out.push('"');
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'\\' && i + 1 < bytes.len() {
                        // `\'` is valid inside a JS single-quoted string but is
                        // NOT a legal JSON escape, so it would make an otherwise
                        // valid literal like `{name: 'can\'t'}` fail JSON
                        // parsing. Emit a plain apostrophe; pass every
                        // JSON-valid escape (\\, \", \n, …) through unchanged.
                        if bytes[i + 1] == b'\'' {
                            out.push('\'');
                        } else {
                            out.push('\\');
                            out.push(bytes[i + 1] as char);
                        }
                        i += 2;
                        continue;
                    }
                    if c == quote {
                        i += 1;
                        break;
                    }
                    if c == b'"' {
                        out.push('\\');
                    }
                    out.push(c as char);
                    i += 1;
                }
                out.push('"');
                expect_key = false;
            }
            b'{' => {
                out.push('{');
                object_stack.push(true);
                expect_key = true;
                i += 1;
            }
            b'[' => {
                out.push('[');
                object_stack.push(false);
                expect_key = false;
                i += 1;
            }
            b'}' | b']' => {
                out.push(ch as char);
                object_stack.pop();
                expect_key = false;
                i += 1;
            }
            b',' => {
                out.push(',');
                expect_key = object_stack.last().copied().unwrap_or(false);
                i += 1;
            }
            b':' => {
                out.push(':');
                expect_key = false;
                i += 1;
            }
            c if c == b'_' || c == b'$' || c.is_ascii_alphabetic() => {
                // Bare identifier run. In key position, JSON-quote it. As a value
                // it can only be a literal keyword (true/false/null); anything
                // else (a context reference) is outside what we normalize here.
                let start = i;
                while i < bytes.len() {
                    let c = bytes[i];
                    if c == b'_' || c == b'$' || c.is_ascii_alphanumeric() {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let ident = &expr[start..i];
                if expect_key {
                    out.push('"');
                    out.push_str(ident);
                    out.push('"');
                    expect_key = false;
                } else if matches!(ident, "true" | "false" | "null") {
                    out.push_str(ident);
                } else {
                    return None;
                }
            }
            _ => {
                out.push(ch as char);
                i += 1;
            }
        }
    }
    Some(out)
}

/// Parse an object/array literal expression into a value. Tries strict JSON
/// first (the common Next.js manifest case), then a lenient JS-literal->JSON
/// normalization. Returns `None` when the text is not a literal we can build.
fn eval_object_or_array_literal(expr: &str) -> Option<f64> {
    let trimmed = expr.trim();
    if !(trimmed.starts_with('{') || trimmed.starts_with('[')) {
        return None;
    }
    let attempt = |text: &str| -> Option<f64> {
        let ptr = string_ptr(text);
        match unsafe { crate::json::js_json_parse_result(ptr) } {
            Ok(value) => Some(f64::from_bits(value.bits())),
            Err(_) => None,
        }
    };
    if let Some(value) = attempt(trimmed) {
        return Some(value);
    }
    let normalized = normalize_literal_to_json(trimmed)?;
    attempt(&normalized)
}

fn is_truthy(value: f64) -> bool {
    crate::value::js_is_truthy(value) != 0
}

fn eval_expr(expr: &str, env: &EvalEnv) -> f64 {
    let expr = strip_wrapping_parens(expr);
    if expr.is_empty() {
        return undefined_value();
    }
    // Logical operators bind looser than comparison/arithmetic, so resolve them
    // first. `find_top_level_operator` returns the last top-level occurrence,
    // which yields correct left-associative short-circuit grouping.
    if let Some(idx) = find_top_level_operator(expr, "||") {
        let left = eval_expr(&expr[..idx], env);
        if is_truthy(left) {
            return left;
        }
        return eval_expr(&expr[idx + 2..], env);
    }
    if let Some(idx) = find_top_level_operator(expr, "&&") {
        let left = eval_expr(&expr[..idx], env);
        if !is_truthy(left) {
            return left;
        }
        return eval_expr(&expr[idx + 2..], env);
    }
    if let Some(idx) = find_top_level_operator(expr, "===") {
        let left = eval_expr(&expr[..idx], env);
        let right = eval_expr(&expr[idx + 3..], env);
        return bool_value(value_same(left, right));
    }
    if let Some(idx) = find_top_level_operator(expr, "!==") {
        let left = eval_expr(&expr[..idx], env);
        let right = eval_expr(&expr[idx + 3..], env);
        return bool_value(!value_same(left, right));
    }
    if let Some(idx) = find_top_level_operator(expr, "+") {
        let left = eval_expr(&expr[..idx], env);
        let right = eval_expr(&expr[idx + 1..], env);
        return add_values(left, right);
    }
    if let Some(idx) = find_top_level_operator(expr, "-") {
        if idx > 0 {
            let left = eval_expr(&expr[..idx], env);
            let right = eval_expr(&expr[idx + 1..], env);
            return number_value(value_to_number(left) - value_to_number(right));
        }
    }
    if let Some(rest) = expr.strip_prefix("typeof ") {
        let value = eval_expr(rest, env);
        let ptr = crate::builtins::js_value_typeof(value);
        return f64::from_bits(JSValue::string_ptr(ptr).bits());
    }
    if let Some(s) = unquote(expr) {
        return string_value(&s);
    }
    if let Ok(n) = expr.parse::<f64>() {
        return number_value(n);
    }
    if let Some(value) = eval_object_or_array_literal(expr) {
        return value;
    }
    eval_property_path(expr, env).unwrap_or_else(undefined_value)
}

fn execute_statement(stmt: &str, env: &mut EvalEnv) -> Option<f64> {
    let stmt = stmt.trim();
    if stmt.is_empty() {
        return Some(undefined_value());
    }
    if let Some(rest) = stmt.strip_prefix("return ") {
        return Some(eval_expr(rest, env));
    }
    let decl = ["var ", "let ", "const "]
        .iter()
        .find_map(|prefix| stmt.strip_prefix(prefix));
    if let Some(rest) = decl {
        let mut last = undefined_value();
        for part in split_top_level(rest, ',') {
            let (name, value) = if let Some((name, rhs)) = part.split_once('=') {
                (name.trim(), eval_expr(rhs, env))
            } else {
                (part.trim(), undefined_value())
            };
            if !name.is_empty() {
                set_reference(name, value, env);
                last = value;
            }
        }
        return Some(last);
    }
    for op in ["+=", "-=", "="] {
        if let Some(idx) = find_top_level_operator(stmt, op) {
            let lhs = stmt[..idx].trim();
            let rhs = stmt[idx + op.len()..].trim();
            let right = eval_expr(rhs, env);
            let value = match op {
                "+=" => add_values(eval_expr(lhs, env), right),
                "-=" => number_value(value_to_number(eval_expr(lhs, env)) - value_to_number(right)),
                _ => right,
            };
            set_reference(lhs, value, env);
            return Some(value);
        }
    }
    Some(eval_expr(stmt, env))
}

fn run_source(source: &str, target: f64, params: HashMap<String, f64>) -> f64 {
    let mut env = EvalEnv { target, params };
    let mut last = undefined_value();
    for stmt in split_top_level(source, ';') {
        if stmt.trim().starts_with("return ") {
            return eval_expr(stmt.trim().trim_start_matches("return "), &env);
        }
        if let Some(value) = execute_statement(stmt, &mut env) {
            last = value;
        }
    }
    last
}

fn install_script_method(
    obj: *mut ObjectHeader,
    obj_value: f64,
    name: &str,
    func: extern "C" fn(*const ClosureHeader, f64, f64) -> f64,
    arity: u32,
) {
    let key = field_key(name);
    let func_ptr = func as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 2);
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    crate::closure::js_closure_set_capture_f64(closure, 0, obj_value);
    crate::object::set_builtin_closure_length(closure as usize, arity);
    let value = crate::value::js_nanbox_pointer(closure as i64);
    crate::object::js_object_set_field_by_name(obj, key, value);
    crate::object::set_builtin_property_attrs(
        obj as usize,
        name.to_string(),
        PropertyAttrs::new(true, false, true),
    );
}

fn make_script(code: String, options: f64) -> f64 {
    let hash = source_hash(CACHE_KIND_SCRIPT, &code, &[]);
    let cached_data = validate_cached_data_option(options);
    let produce_cached_data = validate_produce_cached_data(options);
    let source_map_url = extract_source_map_url(&code);
    let obj = crate::object::js_object_alloc(0, 0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    scripts()
        .lock()
        .unwrap()
        .insert(obj as usize, ScriptMetadata { source: code });
    if let Some(url) = source_map_url {
        set_field(obj, "sourceMapURL", string_value(&url));
    }
    if let Some(bytes) = cached_data {
        set_field(
            obj,
            "cachedDataRejected",
            bool_value(!cache_bytes_accepted(&bytes, CACHE_KIND_SCRIPT, hash)),
        );
    } else if produce_cached_data {
        set_field(
            obj,
            "cachedData",
            cached_data_buffer(CACHE_KIND_SCRIPT, hash),
        );
        set_field(obj, "cachedDataProduced", bool_value(true));
    }
    install_script_method(
        obj,
        value,
        "runInThisContext",
        vm_script_run_in_this_context_method,
        1,
    );
    install_script_method(
        obj,
        value,
        "runInContext",
        vm_script_run_in_context_method,
        2,
    );
    install_script_method(
        obj,
        value,
        "runInNewContext",
        vm_script_run_in_new_context_method,
        2,
    );
    install_script_method(
        obj,
        value,
        "createCachedData",
        vm_script_create_cached_data_method,
        0,
    );
    value
}

extern "C" fn vm_script_create_cached_data_method(
    closure: *const ClosureHeader,
    _unused1: f64,
    _unused2: f64,
) -> f64 {
    let script = crate::closure::js_closure_get_capture_f64(closure, 0);
    let Some(source) = script_source(script) else {
        return cached_data_buffer(CACHE_KIND_SCRIPT, 0);
    };
    cached_data_buffer(
        CACHE_KIND_SCRIPT,
        source_hash(CACHE_KIND_SCRIPT, &source, &[]),
    )
}

extern "C" fn vm_script_run_in_this_context_method(
    closure: *const ClosureHeader,
    _options: f64,
    _unused: f64,
) -> f64 {
    let script = crate::closure::js_closure_get_capture_f64(closure, 0);
    let Some(source) = script_source(script) else {
        return undefined_value();
    };
    run_source(&source, crate::object::js_get_global_this(), HashMap::new())
}

extern "C" fn vm_script_run_in_context_method(
    closure: *const ClosureHeader,
    contextified_object: f64,
    _options: f64,
) -> f64 {
    let script = crate::closure::js_closure_get_capture_f64(closure, 0);
    let Some(source) = script_source(script) else {
        return undefined_value();
    };
    let context = require_context(contextified_object, "contextifiedObject");
    run_source(&source, context, HashMap::new())
}

extern "C" fn vm_script_run_in_new_context_method(
    closure: *const ClosureHeader,
    context_object: f64,
    _options: f64,
) -> f64 {
    let script = crate::closure::js_closure_get_capture_f64(closure, 0);
    let Some(source) = script_source(script) else {
        return undefined_value();
    };
    let context = context_from_arg(context_object, "object");
    run_source(&source, context, HashMap::new())
}

extern "C" fn vm_compiled_function_call(closure: *const ClosureHeader, rest: f64) -> f64 {
    let key = closure as usize;
    let Some(compiled) = functions().lock().unwrap().get(&key).cloned() else {
        return undefined_value();
    };
    let mut params = HashMap::new();
    let rest_arr = array_ptr_from_value(rest);
    for (idx, name) in compiled.params.iter().enumerate() {
        let value = rest_arr
            .map(|arr| crate::array::js_array_get_f64(arr, idx as u32))
            .unwrap_or_else(undefined_value);
        params.insert(name.clone(), value);
    }
    let target = f64::from_bits(compiled.context_bits);
    run_source(&compiled.body, target, params)
}

pub extern "C" fn js_vm_create_script(code: f64, options: f64) -> f64 {
    make_script(code_string_for_script(code), options)
}

pub extern "C" fn js_vm_run_in_context(code: f64, contextified_object: f64, _options: f64) -> f64 {
    let code = code_string_required(code, "code");
    let context = require_context(contextified_object, "contextifiedObject");
    run_source(&code, context, HashMap::new())
}

pub extern "C" fn js_vm_run_in_new_context(code: f64, context_object: f64, _options: f64) -> f64 {
    let code = code_string_required(code, "code");
    let context = context_from_arg(context_object, "object");
    run_source(&code, context, HashMap::new())
}

pub extern "C" fn js_vm_run_in_this_context(code: f64, _options: f64) -> f64 {
    let code = code_string_required(code, "code");
    run_source(&code, crate::object::js_get_global_this(), HashMap::new())
}

pub extern "C" fn js_vm_is_context(object: f64) -> f64 {
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

fn parsing_context_from_options(options: f64) -> f64 {
    let jv = JSValue::from_bits(options.to_bits());
    if jv.is_undefined() || jv.is_null() {
        return crate::object::js_get_global_this();
    }
    let Some(_opts) = object_ptr_from_value(options) else {
        return crate::object::js_get_global_this();
    };
    let parsing = get_object_field(options, "parsingContext");
    let pv = JSValue::from_bits(parsing.to_bits());
    if pv.is_undefined() {
        crate::object::js_get_global_this()
    } else {
        require_context(parsing, "options.parsingContext")
    }
}

pub extern "C" fn js_vm_compile_function(code: f64, params: f64, options: f64) -> f64 {
    let body = code_string_required(code, "code");
    let params = compile_params(params);
    let hash = source_hash(CACHE_KIND_FUNCTION, &body, &params);
    let cached_data = validate_cached_data_option(options);
    let produce_cached_data = validate_produce_cached_data(options);
    let context = parsing_context_from_options(options);
    let func_ptr = vm_compiled_function_call as *const u8;
    crate::closure::js_register_closure_rest(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    crate::object::set_builtin_closure_length(closure as usize, params.len() as u32);
    functions().lock().unwrap().insert(
        closure as usize,
        CompiledFunction {
            body,
            params,
            context_bits: context.to_bits(),
        },
    );
    let value = crate::value::js_nanbox_pointer(closure as i64);
    if let Some(bytes) = cached_data {
        set_value_field(
            value,
            "cachedDataRejected",
            bool_value(!cache_bytes_accepted(&bytes, CACHE_KIND_FUNCTION, hash)),
        );
    } else if produce_cached_data {
        set_value_field(
            value,
            "cachedData",
            cached_data_buffer(CACHE_KIND_FUNCTION, hash),
        );
        set_value_field(value, "cachedDataProduced", bool_value(true));
    }
    value
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
    if let Some(contexts) = VM_CONTEXTS.get() {
        let mut guard = contexts.lock().unwrap();
        let mut rewrites = Vec::new();
        for old in guard.iter().copied().collect::<Vec<_>>() {
            let mut new = old;
            if visitor.visit_metadata_usize_slot(&mut new) && new != old {
                rewrites.push((old, new));
            }
        }
        for (old, new) in rewrites {
            guard.remove(&old);
            if new != 0 {
                guard.insert(new);
            }
        }
    }
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
    if let Some(functions) = VM_FUNCTIONS.get() {
        let mut guard = functions.lock().unwrap();
        let mut rewrites = Vec::new();
        for old in guard.keys().copied().collect::<Vec<_>>() {
            let mut new = old;
            if visitor.visit_metadata_usize_slot(&mut new) && new != old {
                rewrites.push((old, new));
            }
        }
        for compiled in guard.values_mut() {
            visitor.visit_nanbox_u64_slot(&mut compiled.context_bits);
        }
        for (old, new) in rewrites {
            if let Some(compiled) = guard.remove(&old) {
                if new != 0 {
                    guard.insert(new, compiled);
                }
            }
        }
    }
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
    let Some(source) = string_from_value(code) else {
        return throw_vm_type("SourceTextModule source must be a string");
    };
    let hash = source_hash(CACHE_KIND_MODULE, &source, &[]);
    if let Some(bytes) = validate_cached_data_option(options) {
        if !cache_bytes_accepted(&bytes, CACHE_KIND_MODULE, hash) {
            return throw_vm_module_cached_data_rejected();
        }
    }
    let parsed = parse_source(&source);
    let identifier = options_identifier(options).unwrap_or_else(default_identifier);
    let module = new_module_base(KIND_SOURCE, STATUS_UNLINKED, identifier);
    let namespace = crate::object::js_object_alloc_null_proto(0, parsed.exports.len() as u32);
    for export in &parsed.exports {
        set_field(namespace, &export.name, undefined_value());
    }
    set_field(module, FIELD_NAMESPACE, object_value(namespace));
    set_field(module, "namespace", object_value(namespace));
    set_field(module, FIELD_SOURCE, string_value(&source));
    set_field(module, FIELD_REQUESTS, requests_array(&parsed.requests));
    set_field(module, FIELD_IMPORTS, imports_array(&parsed.imports));
    set_field(module, FIELD_EXPORTS, exports_array(&parsed.exports));
    object_value(module)
}

pub extern "C" fn js_vm_synthetic_module_new(
    export_names_value: f64,
    evaluate_callback: f64,
    options: f64,
) -> f64 {
    if !vm_modules_enabled() {
        return throw_vm_unimplemented("SyntheticModule experimental gate", "3133");
    }
    let Some(export_names) = array_ptr_from_value(export_names_value) else {
        return throw_vm_type("SyntheticModule exportNames must be an array");
    };
    let identifier = options_identifier(options).unwrap_or_else(default_identifier);
    let module = new_module_base(KIND_SYNTHETIC, STATUS_LINKED, identifier);
    let namespace = crate::object::js_object_alloc_null_proto(0, 0);
    let len = crate::array::js_array_length(export_names);
    let mut exports = Vec::new();
    for idx in 0..len {
        let value = crate::array::js_array_get_f64(export_names, idx);
        if let Some(name) = string_from_value(value) {
            exports.push(ExportBinding {
                name: name.clone(),
                expr: String::new(),
            });
            set_field(namespace, &name, undefined_value());
        }
    }
    set_field(module, FIELD_NAMESPACE, object_value(namespace));
    set_field(module, "namespace", object_value(namespace));
    set_field(module, FIELD_REQUESTS, requests_array(&[]));
    set_field(module, FIELD_IMPORTS, imports_array(&[]));
    set_field(module, FIELD_EXPORTS, exports_array(&exports));
    set_field(module, FIELD_EVALUATE_CALLBACK, evaluate_callback);
    object_value(module)
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
    match module_kind(module).as_str() {
        KIND_SOURCE => evaluate_source_module(module),
        KIND_SYNTHETIC => evaluate_synthetic_module(module),
        _ => undefined_value(),
    }
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
        "SourceTextModule" => js_vm_source_text_module_new(arg0, arg1),
        "SyntheticModule" => js_vm_synthetic_module_new(arg0, arg1, arg2),
        "createContext" => create_context(arg0),
        "createScript" => js_vm_create_script(arg0, arg1),
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
