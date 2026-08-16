use super::*;
use crate::closure::{js_closure_alloc, ClosureHeader};
use base64::Engine as _;
use std::cell::{Cell, RefCell};

const SOURCE_MAP_CLASS_ID: u32 = 0xFFFF_04D0;

crate::perry_thread_local! {
    static SOURCE_MAP_PROTOTYPE: Cell<u64> = const { Cell::new(0) };
    static SOURCE_MAP_CACHE: RefCell<std::collections::HashMap<String, u64>> =
        RefCell::new(std::collections::HashMap::new());
}

pub(super) fn scan_roots(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    SOURCE_MAP_PROTOTYPE.with(|cell| {
        let mut value = f64::from_bits(cell.get());
        if value.to_bits() != 0 {
            visitor.visit_nanbox_f64_slot(&mut value);
            cell.set(value.to_bits());
        }
    });
    SOURCE_MAP_CACHE.with(|cache| {
        for bits in cache.borrow_mut().values_mut() {
            let mut value = f64::from_bits(*bits);
            visitor.visit_nanbox_f64_slot(&mut value);
            *bits = value.to_bits();
        }
    });
}

/// Constructor for `new module.SourceMap(payload[, options])`.
#[no_mangle]
pub extern "C" fn js_module_source_map_new(payload: f64, options: f64) -> f64 {
    if module_object_ptr(payload).is_none() {
        crate::fs::validate::throw_type_error_with_code(
            "The \"payload\" argument must be of type object",
            "ERR_INVALID_ARG_TYPE",
        );
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    let options = scope.root_nanbox_f64(options);
    let _ = source_map_prototype();
    let cloned_payload = crate::builtins::js_structured_clone(payload.get_nanbox_f64());
    let line_lengths = module_object_ptr(options.get_nanbox_f64())
        .map(|obj| module_get_named_field(obj, "lineLengths"))
        .unwrap_or_else(module_undefined);
    let cloned_payload = scope.root_nanbox_f64(cloned_payload);
    let line_lengths = scope.root_nanbox_f64(line_lengths);
    let keys = b"_payload\0_lineLengths\0";
    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc_with_shape(
        SOURCE_MAP_CLASS_ID,
        2,
        keys.as_ptr(),
        keys.len() as u32,
    ));
    // The first `js_object_alloc_with_shape` argument identifies the cached
    // shape; it does not initialize ObjectHeader::class_id.
    obj.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| unsafe {
        (*obj).class_id = SOURCE_MAP_CLASS_ID;
        // GC_STORE_AUDIT(INIT): the fresh object is still rooted and unpublished.
        (*obj).keys_array = std::ptr::null_mut();
    });
    obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| {
        crate::object::js_object_set_field(
            o,
            0,
            JSValue::from_bits(cloned_payload.get_nanbox_f64().to_bits()),
        );
    });
    obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| {
        crate::object::js_object_set_field(
            o,
            1,
            JSValue::from_bits(line_lengths.get_nanbox_f64().to_bits()),
        );
    });
    // `proto` may have moved while cloning/allocating above; reload the rooted
    // singleton before recording the instance chain.
    let proto = source_map_prototype();
    obj.with_mut_ptr(|o: *mut crate::object::ObjectHeader| {
        crate::object::prototype_chain::object_set_static_prototype(o as usize, proto.to_bits());
    });
    obj.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| module_object_value(obj))
}

type SourceMapThunk = extern "C" fn(*const ClosureHeader, f64) -> f64;

fn source_map_method(name: &str, thunk: SourceMapThunk) -> f64 {
    let func_ptr = thunk as *const u8;
    let closure = js_closure_alloc(func_ptr, 0);
    crate::closure::js_register_closure_rest(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, name);
    crate::object::set_builtin_closure_length(closure as usize, 2);
    crate::object::set_builtin_closure_non_constructable(closure as usize);
    crate::value::js_nanbox_pointer(closure as i64)
}

extern "C" fn source_map_payload_getter(_closure: *const ClosureHeader) -> f64 {
    let obj = source_map_receiver();
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(f64::from_bits(
        crate::object::js_object_get_field(obj, 0).bits(),
    ));
    crate::builtins::js_structured_clone(payload.get_nanbox_f64())
}

extern "C" fn source_map_line_lengths_getter(_closure: *const ClosureHeader) -> f64 {
    let obj = source_map_receiver();
    let scope = crate::gc::RuntimeHandleScope::new();
    let value = scope.root_nanbox_f64(f64::from_bits(
        crate::object::js_object_get_field(obj, 1).bits(),
    ));
    let jv = JSValue::from_bits(value.get_nanbox_f64().to_bits());
    if !jv.is_pointer() {
        return module_undefined();
    }
    let ptr = jv.as_pointer::<u8>();
    if !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return module_undefined();
    }
    let gc = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc.obj_type != crate::gc::GC_TYPE_ARRAY {
        return module_undefined();
    }
    let cloned = crate::array::js_array_clone(ptr as *const crate::array::ArrayHeader);
    f64::from_bits(JSValue::array_ptr(cloned).bits())
}

fn source_map_getter(name: &str, thunk: extern "C" fn(*const ClosureHeader) -> f64) -> f64 {
    let func_ptr = thunk as *const u8;
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let closure = js_closure_alloc(func_ptr, 0);
    crate::object::set_bound_native_closure_name(closure, &format!("get {name}"));
    crate::object::set_builtin_closure_length(closure as usize, 0);
    crate::object::set_builtin_closure_non_constructable(closure as usize);
    crate::value::js_nanbox_pointer(closure as i64)
}

fn source_map_receiver() -> *const crate::object::ObjectHeader {
    let receiver = crate::object::js_implicit_this_get();
    let Some(obj) = module_object_ptr(receiver) else {
        module_throw_plain_type_error("Receiver must be an instance of SourceMap");
    };
    if unsafe { (*obj).class_id } != SOURCE_MAP_CLASS_ID {
        module_throw_plain_type_error("Receiver must be an instance of SourceMap");
    }
    obj
}

fn source_map_prototype() -> f64 {
    SOURCE_MAP_PROTOTYPE.with(|slot| {
        if slot.get() != 0 {
            return f64::from_bits(slot.get());
        }
        // Pre-shape the object so all properties are reflected even when their
        // value is `undefined` (the accessor slots). Store the singleton root
        // immediately: allocating closures below can run a moving GC.
        let keys = b"constructor\0findEntry\0findOrigin\0lineLengths\0payload\0";
        let proto =
            crate::object::js_object_alloc_with_shape(0, 5, keys.as_ptr(), keys.len() as u32);
        let value = module_object_value(proto);
        slot.set(value.to_bits());
        crate::object::js_object_set_field(
            proto,
            0,
            JSValue::from_bits(module_undefined().to_bits()),
        );
        for (index, name, thunk) in [
            (
                1,
                "findEntry",
                source_map_find_entry_thunk as SourceMapThunk,
            ),
            (
                2,
                "findOrigin",
                source_map_find_origin_thunk as SourceMapThunk,
            ),
        ] {
            let value = source_map_method(name, thunk);
            let proto = module_object_ptr(f64::from_bits(slot.get())).expect("SourceMap prototype");
            crate::object::js_object_set_field(
                proto as *mut _,
                index,
                JSValue::from_bits(value.to_bits()),
            );
            crate::object::set_builtin_property_attrs(
                proto as usize,
                name.to_string(),
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
        for (index, name, thunk) in [
            (
                4,
                "payload",
                source_map_payload_getter as extern "C" fn(*const ClosureHeader) -> f64,
            ),
            (
                3,
                "lineLengths",
                source_map_line_lengths_getter as extern "C" fn(*const ClosureHeader) -> f64,
            ),
        ] {
            let getter = source_map_getter(name, thunk);
            let proto = module_object_ptr(f64::from_bits(slot.get())).expect("SourceMap prototype");
            crate::object::js_object_set_field(
                proto as *mut _,
                index,
                JSValue::from_bits(module_undefined().to_bits()),
            );
            crate::object::set_builtin_accessor_descriptor(
                proto as usize,
                name.to_string(),
                crate::object::AccessorDescriptor {
                    get: getter.to_bits(),
                    set: 0,
                },
                crate::object::PropertyAttrs::new(true, false, true),
            );
        }
        f64::from_bits(slot.get())
    })
}

pub fn module_source_map_attach_constructor(closure_addr: usize) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let constructor = scope.root_raw_mut_ptr(closure_addr as *mut ClosureHeader);
    let proto_value = scope.root_nanbox_f64(source_map_prototype());
    let proto = module_object_ptr(proto_value.get_nanbox_f64()).expect("SourceMap prototype");
    let constructor_value = constructor.with_mut_ptr(|constructor: *mut ClosureHeader| {
        crate::value::js_nanbox_pointer(constructor as i64)
    });
    crate::object::js_object_set_field(
        proto as *mut crate::object::ObjectHeader,
        0,
        JSValue::from_bits(constructor_value.to_bits()),
    );
    crate::object::set_builtin_property_attrs(
        proto as usize,
        "constructor".to_string(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
    constructor.with_mut_ptr(|constructor: *mut ClosureHeader| {
        crate::closure::closure_set_dynamic_prop(
            constructor as usize,
            "prototype",
            proto_value.get_nanbox_f64(),
        );
    });
    constructor.with_mut_ptr(|constructor: *mut ClosureHeader| {
        crate::object::set_builtin_property_attrs(
            constructor as usize,
            "prototype".to_string(),
            crate::object::PropertyAttrs::new(false, false, false),
        );
    });
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
        let Some(part) = (digit & 0x1f).checked_shl(shift) else {
            return Vec::new();
        };
        let Some(next) = value.checked_add(part) else {
            return Vec::new();
        };
        if next > u32::MAX as i64 {
            return Vec::new();
        }
        value = next;
        if cont {
            let Some(next_shift) = shift.checked_add(5) else {
                return Vec::new();
            };
            if next_shift >= 35 {
                return Vec::new();
            }
            shift = next_shift;
        } else {
            let negative = (value & 1) != 0;
            let decoded = value >> 1;
            out.push(if negative { -decoded } else { decoded });
            value = 0;
            shift = 0;
        }
    }
    if out.is_empty() && !seg.is_empty() {
        vec![0, 0, 0, 0]
    } else if shift == 0 {
        out
    } else {
        Vec::new()
    }
}

#[derive(Clone)]
struct SourceMapEntry {
    generated_line: i64,
    generated_column: i64,
    section_path: Vec<u32>,
    // `None` for genCol-only (1-field) segments that mark an unmapped position.
    original: Option<(i64, i64, i64, Option<i64>)>, // (source_index, line, column, name_index)
}

/// Decode the full `mappings` string into ordered entries with cumulative
/// source/line/column/name indices per Node's SourceMap behavior.
fn source_map_decode(mappings: &str) -> Vec<SourceMapEntry> {
    let mut entries = Vec::new();
    let (mut src_idx, mut src_line, mut src_col, mut name_idx) = (0i64, 0i64, 0i64, 0i64);
    let mut has_name = false;
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
                    has_name = true;
                    Some(name_idx)
                } else if has_name {
                    // The name index is a running state just like source and
                    // original coordinates. Node carries the most recently
                    // decoded name onto later mapped segments, including a
                    // segment on a subsequent generated line.
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
                section_path: Vec::new(),
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    let key = js_string_from_bytes(field.as_ptr(), field.len() as u32);
    let obj = crate::value::js_nanbox_get_pointer(payload.get_nanbox_f64())
        as *const crate::object::ObjectHeader;
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let arr_value = scope.root_nanbox_f64(arr_value);
    let av = JSValue::from_bits(arr_value.get_nanbox_f64().to_bits());
    if !av.is_pointer() {
        return undefined_value();
    }
    let ptr = crate::value::js_nanbox_get_pointer(arr_value.get_nanbox_f64()) as *const u8;
    if !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return undefined_value();
    }
    let gc = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc.obj_type != crate::gc::GC_TYPE_ARRAY {
        return undefined_value();
    }
    let arr = ptr as *const crate::array::ArrayHeader;
    let len = crate::array::js_array_length(arr);
    if index >= i64::from(len) {
        return undefined_value();
    }
    crate::array::js_array_get_f64(arr, index as u32)
}

fn source_map_arg(rest: f64, index: u32) -> f64 {
    let rv = JSValue::from_bits(rest.to_bits());
    if !rv.is_pointer() {
        return module_undefined();
    }
    let arr = crate::value::js_nanbox_get_pointer(rest) as *const crate::array::ArrayHeader;
    if !crate::value::addr_class::is_plausible_heap_addr(arr as usize) {
        return module_undefined();
    }
    let len = crate::array::js_array_length(arr);
    if index >= len {
        module_undefined()
    } else {
        crate::array::js_array_get_f64(arr, index)
    }
}

/// Coerce call argument `idx` to a finite number, if it is one.
fn source_map_arg_number(value: f64) -> Option<f64> {
    let number = JSValue::from_bits(value.to_bits()).to_number();
    number.is_finite().then_some(number)
}

fn source_map_arg_i64(value: f64) -> i64 {
    source_map_arg_number(value).map(|n| n as i64).unwrap_or(0)
}

/// Decode the payload's `mappings` and return the greatest entry whose
/// generated position is `<=` (line, column). Entries are emitted in
/// non-decreasing order, so the last non-exceeding one wins.
fn source_map_lookup(payload: f64, line: i64, col: i64) -> Option<SourceMapEntry> {
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    if let Some(sections) = source_map_array_value(payload.get_nanbox_f64(), "sections") {
        let sections = scope.root_nanbox_f64(sections);
        let sections_ptr = || {
            crate::value::js_nanbox_get_pointer(sections.get_nanbox_f64())
                as *const crate::array::ArrayHeader
        };
        let len = crate::array::js_array_length(sections_ptr());
        let mut best = None;
        for index in 0..len {
            let section =
                scope.root_nanbox_f64(crate::array::js_array_get_f64(sections_ptr(), index));
            let offset =
                scope.root_nanbox_f64(source_map_field(section.get_nanbox_f64(), "offset"));
            if module_object_ptr(offset.get_nanbox_f64()).is_none() {
                continue;
            }
            let offset_line =
                JSValue::from_bits(source_map_field(offset.get_nanbox_f64(), "line").to_bits())
                    .to_number() as i64;
            let offset_col =
                JSValue::from_bits(source_map_field(offset.get_nanbox_f64(), "column").to_bits())
                    .to_number() as i64;
            if (offset_line, offset_col) > (line, col) {
                break;
            }
            let nested = scope.root_nanbox_f64(source_map_field(section.get_nanbox_f64(), "map"));
            let local_line = line - offset_line;
            let local_col = if local_line == 0 {
                col - offset_col
            } else {
                col
            };
            if let Some(mut entry) =
                source_map_lookup(nested.get_nanbox_f64(), local_line, local_col)
            {
                entry.section_path.insert(0, index);
                entry.generated_line += offset_line;
                if entry.generated_line == offset_line {
                    entry.generated_column += offset_col;
                }
                best = Some(entry);
            }
        }
        return best;
    }
    let payload = payload.get_nanbox_f64();
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

fn source_map_array_value(payload: f64, field: &str) -> Option<f64> {
    let value = source_map_field(payload, field);
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if !crate::value::addr_class::is_plausible_heap_addr(ptr as usize) {
        return None;
    }
    let gc = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    (gc.obj_type == crate::gc::GC_TYPE_ARRAY).then_some(value)
}

fn source_map_entry_payload(payload: f64, section_path: &[u32]) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let current = scope.root_nanbox_f64(payload);
    for &index in section_path {
        let Some(sections) = source_map_array_value(current.get_nanbox_f64(), "sections") else {
            return module_undefined();
        };
        let sections = scope.root_nanbox_f64(sections);
        let sections_ptr = crate::value::js_nanbox_get_pointer(sections.get_nanbox_f64())
            as *const crate::array::ArrayHeader;
        if index >= crate::array::js_array_length(sections_ptr) {
            return module_undefined();
        }
        let section = scope.root_nanbox_f64(crate::array::js_array_get_f64(sections_ptr, index));
        current.set_nanbox_f64(source_map_field(section.get_nanbox_f64(), "map"));
    }
    current.get_nanbox_f64()
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
    let Some(entry) = entry else {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    };
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    let source_payload = scope.root_nanbox_f64(source_map_entry_payload(
        payload.get_nanbox_f64(),
        &entry.section_path,
    ));
    let obj = crate::object::js_object_alloc(0, 4);
    let obj = scope.root_raw_mut_ptr(obj);
    if let SourceMapEntry {
        original: Some((source_index, _, _, name_index)),
        ..
    } = entry
    {
        if let Some(name_index) = name_index {
            let name =
                source_map_array_element(source_payload.get_nanbox_f64(), "names", name_index);
            if JSValue::from_bits(name.to_bits()).is_string() {
                module_set_field_rooted(&obj, "name", name);
            }
        }
        module_set_field_rooted(
            &obj,
            "fileName",
            source_map_array_element(source_payload.get_nanbox_f64(), "sources", source_index),
        );
    }
    let null = f64::from_bits(crate::value::TAG_NULL);
    module_set_field_rooted(&obj, "lineNumber", line.map_or(null, |n| n));
    module_set_field_rooted(&obj, "columnNumber", col.map_or(null, |n| n));
    obj.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| module_object_value(obj))
}

/// `SourceMap#findEntry(lineNumber, columnNumber)` — return the greatest
/// decoded entry whose generated position is `<=` the query, shaped like
/// Node's `{ generatedLine, generatedColumn, originalSource, originalLine,
/// originalColumn, name? }`. Returns `{}` when no entry precedes the query.
extern "C" fn source_map_find_entry_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let _ = closure;
    let receiver = source_map_receiver();
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(f64::from_bits(
        crate::object::js_object_get_field(receiver, 0).bits(),
    ));
    let rest = scope.root_nanbox_f64(rest);
    let line = scope.root_nanbox_f64(source_map_arg(rest.get_nanbox_f64(), 0));
    let column = scope.root_nanbox_f64(source_map_arg(rest.get_nanbox_f64(), 1));
    let query_line = source_map_arg_i64(line.get_nanbox_f64());
    let query_col = source_map_arg_i64(column.get_nanbox_f64());

    let Some(entry) = source_map_lookup(payload.get_nanbox_f64(), query_line, query_col) else {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    };

    let obj = scope.root_raw_mut_ptr(crate::object::js_object_alloc(0, 6));
    module_set_field_rooted(&obj, "generatedLine", entry.generated_line as f64);
    module_set_field_rooted(&obj, "generatedColumn", entry.generated_column as f64);
    if let Some((source_index, original_line, original_column, name_index)) = entry.original {
        let source_payload = scope.root_nanbox_f64(source_map_entry_payload(
            payload.get_nanbox_f64(),
            &entry.section_path,
        ));
        module_set_field_rooted(
            &obj,
            "originalSource",
            source_map_array_element(source_payload.get_nanbox_f64(), "sources", source_index),
        );
        module_set_field_rooted(&obj, "originalLine", original_line as f64);
        module_set_field_rooted(&obj, "originalColumn", original_column as f64);
        if let Some(name_index) = name_index {
            let name =
                source_map_array_element(source_payload.get_nanbox_f64(), "names", name_index);
            if JSValue::from_bits(name.to_bits()).is_string() {
                module_set_field_rooted(&obj, "name", name);
            }
        }
    }
    obj.with_mut_ptr(|obj: *mut crate::object::ObjectHeader| module_object_value(obj))
}

/// `SourceMap#findOrigin(lineNumber, columnNumber)`. Node echoes the queried
/// coordinates (as `lineNumber`/`columnNumber`, or `null` when an argument is
/// not a finite number) and tags on the `name`/`fileName` of the entry at that
/// generated position. The lone special case is a numeric `(0, 0)` query, for
/// which Node returns an empty object.
extern "C" fn source_map_find_origin_thunk(closure: *const ClosureHeader, rest: f64) -> f64 {
    let _ = closure;
    let receiver = source_map_receiver();
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(f64::from_bits(
        crate::object::js_object_get_field(receiver, 0).bits(),
    ));
    let rest = scope.root_nanbox_f64(rest);
    let line_arg = scope.root_nanbox_f64(source_map_arg(rest.get_nanbox_f64(), 0));
    let col_arg = scope.root_nanbox_f64(source_map_arg(rest.get_nanbox_f64(), 1));
    let line = source_map_arg_number(line_arg.get_nanbox_f64());
    let col = source_map_arg_number(col_arg.get_nanbox_f64());

    if line == Some(0.0) && col == Some(0.0) {
        return module_object_value(crate::object::js_object_alloc(0, 0));
    }

    // `findOrigin` consumes 1-based generated coordinates, unlike findEntry's
    // 0-based coordinates. Node's native search falls through to the last
    // mapping for a non-numeric line receiver; preserve that observable quirk.
    let entry = if let Some(line) = line {
        source_map_lookup(
            payload.get_nanbox_f64(),
            (line as i64).saturating_sub(1),
            col.map(|n| (n as i64).saturating_sub(1))
                .unwrap_or(i64::MAX),
        )
    } else {
        source_map_lookup(payload.get_nanbox_f64(), i64::MAX, i64::MAX)
    };
    source_map_origin_object(payload.get_nanbox_f64(), entry, line, col)
}

fn source_map_normalize_inline_sources(payload: f64, generated_file: &std::path::Path) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    let Some(sources) = source_map_array_value(payload.get_nanbox_f64(), "sources") else {
        return;
    };
    let sources = scope.root_nanbox_f64(sources);
    let source_root =
        source_map_field_string(payload.get_nanbox_f64(), "sourceRoot").unwrap_or_default();
    let base = generated_file
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));
    let sources_ptr = || {
        crate::value::js_nanbox_get_pointer(sources.get_nanbox_f64())
            as *const crate::array::ArrayHeader
    };
    let len = crate::array::js_array_length(sources_ptr());
    for index in 0..len {
        let raw = crate::array::js_array_get_f64(sources_ptr(), index);
        let Some(source) = module_value_to_string(raw) else {
            continue;
        };
        if source.starts_with("file:") || source.contains("://") {
            continue;
        }
        let path = base.join(&source_root).join(source);
        // The source file need not exist, so canonicalize can legitimately
        // fail. Components still removes lexical `.` segments in that case.
        let normalized: std::path::PathBuf = path.components().collect();
        let path = std::fs::canonicalize(&path).unwrap_or(normalized);
        let url = crate::url::node_compat::path_to_file_url_string(
            &path.to_string_lossy(),
            cfg!(windows),
        );
        let url = module_string_value(&url);
        crate::array::js_array_set_f64(sources_ptr() as *mut _, index, url);
    }
}

/// `module.findSourceMap(filename)` — lazily materialize inline source maps.
/// Perry's AOT loader already resolves the generated file; parsing the inline
/// payload on first lookup avoids a second loader-side registry.
#[no_mangle]
pub extern "C" fn js_module_find_source_map(filename: f64) -> f64 {
    if !SOURCE_MAPS_ENABLED.load(Ordering::Relaxed) {
        return module_undefined();
    }
    let Some(filename) = module_value_to_string(filename) else {
        return module_undefined();
    };
    let filename = std::fs::canonicalize(&filename)
        .unwrap_or_else(|_| std::path::PathBuf::from(&filename))
        .to_string_lossy()
        .into_owned();
    if let Some(bits) = SOURCE_MAP_CACHE.with(|cache| cache.borrow().get(&filename).copied()) {
        return f64::from_bits(bits);
    }
    let Ok(source) = std::fs::read_to_string(&filename) else {
        return module_undefined();
    };
    let prefix = "sourceMappingURL=data:application/json";
    let Some(marker) = source.rfind(prefix) else {
        return module_undefined();
    };
    let suffix = source[marker + prefix.len()..].lines().next().unwrap_or("");
    let Some(encoded) = suffix
        .strip_prefix(";base64,")
        .or_else(|| suffix.strip_prefix(";charset=utf-8;base64,"))
    else {
        return module_undefined();
    };
    let encoded = encoded
        .trim_start()
        .split(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')))
        .next()
        .unwrap_or("");
    let engine = base64::engine::general_purpose::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::general_purpose::GeneralPurposeConfig::new()
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
    );
    let Ok(decoded) = engine.decode(encoded) else {
        return module_undefined();
    };
    let text = js_string_from_bytes(decoded.as_ptr(), decoded.len() as u32);
    let payload = unsafe { crate::json::js_json_parse(text) };
    let payload = f64::from_bits(payload.bits());
    if module_object_ptr(payload).is_none() {
        return module_undefined();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let payload = scope.root_nanbox_f64(payload);
    source_map_normalize_inline_sources(payload.get_nanbox_f64(), std::path::Path::new(&filename));
    let map = js_module_source_map_new(payload.get_nanbox_f64(), module_undefined());
    SOURCE_MAP_CACHE.with(|cache| {
        cache.borrow_mut().insert(filename, map.to_bits());
    });
    crate::gc::runtime_write_barrier_root_nanbox(map.to_bits());
    map
}
