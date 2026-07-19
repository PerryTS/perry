//! `node:module` runtime API: builtin-module inventory, the CJS resolver
//! subset (`_resolveFilename`/`_findPath`/`_nodeModulePaths`/…), `SourceMap`,
//! `findPackageJSON`, the compile-cache + source-maps-support state, loader
//! hooks (`register`/`registerHooks`), and the strip-types helper. Also hosts
//! `process.getBuiltinModule`. Split out of the `process` trunk. Pure code
//! move — no behavior change.

use super::*;
use crate::closure::{
    js_closure_alloc, js_closure_get_capture_f64, js_closure_set_capture_f64, ClosureHeader,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

pub fn scan_process_module_loader_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    MODULE_LOADER_HOOKS.with(|hooks| {
        for entry in hooks.borrow_mut().iter_mut() {
            visitor.visit_nanbox_f64_slot(&mut entry.resolve);
            visitor.visit_nanbox_f64_slot(&mut entry.load);
        }
    });
    MODULE_LOADER_NEXT_RESOLVE.with(|cell| {
        let mut callback = cell.get();
        if !callback.is_null() && visitor.visit_raw_const_ptr_slot(&mut callback) {
            cell.set(callback);
        }
    });
    MODULE_LOADER_NEXT_LOAD.with(|cell| {
        let mut callback = cell.get();
        if !callback.is_null() && visitor.visit_raw_const_ptr_slot(&mut callback) {
            cell.set(callback);
        }
    });
}

/// `module.builtinModules` — Node exposes this as an Array of builtin module
/// specifiers. Perry's supported subset is smaller, but the public inventory
/// shape should still match Node's module API.
#[no_mangle]
pub extern "C" fn js_module_builtin_modules() -> f64 {
    let arr = crate::array::js_array_alloc_with_length(MODULE_BUILTIN_MODULES.len() as u32);
    for (i, name) in MODULE_BUILTIN_MODULES.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(name));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

/// Minimal `module.constants` shape. The compile-cache status values are not
/// backed by an actual bytecode cache in Perry, but Node exposes the enum as
/// stable process state for feature detection.
#[no_mangle]
pub extern "C" fn js_module_constants() -> f64 {
    let constants = crate::object::js_object_alloc(0, 1);
    let compile_cache_status = crate::object::js_object_alloc(0, 4);
    module_set_field(compile_cache_status, "FAILED", 0.0);
    module_set_field(compile_cache_status, "ENABLED", 1.0);
    module_set_field(compile_cache_status, "ALREADY_ENABLED", 2.0);
    module_set_field(compile_cache_status, "DISABLED", 3.0);
    module_set_field(
        constants,
        "compileCacheStatus",
        module_object_value(compile_cache_status),
    );
    module_object_value(constants)
}

extern "C" fn module_require_thunk(
    _closure: *const crate::closure::ClosureHeader,
    _specifier: f64,
) -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

fn module_null() -> f64 {
    f64::from_bits(crate::value::TAG_NULL)
}

/// `new Module(id)` — CommonJS module record constructor shape. Perry does not
/// execute CJS modules through this object yet; this mirrors Node's observable
/// constructor fields and leaves loading to the resolver helpers below.
#[no_mangle]
pub extern "C" fn js_module_module_new(id: f64) -> f64 {
    let id_string = module_value_to_string(id).unwrap_or_default();
    let keys = b"id\0path\0exports\0filename\0loaded\0children\0parent\0require\0";
    let obj =
        crate::object::js_object_alloc_with_shape(0xC0_00_4D, 8, keys.as_ptr(), keys.len() as u32);
    let exports = crate::object::js_object_alloc(0, 0);
    let children = crate::array::js_array_alloc_with_length(0);
    crate::object::js_object_set_field(
        obj,
        0,
        JSValue::from_bits(module_string_value(&id_string).to_bits()),
    );
    crate::object::js_object_set_field(
        obj,
        1,
        JSValue::from_bits(module_string_value(&module_cjs_dirname(&id_string)).to_bits()),
    );
    crate::object::js_object_set_field(
        obj,
        2,
        JSValue::from_bits(module_object_value(exports).to_bits()),
    );
    crate::object::js_object_set_field(obj, 3, JSValue::from_bits(module_null().to_bits()));
    crate::object::js_object_set_field(
        obj,
        4,
        JSValue::from_bits(module_bool_value(false).to_bits()),
    );
    crate::object::js_object_set_field(
        obj,
        5,
        JSValue::from_bits(JSValue::array_ptr(children).bits()),
    );
    crate::object::js_object_set_field(obj, 6, JSValue::from_bits(module_null().to_bits()));
    crate::object::js_object_set_field(
        obj,
        7,
        JSValue::from_bits(module_function1("require", module_require_thunk, 1).to_bits()),
    );
    module_object_value(obj)
}

fn module_cjs_dirname(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    std::path::Path::new(path)
        .parent()
        .map(|p| {
            let s = p.to_string_lossy();
            if s.is_empty() {
                ".".to_string()
            } else {
                s.into_owned()
            }
        })
        .unwrap_or_else(|| ".".to_string())
}

fn module_cjs_string_array(items: Vec<String>) -> f64 {
    let arr = crate::array::js_array_alloc_with_length(items.len() as u32);
    for (i, item) in items.iter().enumerate() {
        crate::array::js_array_set_f64(arr, i as u32, module_string_value(item));
    }
    f64::from_bits(JSValue::array_ptr(arr).bits())
}

fn module_cjs_array_strings(value: f64) -> Option<Vec<String>> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type != crate::gc::GC_TYPE_ARRAY {
        return None;
    }
    let arr = ptr as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    let mut out = Vec::with_capacity(len as usize);
    for i in 0..len {
        if let Some(item) = module_value_to_string(crate::array::js_array_get_f64(arr, i)) {
            out.push(item);
        }
    }
    Some(out)
}

fn module_is_builtin_specifier(specifier: &str) -> bool {
    if let Some(name) = specifier.strip_prefix("node:") {
        MODULE_BUILTIN_MODULES.contains(&specifier) || MODULE_BUILTIN_MODULES.contains(&name)
    } else {
        MODULE_BUILTIN_MODULES.contains(&specifier)
    }
}

fn module_parent_base_dir(parent: f64) -> std::path::PathBuf {
    if let Some(parent_obj) = module_object_ptr(parent) {
        if let Some(filename) =
            module_value_to_string(module_get_named_field(parent_obj, "filename"))
        {
            return std::path::PathBuf::from(module_cjs_dirname(&filename));
        }
        if let Some(path) = module_value_to_string(module_get_named_field(parent_obj, "path")) {
            return std::path::PathBuf::from(path);
        }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
}

fn module_parent_lookup_paths(parent: f64) -> Option<Vec<String>> {
    let parent_obj = module_object_ptr(parent)?;
    module_cjs_array_strings(module_get_named_field(parent_obj, "paths"))
}

fn module_node_module_paths_vec(from: &str) -> Vec<String> {
    let mut current = std::path::PathBuf::from(from);
    if !current.is_absolute() {
        current = std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(current);
    }
    let mut out = Vec::new();
    loop {
        out.push(current.join("node_modules").to_string_lossy().into_owned());
        if !current.pop() {
            break;
        }
    }
    out
}

fn module_resolve_file(path: &std::path::Path) -> Option<std::path::PathBuf> {
    if path.is_file() {
        return Some(std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()));
    }
    for ext in ["js", "json", "node"] {
        let candidate = path.with_extension(ext);
        if candidate.is_file() {
            return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
        }
    }
    if path.is_dir() {
        for ext in ["js", "json", "node"] {
            let candidate = path.join(format!("index.{ext}"));
            if candidate.is_file() {
                return Some(std::fs::canonicalize(&candidate).unwrap_or(candidate));
            }
        }
    }
    None
}

fn module_resolve_local_request(
    request: &str,
    parent: f64,
    lookup_paths: Option<Vec<String>>,
) -> Option<std::path::PathBuf> {
    if request.starts_with('/') {
        return module_resolve_file(std::path::Path::new(request));
    }
    if request.starts_with("./") || request.starts_with("../") {
        let base = module_parent_base_dir(parent);
        return module_resolve_file(&base.join(request));
    }
    let paths = lookup_paths
        .or_else(|| module_parent_lookup_paths(parent))
        .unwrap_or_else(|| {
            module_node_module_paths_vec(&module_parent_base_dir(parent).to_string_lossy())
        });
    for lookup in paths {
        if let Some(path) = module_resolve_file(&std::path::PathBuf::from(lookup).join(request)) {
            return Some(path);
        }
    }
    None
}

fn module_throw_not_found(request: &str) -> ! {
    let message = format!("Cannot find module '{request}'");
    crate::fs::validate::throw_error_with_code(&message, "MODULE_NOT_FOUND")
}

/// `Module._nodeModulePaths(from)` — directory ancestry search order.
#[no_mangle]
pub extern "C" fn js_module_node_module_paths(from: f64) -> f64 {
    let from = module_value_to_string(from).unwrap_or_else(|| ".".to_string());
    module_cjs_string_array(module_node_module_paths_vec(&from))
}

/// `Module._resolveLookupPaths(request, parent)` — builtin requests return
/// `null`; local paths return the parent directory; package requests return
/// the parent's `paths` array (or a generated node_modules ancestry).
#[no_mangle]
pub extern "C" fn js_module_resolve_lookup_paths(request: f64, parent: f64) -> f64 {
    let Some(request) = module_value_to_string(request) else {
        return module_cjs_string_array(Vec::new());
    };
    if module_is_builtin_specifier(&request) {
        return module_null();
    }
    if request.starts_with("./") || request.starts_with("../") || request.starts_with('/') {
        return module_cjs_string_array(vec![module_parent_base_dir(parent)
            .to_string_lossy()
            .into_owned()]);
    }
    let mut paths = module_parent_lookup_paths(parent).unwrap_or_else(|| {
        module_node_module_paths_vec(&module_parent_base_dir(parent).to_string_lossy())
    });
    if let Some(global_paths) =
        module_cjs_array_strings(crate::object::module_cjs_global_paths_value())
    {
        paths.extend(global_paths);
    }
    module_cjs_string_array(paths)
}

/// `Module._resolveFilename(request, parent, isMain, options)` — deterministic
/// builtin and local-file resolver subset.
#[no_mangle]
pub extern "C" fn js_module_resolve_filename(
    request: f64,
    parent: f64,
    _is_main: f64,
    _options: f64,
) -> f64 {
    let Some(request) = module_value_to_string(request) else {
        module_throw_not_found("");
    };
    if module_is_builtin_specifier(&request) {
        return module_string_value(&request);
    }
    match module_resolve_local_request(&request, parent, None) {
        Some(path) => module_string_value(&path.to_string_lossy()),
        None => module_throw_not_found(&request),
    }
}

/// `Module._findPath(request, paths, isMain)` — search explicit lookup
/// directories for the same deterministic file cases `_resolveFilename`
/// supports.
#[no_mangle]
pub extern "C" fn js_module_find_path(request: f64, paths: f64, _is_main: f64) -> f64 {
    let Some(request) = module_value_to_string(request) else {
        return module_bool_value(false);
    };
    if module_is_builtin_specifier(&request) {
        return module_bool_value(false);
    }
    let lookup_paths = module_cjs_array_strings(paths).unwrap_or_default();
    for lookup in lookup_paths {
        let candidate = if request.starts_with('/') {
            std::path::PathBuf::from(&request)
        } else {
            std::path::PathBuf::from(lookup).join(&request)
        };
        if let Some(path) = module_resolve_file(&candidate) {
            return module_string_value(&path.to_string_lossy());
        }
    }
    module_bool_value(false)
}

#[no_mangle]
pub extern "C" fn js_module_init_paths() -> f64 {
    let _ = crate::object::module_cjs_global_paths_value();
    module_undefined()
}

#[no_mangle]
pub extern "C" fn js_module_preload_modules(_modules: f64) -> f64 {
    module_undefined()
}

/// `Module._load(request, parent, isMain)` — currently implements the safe
/// builtin path used by feature detection. Non-builtin CJS execution remains
/// outside this compatibility cut.
#[no_mangle]
pub extern "C" fn js_module_load(request: f64, _parent: f64, _is_main: f64) -> f64 {
    let Some(request) = module_value_to_string(request) else {
        return module_undefined();
    };
    if module_is_builtin_specifier(&request) {
        return js_process_get_builtin_module(module_string_value(&request));
    }
    module_undefined()
}

/// Constructor for `new module.SourceMap(payload)`. Preserves the payload
/// object and exposes working `findEntry`/`findOrigin` lookups. The bound
/// method closures capture the payload (slot 0) so the lookup thunks can
/// decode its `mappings`/`sources`/`names` without a separate `this` channel
/// (mirrors the dgram socket-method pattern). #3675.
#[no_mangle]
pub extern "C" fn js_module_source_map_new(payload: f64) -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(obj, "payload", payload);
    module_set_field(
        obj,
        "findEntry",
        source_map_method(payload, "findEntry", source_map_find_entry_thunk),
    );
    module_set_field(
        obj,
        "findOrigin",
        source_map_method(payload, "findOrigin", source_map_find_origin_thunk),
    );
    module_object_value(obj)
}

type SourceMapThunk = extern "C" fn(*const ClosureHeader, f64) -> f64;

/// Build a bound SourceMap method closure that captures `payload` in slot 0
/// and packs all call arguments into a single rest array.
fn source_map_method(payload: f64, name: &str, thunk: SourceMapThunk) -> f64 {
    let func_ptr = thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 1);
    js_closure_set_capture_f64(closure, 0, payload);
    crate::closure::js_register_closure_rest(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::value::js_nanbox_pointer(closure as i64)
}

/// Decode a base64 VLQ alphabet byte to its 0–63 value.
fn source_map_b64(c: u8) -> Option<i64> {
    match c {
        b'A'..=b'Z' => Some((c - b'A') as i64),
        b'a'..=b'z' => Some((c - b'a' + 26) as i64),
        b'0'..=b'9' => Some((c - b'0' + 52) as i64),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decode one comma-delimited segment's VLQ fields.
fn source_map_decode_segment(seg: &[u8]) -> Vec<i64> {
    let mut out = Vec::new();
    let mut value: i64 = 0;
    let mut shift: u32 = 0;
    for &b in seg {
        let Some(digit) = source_map_b64(b) else {
            continue;
        };
        let cont = (digit & 0x20) != 0;
        value += (digit & 0x1f) << shift;
        if cont {
            shift += 5;
        } else {
            let negative = (value & 1) != 0;
            let decoded = value >> 1;
            out.push(if negative { -decoded } else { decoded });
            value = 0;
            shift = 0;
        }
    }
    out
}

#[derive(Clone, Copy)]
struct SourceMapEntry {
    generated_line: i64,
    generated_column: i64,
    // `None` for genCol-only (1-field) segments that mark an unmapped position.
    // The inner name index is `Some` only for segments that carried an explicit
    // 5th VLQ field (a named mapping).
    original: Option<(i64, i64, i64, Option<i64>)>, // (source_index, line, column, name_index)
}

/// Decode the full `mappings` string into ordered entries with cumulative
/// source/line/column/name indices per the Source Map v3 grammar. `name_index`
/// is attached only to genuinely-named (5-field) segments, matching how a
/// position with no explicit name resolves (Node returns no `name` for the
/// names-less mapping in the issue repro).
fn source_map_decode(mappings: &str) -> Vec<SourceMapEntry> {
    let mut entries = Vec::new();
    let (mut src_idx, mut src_line, mut src_col, mut name_idx) = (0i64, 0i64, 0i64, 0i64);
    for (gen_line, line) in mappings.split(';').enumerate() {
        let mut gen_col = 0i64;
        for seg in line.split(',') {
            if seg.is_empty() {
                continue;
            }
            let fields = source_map_decode_segment(seg.as_bytes());
            if fields.is_empty() {
                continue;
            }
            gen_col += fields[0];
            let original = if fields.len() >= 4 {
                src_idx += fields[1];
                src_line += fields[2];
                src_col += fields[3];
                let name = if fields.len() >= 5 {
                    name_idx += fields[4];
                    Some(name_idx)
                } else {
                    None
                };
                Some((src_idx, src_line, src_col, name))
            } else {
                None
            };
            entries.push(SourceMapEntry {
                generated_line: gen_line as i64,
                generated_column: gen_col,
                original,
            });
        }
    }
    entries
}

/// Read `payload.<field>` as a raw JSValue f64 (undefined when absent or when
/// the payload is not a heap object).
fn source_map_field(payload: f64, field: &str) -> f64 {
    let p = JSValue::from_bits(payload.to_bits());
    if !p.is_pointer() {
        return undefined_value();
    }
    let obj = crate::value::js_nanbox_get_pointer(payload) as *const crate::object::ObjectHeader;
    if obj.is_null() {
        return undefined_value();
    }
    let key = js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let v = crate::object::js_object_get_field_by_name(obj, key);
    f64::from_bits(v.bits())
}

/// Read `payload.<field>` as a Rust string, if it is a string value.
fn source_map_field_string(payload: f64, field: &str) -> Option<String> {
    let value = JSValue::from_bits(source_map_field(payload, field).to_bits());
    let mut sso = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let bytes = unsafe { crate::string::js_string_key_bytes(value, &mut sso) }?;
    Some(String::from_utf8_lossy(bytes).into_owned())
}

/// Read `payload.<arrayField>[index]` as a raw JSValue f64 (undefined when out
/// of range or not an array).
fn source_map_array_element(payload: f64, field: &str, index: i64) -> f64 {
    if index < 0 {
        return undefined_value();
    }
    let arr_value = source_map_field(payload, field);
    let av = JSValue::from_bits(arr_value.to_bits());
    if !av.is_pointer() {
        return undefined_value();
    }
    let arr = crate::value::js_nanbox_get_pointer(arr_value) as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return undefined_value();
    }
    let len = crate::array::js_array_length(arr);
    if index as u32 >= len {
        return undefined_value();
    }
    crate::array::js_array_get_f64(arr, index as u32)
}

fn source_map_collect_args(rest: f64) -> Vec<f64> {
    let rv = JSValue::from_bits(rest.to_bits());
    if !rv.is_pointer() {
        return Vec::new();
    }
    let arr = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
    if arr.is_null() {
        return Vec::new();
    }
    let len = crate::array::js_array_length(arr);
    (0..len)
        .map(|i| crate::array::js_array_get_f64(arr, i))
        .collect()
}

/// Coerce call argument `idx` to a finite number, if it is one.
fn source_map_arg_number(args: &[f64], idx: usize) -> Option<f64> {
    args.get(idx)
        .map(|v| JSValue::from_bits(v.to_bits()).to_number())
        .filter(|n| n.is_finite())
}

fn source_map_arg_i64(args: &[f64], idx: usize) -> i64 {
    source_map_arg_number(args, idx)
        .map(|n| n as i64)
        .unwrap_or(0)
}

/// Decode the payload's `mappings` and return the greatest entry whose
/// generated position is `<=` (line, column). Entries are emitted in
/// non-decreasing order, so the last non-exceeding one wins.
fn source_map_lookup(payload: f64, line: i64, col: i64) -> Option<SourceMapEntry> {
    let mappings = source_map_field_string(payload, "mappings")?;
    let mut best = None;
    for entry in source_map_decode(&mappings) {
        if (entry.generated_line, entry.generated_column) <= (line, col) {
            best = Some(entry);
        } else {
            break;
        }
    }
    best
}

/// Build the `{ name?, fileName, lineNumber, columnNumber }` shape Node's
/// `findOrigin` echoes (name/fileName from the matched entry; line/column from
/// the call arguments). Insertion order matches Node for byte-identical JSON.
fn source_map_origin_object(
    payload: f64,
    entry: Option<SourceMapEntry>,
    line: Option<f64>,
    col: Option<f64>,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 4);
    if let Some(SourceMapEntry {
        original: Some((source_index, _, _, name_index)),
        ..
    }) = entry
    {
        if let Some(name_index) = name_index {
            let name = source_map_array_element(payload, "names", name_index);
            if JSValue::from_bits(name.to_bits()).is_string() {
                module_set_field(obj, "name", name);
            }
        }
        module_set_field(
            obj,
            "fileName",
            source_map_array_element(payload, "sources", source_index),
        );
    }
    let null = f64::from_bits(crate::value::TAG_NULL);
    module_set_field(obj, "lineNumber", line.map_or(null, |n| n));
    module_set_field(obj, "columnNumber", col.map_or(null, |n| n));
    module_object_value(obj)
}

/// `SourceMap#findEntry(lineNumber, columnNumber)` — return the greatest
/// decoded entry whose generated position is `<=` the query, shaped like
/// Node's `{ generatedLine, generatedColumn, originalSource, originalLine,
/// originalColumn, name? }`. Returns `{}` when no entry precedes the query.
extern "C" fn source_map_find_entry_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let payload = js_closure_get_capture_f64(closure, 0);
    let args = source_map_collect_args(rest);
    let query_line = source_map_arg_i64(&args, 0);
    let query_col = source_map_arg_i64(&args, 1);

    let Some(entry) = source_map_lookup(payload, query_line, query_col) else {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    };

    let obj = crate::object::js_object_alloc(0, 6);
    module_set_field(obj, "generatedLine", entry.generated_line as f64);
    module_set_field(obj, "generatedColumn", entry.generated_column as f64);
    if let Some((source_index, original_line, original_column, name_index)) = entry.original {
        module_set_field(
            obj,
            "originalSource",
            source_map_array_element(payload, "sources", source_index),
        );
        module_set_field(obj, "originalLine", original_line as f64);
        module_set_field(obj, "originalColumn", original_column as f64);
        if let Some(name_index) = name_index {
            let name = source_map_array_element(payload, "names", name_index);
            if JSValue::from_bits(name.to_bits()).is_string() {
                module_set_field(obj, "name", name);
            }
        }
    }
    module_object_value(obj)
}

/// `SourceMap#findOrigin(lineNumber, columnNumber)`. Node echoes the queried
/// coordinates (as `lineNumber`/`columnNumber`, or `null` when an argument is
/// not a finite number) and tags on the `name`/`fileName` of the entry at that
/// generated position. The lone special case is a numeric `(0, 0)` query, for
/// which Node returns an empty object.
extern "C" fn source_map_find_origin_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let payload = js_closure_get_capture_f64(closure, 0);
    let args = source_map_collect_args(rest);
    let line = source_map_arg_number(&args, 0);
    let col = source_map_arg_number(&args, 1);

    if line == Some(0.0) && col == Some(0.0) {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    }

    let entry = source_map_lookup(
        payload,
        line.map(|n| n as i64).unwrap_or(0),
        col.map(|n| n as i64).unwrap_or(0),
    );
    source_map_origin_object(payload, entry, line, col)
}

/// Module.isBuiltin(id) -> boolean
#[no_mangle]
pub extern "C" fn js_module_is_builtin(id: f64) -> f64 {
    let value = JSValue::from_bits(id.to_bits());
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(value, &mut sso_buf) }) else {
        return f64::from_bits(crate::value::TAG_FALSE);
    };
    let Ok(specifier) = std::str::from_utf8(bytes) else {
        return f64::from_bits(crate::value::TAG_FALSE);
    };
    let is_builtin = if let Some(name) = specifier.strip_prefix("node:") {
        MODULE_BUILTIN_MODULES.contains(&specifier) || MODULE_BUILTIN_MODULES.contains(&name)
    } else {
        MODULE_BUILTIN_MODULES.contains(&specifier)
    };
    f64::from_bits(if is_builtin {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `module.findPackageJSON(specifier[, base])` — resolve the nearest
/// `package.json` for a resolved specifier (#3120). Perry implements the
/// local-specifier path: the `specifier` is resolved against `base`'s
/// directory (when relative/absolute) and Perry walks parent directories
/// looking for `package.json`, returning its absolute path. The result is
/// canonicalized to match Node's realpath-based output.
///
/// Argument validation matches Node's observable surface:
///   * missing `specifier` → `TypeError [ERR_MISSING_ARGS]`
///   * `base` that is not a string/URL (number, null, …) →
///     `TypeError [ERR_INVALID_ARG_TYPE]`
///   * no enclosing `package.json` → `undefined`
#[no_mangle]
pub extern "C" fn js_module_find_package_json(specifier: f64, base: f64) -> f64 {
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);

    // `specifier` is required and must be a string (Perry covers the
    // local-path/file-URL specifier shape).
    if specifier.to_bits() == crate::value::TAG_UNDEFINED {
        crate::fs::validate::throw_error_with_code(
            "The \"specifier\" argument must be specified",
            "ERR_MISSING_ARGS",
        );
    }
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let spec_value = JSValue::from_bits(specifier.to_bits());
    let Some(spec_bytes) =
        (unsafe { crate::string::js_string_key_bytes(spec_value, &mut sso_buf) })
    else {
        let message = format!(
            "The \"specifier\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(specifier)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let specifier_str = String::from_utf8_lossy(spec_bytes).into_owned();

    // Resolve `base` to a directory. A missing/undefined base anchors at the
    // current working directory (Node requires a base for relative specifiers,
    // but the observable test surface always passes one).
    let base_path = if base.to_bits() == crate::value::TAG_UNDEFINED {
        std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    } else {
        match crate::url::node_compat::module_base_to_path(base) {
            Some(p) => p,
            None => {
                let message = format!(
                    "The \"base\" argument must be of type string or an instance of URL. Received {}",
                    crate::fs::validate::describe_received(base)
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
            }
        }
    };

    let Some(pkg_path) = find_nearest_package_json(&specifier_str, &base_path) else {
        return undefined;
    };
    module_string_value(&pkg_path)
}

/// Resolve `specifier` against `base`'s directory, then walk parent
/// directories looking for a `package.json`. Returns the canonicalized
/// absolute path of the first match. `base` may name a file or a directory
/// (trailing separator); both anchor at the containing directory.
fn find_nearest_package_json(specifier: &str, base: &str) -> Option<String> {
    use std::path::{Path, PathBuf};

    let base_path = Path::new(base);
    // A directory base (trailing separator) or an existing directory anchors
    // resolution at itself; otherwise resolve against the parent directory of
    // the base file.
    let base_dir: PathBuf = if base.ends_with(std::path::MAIN_SEPARATOR) || base_path.is_dir() {
        base_path.to_path_buf()
    } else {
        base_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."))
    };

    let resolved = if Path::new(specifier).is_absolute() {
        PathBuf::from(specifier)
    } else {
        base_dir.join(specifier)
    };

    // Start the upward walk at the directory containing the resolved target.
    let mut dir = if resolved.is_dir() {
        resolved
    } else {
        resolved
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or(base_dir)
    };

    loop {
        let candidate = dir.join("package.json");
        if candidate.is_file() {
            let canonical = std::fs::canonicalize(&candidate).unwrap_or(candidate);
            return Some(canonical.to_string_lossy().into_owned());
        }
        match dir.parent() {
            Some(parent) => dir = parent.to_path_buf(),
            None => return None,
        }
    }
}

/// Devirt codegen entry for `process.getBuiltinModule(...)`. Arms the install-all
/// hook (so the dynamically-resolved namespace can dispatch methods) and
/// delegates. Codegen targets THIS symbol, so `js_nm_enable_install_all` — and
/// thus the all-buckets `js_nm_install_all` — is referenced only by programs
/// whose source actually calls `getBuiltinModule`. The plain
/// `js_process_get_builtin_module` (pinned by the runtime process method table in
/// every program) stays free of that reference, preserving per-module stripping.
#[no_mangle]
pub extern "C" fn js_process_get_builtin_module_devirt(id: f64) -> f64 {
    crate::object::js_nm_enable_install_all();
    crate::node_submodules::js_node_submod_enable_install_all();
    js_process_get_builtin_module(id)
}

/// process.getBuiltinModule(id) -> module namespace | undefined
#[no_mangle]
pub extern "C" fn js_process_get_builtin_module(id: f64) -> f64 {
    let value = JSValue::from_bits(id.to_bits());
    let mut sso_buf = [0u8; crate::value::SHORT_STRING_MAX_LEN];
    let Some(bytes) = (unsafe { crate::string::js_string_key_bytes(value, &mut sso_buf) }) else {
        let message = format!(
            "The \"id\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(id)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };
    let Ok(specifier) = std::str::from_utf8(bytes) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    // #6651: shared allowlist + routing with createRequire's `require` — one
    // source of truth (`MODULE_BUILTIN_MODULES`), including the `node:` strip
    // and the scheme-only / `_`-internal carve-outs.
    let Some(module_name) = supported_builtin_module_name(specifier) else {
        return f64::from_bits(crate::value::TAG_UNDEFINED);
    };
    crate::process::builtin_module_value(module_name)
}

fn module_bool_value(value: bool) -> f64 {
    f64::from_bits(if value {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

fn module_undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `process.sourceMapsEnabled` getter — returns the current toggle as a
/// NaN-boxed boolean.
#[no_mangle]
pub extern "C" fn js_process_source_maps_enabled() -> f64 {
    let on = SOURCE_MAPS_ENABLED.load(Ordering::Relaxed);
    f64::from_bits(if on {
        crate::value::TAG_TRUE
    } else {
        crate::value::TAG_FALSE
    })
}

/// `process.setSourceMapsEnabled(enabled)` — validates that `enabled` is a
/// boolean (else `TypeError [ERR_INVALID_ARG_TYPE]`), stores it, and returns
/// `undefined`. Receives the full NaN-boxed value so missing/null/numeric/
/// string/object arguments are rejected exactly as Node does.
#[no_mangle]
pub extern "C" fn js_process_set_source_maps_enabled(value: f64) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_bool() {
        let message = format!(
            "The \"enabled\" argument must be of type boolean. Received {}",
            crate::fs::validate::describe_received(value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }
    SOURCE_MAPS_ENABLED.store(jv.as_bool(), Ordering::Relaxed);
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

/// `module.getSourceMapsSupport()` mirrors Node's state object. Perry does not
/// consume source maps during AOT execution, but the helper state is observable
/// through `node:module` and shares the enabled flag with `process`.
#[no_mangle]
pub extern "C" fn js_module_get_source_maps_support() -> f64 {
    let obj = crate::object::js_object_alloc(0, 3);
    module_set_field(
        obj,
        "enabled",
        module_bool_value(SOURCE_MAPS_ENABLED.load(Ordering::Relaxed)),
    );
    module_set_field(
        obj,
        "nodeModules",
        module_bool_value(SOURCE_MAPS_NODE_MODULES.load(Ordering::Relaxed)),
    );
    module_set_field(
        obj,
        "generatedCode",
        module_bool_value(SOURCE_MAPS_GENERATED_CODE.load(Ordering::Relaxed)),
    );
    module_object_value(obj)
}

#[no_mangle]
pub extern "C" fn js_module_set_source_maps_support(enabled: f64, options: f64) -> f64 {
    let enabled_value = JSValue::from_bits(enabled.to_bits());
    if !enabled_value.is_bool() {
        let message = format!(
            "The \"enabled\" argument must be of type boolean. Received {}",
            crate::fs::validate::describe_received(enabled)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    }

    let mut node_modules = false;
    let mut generated_code = false;
    if enabled_value.as_bool() {
        if let Some(options_obj) = module_required_options_object(options, "options") {
            if let Some(value) = module_validate_bool_property(
                module_get_named_field(options_obj, "nodeModules"),
                "nodeModules",
            ) {
                node_modules = value;
            }
            if let Some(value) = module_validate_bool_property(
                module_get_named_field(options_obj, "generatedCode"),
                "generatedCode",
            ) {
                generated_code = value;
            }
        }
    } else if !JSValue::from_bits(options.to_bits()).is_undefined() {
        module_required_options_object(options, "options");
    }

    SOURCE_MAPS_ENABLED.store(enabled_value.as_bool(), Ordering::Relaxed);
    SOURCE_MAPS_NODE_MODULES.store(node_modules, Ordering::Relaxed);
    SOURCE_MAPS_GENERATED_CODE.store(generated_code, Ordering::Relaxed);
    module_undefined()
}

#[no_mangle]
pub extern "C" fn js_module_get_compile_cache_dir() -> f64 {
    let guard = MODULE_COMPILE_CACHE_DIR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match guard.as_deref() {
        Some(dir) => module_string_value(dir),
        None => module_undefined(),
    }
}

#[no_mangle]
pub extern "C" fn js_module_enable_compile_cache(cache_dir: f64) -> f64 {
    let requested_dir = {
        let value = JSValue::from_bits(cache_dir.to_bits());
        if value.is_undefined() {
            std::env::temp_dir()
                .join("node-compile-cache")
                .to_string_lossy()
                .into_owned()
        } else if let Some(dir) = module_value_to_string(cache_dir) {
            dir
        } else {
            crate::fs::validate::throw_type_error_with_code(
                "cacheDir should be a string",
                "ERR_INVALID_ARG_TYPE",
            );
        }
    };

    let mut guard = MODULE_COMPILE_CACHE_DIR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let status = if guard.is_some() {
        2.0
    } else {
        *guard = Some(requested_dir);
        1.0
    };
    let directory = guard.as_deref().unwrap_or("");

    let obj = crate::object::js_object_alloc(0, 2);
    module_set_field(obj, "status", status);
    module_set_field(obj, "directory", module_string_value(directory));
    module_object_value(obj)
}

#[no_mangle]
pub extern "C" fn js_module_flush_compile_cache() -> f64 {
    module_undefined()
}

fn module_hook_member(value: f64, name: &str) -> f64 {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() || jv.is_null() || is_function_value(value) {
        return value;
    }
    let message = format!(
        "The \"hooks.{}\" property must be of type function. Received {}",
        name,
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
}

extern "C" fn module_hooks_deregister(closure: *const crate::closure::ClosureHeader) -> f64 {
    let id = js_closure_get_capture_f64(closure, 0) as u64;
    // 2026-07-09 GC audit wave 2: deregister used to only flip `active`,
    // leaving the resolve/load closures strongly rooted by
    // `scan_process_module_loader_roots_mut` forever. Every consumer
    // filters on `active`, so removing the entry is observably identical —
    // and actually releases the hook closures.
    MODULE_LOADER_HOOKS.with(|hooks| {
        hooks.borrow_mut().retain(|entry| entry.id != id);
    });
    module_undefined()
}

fn module_hooks_deregister_function(id: u64) -> f64 {
    let func_ptr = module_hooks_deregister as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    crate::closure::js_register_closure_length(func_ptr, 0);
    let closure = crate::closure::js_closure_alloc(func_ptr, 1);
    js_closure_set_capture_f64(closure, 0, id as f64);
    crate::object::set_bound_native_closure_name(closure, "deregister");
    crate::object::set_builtin_closure_length(closure as usize, 0);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn module_hooks_deregister_prototype(id: u64) -> *mut crate::object::ObjectHeader {
    let proto = crate::object::js_object_alloc(0, 1);
    module_set_field(proto, "deregister", module_hooks_deregister_function(id));
    crate::object::set_property_attrs(
        proto as usize,
        "deregister".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
    proto
}

/// `module.registerHooks(options)` — synchronous loader customization entry
/// surface. Perry records Node-compatible hook handles and validation, while
/// dynamic import resolution/loading still follows Perry's compile-time graph.
#[no_mangle]
pub extern "C" fn js_module_register_hooks(hooks: f64) -> f64 {
    let hooks_value = JSValue::from_bits(hooks.to_bits());
    if hooks_value.is_undefined() {
        module_throw_plain_type_error(
            "Cannot destructure property 'resolve' of 'hooks' as it is undefined.",
        );
    }
    if hooks_value.is_null() {
        module_throw_plain_type_error(
            "Cannot destructure property 'resolve' of 'hooks' as it is null.",
        );
    }

    let mut resolve = module_undefined();
    let mut load = module_undefined();
    if let Some(hooks_obj) = module_object_ptr(hooks) {
        resolve = module_hook_member(module_get_named_field(hooks_obj, "resolve"), "resolve");
        load = module_hook_member(module_get_named_field(hooks_obj, "load"), "load");
    }

    let id = MODULE_LOADER_HOOK_NEXT_ID.with(|next| {
        let id = next.get();
        next.set(id.saturating_add(1).max(1));
        id
    });
    MODULE_LOADER_HOOKS.with(|hooks| {
        hooks.borrow_mut().push(ModuleLoaderHookEntry {
            id,
            resolve,
            load,
            active: true,
        });
    });
    crate::gc::runtime_write_barrier_root_nanbox(resolve.to_bits());
    crate::gc::runtime_write_barrier_root_nanbox(load.to_bits());

    let handle = crate::object::js_object_alloc(0, 2);
    module_set_field(handle, "resolve", resolve);
    module_set_field(handle, "load", load);

    let proto = module_hooks_deregister_prototype(id);
    let proto_value = module_object_value(proto);
    crate::object::prototype_chain::object_set_static_prototype(
        handle as usize,
        proto_value.to_bits(),
    );
    module_object_value(handle)
}

extern "C" fn module_loader_next_resolve(
    _closure: *const crate::closure::ClosureHeader,
    specifier: f64,
    _context: f64,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    module_set_field(obj, "url", specifier);
    module_set_field(obj, "format", module_string_value("module-typescript"));
    module_object_value(obj)
}

extern "C" fn module_loader_next_load(
    _closure: *const crate::closure::ClosureHeader,
    _url: f64,
    _context: f64,
) -> f64 {
    let obj = crate::object::js_object_alloc(0, 2);
    module_set_field(obj, "format", module_string_value("module-typescript"));
    module_set_field(obj, "source", module_string_value(""));
    module_object_value(obj)
}

fn module_loader_callback(
    slot: &'static std::thread::LocalKey<Cell<*const crate::closure::ClosureHeader>>,
    name: &str,
    func: extern "C" fn(*const crate::closure::ClosureHeader, f64, f64) -> f64,
) -> f64 {
    let ptr = slot.with(|cell| {
        let existing = cell.get();
        if !existing.is_null() {
            return existing;
        }
        let func_ptr = func as *const u8;
        crate::closure::js_register_closure_arity(func_ptr, 2);
        crate::closure::js_register_closure_length(func_ptr, 2);
        let closure = crate::closure::js_closure_alloc(func_ptr, 0);
        crate::object::set_bound_native_closure_name(closure, name);
        crate::object::set_builtin_closure_length(closure as usize, 2);
        cell.set(closure);
        closure
    });
    crate::value::js_nanbox_pointer(ptr as i64)
}

fn module_loader_resolve_context() -> f64 {
    let obj = crate::object::js_object_alloc(0, 1);
    module_set_field(obj, "parentURL", module_string_value(""));
    module_object_value(obj)
}

fn module_loader_load_context() -> f64 {
    let obj = crate::object::js_object_alloc(0, 1);
    module_set_field(obj, "format", module_string_value("module-typescript"));
    module_object_value(obj)
}

fn module_loader_result_url(result: f64, fallback: f64) -> f64 {
    let Some(obj) = module_object_ptr(result) else {
        return fallback;
    };
    let url = module_get_named_field(obj, "url");
    if module_value_to_string(url).is_some() {
        url
    } else {
        fallback
    }
}

/// Apply active synchronous `module.registerHooks()` callbacks to a dynamic
/// import known to Perry's compile-time graph. This supports observable
/// resolve/load callback participation and deregistration; arbitrary new
/// runtime-loaded modules remain outside Perry's static import model.
#[no_mangle]
pub extern "C" fn js_module_dynamic_import_apply_hooks(specifier: f64) -> f64 {
    let entries = MODULE_LOADER_HOOKS.with(|hooks| {
        hooks
            .borrow()
            .iter()
            .copied()
            .filter(|entry| entry.active)
            .collect::<Vec<_>>()
    });
    if entries.is_empty() {
        return specifier;
    }

    let scope = crate::gc::RuntimeHandleScope::new();
    let mut current = specifier;
    for entry in entries {
        if is_function_value(entry.resolve) {
            let current_handle = scope.root_nanbox_f64(current);
            let callback_handle = scope.root_nanbox_f64(entry.resolve);
            let context_handle = scope.root_nanbox_f64(module_loader_resolve_context());
            let next_handle = scope.root_nanbox_f64(module_loader_callback(
                &MODULE_LOADER_NEXT_RESOLVE,
                "nextResolve",
                module_loader_next_resolve,
            ));
            let args = [
                current_handle.get_nanbox_f64(),
                context_handle.get_nanbox_f64(),
                next_handle.get_nanbox_f64(),
            ];
            let result = unsafe {
                crate::closure::js_native_call_value(
                    callback_handle.get_nanbox_f64(),
                    args.as_ptr(),
                    args.len(),
                )
            };
            let result_handle = scope.root_nanbox_f64(result);
            current = module_loader_result_url(
                result_handle.get_nanbox_f64(),
                current_handle.get_nanbox_f64(),
            );
        }

        if is_function_value(entry.load) {
            let current_handle = scope.root_nanbox_f64(current);
            let callback_handle = scope.root_nanbox_f64(entry.load);
            let context_handle = scope.root_nanbox_f64(module_loader_load_context());
            let next_handle = scope.root_nanbox_f64(module_loader_callback(
                &MODULE_LOADER_NEXT_LOAD,
                "nextLoad",
                module_loader_next_load,
            ));
            let args = [
                current_handle.get_nanbox_f64(),
                context_handle.get_nanbox_f64(),
                next_handle.get_nanbox_f64(),
            ];
            unsafe {
                crate::closure::js_native_call_value(
                    callback_handle.get_nanbox_f64(),
                    args.as_ptr(),
                    args.len(),
                );
            }
        }
    }

    current
}

fn module_register_invalid_specifier(specifier: &str) -> bool {
    if specifier.starts_with("data:")
        || specifier.starts_with("file:")
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
    {
        return false;
    }
    specifier.is_empty()
        || specifier.contains('%')
        || specifier.chars().any(|ch| ch.is_ascii_whitespace())
}

/// `module.register(specifier[, parentURL][, options])`. Perry does not load
/// customization modules into the resolver pipeline yet; this entry point
/// matches Node's observable return value for accepted registrations and
/// deterministic invalid specifier errors.
#[no_mangle]
pub extern "C" fn js_module_register(specifier: f64, _parent_url: f64, _options: f64) -> f64 {
    let Some(specifier_str) = module_value_to_string(specifier) else {
        return module_undefined();
    };
    if module_register_invalid_specifier(&specifier_str) {
        let message = format!(
            "Invalid module \"{}\" is not a valid package name",
            specifier_str
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_MODULE_SPECIFIER");
    }
    module_undefined()
}

fn module_word_at(bytes: &[u8], index: usize, word: &[u8]) -> bool {
    if index + word.len() > bytes.len() || &bytes[index..index + word.len()] != word {
        return false;
    }
    let before = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
    let after = bytes.get(index + word.len()).copied();
    !before.is_some_and(module_is_ident_byte) && !after.is_some_and(module_is_ident_byte)
}

fn module_is_ident_byte(byte: u8) -> bool {
    byte == b'_' || byte == b'$' || byte.is_ascii_alphanumeric()
}

fn module_skip_ws(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

fn module_space_span(bytes: &mut [u8], start: usize, end: usize) {
    for byte in &mut bytes[start..end] {
        if *byte != b'\n' && *byte != b'\r' {
            *byte = b' ';
        }
    }
}

fn module_strip_interfaces(bytes: &mut [u8]) {
    let mut index = 0;
    while index < bytes.len() {
        if !module_word_at(bytes, index, b"interface") {
            index += 1;
            continue;
        }
        let mut cursor = index + "interface".len();
        cursor = module_skip_ws(bytes, cursor);
        while cursor < bytes.len() && module_is_ident_byte(bytes[cursor]) {
            cursor += 1;
        }
        cursor = module_skip_ws(bytes, cursor);
        if cursor >= bytes.len() || bytes[cursor] != b'{' {
            index += 1;
            continue;
        }
        let mut depth = 0usize;
        let mut end = cursor;
        while end < bytes.len() {
            match bytes[end] {
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        end += 1;
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        module_space_span(bytes, index, end.min(bytes.len()));
        index = end;
    }
}

fn module_strip_type_annotations(bytes: &mut [u8]) {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b':' {
            index += 1;
            continue;
        }

        let mut before = index;
        while before > 0 && bytes[before - 1].is_ascii_whitespace() {
            before -= 1;
        }
        if before == 0 || !module_is_ident_byte(bytes[before - 1]) {
            index += 1;
            continue;
        }

        let after = module_skip_ws(bytes, index + 1);
        if after >= bytes.len()
            || matches!(
                bytes[after],
                b'\'' | b'"' | b'`' | b'0'..=b'9' | b'{' | b'[' | b':' | b',' | b')' | b';'
            )
        {
            index += 1;
            continue;
        }

        let mut end = after;
        while end < bytes.len()
            && !matches!(bytes[end], b'=' | b',' | b')' | b';' | b'{' | b'\n' | b'\r')
        {
            end += 1;
        }
        module_space_span(bytes, index, end);
        index = end;
    }
}

fn module_strip_type_syntax(source: &str) -> String {
    let mut bytes = source.as_bytes().to_vec();
    module_strip_interfaces(&mut bytes);
    module_strip_type_annotations(&mut bytes);
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_string())
}

fn module_contains_enum(source: &str) -> bool {
    let bytes = source.as_bytes();
    (0..bytes.len()).any(|index| module_word_at(bytes, index, b"enum"))
}

fn module_invalid_option_received(value: f64) -> String {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return "undefined".to_string();
    }
    if jv.is_null() {
        return "null".to_string();
    }
    if jv.is_bool() {
        return jv.as_bool().to_string();
    }
    if let Some(value) = module_value_to_string(value) {
        return format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"));
    }
    if jv.is_int32() {
        return jv.as_int32().to_string();
    }
    if jv.is_number() {
        let number = jv.as_number();
        if number.fract() == 0.0 {
            return format!("{number:.0}");
        }
        return number.to_string();
    }
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<u8>();
        if !ptr.is_null() && (ptr as usize) >= crate::gc::GC_HEADER_SIZE + 0x1000 {
            let gc_header =
                unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
            return if gc_header.obj_type == crate::gc::GC_TYPE_ARRAY {
                "[]".to_string()
            } else {
                "{}".to_string()
            };
        }
    }
    crate::fs::validate::describe_received(value)
}

#[no_mangle]
pub extern "C" fn js_module_strip_typescript_types(code: f64, options: f64) -> f64 {
    let Some(source) = module_value_to_string(code) else {
        let message = format!(
            "The \"code\" argument must be of type string. Received {}",
            crate::fs::validate::describe_received(code)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };

    if let Some(options_obj) = module_required_options_object(options, "options") {
        let mode_value = module_get_named_field(options_obj, "mode");
        if !JSValue::from_bits(mode_value.to_bits()).is_undefined() {
            let mode_string = module_value_to_string(mode_value);
            if mode_string.as_deref() != Some("strip") {
                let message = format!(
                    "The property 'options.mode' must be one of: 'strip'. Received {}",
                    module_invalid_option_received(mode_value)
                );
                crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
            }
        }

        let source_map_value = module_get_named_field(options_obj, "sourceMap");
        let source_map = JSValue::from_bits(source_map_value.to_bits());
        if !source_map.is_undefined() && !(source_map.is_bool() && !source_map.as_bool()) {
            let message = format!(
                "The property 'options.sourceMap' must be one of: false, undefined. Received {}",
                module_invalid_option_received(source_map_value)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_VALUE");
        }
    }

    if module_contains_enum(&source) {
        module_throw_syntax_error_with_code(
            "TypeScript enum is not supported in strip-only mode",
            "ERR_UNSUPPORTED_TYPESCRIPT_SYNTAX",
        );
    }

    let output = module_strip_type_syntax(&source);
    module_string_value(&output)
}
