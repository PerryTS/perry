//! Minimal `node:trace_events` category-control surface.
//!
//! This implements the observable controller state exposed by
//! `createTracing()`, `Tracing#enable()`, `Tracing#disable()`, and
//! `getEnabledCategories()` without attempting to emit or serialize trace
//! events.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Once;

use crate::array::{js_array_get_f64, js_array_length};
use crate::closure::ClosureHeader;
use crate::object::{js_object_alloc, js_object_get_field, js_object_set_field, ObjectHeader};
use crate::string::js_string_from_bytes;
use crate::value::JSValue;

use super::diagnostics::{bool_value, boxed_ptr, decode_string_value};
use super::TAG_UNDEFINED;

const TRACING_CLASS_ID: u32 = 0xFFFF_00C1;

struct TraceControllerState {
    category_list: String,
    count_categories: Vec<String>,
    enabled: bool,
}

thread_local! {
    static TRACE_CONTROLLERS: RefCell<BTreeMap<i64, TraceControllerState>> =
        const { RefCell::new(BTreeMap::new()) };
    static TRACE_ENABLED_COUNTS: RefCell<BTreeMap<String, usize>> =
        const { RefCell::new(BTreeMap::new()) };
    static NEXT_TRACE_ID: RefCell<i64> = const { RefCell::new(1) };
}

fn next_trace_id() -> i64 {
    NEXT_TRACE_ID.with(|next| {
        let mut next = next.borrow_mut();
        let id = *next;
        *next += 1;
        id
    })
}

fn undefined() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn string_value(value: &str) -> f64 {
    let ptr = js_string_from_bytes(value.as_ptr(), value.len() as u32);
    f64::from_bits(JSValue::string_ptr(ptr).bits())
}

fn throw_invalid_arg_type(message: String) -> ! {
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn throw_category_required() -> ! {
    crate::fs::validate::throw_type_error_with_code(
        "At least one category is required",
        "ERR_TRACE_EVENTS_CATEGORY_REQUIRED",
    )
}

fn is_plain_options_object(value: f64) -> bool {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() || jv.is_any_string() || jv.is_bigint() {
        return false;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return false;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    !matches!(
        gc_header.obj_type,
        crate::gc::GC_TYPE_ARRAY
            | crate::gc::GC_TYPE_LAZY_ARRAY
            | crate::gc::GC_TYPE_STRING
            | crate::gc::GC_TYPE_CLOSURE
            | crate::gc::GC_TYPE_BIGINT
    )
}

fn array_ptr_from_value(value: f64) -> Option<*mut crate::array::ArrayHeader> {
    let jv = JSValue::from_bits(value.to_bits());
    if !jv.is_pointer() {
        return None;
    }
    let ptr = jv.as_pointer::<u8>();
    if ptr.is_null() || (ptr as usize) < crate::gc::GC_HEADER_SIZE + 0x1000 {
        return None;
    }
    let gc_header = unsafe { &*(ptr.sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader) };
    if gc_header.obj_type == crate::gc::GC_TYPE_ARRAY {
        Some(ptr as *mut crate::array::ArrayHeader)
    } else {
        None
    }
}

fn validate_options(options: f64) {
    if is_plain_options_object(options) {
        return;
    }
    let message = format!(
        "The \"options\" argument must be of type object. Received {}",
        crate::fs::validate::describe_received(options)
    );
    throw_invalid_arg_type(message);
}

fn trace_categories(options: f64) -> (Vec<String>, String) {
    let categories_value = super::stream_promises::get_object_property(options, b"categories")
        .unwrap_or_else(undefined);
    let Some(categories_array) = array_ptr_from_value(categories_value) else {
        let message = format!(
            "The \"options.categories\" property must be an instance of Array. Received {}",
            crate::fs::validate::describe_received(categories_value)
        );
        throw_invalid_arg_type(message);
    };
    let len = js_array_length(categories_array);
    if len == 0 {
        throw_category_required();
    }

    let mut categories = Vec::with_capacity(len as usize);
    for i in 0..len {
        let value = js_array_get_f64(categories_array, i);
        let Some(category) = decode_string_value(value) else {
            let message = format!(
                "The \"options.categories[{}]\" property must be of type string. Received {}",
                i,
                crate::fs::validate::describe_received(value)
            );
            throw_invalid_arg_type(message);
        };
        categories.push(category);
    }

    let category_list = categories.join(",");
    (categories, category_list)
}

fn unique_categories(categories: &[String]) -> Vec<String> {
    categories
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn tracing_id(this_value: f64) -> Option<i64> {
    let value = JSValue::from_bits(this_value.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let obj = value.as_pointer::<ObjectHeader>();
    if obj.is_null() || (obj as usize) < 0x10000 {
        return None;
    }
    unsafe {
        if (*obj).class_id != TRACING_CLASS_ID {
            return None;
        }
    }
    let id = js_object_get_field(obj, 0);
    if id.is_int32() {
        Some(id.as_int32() as i64)
    } else {
        None
    }
}

extern "C" fn trace_categories_getter(this_value: f64) -> f64 {
    let Some(id) = tracing_id(this_value) else {
        return undefined();
    };
    TRACE_CONTROLLERS.with(|controllers| {
        controllers
            .borrow()
            .get(&id)
            .map(|state| string_value(&state.category_list))
            .unwrap_or_else(undefined)
    })
}

extern "C" fn trace_enabled_getter(this_value: f64) -> f64 {
    let Some(id) = tracing_id(this_value) else {
        return bool_value(false);
    };
    TRACE_CONTROLLERS.with(|controllers| {
        controllers
            .borrow()
            .get(&id)
            .map(|state| bool_value(state.enabled))
            .unwrap_or_else(|| bool_value(false))
    })
}

extern "C" fn trace_enable(this_value: f64) -> f64 {
    let Some(id) = tracing_id(this_value) else {
        return undefined();
    };
    TRACE_CONTROLLERS.with(|controllers| {
        let mut controllers = controllers.borrow_mut();
        let Some(state) = controllers.get_mut(&id) else {
            return;
        };
        if state.enabled {
            return;
        }
        state.enabled = true;
        TRACE_ENABLED_COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            for category in &state.count_categories {
                *counts.entry(category.clone()).or_insert(0) += 1;
            }
        });
    });
    undefined()
}

extern "C" fn trace_disable(this_value: f64) -> f64 {
    let Some(id) = tracing_id(this_value) else {
        return undefined();
    };
    TRACE_CONTROLLERS.with(|controllers| {
        let mut controllers = controllers.borrow_mut();
        let Some(state) = controllers.get_mut(&id) else {
            return;
        };
        if !state.enabled {
            return;
        }
        state.enabled = false;
        TRACE_ENABLED_COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            for category in &state.count_categories {
                if let Some(count) = counts.get_mut(category) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        counts.remove(category);
                    }
                }
            }
        });
    });
    undefined()
}

fn ensure_tracing_class_registered() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| unsafe {
        crate::object::js_register_class_id(TRACING_CLASS_ID);
        crate::object::js_register_class_name(
            TRACING_CLASS_ID,
            b"Tracing".as_ptr(),
            b"Tracing".len() as u32,
        );
        for (name, func) in [
            ("enable", trace_enable as *const u8),
            ("disable", trace_disable as *const u8),
        ] {
            crate::object::js_register_class_method(
                TRACING_CLASS_ID as i64,
                name.as_ptr(),
                name.len() as i64,
                func as i64,
                0,
            );
        }
        for (name, getter) in [
            ("categories", trace_categories_getter as *const u8),
            ("enabled", trace_enabled_getter as *const u8),
        ] {
            crate::object::js_register_class_getter(
                TRACING_CLASS_ID as i64,
                name.as_ptr(),
                name.len() as i64,
                getter as i64,
            );
        }
    });
}

pub(crate) extern "C" fn trace_events_create_tracing(
    _closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    ensure_tracing_class_registered();
    validate_options(options);
    let (categories, category_list) = trace_categories(options);
    let count_categories = unique_categories(&categories);
    let id = next_trace_id();
    let obj = js_object_alloc(TRACING_CLASS_ID, 1);
    js_object_set_field(obj, 0, JSValue::int32(id as i32));

    TRACE_CONTROLLERS.with(|controllers| {
        controllers.borrow_mut().insert(
            id,
            TraceControllerState {
                category_list,
                count_categories,
                enabled: false,
            },
        );
    });
    super::ANY_SINGLETON_ALLOCATED.store(1, std::sync::atomic::Ordering::Release);
    boxed_ptr(obj)
}

pub(crate) extern "C" fn trace_events_get_enabled_categories(
    _closure: *const ClosureHeader,
) -> f64 {
    TRACE_ENABLED_COUNTS.with(|counts| {
        let counts = counts.borrow();
        if counts.is_empty() {
            return undefined();
        }
        let joined = counts.keys().cloned().collect::<Vec<_>>().join(",");
        string_value(&joined)
    })
}

pub(crate) fn scan_trace_events_roots_mut(_visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
    // Controller state stores only Rust-owned strings and counters. The
    // Tracing objects themselves are ordinary caller-owned JS objects.
}
