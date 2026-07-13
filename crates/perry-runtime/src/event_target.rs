//! Minimal WHATWG `EventTarget` storage used by Node's `events` helpers.
//!
//! Perry models `EventTarget` as a regular runtime object with hidden fields:
//! a marker, a listener-bag object keyed by event type, and a max-listener
//! number. Keeping the listener arrays in object fields lets the normal GC
//! trace callbacks without a separate native handle registry.

use crate::{
    js_array_alloc, js_array_get, js_array_length, js_array_push_f64, js_nanbox_pointer,
    js_object_alloc, js_object_get_field_by_name, js_object_get_field_by_name_f64,
    js_object_set_field_by_name, js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader,
    StringHeader,
};
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

pub const CLASS_ID_EVENT: u32 = 0xFFFF_2403;
pub const CLASS_ID_CUSTOM_EVENT: u32 = 0xFFFF_2404;
pub const CLASS_ID_DOM_EXCEPTION: u32 = 0xFFFF_2405;
/// `EventTarget` base class. Stamped on `new EventTarget()` instances and used
/// as the PARENT class id of a user `class X extends EventTarget` (wired by
/// `js_register_class_parent_dynamic` via `global_builtin_constructor_class_id`).
/// Walking to it through the class chain is what lets a subclass instance be
/// recognized as an event target (#6301). Keep in sync with the reserved id in
/// perry-codegen/src/expr/instance_misc1.rs.
pub const CLASS_ID_EVENT_TARGET: u32 = 0xFFFF_2406;

const TAG_UNDEFINED: u64 = 0x7FFC_0000_0000_0001;
const TAG_NULL: u64 = 0x7FFC_0000_0000_0002;

fn key(bytes: &[u8]) -> *mut StringHeader {
    js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32)
}

fn boxed_ptr<T>(ptr: *mut T) -> f64 {
    js_nanbox_pointer(ptr as i64)
}

fn value_as_ptr<T>(value: f64) -> Option<*mut T> {
    let value = JSValue::from_bits(value.to_bits());
    if value.is_pointer() {
        Some(value.as_pointer::<T>() as *mut T)
    } else {
        None
    }
}

fn bool_value(value: bool) -> f64 {
    f64::from_bits(JSValue::bool(value).bits())
}

fn number_value(value: f64) -> f64 {
    f64::from_bits(JSValue::number(value).bits())
}

fn undefined_value() -> f64 {
    f64::from_bits(TAG_UNDEFINED)
}

fn null_value() -> f64 {
    f64::from_bits(TAG_NULL)
}

fn string_value(bytes: &[u8]) -> f64 {
    let s = js_string_from_bytes(bytes.as_ptr(), bytes.len() as u32);
    crate::value::js_nanbox_string(s as i64)
}

fn is_undefined(value: f64) -> bool {
    JSValue::from_bits(value.to_bits()).is_undefined()
}

fn throw_missing_arg(name: &str) -> ! {
    let message = format!("The \"{name}\" argument must be specified");
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_MISSING_ARGS")
}

fn throw_invalid_event(value: f64) -> ! {
    let message = format!(
        "The \"event\" argument must be an instance of Event. Received {}",
        crate::fs::validate::describe_received(value)
    );
    crate::fs::validate::throw_type_error_with_code(&message, "ERR_INVALID_ARG_TYPE")
}

fn string_from_value(value: f64) -> *mut StringHeader {
    crate::builtins::js_string_coerce(value)
}

fn optional_string_from_value(value: f64, default: &[u8]) -> *mut StringHeader {
    let jv = JSValue::from_bits(value.to_bits());
    if jv.is_undefined() {
        return js_string_from_bytes(default.as_ptr(), default.len() as u32);
    }
    string_from_value(value)
}

unsafe fn own_option_value(options: f64, name: &[u8]) -> f64 {
    let Some(opts) = value_as_ptr::<ObjectHeader>(options) else {
        return undefined_value();
    };
    if opts.is_null() {
        return undefined_value();
    }
    js_object_get_field_by_name_f64(opts, key(name))
}

unsafe fn option_bool(options: f64, name: &[u8]) -> bool {
    let value = own_option_value(options, name);
    !JSValue::from_bits(value.to_bits()).is_undefined() && crate::value::js_is_truthy(value) != 0
}

unsafe fn option_detail(options: f64) -> f64 {
    let value = own_option_value(options, b"detail");
    if JSValue::from_bits(value.to_bits()).is_undefined() {
        null_value()
    } else {
        value
    }
}

unsafe fn listener_capture(options: f64) -> bool {
    let js_value = JSValue::from_bits(options.to_bits());
    if js_value.is_undefined() || js_value.is_null() {
        return false;
    }
    if js_value.is_pointer() {
        return crate::value::js_is_truthy(own_option_value(options, b"capture")) != 0;
    }
    crate::value::js_is_truthy(options) != 0
}

unsafe fn listener_option_bool(options: f64, name: &[u8]) -> bool {
    let js_value = JSValue::from_bits(options.to_bits());
    if js_value.is_undefined() || js_value.is_null() || !js_value.is_pointer() {
        return false;
    }
    crate::value::js_is_truthy(own_option_value(options, name)) != 0
}

unsafe fn listener_signal(options: f64) -> Option<*mut ObjectHeader> {
    let js_value = JSValue::from_bits(options.to_bits());
    if js_value.is_undefined() || js_value.is_null() || !js_value.is_pointer() {
        return None;
    }
    crate::url::abort::abort_signal_ptr_from_value(own_option_value(options, b"signal"))
}

fn set_event_field(event: *mut ObjectHeader, name: &[u8], value: f64) {
    js_object_set_field_by_name(event, key(name), value);
    crate::object::set_builtin_property_attrs(
        event as usize,
        String::from_utf8_lossy(name).into_owned(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
}

fn event_bool_field(event: *mut ObjectHeader, name: &[u8]) -> bool {
    if event.is_null() {
        return false;
    }
    let value = js_object_get_field_by_name_f64(event, key(name));
    crate::value::js_is_truthy(value) != 0
}

extern "C" fn event_prevent_default_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this_value = crate::object::js_implicit_this_get();
    let Some(event) = value_as_ptr::<ObjectHeader>(this_value) else {
        return undefined_value();
    };
    if event_bool_field(event, b"cancelable") {
        set_event_field(event, b"defaultPrevented", bool_value(true));
    }
    undefined_value()
}

extern "C" fn event_stop_propagation_thunk(_closure: *const crate::closure::ClosureHeader) -> f64 {
    let this_value = crate::object::js_implicit_this_get();
    if let Some(event) = value_as_ptr::<ObjectHeader>(this_value) {
        set_event_field(event, b"_stopped", bool_value(true));
    }
    undefined_value()
}

extern "C" fn event_stop_immediate_propagation_thunk(
    _closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let this_value = crate::object::js_implicit_this_get();
    if let Some(event) = value_as_ptr::<ObjectHeader>(this_value) {
        set_event_field(event, b"_stopped", bool_value(true));
        set_event_field(event, b"_immediateStopped", bool_value(true));
    }
    undefined_value()
}

fn install_event_method(
    event: *mut ObjectHeader,
    name: &str,
    func: extern "C" fn(*const crate::closure::ClosureHeader) -> f64,
) {
    let func_ptr = func as *const u8;
    let closure = crate::closure::js_closure_alloc(func_ptr, 0);
    if closure.is_null() {
        return;
    }
    crate::closure::js_register_closure_arity(func_ptr, 0);
    let name_ptr = js_string_from_bytes(name.as_ptr(), name.len() as u32);
    crate::closure::closure_set_dynamic_prop(
        closure as usize,
        "name",
        crate::value::js_nanbox_string(name_ptr as i64),
    );
    set_event_field(event, name.as_bytes(), boxed_ptr(closure));
}

fn construct_event(
    type_value: f64,
    options: f64,
    class_id: u32,
    constructor_name: &[u8],
    detail: Option<f64>,
) -> *mut ObjectHeader {
    let event = js_object_alloc(class_id, 0);
    if event.is_null() {
        return std::ptr::null_mut();
    }
    init_event_fields(event, type_value, options, constructor_name, detail);
    event
}

/// Shared Event field/method initialization, applied either to a freshly
/// allocated Event (`construct_event`) or to an existing subclass instance
/// (`js_event_subclass_init` — `super(type, options)` from
/// `class X extends Event`).
fn init_event_fields(
    event: *mut ObjectHeader,
    type_value: f64,
    options: f64,
    constructor_name: &[u8],
    detail: Option<f64>,
) {
    let type_ptr = string_from_value(type_value);
    set_event_field(
        event,
        b"type",
        crate::value::js_nanbox_string(type_ptr as i64),
    );
    unsafe {
        set_event_field(
            event,
            b"bubbles",
            bool_value(option_bool(options, b"bubbles")),
        );
        set_event_field(
            event,
            b"cancelable",
            bool_value(option_bool(options, b"cancelable")),
        );
        set_event_field(
            event,
            b"composed",
            bool_value(option_bool(options, b"composed")),
        );
    }
    set_event_field(event, b"defaultPrevented", bool_value(false));
    set_event_field(event, b"target", null_value());
    set_event_field(event, b"currentTarget", null_value());
    set_event_field(event, b"eventPhase", number_value(0.0));
    set_event_field(event, b"isTrusted", bool_value(false));
    set_event_field(event, b"timeStamp", number_value(0.0));
    set_event_field(event, b"_stopped", bool_value(false));
    set_event_field(event, b"_immediateStopped", bool_value(false));
    if let Some(detail) = detail {
        set_event_field(event, b"detail", detail);
    }
    let ctor = crate::object::js_get_global_this_builtin_value(
        constructor_name.as_ptr(),
        constructor_name.len(),
    );
    set_event_field(event, b"constructor", ctor);
    install_event_method(event, "preventDefault", event_prevent_default_thunk);
    install_event_method(event, "stopPropagation", event_stop_propagation_thunk);
    install_event_method(
        event,
        "stopImmediatePropagation",
        event_stop_immediate_propagation_thunk,
    );
}

/// `super(type, options)` from a user `class X extends Event` /
/// `extends CustomEvent`: initialize the standard Event fields and methods
/// onto the EXISTING subclass instance (`this`) instead of allocating a new
/// Event. The subclass's own class id stays on the header — the
/// `Subclass → Event` registry edge registered at class-definition time
/// keeps `instanceof Event` and dispatch acceptance working.
#[no_mangle]
pub extern "C" fn js_event_subclass_init(
    this_value: f64,
    type_value: f64,
    options: f64,
    argc: u32,
    is_custom: u32,
) -> f64 {
    let Some(event) = value_as_ptr::<ObjectHeader>(this_value) else {
        return undefined_value();
    };
    if argc == 0 {
        throw_missing_arg("type");
    }
    // `class X extends CustomEvent` must initialize as a CustomEvent: the
    // `constructor` field resolves to the CustomEvent global and `detail` is
    // read off the options bag (mirroring the direct `new CustomEvent(...)`
    // path). Plain `extends Event` keeps `b"Event"` and no `detail`.
    if is_custom != 0 {
        let detail = unsafe { option_detail(options) };
        init_event_fields(event, type_value, options, b"CustomEvent", Some(detail));
    } else {
        init_event_fields(event, type_value, options, b"Event", None);
    }
    undefined_value()
}

/// Keepalive anchor for the auto-optimize whole-program build —
/// `js_event_subclass_init` is a generated-code-only callee.
#[used]
static KEEP_JS_EVENT_SUBCLASS_INIT: extern "C" fn(f64, f64, f64, u32, u32) -> f64 =
    js_event_subclass_init;

fn is_event_instance(event: *const ObjectHeader) -> bool {
    if event.is_null() {
        return false;
    }
    let class_id = unsafe { (*event).class_id };
    if class_id == CLASS_ID_EVENT || class_id == CLASS_ID_CUSTOM_EVENT {
        return true;
    }
    // A user subclass (`class CloseEvent extends Event`, e.g. the `ws`
    // package's WebSocket events) carries its own class id; walk the
    // registered parent chain looking for the Event base.
    let mut cur = class_id;
    for _ in 0..64 {
        match crate::object::get_parent_class_id(cur) {
            Some(parent) if parent != 0 && parent != cur => {
                if parent == CLASS_ID_EVENT || parent == CLASS_ID_CUSTOM_EVENT {
                    return true;
                }
                cur = parent;
            }
            _ => return false,
        }
    }
    false
}

/// `new Event(type, options?)`.
#[no_mangle]
pub extern "C" fn js_event_new(type_value: f64, options: f64, argc: u32) -> *mut ObjectHeader {
    if argc == 0 {
        throw_missing_arg("type");
    }
    construct_event(type_value, options, CLASS_ID_EVENT, b"Event", None)
}

/// `new CustomEvent(type, options?)`.
#[no_mangle]
pub extern "C" fn js_custom_event_new(
    type_value: f64,
    options: f64,
    argc: u32,
) -> *mut ObjectHeader {
    if argc == 0 {
        throw_missing_arg("type");
    }
    let detail = unsafe { option_detail(options) };
    construct_event(
        type_value,
        options,
        CLASS_ID_CUSTOM_EVENT,
        b"CustomEvent",
        Some(detail),
    )
}

fn dom_exception_errors() -> &'static Mutex<HashSet<usize>> {
    static DOM_EXCEPTION_ERRORS: OnceLock<Mutex<HashSet<usize>>> = OnceLock::new();
    DOM_EXCEPTION_ERRORS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Latched true by the first `DOMException` construction, so the per-dead-
/// error GC cleanup below skips the mutex entirely in the (overwhelmingly
/// common) processes that never create one.
static DOM_EXCEPTIONS_CREATED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Death cleanup (2026-07-09 GC audit wave 2): `DOM_EXCEPTION_ERRORS` is a
/// raw-address HashSet with zero removals, so it grew per DOMException and
/// misidentified recycled addresses as DOMExceptions. Errors already run the
/// `ErrorSideTables` finalize hook on death — this is called from
/// `error_side_tables_clear_dead` for every dead error.
pub(crate) fn dom_exception_error_clear_dead(err_addr: usize) {
    if !DOM_EXCEPTIONS_CREATED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    if let Ok(mut set) = dom_exception_errors().lock() {
        set.remove(&err_addr);
    }
}

#[cfg(test)]
pub(crate) fn test_seed_dom_exception_error(err_addr: usize) {
    DOM_EXCEPTIONS_CREATED.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut set) = dom_exception_errors().lock() {
        set.insert(err_addr);
    }
}

#[cfg(test)]
pub(crate) fn test_dom_exception_error_registered(err_addr: usize) -> bool {
    dom_exception_errors()
        .lock()
        .is_ok_and(|set| set.contains(&err_addr))
}

fn dom_exception_code(name: &str) -> f64 {
    let code = match name {
        "IndexSizeError" => 1,
        "DOMStringSizeError" => 2,
        "HierarchyRequestError" => 3,
        "WrongDocumentError" => 4,
        "InvalidCharacterError" => 5,
        "NoDataAllowedError" => 6,
        "NoModificationAllowedError" => 7,
        "NotFoundError" => 8,
        "NotSupportedError" => 9,
        "InUseAttributeError" => 10,
        "InvalidStateError" => 11,
        "SyntaxError" => 12,
        "InvalidModificationError" => 13,
        "NamespaceError" => 14,
        "InvalidAccessError" => 15,
        "ValidationError" => 16,
        "TypeMismatchError" => 17,
        "SecurityError" => 18,
        "NetworkError" => 19,
        "AbortError" => 20,
        "URLMismatchError" => 21,
        "QuotaExceededError" => 22,
        "TimeoutError" => 23,
        "InvalidNodeTypeError" => 24,
        "DataCloneError" => 25,
        _ => 0,
    };
    number_value(code as f64)
}

/// `new DOMException(message?, name?)`.
#[no_mangle]
pub extern "C" fn js_dom_exception_new(message: f64, name: f64) -> *mut crate::error::ErrorHeader {
    let message_ptr = optional_string_from_value(message, b"");
    let name_ptr = optional_string_from_value(name, b"Error");
    let name_string = unsafe {
        let len = (*name_ptr).byte_len as usize;
        let data = (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>());
        String::from_utf8_lossy(std::slice::from_raw_parts(data, len)).into_owned()
    };
    let err =
        crate::error::js_error_new_with_name_message_bytes(name_string.as_bytes(), message_ptr);
    if !err.is_null() {
        DOM_EXCEPTIONS_CREATED.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Ok(mut set) = dom_exception_errors().lock() {
            set.insert(err as usize);
        }
        let ctor = crate::object::js_get_global_this_builtin_value(b"DOMException".as_ptr(), 12);
        crate::node_submodules::set_error_user_prop(err as usize, "constructor", ctor);
        crate::node_submodules::set_error_user_prop(
            err as usize,
            "code",
            dom_exception_code(&name_string),
        );
    }
    err
}

/// Runtime predicate for `value instanceof DOMException`.
pub(crate) fn is_dom_exception_error(err: *const crate::error::ErrorHeader) -> bool {
    if err.is_null() {
        return false;
    }
    dom_exception_errors()
        .lock()
        .is_ok_and(|set| set.contains(&(err as usize)))
}

pub(crate) fn abort_dom_exception_value() -> f64 {
    let message = string_value(b"This operation was aborted");
    let name = string_value(b"AbortError");
    let err = js_dom_exception_new(message, name);
    crate::value::js_nanbox_pointer(err as i64)
}

/// True for `EventTarget`'s own reserved class id and for any user class whose
/// registered parent chain reaches it — `class Bus extends EventTarget {}` and
/// `class B extends A extends EventTarget` alike. The edge is wired at class-
/// definition time by `js_register_class_parent_dynamic`, which resolves the
/// `EventTarget` global through `global_builtin_constructor_class_id` (#6301).
/// Mirrors `is_event_instance`'s walk for the `Event` base.
pub(crate) fn class_chain_is_event_target(class_id: u32) -> bool {
    if class_id == 0 {
        return false;
    }
    if class_id == CLASS_ID_EVENT_TARGET {
        return true;
    }
    let mut cur = class_id;
    for _ in 0..64 {
        match crate::object::get_parent_class_id(cur) {
            Some(parent) if parent != 0 && parent != cur => {
                if parent == CLASS_ID_EVENT_TARGET {
                    return true;
                }
                cur = parent;
            }
            _ => return false,
        }
    }
    false
}

pub(crate) unsafe fn is_event_target(target: *const ObjectHeader) -> bool {
    if target.is_null() {
        return false;
    }
    // Handle-based receivers (EventEmitter ids live at 0x38000..0x40000,
    // widget/stream handles lower) are small integers, not heap pointers.
    // Probing the GcHeader at handle-8 read unmapped memory and SIGSEGV'd
    // when events.on(emitter, ...) validated its target (#4633).
    if crate::value::addr_class::is_handle_band(target as usize) {
        return false;
    }
    let gc_header =
        (target as *const u8).sub(crate::gc::GC_HEADER_SIZE) as *const crate::gc::GcHeader;
    if (*gc_header).obj_type != crate::gc::GC_TYPE_OBJECT {
        return false;
    }
    // A subclass instance carries its own class id and never ran
    // `js_event_target_new`, so it has no `_eventTarget` marker field until
    // `seed_event_target_state` lazily installs one. The class-chain edge is
    // what identifies it (#6301).
    if class_chain_is_event_target((*target).class_id) {
        return true;
    }
    let marker = js_object_get_field_by_name_f64(target, key(b"_eventTarget"));
    marker.to_bits() == JSValue::bool(true).bits()
}

/// Install the hidden EventTarget state (marker + listener bag + max-listener
/// count) on an object that is an event target by class chain but has never
/// been seeded — i.e. a `class Bus extends EventTarget` instance, whose
/// constructor path allocates a plain subclass object rather than calling
/// `js_event_target_new`. Seeding on first listener/dispatch use keeps the
/// subclass ctor lowering untouched.
///
/// `js_object_alloc` and `key` (which interns a string) both allocate and can
/// therefore GC-move `target`, so every heap value is rooted up front and
/// re-read from its handle at each use. The bag and all three key strings are
/// allocated BEFORE the first store on purpose: the inline form
/// `js_object_set_field_by_name(target, key(b"…"), …)` allocates the key
/// *after* Rust has already evaluated `target` (arguments evaluate
/// left-to-right), which would leave a stale receiver pointer if interning that
/// key triggered a collection.
unsafe fn seed_event_target_state(target: *mut ObjectHeader) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_raw_mut_ptr(target);
    let bag_handle = scope.root_raw_mut_ptr(js_object_alloc(0, 0));
    let marker_key = scope.root_string_ptr(key(b"_eventTarget"));
    let listeners_key = scope.root_string_ptr(key(b"_eventTargetListeners"));
    let max_listeners_key = scope.root_string_ptr(key(b"_eventTargetMaxListeners"));

    js_object_set_field_by_name(
        target_handle.get_raw_mut_ptr::<ObjectHeader>(),
        marker_key.get_raw_mut_ptr::<StringHeader>(),
        bool_value(true),
    );
    js_object_set_field_by_name(
        target_handle.get_raw_mut_ptr::<ObjectHeader>(),
        listeners_key.get_raw_mut_ptr::<StringHeader>(),
        boxed_ptr(bag_handle.get_raw_mut_ptr::<ObjectHeader>()),
    );
    js_object_set_field_by_name(
        target_handle.get_raw_mut_ptr::<ObjectHeader>(),
        max_listeners_key.get_raw_mut_ptr::<StringHeader>(),
        10.0,
    );

    bag_handle.get_raw_mut_ptr::<ObjectHeader>()
}

unsafe fn listeners_bag(target: *mut ObjectHeader) -> Option<*mut ObjectHeader> {
    if !is_event_target(target) {
        return None;
    }
    let bag = js_object_get_field_by_name_f64(target, key(b"_eventTargetListeners"));
    if let Some(bag) = value_as_ptr::<ObjectHeader>(bag) {
        return Some(bag);
    }
    Some(seed_event_target_state(target))
}

unsafe fn event_array(
    bag: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    create: bool,
) -> Option<*mut ArrayHeader> {
    if bag.is_null() || event_name_ptr.is_null() {
        return None;
    }
    let existing = js_object_get_field_by_name_f64(bag, event_name_ptr);
    if let Some(arr) = value_as_ptr::<ArrayHeader>(existing) {
        return Some(arr);
    }
    if !create {
        return None;
    }
    let arr = js_array_alloc(0);
    js_object_set_field_by_name(bag, event_name_ptr, boxed_ptr(arr));
    Some(arr)
}

fn listener_record_object(record: f64) -> Option<*mut ObjectHeader> {
    let value = JSValue::from_bits(record.to_bits());
    if !value.is_pointer() {
        return None;
    }
    let ptr = value.as_pointer::<u8>() as usize;
    if crate::closure::is_closure_ptr(ptr) {
        return None;
    }
    value_as_ptr::<ObjectHeader>(record)
}

fn listener_record_callback(record: f64) -> f64 {
    let Some(record_ptr) = listener_record_object(record) else {
        return record;
    };
    let callback = js_object_get_field_by_name_f64(record_ptr, key(b"_callback"));
    if is_undefined(callback) {
        record
    } else {
        callback
    }
}

fn listener_record_capture(record: f64) -> bool {
    let Some(record_ptr) = listener_record_object(record) else {
        return false;
    };
    crate::value::js_is_truthy(js_object_get_field_by_name_f64(
        record_ptr,
        key(b"_capture"),
    )) != 0
}

fn listener_record_once(record: f64) -> bool {
    let Some(record_ptr) = listener_record_object(record) else {
        return false;
    };
    crate::value::js_is_truthy(js_object_get_field_by_name_f64(record_ptr, key(b"_once"))) != 0
}

fn listener_record_matches(record: f64, listener: f64, capture: bool) -> bool {
    listener_record_callback(record).to_bits() == listener.to_bits()
        && listener_record_capture(record) == capture
}

fn make_listener_record(listener: f64, capture: bool, once: bool) -> f64 {
    let record = js_object_alloc(0, 0);
    js_object_set_field_by_name(record, key(b"_callback"), listener);
    js_object_set_field_by_name(record, key(b"_capture"), bool_value(capture));
    js_object_set_field_by_name(record, key(b"_once"), bool_value(once));
    boxed_ptr(record)
}

unsafe fn remove_event_listener_value_with_capture(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    listener: f64,
    capture: bool,
) {
    let Some(bag) = listeners_bag(target) else {
        return;
    };
    let Some(arr) = event_array(bag, event_name_ptr, false) else {
        return;
    };
    let out = js_array_alloc(0);
    let len = js_array_length(arr);
    let mut changed = false;
    let mut result = out;
    for i in 0..len {
        let current = f64::from_bits(js_array_get(arr, i).bits());
        if !changed && listener_record_matches(current, listener, capture) {
            changed = true;
            continue;
        }
        result = js_array_push_f64(result, current);
    }
    if changed {
        js_object_set_field_by_name(bag, event_name_ptr, boxed_ptr(result));
    }
}

unsafe fn remove_event_listener_with_capture(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
    capture: bool,
) {
    if callback_ptr == 0 {
        return;
    }
    remove_event_listener_value_with_capture(
        target,
        event_name_ptr,
        boxed_ptr(callback_ptr as *mut u8),
        capture,
    );
}

extern "C" fn event_target_abort_remove_listener(
    closure: *const crate::closure::ClosureHeader,
) -> f64 {
    let target = crate::closure::js_closure_get_capture_ptr(closure, 0) as *mut ObjectHeader;
    let event_name_ptr =
        crate::closure::js_closure_get_capture_ptr(closure, 1) as *const StringHeader;
    let callback_ptr = crate::closure::js_closure_get_capture_ptr(closure, 2);
    let capture =
        crate::value::js_is_truthy(crate::closure::js_closure_get_capture_f64(closure, 3)) != 0;
    unsafe {
        remove_event_listener_with_capture(target, event_name_ptr, callback_ptr, capture);
    }
    undefined_value()
}

/// `new EventTarget()`.
///
/// The instance carries `CLASS_ID_EVENT_TARGET` on its header (mirroring
/// `construct_event`'s `CLASS_ID_EVENT`), so `t instanceof EventTarget` holds
/// for the base the same way it holds for a subclass through the registered
/// parent edge (#6301). The `_eventTarget` marker field stays: the Node
/// `events` helpers' target probe predates the class id, and a subclass
/// instance is seeded with the same marker on first use.
#[no_mangle]
pub extern "C" fn js_event_target_new() -> *mut ObjectHeader {
    let target = js_object_alloc(CLASS_ID_EVENT_TARGET, 0);
    let bag = js_object_alloc(0, 0);
    js_object_set_field_by_name(
        target,
        key(b"_eventTarget"),
        f64::from_bits(JSValue::bool(true).bits()),
    );
    js_object_set_field_by_name(target, key(b"_eventTargetListeners"), boxed_ptr(bag));
    js_object_set_field_by_name(target, key(b"_eventTargetMaxListeners"), 10.0);
    target
}

/// `target.addEventListener(type, listener)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_add_event_listener(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) {
    js_event_target_add_event_listener_with_options(
        target,
        event_name_ptr,
        callback_ptr,
        undefined_value(),
    );
}

/// `target.addEventListener(type, listener, options)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_add_event_listener_with_options(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
    options: f64,
) {
    if callback_ptr == 0 {
        return;
    }
    let capture = listener_capture(options);
    let once = listener_option_bool(options, b"once");
    if let Some(signal) = listener_signal(options) {
        if crate::url::js_abort_signal_is_aborted(signal) != 0 {
            return;
        }
    }
    let Some(bag) = listeners_bag(target) else {
        return;
    };
    let Some(arr) = event_array(bag, event_name_ptr, true) else {
        return;
    };
    let listener = boxed_ptr(callback_ptr as *mut u8);
    let len = js_array_length(arr);
    for i in 0..len {
        let current = f64::from_bits(js_array_get(arr, i).bits());
        if listener_record_matches(current, listener, capture) {
            return;
        }
    }
    let updated = js_array_push_f64(arr, make_listener_record(listener, capture, once));
    if updated != arr {
        js_object_set_field_by_name(bag, event_name_ptr, boxed_ptr(updated));
    }
    if let Some(signal) = listener_signal(options) {
        let func = event_target_abort_remove_listener as *const u8;
        crate::closure::js_register_closure_arity(func, 0);
        let abort_listener = crate::closure::js_closure_alloc(func, 4);
        crate::closure::js_closure_set_capture_ptr(abort_listener, 0, target as i64);
        crate::closure::js_closure_set_capture_ptr(abort_listener, 1, event_name_ptr as i64);
        crate::closure::js_closure_set_capture_ptr(abort_listener, 2, callback_ptr);
        crate::closure::js_closure_set_capture_f64(abort_listener, 3, bool_value(capture));
        crate::url::js_abort_signal_add_listener(
            signal,
            string_value(b"abort"),
            boxed_ptr(abort_listener),
        );
    }
}

/// `target.removeEventListener(type, listener)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_remove_event_listener(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) {
    js_event_target_remove_event_listener_with_options(
        target,
        event_name_ptr,
        callback_ptr,
        undefined_value(),
    );
}

/// `target.removeEventListener(type, listener, options)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_remove_event_listener_with_options(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
    options: f64,
) {
    remove_event_listener_with_capture(
        target,
        event_name_ptr,
        callback_ptr,
        listener_capture(options),
    );
}

fn event_type_ptr(event: f64) -> Option<*const StringHeader> {
    let event_ptr = value_as_ptr::<ObjectHeader>(event)?;
    let type_value = js_object_get_field_by_name(event_ptr, key(b"type"));
    let type_box = f64::from_bits(type_value.bits());
    let ptr = crate::value::js_get_string_pointer_unified(type_box) as *const StringHeader;
    (!ptr.is_null()).then_some(ptr)
}

fn closure_value_from_listener(listener: f64) -> Option<f64> {
    let jv = JSValue::from_bits(listener.to_bits());
    if jv.is_pointer() {
        let ptr = jv.as_pointer::<crate::closure::ClosureHeader>();
        if !ptr.is_null() && crate::closure::is_closure_ptr(ptr as usize) {
            return Some(listener);
        }
    }
    None
}

/// `target.dispatchEvent(event)`.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_dispatch_event(
    target: *mut ObjectHeader,
    event: f64,
) -> f64 {
    if !is_event_target(target) {
        return bool_value(true);
    }
    let Some(event_ptr) = value_as_ptr::<ObjectHeader>(event) else {
        if is_undefined(event) {
            throw_missing_arg("event");
        }
        throw_invalid_event(event);
    };
    if !is_event_instance(event_ptr) {
        throw_invalid_event(event);
    }
    let Some(event_name_ptr) = event_type_ptr(event) else {
        return bool_value(true);
    };
    let target_value = boxed_ptr(target);
    set_event_field(event_ptr, b"target", target_value);
    set_event_field(event_ptr, b"currentTarget", target_value);
    set_event_field(event_ptr, b"eventPhase", number_value(2.0));

    let callbacks = listeners_bag(target)
        .and_then(|bag| event_array(bag, event_name_ptr, false))
        .map(|arr| {
            let len = js_array_length(arr);
            let mut callbacks = Vec::with_capacity(len as usize);
            for i in 0..len {
                let record = f64::from_bits(js_array_get(arr, i).bits());
                callbacks.push((
                    listener_record_callback(record),
                    listener_record_capture(record),
                    listener_record_once(record),
                ));
            }
            callbacks
        })
        .unwrap_or_default();

    let args = [event];
    for (callback, capture, once) in callbacks {
        let Some(callable) = closure_value_from_listener(callback) else {
            continue;
        };
        if once {
            remove_event_listener_value_with_capture(target, event_name_ptr, callback, capture);
        }
        let prev_this = crate::object::js_implicit_this_set(target_value);
        let _ = crate::closure::js_native_call_value(callable, args.as_ptr(), args.len());
        crate::object::js_implicit_this_set(prev_this);
        if event_bool_field(event_ptr, b"_immediateStopped") {
            break;
        }
    }

    set_event_field(event_ptr, b"currentTarget", null_value());
    set_event_field(event_ptr, b"eventPhase", number_value(0.0));
    let canceled = event_bool_field(event_ptr, b"cancelable")
        && event_bool_field(event_ptr, b"defaultPrevented");
    bool_value(!canceled)
}

/// Runtime predicate used by the Node `events` module helpers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_is_event_target(target: *const ObjectHeader) -> i32 {
    if is_event_target(target) {
        1
    } else {
        0
    }
}

/// `events.getEventListeners(target, type)` for EventTarget receivers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_get_event_listeners(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    let out = js_array_alloc(0);
    let Some(bag) = listeners_bag(target) else {
        return out;
    };
    let Some(arr) = event_array(bag, event_name_ptr, false) else {
        return out;
    };
    let len = js_array_length(arr);
    let mut result = out;
    for i in 0..len {
        let current = js_array_get(arr, i);
        result = js_array_push_f64(
            result,
            listener_record_callback(f64::from_bits(current.bits())),
        );
    }
    result
}

/// `events.getMaxListeners(target)` for EventTarget receivers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_get_max_listeners(target: *mut ObjectHeader) -> f64 {
    if !is_event_target(target) {
        return 10.0;
    }
    let value = js_object_get_field_by_name_f64(target, key(b"_eventTargetMaxListeners"));
    if JSValue::from_bits(value.to_bits()).is_number() {
        value
    } else {
        10.0
    }
}

/// `events.setMaxListeners(n, target)` for EventTarget receivers.
#[no_mangle]
pub unsafe extern "C" fn js_event_target_set_max_listeners(
    target: *mut ObjectHeader,
    n: f64,
) -> i32 {
    if !is_event_target(target) {
        return 0;
    }
    js_object_set_field_by_name(target, key(b"_eventTargetMaxListeners"), n);
    1
}

// ─────────────────────────────────────────────────────────────────
// #6301 — the EventTarget method surface as real function VALUES.
//
// `addEventListener` / `removeEventListener` / `dispatchEvent` used to exist
// only as compile-time lowerings keyed on a receiver whose static class name
// was literally `EventTarget` (`lower_call/event_target.rs`). Nothing lived on
// a prototype, so `typeof t.addEventListener` was `undefined` even on a plain
// `new EventTarget()`, and a `class Bus extends EventTarget {}` instance —
// whose static class name is `Bus` — inherited nothing at all and died with
// `TypeError: value is not a function` (the real root cause of #5931: cac v7's
// `class CAC extends EventTarget` calls `this.dispatchEvent(...)`).
//
// The methods are now resolved off the receiver's *class chain* — the same
// mechanism `Event` subclasses already use — and materialized as bound-method
// closures (the AbortSignal `abort_signal_method_bind` shape, and the
// value-read-materializes-a-bound-method shape of #6281). Both the value-read
// path (`object/field_get_set/get_field_by_name_tail.rs`) and the dynamic-call
// path (`object/native_call_method.rs`) consult `event_target_method_bind`, so
// a subclass instance at any depth reads them as functions AND calls them.
// ─────────────────────────────────────────────────────────────────

/// The receiver a bound EventTarget method closure captured in slot 0 (stored
/// as NaN-boxed bits so the GC's closure scan keeps it alive and relocates it).
unsafe fn bound_event_target(closure: *const crate::closure::ClosureHeader) -> *mut ObjectHeader {
    let bits = crate::closure::js_closure_get_capture_ptr(closure, 0) as u64;
    crate::value::js_nanbox_get_pointer(f64::from_bits(bits)) as *mut ObjectHeader
}

/// Shared body of the add/remove listener thunks. `string_from_value` can
/// allocate (a non-string `type` is coerced), so the receiver and the value
/// arguments are rooted across it and re-read afterwards.
unsafe fn bound_listener_call(
    closure: *const crate::closure::ClosureHeader,
    event_type: f64,
    listener: f64,
    options: f64,
    add: bool,
) -> f64 {
    let listener_value = JSValue::from_bits(listener.to_bits());
    if !listener_value.is_pointer() {
        // Node ignores a nullish listener and rejects a non-object one; a
        // non-pointer here is neither a closure nor a handler object, so there
        // is nothing to register or remove.
        return undefined_value();
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_raw_mut_ptr(bound_event_target(closure));
    let listener_handle = scope.root_nanbox_f64(listener);
    let options_handle = scope.root_nanbox_f64(options);

    let name_handle = scope.root_string_ptr(string_from_value(event_type));

    let target = target_handle.get_raw_mut_ptr::<ObjectHeader>();
    let name = name_handle.get_raw_const_ptr::<StringHeader>();
    let callback_ptr = crate::value::js_nanbox_get_pointer(listener_handle.get_nanbox_f64());
    let options = options_handle.get_nanbox_f64();
    if add {
        js_event_target_add_event_listener_with_options(target, name, callback_ptr, options);
    } else {
        js_event_target_remove_event_listener_with_options(target, name, callback_ptr, options);
    }
    undefined_value()
}

extern "C" fn event_target_add_event_listener_thunk(
    closure: *const crate::closure::ClosureHeader,
    event_type: f64,
    listener: f64,
    options: f64,
) -> f64 {
    unsafe { bound_listener_call(closure, event_type, listener, options, true) }
}

extern "C" fn event_target_remove_event_listener_thunk(
    closure: *const crate::closure::ClosureHeader,
    event_type: f64,
    listener: f64,
    options: f64,
) -> f64 {
    unsafe { bound_listener_call(closure, event_type, listener, options, false) }
}

extern "C" fn event_target_dispatch_event_thunk(
    closure: *const crate::closure::ClosureHeader,
    event: f64,
) -> f64 {
    unsafe { js_event_target_dispatch_event(bound_event_target(closure), event) }
}

/// The EventTarget method names, in the order Node's `EventTarget.prototype`
/// declares them. Callers gate on this before paying for the receiver probe.
pub(crate) fn is_event_target_method_name(name: &[u8]) -> bool {
    matches!(
        name,
        b"addEventListener" | b"removeEventListener" | b"dispatchEvent"
    )
}

/// Materialize a bound EventTarget method for `name` when `target` is an event
/// target (a plain `new EventTarget()` or a subclass instance at any depth).
/// Returns `None` for any other name or receiver, so an unknown property still
/// reads as `undefined` and a non-target object keeps its existing dispatch.
pub(crate) fn event_target_method_bind(target: *mut ObjectHeader, name: &[u8]) -> Option<f64> {
    let (func, arity): (*const u8, u32) = match name {
        b"addEventListener" => (event_target_add_event_listener_thunk as *const u8, 3),
        b"removeEventListener" => (event_target_remove_event_listener_thunk as *const u8, 3),
        b"dispatchEvent" => (event_target_dispatch_event_thunk as *const u8, 1),
        _ => return None,
    };
    if !unsafe { is_event_target(target) } {
        return None;
    }
    crate::closure::js_register_closure_arity(func, arity);
    // `js_closure_alloc` can GC-move the receiver — root it and re-read.
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_raw_mut_ptr(target);
    let closure = crate::closure::js_closure_alloc(func, 1);
    let target_bits = boxed_ptr(target_handle.get_raw_mut_ptr::<ObjectHeader>()).to_bits();
    crate::closure::js_closure_set_capture_ptr(closure, 0, target_bits as i64);
    Some(boxed_ptr(closure))
}

/// Value-read entry point for the object field-get tail (#6301): resolve
/// `addEventListener` / `removeEventListener` / `dispatchEvent` off the
/// receiver's class chain when nothing earlier in the property walk claimed the
/// name. Gated on the name first, so a non-EventTarget read pays only a
/// length+compare, never the receiver probe.
///
/// The call site sits AFTER the tail's `keys_array.is_null()` early return, and
/// that placement is correct: a `class X extends EventTarget` instance never
/// takes the keyless branch. Every class-instance allocator
/// (`js_object_alloc_class_inline_keys`, `js_object_alloc_class_with_keys`,
/// `js_object_alloc_class_dynamic_parent`) installs the shape-cached keys array
/// unconditionally, and `js_build_class_keys_array` hands back a zero-LENGTH
/// array — never NULL — for a class that declares no fields. So even the
/// emptiest subclass (`class Bus extends EventTarget {}`, no fields, no ctor)
/// lands on the shaped path with an empty keys array and reaches this fallback.
///
/// The keyless branch is for `Object.create(proto)` / bare
/// `js_new_function_construct` receivers, whose inherited methods resolve one
/// hop earlier through `resolve_proto_chain_field_with_receiver` (the prototype
/// object is itself a shaped, class-id-stamped receiver) — so they never reach
/// that branch's dead end either. The empty-shape cases in
/// `test-files/test_gap_6301_event_target_subclass.ts` pin this.
pub(crate) fn event_target_value_read(target: *mut ObjectHeader, name: &[u8]) -> Option<f64> {
    if !is_event_target_method_name(name) {
        return None;
    }
    event_target_method_bind(target, name)
}
