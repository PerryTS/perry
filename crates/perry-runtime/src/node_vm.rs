//! Narrow `node:vm` execution support.
//!
//! Perry is V8-free, so this is not a full JavaScript interpreter. It models the
//! Node-observable VM shape plus a deterministic local expression subset used by
//! package feature checks and the VM parity fixtures: context markers,
//! object-backed sandbox reads/writes, repeated `Script` execution,
//! `runIn*Context`, and `compileFunction` functions with parameter/context
//! binding. VM modules, cached-data/source-map metadata, and `measureMemory`
//! remain intentionally out of scope.

use crate::array::ArrayHeader;
use crate::closure::ClosureHeader;
use crate::object::{ObjectHeader, PropertyAttrs};
use crate::string::StringHeader;
use crate::value::{JSValue, TAG_UNDEFINED};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

#[derive(Clone)]
struct CompiledFunction {
    body: String,
    params: Vec<String>,
    context_bits: u64,
}

struct EvalEnv {
    target: f64,
    params: HashMap<String, f64>,
}

static VM_CONTEXTS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
static VM_SCRIPTS: OnceLock<Mutex<HashMap<usize, String>>> = OnceLock::new();
static VM_FUNCTIONS: OnceLock<Mutex<HashMap<usize, CompiledFunction>>> = OnceLock::new();

fn contexts() -> &'static Mutex<HashSet<usize>> {
    VM_CONTEXTS.get_or_init(|| Mutex::new(HashSet::new()))
}

fn scripts() -> &'static Mutex<HashMap<usize, String>> {
    VM_SCRIPTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn functions() -> &'static Mutex<HashMap<usize, CompiledFunction>> {
    VM_FUNCTIONS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn string_value(value: &str) -> f64 {
    let ptr = crate::string::js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn number_value(value: f64) -> f64 {
    f64::from_bits(JSValue::number(value).bits())
}

fn throw_vm_unimplemented(api: &str, issue: &str) -> f64 {
    let message = format!("node:vm {api} is not implemented in Perry (tracked by #{issue}).");
    crate::fs::validate::throw_error_with_code(&message, "ERR_PERRY_VM_UNIMPLEMENTED")
}

// `createContext` is handled by the working implementation in
// `object::native_module.rs` (#4050); routed there from dispatch.

fn throw_invalid_arg(message: &str) -> ! {
    crate::fs::validate::throw_type_error_with_code(message, "ERR_INVALID_ARG_TYPE")
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

fn array_ptr_from_value(value: f64) -> Option<*const ArrayHeader> {
    if crate::array::js_array_is_array(value).to_bits() != JSValue::bool(true).bits() {
        return None;
    }
    let raw = crate::value::js_nanbox_get_pointer(value);
    if raw == 0 {
        None
    } else {
        Some(raw as *const ArrayHeader)
    }
}

fn field_key(name: &str) -> *mut StringHeader {
    crate::string::js_string_from_bytes(name.as_ptr(), name.len() as u32)
}

fn get_object_field(object: f64, name: &str) -> f64 {
    let Some(ptr) = object_ptr_from_value(object) else {
        return undefined_value();
    };
    let key = field_key(name);
    f64::from_bits(crate::object::js_object_get_field_by_name(ptr, key).bits())
}

fn set_object_field(object: f64, name: &str, value: f64) {
    if let Some(ptr) = object_ptr_from_value(object) {
        let key = field_key(name);
        crate::object::js_object_set_field_by_name(ptr, key, value);
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

fn value_to_string(value: f64) -> String {
    let ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    rust_string_from_header(ptr).unwrap_or_default()
}

fn add_values(a: f64, b: f64) -> f64 {
    let aj = JSValue::from_bits(a.to_bits());
    let bj = JSValue::from_bits(b.to_bits());
    if aj.is_any_string() || bj.is_any_string() {
        return string_value(&format!("{}{}", value_to_string(a), value_to_string(b)));
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

fn set_reference(lhs: &str, value: f64, env: &mut EvalEnv) {
    let lhs = lhs.trim();
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

fn eval_expr(expr: &str, env: &EvalEnv) -> f64 {
    let expr = strip_wrapping_parens(expr);
    if expr.is_empty() {
        return undefined_value();
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

fn make_script(code: String) -> f64 {
    let obj = crate::object::js_object_alloc(0, 0);
    let value = crate::value::js_nanbox_pointer(obj as i64);
    scripts().lock().unwrap().insert(obj as usize, code);
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
    value
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
    let context = context_from_arg(context_object, "contextObject");
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

pub extern "C" fn js_vm_create_context(context_object: f64, _options: f64) -> f64 {
    context_from_arg(context_object, "object")
}

pub extern "C" fn js_vm_create_script(code: f64, _options: f64) -> f64 {
    make_script(code_string_for_script(code))
}

pub extern "C" fn js_vm_run_in_context(code: f64, contextified_object: f64, _options: f64) -> f64 {
    let code = code_string_required(code, "code");
    let context = require_context(contextified_object, "contextifiedObject");
    run_source(&code, context, HashMap::new())
}

pub extern "C" fn js_vm_run_in_new_context(code: f64, context_object: f64, _options: f64) -> f64 {
    let code = code_string_required(code, "code");
    let context = context_from_arg(context_object, "contextObject");
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
    crate::value::js_nanbox_pointer(closure as i64)
}

pub extern "C" fn js_vm_measure_memory(_options: f64) -> f64 {
    throw_vm_unimplemented("measureMemory", "3284")
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

/// Dispatch a `node:vm` module method reached as a value/namespace call
/// (e.g. `vm.createScript(...)` or a bound export). `createContext` routes to
/// the working #4050 contextification helper; the rest are the shape-only
/// scaffold (#4079) plus measureMemory validation (#4087).
pub fn dispatch_vm_method(method: &str, arg0: f64, arg1: f64, arg2: f64) -> f64 {
    match method {
        "Script" => js_vm_script_call(arg0, arg1),
        "createContext" => crate::object::js_vm_create_context(arg0),
        "createScript" => js_vm_create_script(arg0, arg1),
        "runInContext" => js_vm_run_in_context(arg0, arg1, arg2),
        "runInNewContext" => js_vm_run_in_new_context(arg0, arg1, arg2),
        "runInThisContext" => js_vm_run_in_this_context(arg0, arg1),
        "isContext" => js_vm_is_context(arg0),
        "compileFunction" => js_vm_compile_function(arg0, arg1, arg2),
        "measureMemory" => js_vm_measure_memory(arg0),
        _ => f64::from_bits(JSValue::undefined().bits()),
    }
}
