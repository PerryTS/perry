//! `node:vm` import/require scaffold and experimental module lifecycle model.
//!
//! Perry still does not host a full VM JavaScript interpreter. The module APIs
//! below intentionally model only the deterministic ESM lifecycle surface that
//! parity fixtures can observe: gated constructors, status/identifier/error
//! state, request metadata, linker callbacks, synthetic exports, namespace
//! values, and simple source-module export evaluation.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::array::ArrayHeader;
use crate::object::ObjectHeader;
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

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_pointer() {
        Some(js.as_pointer::<ObjectHeader>() as *mut ObjectHeader)
    } else {
        None
    }
}

fn array_ptr_from_value(value: f64) -> Option<*mut ArrayHeader> {
    let js = JSValue::from_bits(value.to_bits());
    if js.is_pointer() {
        Some(js.as_pointer::<ArrayHeader>() as *mut ArrayHeader)
    } else {
        None
    }
}

fn value_to_string(value: f64) -> Option<String> {
    let js = JSValue::from_bits(value.to_bits());
    if !js.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() {
        return Some(String::new());
    }
    unsafe {
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        let bytes = std::slice::from_raw_parts(data, (*ptr).byte_len as usize);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }
}

fn set_field(obj: *mut ObjectHeader, name: &str, value: f64) {
    crate::object::js_object_set_field_by_name(obj, string_ptr(name), value);
}

fn get_field(obj: *mut ObjectHeader, name: &str) -> f64 {
    crate::object::js_object_get_field_by_name_f64(obj, string_ptr(name))
}

fn get_string_field(obj: *mut ObjectHeader, name: &str) -> Option<String> {
    value_to_string(get_field(obj, name))
}

fn options_identifier(options: f64) -> Option<String> {
    object_ptr_from_value(options)
        .map(|obj| get_field(obj, "identifier"))
        .and_then(value_to_string)
}

fn default_identifier() -> String {
    let id = MODULE_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("vm:module({id})")
}

fn throw_vm_unimplemented(api: &str, issue: &str) -> f64 {
    let message = format!("node:vm {api} is not implemented in Perry (tracked by #{issue}).");
    crate::fs::validate::throw_error_with_code(&message, "ERR_PERRY_VM_UNIMPLEMENTED")
}

fn throw_vm_status(message: &str) -> f64 {
    crate::fs::validate::throw_error_with_code(message, "ERR_VM_MODULE_STATUS")
}

fn throw_vm_type(message: &str) -> f64 {
    crate::fs::validate::throw_error_with_code(message, "ERR_INVALID_ARG_TYPE")
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

fn term_value(term: &str, env: &HashMap<String, f64>) -> f64 {
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
    if let Some(s) = value_to_string(value) {
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

fn eval_expr(expr: &str, env: &HashMap<String, f64>) -> f64 {
    let mut parts = expr.split('+').map(str::trim);
    let Some(first) = parts.next() else {
        return undefined_value();
    };
    let mut acc = term_value(first, env);
    for part in parts {
        let rhs = term_value(part, env);
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
        let value = eval_expr(&export.expr, &env);
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

pub extern "C" fn js_vm_create_script(_code: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("createScript/Script compilation", "3127")
}

pub extern "C" fn js_vm_run_in_context(
    _code: f64,
    _contextified_object: f64,
    _options: f64,
) -> f64 {
    throw_vm_unimplemented("runInContext execution", "3128")
}

pub extern "C" fn js_vm_run_in_new_context(_code: f64, _context_object: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("runInNewContext execution", "3128")
}

pub extern "C" fn js_vm_run_in_this_context(_code: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("runInThisContext execution", "3127")
}

pub extern "C" fn js_vm_is_context(_object: f64) -> f64 {
    bool_value(false)
}

pub extern "C" fn js_vm_compile_function(_code: f64, _params: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("compileFunction runtime function construction", "3130")
}

pub extern "C" fn js_vm_measure_memory(_options: f64) -> f64 {
    throw_vm_unimplemented("measureMemory", "3284")
}

pub extern "C" fn js_vm_script_call(_code: f64, _options: f64) -> f64 {
    throw_vm_unimplemented("Script constructor execution", "3127")
}

pub extern "C" fn js_vm_module_call() -> f64 {
    crate::fs::validate::throw_error_with_code(
        "Module is not a constructor",
        "ERR_ILLEGAL_CONSTRUCTOR",
    )
}

pub extern "C" fn js_vm_source_text_module_new(code: f64, options: f64) -> f64 {
    if !vm_modules_enabled() {
        return throw_vm_unimplemented("SourceTextModule experimental gate", "3132");
    }
    let Some(source) = value_to_string(code) else {
        return throw_vm_type("SourceTextModule source must be a string");
    };
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
        if let Some(name) = value_to_string(value) {
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
    let Some(name) = value_to_string(name_value) else {
        return throw_vm_type("SyntheticModule export name must be a string");
    };
    let exports = read_exports(module);
    if !exports.iter().any(|export| export.name == name) {
        return throw_vm_status("SyntheticModule export is not declared");
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
        "createContext" => crate::object::js_vm_create_context(arg0),
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
        "linkRequests" => js_vm_source_text_module_link_requests(arg0, arg1),
        "instantiate" => js_vm_source_text_module_instantiate(arg0),
        "hasTopLevelAwait" => js_vm_source_text_module_has_top_level_await(arg0),
        "hasAsyncGraph" => js_vm_source_text_module_has_async_graph(arg0),
        "setExport" => js_vm_synthetic_module_set_export(arg0, arg1, arg2),
        _ => undefined_value(),
    }
}
