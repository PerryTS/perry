//! `node:trace_events` category-control surface.
//!
//! This implements the small public control API (`createTracing`,
//! `getEnabledCategories`, and `Tracing` enable/disable/category state) without
//! attempting to emit Chrome trace events.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicI64, Ordering};

use crate::array::{js_array_get_f64, js_array_length, ArrayHeader};
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{
    js_object_alloc, js_object_create, AccessorDescriptor, ObjectHeader, PropertyAttrs,
};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

use super::{bool_value, boxed_ptr, get_field_value, set_field_value, throw_type_error_no_code};

const TRACE_ID_FIELD: &str = "__perryTraceEventsId";

struct TraceState {
    categories_joined: String,
    active_categories: Vec<String>,
    enabled: bool,
}

thread_local! {
    static TRACE_STATES: RefCell<HashMap<i64, TraceState>> = RefCell::new(HashMap::new());
    static TRACE_ENABLED_COUNTS: RefCell<BTreeMap<String, usize>> = const { RefCell::new(BTreeMap::new()) };
    static TRACE_PROTOTYPE: RefCell<Option<*mut ObjectHeader>> = const { RefCell::new(None) };
    static NEXT_TRACE_ID: RefCell<i64> = const { RefCell::new(1) };
}

static TRACE_EVENTS_ALLOCATED: AtomicI64 = AtomicI64::new(0);

#[inline]
fn undefined() -> f64 {
    f64::from_bits(crate::value::TAG_UNDEFINED)
}

#[inline]
fn string_value(s: &str) -> f64 {
    let ptr = js_string_from_bytes(s.as_ptr(), s.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

#[inline]
fn raw_ptr_from_value(value: f64) -> usize {
    let bits = value.to_bits();
    let jsval = JSValue::from_bits(bits);
    if jsval.is_pointer() || jsval.is_string() || jsval.is_bigint() {
        return (bits & crate::value::POINTER_MASK) as usize;
    }
    if bits != 0 && bits < 0x0001_0000_0000_0000 {
        return bits as usize;
    }
    0
}

#[inline]
unsafe fn gc_type_for_ptr(raw: usize) -> Option<u8> {
    if raw < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let header = (raw as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    let gc_type = (*header).obj_type;
    if gc_type <= crate::gc::GC_TYPE_MAX {
        Some(gc_type)
    } else {
        None
    }
}

fn object_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_OBJECT) {
            return None;
        }
    }
    Some(raw as *mut ObjectHeader)
}

fn array_ptr_from_value(value: f64) -> Option<*const ArrayHeader> {
    let raw = raw_ptr_from_value(value);
    if raw < 0x10000 || crate::buffer::is_registered_buffer(raw) {
        return None;
    }
    unsafe {
        if gc_type_for_ptr(raw) != Some(crate::gc::GC_TYPE_ARRAY) {
            return None;
        }
    }
    Some(raw as *const ArrayHeader)
}

fn string_from_value(value: f64) -> Option<String> {
    let jsval = JSValue::from_bits(value.to_bits());
    if !jsval.is_any_string() {
        return None;
    }
    let ptr = crate::value::js_get_string_pointer_unified(value) as *const StringHeader;
    if ptr.is_null() || (ptr as usize) < 0x10000 {
        return None;
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        Some(String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned())
    }
}

fn next_trace_id() -> i64 {
    NEXT_TRACE_ID.with(|n| {
        let mut n = n.borrow_mut();
        let id = *n;
        *n += 1;
        id
    })
}

fn define_non_enum_data(obj: *mut ObjectHeader, name: &str, value: f64, writable: bool) {
    set_field_value(obj, name, value);
    crate::object::set_property_attrs(
        obj as usize,
        name.to_string(),
        PropertyAttrs::new(writable, false, true),
    );
}

fn define_non_enum_accessor(obj: *mut ObjectHeader, name: &str, getter: f64) {
    set_field_value(obj, name, getter);
    crate::object::set_accessor_descriptor(
        obj as usize,
        name.to_string(),
        AccessorDescriptor {
            get: getter.to_bits(),
            set: 0,
        },
    );
    crate::object::set_property_attrs(
        obj as usize,
        name.to_string(),
        PropertyAttrs::new(true, false, true),
    );
}

fn set_function_name(closure: *mut ClosureHeader, name: &str) {
    crate::closure::closure_set_dynamic_prop(closure as usize, "name", string_value(name));
}

fn function_value(func: *const u8, arity: u32, name: &str) -> f64 {
    let closure = js_closure_alloc(func, 0);
    js_register_closure_arity(func, arity);
    set_function_name(closure, name);
    boxed_ptr(closure)
}

fn throw_invalid_this() -> ! {
    throw_type_error_no_code(b"Method called on incompatible receiver")
}

fn this_trace_id() -> i64 {
    let this_value = crate::object::js_implicit_this_get();
    let Some(obj) = object_ptr_from_value(this_value) else {
        throw_invalid_this();
    };
    let id_value = get_field_value(obj, TRACE_ID_FIELD);
    if id_value.is_finite() && id_value > 0.0 {
        id_value as i64
    } else {
        throw_invalid_this();
    }
}

fn trace_state_value<T>(id: i64, f: impl FnOnce(&TraceState) -> T) -> T {
    TRACE_STATES.with(|states| {
        let states = states.borrow();
        let Some(state) = states.get(&id) else {
            throw_invalid_this();
        };
        f(state)
    })
}

fn adjust_enabled_counts(categories: &[String], enable: bool) {
    TRACE_ENABLED_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        for category in categories {
            if category.is_empty() {
                continue;
            }
            if enable {
                *counts.entry(category.clone()).or_insert(0) += 1;
            } else if let Some(count) = counts.get_mut(category) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(category);
                }
            }
        }
    });
}

fn set_trace_enabled(id: i64, enabled: bool) {
    let categories = TRACE_STATES.with(|states| {
        let mut states = states.borrow_mut();
        let Some(state) = states.get_mut(&id) else {
            throw_invalid_this();
        };
        if state.enabled == enabled {
            return None;
        }
        state.enabled = enabled;
        Some(state.active_categories.clone())
    });
    if let Some(categories) = categories {
        adjust_enabled_counts(&categories, enabled);
    }
}

extern "C" fn trace_tracing_constructor(_closure: *const ClosureHeader) -> f64 {
    throw_type_error_no_code(b"Tracing is not a constructor")
}

extern "C" fn trace_tracing_enable(_closure: *const ClosureHeader) -> f64 {
    set_trace_enabled(this_trace_id(), true);
    undefined()
}

extern "C" fn trace_tracing_disable(_closure: *const ClosureHeader) -> f64 {
    set_trace_enabled(this_trace_id(), false);
    undefined()
}

extern "C" fn trace_categories_getter(_closure: *const ClosureHeader) -> f64 {
    let id = this_trace_id();
    trace_state_value(id, |state| string_value(&state.categories_joined))
}

extern "C" fn trace_enabled_getter(_closure: *const ClosureHeader) -> f64 {
    let id = this_trace_id();
    trace_state_value(id, |state| bool_value(state.enabled))
}

fn ensure_trace_prototype() -> *mut ObjectHeader {
    if let Some(proto) = TRACE_PROTOTYPE.with(|slot| *slot.borrow()) {
        return proto;
    }

    let proto = js_object_alloc(0, 5);
    let ctor = function_value(trace_tracing_constructor as *const u8, 0, "Tracing");
    let enable = function_value(trace_tracing_enable as *const u8, 0, "enable");
    let disable = function_value(trace_tracing_disable as *const u8, 0, "disable");
    let categories = function_value(trace_categories_getter as *const u8, 0, "get categories");
    let enabled = function_value(trace_enabled_getter as *const u8, 0, "get enabled");

    define_non_enum_data(proto, "constructor", ctor, true);
    define_non_enum_data(proto, "enable", enable, true);
    define_non_enum_data(proto, "disable", disable, true);
    define_non_enum_accessor(proto, "categories", categories);
    define_non_enum_accessor(proto, "enabled", enabled);

    let ctor_ptr = crate::value::js_nanbox_get_pointer(ctor) as usize;
    crate::closure::closure_set_dynamic_prop(ctor_ptr, "prototype", boxed_ptr(proto));

    TRACE_PROTOTYPE.with(|slot| {
        *slot.borrow_mut() = Some(proto);
    });
    TRACE_EVENTS_ALLOCATED.store(1, Ordering::Release);
    proto
}

fn trace_constructor_value() -> f64 {
    let proto = ensure_trace_prototype();
    get_field_value(proto, "constructor")
}

fn validate_options(options: f64) -> *mut ObjectHeader {
    if let Some(obj) = object_ptr_from_value(options) {
        return obj;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(options)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn validate_categories(options_obj: *mut ObjectHeader) -> Vec<String> {
    let categories_value = get_field_value(options_obj, "categories");
    let Some(categories) = array_ptr_from_value(categories_value) else {
        let message = format!(
            "The \"options.categories\" property must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(categories_value)
        );
        crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
    };

    let len = js_array_length(categories);
    if len == 0 {
        crate::fs::validate::throw_type_error_with_code(
            "At least one category is required",
            "ERR_TRACE_EVENTS_CATEGORY_REQUIRED",
        );
    }

    let mut result = Vec::with_capacity(len as usize);
    for idx in 0..len {
        let value = js_array_get_f64(categories, idx);
        let Some(category) = string_from_value(value) else {
            let message = format!(
                "The \"options.categories[{}]\" property must be of type string. Received {}",
                idx,
                crate::fs::validate::describe_received(value)
            );
            crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE");
        };
        result.push(category);
    }
    result
}

pub(crate) extern "C" fn thunk_trace_events_createTracing(
    _closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    let options_obj = validate_options(options);
    let categories = validate_categories(options_obj);
    let categories_joined = categories.join(",");
    let mut unique = BTreeSet::new();
    for category in &categories {
        unique.insert(category.clone());
    }
    let active_categories: Vec<String> = unique.into_iter().collect();

    let id = next_trace_id();
    TRACE_STATES.with(|states| {
        states.borrow_mut().insert(
            id,
            TraceState {
                categories_joined,
                active_categories,
                enabled: false,
            },
        );
    });

    let proto_value = boxed_ptr(ensure_trace_prototype());
    let obj_value = js_object_create(proto_value);
    let obj = crate::value::js_nanbox_get_pointer(obj_value) as *mut ObjectHeader;
    define_non_enum_data(obj, TRACE_ID_FIELD, id as f64, false);
    define_non_enum_data(obj, "constructor", trace_constructor_value(), true);
    TRACE_EVENTS_ALLOCATED.store(1, Ordering::Release);
    obj_value
}

pub(crate) extern "C" fn thunk_trace_events_getEnabledCategories(
    _closure: *const ClosureHeader,
    _arg: f64,
) -> f64 {
    TRACE_ENABLED_COUNTS.with(|counts| {
        let counts = counts.borrow();
        if counts.is_empty() {
            return undefined();
        }
        let joined = counts.keys().cloned().collect::<Vec<_>>().join(",");
        if joined.is_empty() {
            undefined()
        } else {
            string_value(&joined)
        }
    })
}

pub(crate) fn scan_trace_events_roots_mut(visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    if TRACE_EVENTS_ALLOCATED.load(Ordering::Acquire) == 0 {
        return;
    }
    TRACE_PROTOTYPE.with(|slot| {
        if let Some(proto) = slot.borrow_mut().as_mut() {
            visitor.visit_raw_mut_ptr_slot(proto);
        }
    });
}
