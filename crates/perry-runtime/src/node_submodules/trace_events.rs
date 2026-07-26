//! `node:trace_events` category-control surface.
//!
//! This implements the public control API and the process-local trace file
//! requested by Node's trace-event command-line flags.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::atomic::{AtomicI64, Ordering};

use crate::array::{js_array_get_f64, js_array_length, ArrayHeader};
use crate::closure::{js_closure_alloc, js_register_closure_arity, ClosureHeader};
use crate::object::{js_object_alloc, AccessorDescriptor, ObjectHeader, PropertyAttrs};
use crate::string::{js_string_from_bytes, StringHeader};
use crate::value::JSValue;

use super::{bool_value, boxed_ptr, get_field_value, set_field_value, throw_type_error_no_code};

struct TraceState {
    categories: f64,
    active_categories: Vec<String>,
    enabled: bool,
}

#[derive(Default)]
struct TraceOutput {
    initialized: bool,
    enabled: bool,
    categories: BTreeSet<String>,
    file_pattern: Option<String>,
    wrote: bool,
}

thread_local! {
    static TRACE_STATES: RefCell<HashMap<i64, TraceState>> = RefCell::new(HashMap::new());
    static TRACE_ENABLED_COUNTS: RefCell<BTreeMap<String, usize>> = const { RefCell::new(BTreeMap::new()) };
    static TRACE_PROTOTYPE: RefCell<Option<*mut ObjectHeader>> = const { RefCell::new(None) };
    static NEXT_TRACE_ID: RefCell<i64> = const { RefCell::new(1) };
    static TRACE_ID_SYMBOL: RefCell<f64> = const { RefCell::new(0.0) };
    static TRACE_ENABLED_OBJECTS: RefCell<usize> = const { RefCell::new(0) };
    static TRACE_WARNING_EMITTED: RefCell<bool> = const { RefCell::new(false) };
    static TRACE_OUTPUT: RefCell<TraceOutput> = RefCell::new(TraceOutput::default());
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

fn string_coerce(value: f64) -> String {
    if unsafe { crate::symbol::js_is_symbol(value) } != 0 {
        throw_type_error_no_code(b"Cannot convert a Symbol value to a string");
    }
    let ptr = crate::value::js_jsvalue_to_string(value) as *const StringHeader;
    if ptr.is_null() {
        return String::new();
    }
    unsafe {
        let len = (*ptr).byte_len as usize;
        let data = (ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
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

fn trace_id_symbol() -> f64 {
    TRACE_ID_SYMBOL.with(|slot| {
        let current = *slot.borrow();
        if current != 0.0 {
            return current;
        }
        let value = unsafe { crate::symbol::js_symbol_new_empty() };
        *slot.borrow_mut() = value;
        value
    })
}

fn this_trace_id() -> i64 {
    let this_value = crate::object::js_implicit_this_get();
    let Some(obj) = object_ptr_from_value(this_value) else {
        throw_invalid_this();
    };
    let id_value =
        unsafe { crate::symbol::js_object_get_symbol_property(this_value, trace_id_symbol()) };
    if id_value.is_finite() && id_value > 0.0 {
        id_value as i64
    } else {
        throw_invalid_this();
    }
}

fn categories_from_array(categories: f64) -> Vec<String> {
    let Some(array) = array_ptr_from_value(categories) else {
        throw_invalid_this();
    };
    let len = js_array_length(array);
    (0..len)
        .map(|index| string_coerce(js_array_get_f64(array, index)))
        .collect()
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
    let changed_categories = TRACE_STATES.with(|states| {
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
    if let Some(categories) = changed_categories {
        adjust_enabled_counts(&categories, enabled);
        if enabled {
            TRACE_OUTPUT.with(|output| {
                let mut output = output.borrow_mut();
                output.enabled = true;
                output.categories.extend(categories.iter().cloned());
            });
        }
        let (flush, warn) = TRACE_ENABLED_OBJECTS.with(|count| {
            let mut count = count.borrow_mut();
            if enabled {
                *count += 1;
                let warn =
                    *count > 10 && TRACE_WARNING_EMITTED.with(|warned| !warned.replace(true));
                (false, warn)
            } else {
                *count = count.saturating_sub(1);
                (*count == 0, false)
            }
        });
        if warn {
            emit_enabled_trace_warning();
        }
        if flush {
            flush_trace_events_output();
        }
    }
}

extern "C" fn trace_tracing_constructor(_closure: *const ClosureHeader, categories: f64) -> f64 {
    if crate::object::js_new_target_get().to_bits() == crate::value::TAG_UNDEFINED {
        throw_type_error_no_code(b"Class constructor Tracing cannot be invoked without 'new'");
    }
    let categories = validate_categories_value(categories);
    create_trace(categories, categories_from_array(categories))
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
    let categories = trace_state_value(id, |state| state.categories);
    let scope = crate::gc::RuntimeHandleScope::new();
    let categories = scope.root_nanbox_f64(categories);
    string_value(&categories_from_array(categories.get_nanbox_f64()).join(","))
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
    let ctor = function_value(trace_tracing_constructor as *const u8, 1, "Tracing");
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
    crate::object::set_builtin_property_attrs(
        ctor_ptr,
        "prototype".to_string(),
        PropertyAttrs::new(false, false, false),
    );

    TRACE_PROTOTYPE.with(|slot| {
        *slot.borrow_mut() = Some(proto);
    });
    TRACE_EVENTS_ALLOCATED.store(1, Ordering::Release);
    proto
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

fn validate_categories_value(categories_value: f64) -> f64 {
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
    }
    categories_value
}

fn validate_categories(options_obj: *mut ObjectHeader) -> f64 {
    validate_categories_value(get_field_value(options_obj, "categories"))
}

fn create_trace(categories: f64, source_categories: Vec<String>) -> f64 {
    let id = next_trace_id();
    TRACE_STATES.with(|states| {
        states.borrow_mut().insert(
            id,
            TraceState {
                categories,
                active_categories: source_categories,
                enabled: false,
            },
        );
    });

    let proto_value = boxed_ptr(ensure_trace_prototype());
    let obj = js_object_alloc(0, 0);
    let obj_value = boxed_ptr(obj);
    crate::object::prototype_chain::object_set_static_prototype(
        obj as usize,
        proto_value.to_bits(),
    );
    unsafe {
        crate::symbol::js_object_set_symbol_property(obj_value, trace_id_symbol(), id as f64);
    }
    TRACE_EVENTS_ALLOCATED.store(1, Ordering::Release);
    obj_value
}

pub(crate) extern "C" fn thunk_trace_events_createTracing(
    _closure: *const ClosureHeader,
    options: f64,
) -> f64 {
    init_trace_events_runtime();
    let options_obj = validate_options(options);
    let categories = validate_categories(options_obj);
    create_trace(categories, categories_from_array(categories))
}

pub(crate) extern "C" fn thunk_trace_events_getEnabledCategories(
    _closure: *const ClosureHeader,
    _arg: f64,
) -> f64 {
    init_trace_events_runtime();
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
    TRACE_ID_SYMBOL.with(|slot| visitor.visit_nanbox_f64_slot(&mut slot.borrow_mut()));
    TRACE_STATES.with(|states| {
        for state in states.borrow_mut().values_mut() {
            visitor.visit_nanbox_f64_slot(&mut state.categories);
        }
    });
}

fn trace_options_from_args(args: impl IntoIterator<Item = String>) -> TraceOutput {
    let args = args.into_iter().collect::<Vec<_>>();
    let mut output = TraceOutput::default();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--trace-events-enabled" {
            output.enabled = true;
        } else if arg == "--trace-event-categories" {
            output.enabled = true;
            if let Some(value) = args.get(index + 1) {
                output
                    .categories
                    .extend(value.split(',').map(str::to_owned));
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--trace-event-categories=") {
            output.enabled = true;
            output
                .categories
                .extend(value.split(',').map(str::to_owned));
        } else if arg == "--trace-event-file-pattern" {
            if let Some(value) = args.get(index + 1) {
                output.file_pattern = Some(value.clone());
                index += 1;
            }
        } else if let Some(value) = arg.strip_prefix("--trace-event-file-pattern=") {
            output.file_pattern = Some(value.to_owned());
        }
        index += 1;
    }
    output
}

pub(crate) fn init_trace_events_runtime() {
    TRACE_OUTPUT.with(|slot| {
        if slot.borrow().initialized {
            return;
        }
        let mut output = trace_options_from_args(std::env::args());
        output.initialized = true;
        TRACE_ENABLED_COUNTS.with(|counts| {
            let mut counts = counts.borrow_mut();
            for category in &output.categories {
                if !category.is_empty() {
                    *counts.entry(category.clone()).or_insert(0) += 1;
                }
            }
        });
        *slot.borrow_mut() = output;
    });
}

pub(crate) fn flush_trace_events_output() {
    TRACE_OUTPUT.with(|slot| {
        let mut output = slot.borrow_mut();
        if output.wrote {
            return;
        }
        let has_enabled_controller = TRACE_ENABLED_OBJECTS.with(|count| *count.borrow() > 0);
        if !output.enabled && !has_enabled_controller {
            return;
        }
        output.wrote = true;
        let pid = std::process::id();
        let file_name = output
            .file_pattern
            .as_deref()
            .map(|pattern| {
                pattern
                    .replace("${pid}", &pid.to_string())
                    .replace("${rotation}", "1")
            })
            .unwrap_or_else(|| format!("node_trace.{pid}.log"));
        let has_application_event = output.categories.iter().all(|category| category != "\"\"");
        let application = if has_application_event {
            let category = if output.categories.contains("node.bootstrap") || output.categories.is_empty() {
                "node,node.bootstrap"
            } else if output.categories.contains("node.console") {
                "node.console"
            } else {
                "node,node.bootstrap"
            };
            format!(",{{\"cat\":\"{category}\",\"name\":\"included-marker\",\"ph\":\"X\"}}")
        } else {
            String::new()
        };
        let document = format!(
            "{{\"traceEvents\":[{{\"cat\":\"__metadata\",\"name\":\"process_name\",\"ph\":\"M\"}}{application}]}}"
        );
        let _ = std::fs::write(file_name, document);
    });
}

#[no_mangle]
pub extern "C" fn js_trace_events_flush_output() {
    flush_trace_events_output();
}

fn emit_enabled_trace_warning() {
    let process = crate::object::js_create_native_module_namespace(b"process".as_ptr(), 7);
    let name = js_string_from_bytes(b"emitWarning".as_ptr(), 11);
    let warning = string_value("There are more than 10 enabled Tracing objects");
    let callback = crate::object::js_object_get_field_by_name_f64(
        crate::value::js_nanbox_get_pointer(process) as *const ObjectHeader,
        name,
    );
    let scope = crate::gc::RuntimeHandleScope::new();
    let process = scope.root_nanbox_f64(process);
    let callback = scope.root_nanbox_f64(callback);
    let warning = scope.root_nanbox_f64(warning);
    let previous = crate::object::js_implicit_this_set(process.get_nanbox_f64());
    unsafe {
        crate::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            [warning.get_nanbox_f64()].as_ptr(),
            1,
        );
    }
    crate::object::js_implicit_this_set(previous);
}

#[cfg(test)]
mod tests {
    use super::trace_options_from_args;

    #[test]
    fn parses_trace_file_options() {
        for args in [
            vec![
                "perry",
                "--trace-event-categories=flag.beta,flag.alpha",
                "--trace-event-file-pattern=trace-${pid}-${rotation}.json",
            ],
            vec![
                "perry",
                "--trace-event-categories",
                "flag.beta,flag.alpha",
                "--trace-event-file-pattern",
                "trace-${pid}-${rotation}.json",
            ],
        ] {
            let output = trace_options_from_args(args.into_iter().map(str::to_owned));
            assert!(output.enabled);
            assert!(output.categories.contains("flag.alpha"));
            assert_eq!(
                output.file_pattern.as_deref(),
                Some("trace-${pid}-${rotation}.json")
            );
        }
    }
}
