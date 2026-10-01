//! WHATWG event classes with shared prototypes and symbol-keyed own state.
//! Public and private state uses the ordinary property shape and GC barriers.

use crate::{
    js_array_alloc, js_array_get, js_array_length, js_array_push_f64, js_nanbox_pointer,
    js_object_alloc, js_object_get_field_by_name_f64, js_object_set_field_by_name,
    js_string_from_bytes, ArrayHeader, JSValue, ObjectHeader, StringHeader,
};
pub(crate) mod prototype;
pub(crate) mod state;

pub const CLASS_ID_EVENT: u32 = crate::native_class_ids::EVENT;
pub const CLASS_ID_CUSTOM_EVENT: u32 = crate::native_class_ids::CUSTOM_EVENT;
pub const CLASS_ID_DOM_EXCEPTION: u32 = crate::native_class_ids::DOM_EXCEPTION;
/// `EventTarget` base class. Stamped on `new EventTarget()` instances and used
/// as the PARENT class id of a user `class X extends EventTarget` (wired by
/// `js_register_class_parent_dynamic` via `global_builtin_constructor_class_id`).
/// The class chain selects native-base initialization for subclasses (#6301).
/// Instance branding follows the prototype constructor's EventTarget marker.
/// Keep in sync with perry-codegen/src/expr/instance_misc1.rs.
pub const CLASS_ID_EVENT_TARGET: u32 = crate::native_class_ids::EVENT_TARGET;

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

fn set_event_field(event: *mut ObjectHeader, name: u32, value: f64) {
    state::set(event, name, value);
}

fn event_bool_field(event: *mut ObjectHeader, name: u32) -> bool {
    if event.is_null() {
        return false;
    }
    let value = state::get(event, name);
    crate::value::js_is_truthy(value) != 0
}

extern "C" fn event_prevent_default_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this_value = this.as_f64();
    let Some(event) = value_as_ptr::<ObjectHeader>(this_value) else {
        return undefined_value();
    };
    if event_bool_field(event, state::CANCELABLE) && !event_bool_field(event, state::PASSIVE) {
        set_event_field(event, state::DEFAULT_PREVENTED, bool_value(true));
    }
    undefined_value()
}

extern "C" fn event_stop_propagation_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this_value = this.as_f64();
    if let Some(event) = value_as_ptr::<ObjectHeader>(this_value) {
        state::require_private(event, state::STOPPED);
        set_event_field(event, state::STOPPED, bool_value(true));
    }
    undefined_value()
}

extern "C" fn event_stop_immediate_propagation_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let this_value = this.as_f64();
    if let Some(event) = value_as_ptr::<ObjectHeader>(this_value) {
        state::require_private(event, state::STOPPED);
        set_event_field(event, state::STOPPED, bool_value(true));
        set_event_field(event, state::IMMEDIATE_STOPPED, bool_value(true));
    }
    undefined_value()
}

fn construct_event(
    type_value: f64,
    options: f64,
    class_id: u32,
    constructor_name: &[u8],
    detail: Option<f64>,
) -> *mut ObjectHeader {
    let scope = crate::gc::RuntimeHandleScope::new();
    let type_value = scope.root_nanbox_f64(type_value);
    let options = scope.root_nanbox_f64(options);
    let detail = detail.map(|value| scope.root_nanbox_f64(value));
    note_constructed();
    let event = scope.root_raw_mut_ptr(js_object_alloc(class_id, 0));
    let (_, result) = event.across_mut(|| {
        event.with_mut_ptr(|ptr| state::link(ptr, std::str::from_utf8(constructor_name).unwrap()));
        event.with_mut_ptr(|ptr| {
            init_event_fields(
                ptr,
                type_value.get_nanbox_f64(),
                options.get_nanbox_f64(),
                constructor_name,
                detail.map(|v| v.get_nanbox_f64()),
            );
        });
    });
    result
}

/// Shared Event field/method initialization, applied either to a freshly
/// allocated Event (`construct_event`) or to an existing subclass instance
/// (`js_event_subclass_init` — `super(type, options)` from
/// `class X extends Event`).
fn init_event_fields(
    event: *mut ObjectHeader,
    type_value: f64,
    options: f64,
    _constructor_name: &[u8],
    detail: Option<f64>,
) {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_raw_mut_ptr(event);
    let options = scope.root_nanbox_f64(options);
    let detail = detail.map(|value| scope.root_nanbox_f64(value));
    let type_ptr = string_from_value(type_value);
    event.with_mut_ptr(|ptr| {
        set_event_field(
            ptr,
            state::TYPE,
            crate::value::js_nanbox_string(type_ptr as i64),
        )
    });
    for (name, slot) in [
        (b"bubbles".as_slice(), state::BUBBLES),
        (b"cancelable", state::CANCELABLE),
        (b"composed", state::COMPOSED),
    ] {
        let value = unsafe { option_bool(options.get_nanbox_f64(), name) };
        event.with_mut_ptr(|ptr| set_event_field(ptr, slot, bool_value(value)));
    }
    for (name, value) in [
        (state::DEFAULT_PREVENTED, bool_value(false)),
        (state::TARGET, null_value()),
        (state::TRUSTED, bool_value(false)),
        (state::TIMESTAMP, number_value(0.0)),
        (state::STOPPED, bool_value(false)),
        (state::DISPATCHED, bool_value(false)),
        (state::PASSIVE, bool_value(false)),
    ] {
        event.with_mut_ptr(|ptr| set_event_field(ptr, name, value));
    }
    if let Some(detail) = detail {
        event.with_mut_ptr(|ptr| set_event_field(ptr, state::DETAIL, detail.get_nanbox_f64()));
    }
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_raw_mut_ptr(event);
    let type_value = scope.root_nanbox_f64(type_value);
    let options = scope.root_nanbox_f64(options);
    if is_custom != 0 {
        let detail = unsafe { option_detail(options.get_nanbox_f64()) };
        event.with_mut_ptr(|ptr| {
            init_event_fields(
                ptr,
                type_value.get_nanbox_f64(),
                options.get_nanbox_f64(),
                b"CustomEvent",
                Some(detail),
            )
        });
    } else {
        event.with_mut_ptr(|ptr| {
            init_event_fields(
                ptr,
                type_value.get_nanbox_f64(),
                options.get_nanbox_f64(),
                b"Event",
                None,
            )
        });
    }
    undefined_value()
}

/// Keepalive anchor for the auto-optimize whole-program build —
/// `js_event_subclass_init` is a generated-code-only callee.
#[cfg(feature = "keepalive-anchors")]
#[used(compiler)]
static KEEP_JS_EVENT_SUBCLASS_INIT: extern "C" fn(f64, f64, f64, u32, u32) -> f64 =
    js_event_subclass_init;

/// `class X extends DOMException` — `super(message, name)` initializer
/// (undici's `WebSocketError`, and its module-init `class Test extends
/// DOMException` capability probe). The subclass instance is a registry-class
/// object, not the ErrorHeader `new DOMException(...)` allocates, so stamp the
/// DOMException surface onto `this`: `message`, `name` (default `"Error"`,
/// matching `js_dom_exception_new`), and the legacy numeric `code` for that
/// name.
#[no_mangle]
pub extern "C" fn js_dom_exception_subclass_init(this_value: f64, message: f64, name: f64) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let exception = scope.root_nanbox_f64(this_value);
    let message = scope.root_nanbox_f64(message);
    let name = scope.root_nanbox_f64(name);
    if value_as_ptr::<ObjectHeader>(exception.get_nanbox_f64()).is_none() {
        return undefined_value();
    }
    let message_ptr = optional_string_from_value(message.get_nanbox_f64(), b"");
    let message_ptr = scope.root_string_ptr(message_ptr);
    let name_ptr = optional_string_from_value(name.get_nanbox_f64(), b"Error");
    let name_ptr = scope.root_string_ptr(name_ptr);
    set_event_field(
        value_as_ptr::<ObjectHeader>(exception.get_nanbox_f64()).unwrap(),
        state::DOM_MESSAGE,
        message_ptr
            .with_const_ptr::<StringHeader, _>(|ptr| crate::value::js_nanbox_string(ptr as i64)),
    );
    set_event_field(
        value_as_ptr::<ObjectHeader>(exception.get_nanbox_f64()).unwrap(),
        state::DOM_NAME,
        name_ptr
            .with_const_ptr::<StringHeader, _>(|ptr| crate::value::js_nanbox_string(ptr as i64)),
    );
    let error = js_dom_exception_new(
        message_ptr
            .with_const_ptr::<StringHeader, _>(|ptr| crate::value::js_nanbox_string(ptr as i64)),
        name_ptr
            .with_const_ptr::<StringHeader, _>(|ptr| crate::value::js_nanbox_string(ptr as i64)),
    );
    let stack = crate::error::js_error_get_stack(error);
    let _gc = crate::gc::GcSuppressScope::new();
    let obj = value_as_ptr::<ObjectHeader>(exception.get_nanbox_f64()).unwrap();
    js_object_set_field_by_name(
        obj,
        key(b"stack"),
        crate::value::js_nanbox_string(stack as i64),
    );
    crate::object::set_builtin_property_attrs(
        obj as usize,
        "stack".to_owned(),
        crate::object::PropertyAttrs::new(true, false, true),
    );
    undefined_value()
}

/// Keepalive anchor for the auto-optimize whole-program build —
/// `js_dom_exception_subclass_init` is a generated-code-only callee.
#[used(compiler)]
static KEEP_JS_DOM_EXCEPTION_SUBCLASS_INIT: extern "C" fn(f64, f64, f64) -> f64 =
    js_dom_exception_subclass_init;

fn is_event_instance(event: *const ObjectHeader) -> bool {
    let valid = unsafe { crate::object::shaped_symbols::owner(event as usize).is_some() };
    valid
        && crate::value::JSValue::from_bits(
            state::get(event as *mut ObjectHeader, state::TYPE).to_bits(),
        )
        .is_string()
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let type_value = scope.root_nanbox_f64(type_value);
    let options = scope.root_nanbox_f64(options);
    let detail = unsafe { option_detail(options.get_nanbox_f64()) };
    construct_event(
        type_value.get_nanbox_f64(),
        options.get_nanbox_f64(),
        CLASS_ID_CUSTOM_EVENT,
        b"CustomEvent",
        Some(detail),
    )
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let name = scope.root_nanbox_f64(name);
    let message_ptr = scope.root_string_ptr(optional_string_from_value(message, b""));
    let name_ptr = optional_string_from_value(name.get_nanbox_f64(), b"Error");
    let name_bytes = unsafe {
        std::slice::from_raw_parts(
            (name_ptr as *const u8).add(std::mem::size_of::<StringHeader>()),
            (*name_ptr).byte_len as usize,
        )
        .to_vec()
    };
    let err = message_ptr
        .with_mut_ptr(|ptr| crate::error::js_error_new_with_name_message_bytes(&name_bytes, ptr));
    note_constructed();
    unsafe {
        crate::error::mark_dom_exception(err);
    }
    state::link(err.cast(), "DOMException").cast()
}

pub(crate) fn is_dom_exception_error(err: *const crate::error::ErrorHeader) -> bool {
    unsafe {
        crate::value::addr_class::try_read_tracked_gc_header(err as usize)
            .is_some_and(|h| h.as_ref().obj_type == crate::gc::GC_TYPE_ERROR)
            && (*err).error_kind == crate::error::ERROR_KIND_DOM_EXCEPTION
    }
}

/// Test-only observation of a cell's own brand, with allocator ownership
/// checked first; no address registry survives the owning heap.
#[doc(hidden)]
pub fn dom_exception_has_error_brand_for_test(addr: usize) -> bool {
    is_dom_exception_error(addr as *const crate::error::ErrorHeader)
}

pub(crate) fn is_dom_exception_object(obj: *const ObjectHeader) -> bool {
    unsafe {
        if !crate::value::addr_class::try_read_tracked_gc_header(obj as usize)
            .is_some_and(|h| h.as_ref().obj_type == crate::gc::GC_TYPE_OBJECT)
        {
            return false;
        }
        if !state::has(obj as *mut ObjectHeader, state::DOM_NAME) {
            return false;
        }
        true
    }
}

pub(crate) fn abort_dom_exception_value() -> f64 {
    let message = string_value(b"This operation was aborted");
    let name = string_value(b"AbortError");
    let err = js_dom_exception_new(message, name);
    crate::value::js_nanbox_pointer(err as i64)
}

pub(crate) unsafe fn is_event_target(target: *const ObjectHeader) -> bool {
    if crate::object::shaped_symbols::owner(target as usize).is_none() {
        return false;
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_raw_mut_ptr(target as *mut ObjectHeader);
    let name = key(b"constructor");
    let constructor = scope
        .root_nanbox_f64(target.with_mut_ptr(|ptr| js_object_get_field_by_name_f64(ptr, name)));
    let description = string_value(b"nodejs.event_target");
    let marker = crate::symbol::js_symbol_for(description);
    crate::value::js_is_truthy(crate::symbol::js_object_get_symbol_property(
        constructor.get_nanbox_f64(),
        marker,
    )) != 0
}

unsafe fn listeners_bag(target: *mut ObjectHeader) -> Option<*mut ObjectHeader> {
    if !is_event_target(target) {
        return None;
    }
    let _gc = crate::gc::GcSuppressScope::new();
    if let Some(bag) = value_as_ptr::<ObjectHeader>(state::get(target, state::LISTENERS)) {
        return Some(bag);
    }
    let bag = crate::map::js_map_alloc(0).cast::<ObjectHeader>();
    state::set(target, state::LISTENERS, boxed_ptr(bag));
    Some(bag)
}

unsafe fn event_array(
    bag: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    create: bool,
) -> Option<*mut ArrayHeader> {
    if bag.is_null() || event_name_ptr.is_null() {
        return None;
    }
    let existing = crate::map::js_map_get(
        bag.cast(),
        crate::value::js_nanbox_string(event_name_ptr as i64),
    );
    if let Some(arr) = value_as_ptr::<ArrayHeader>(existing) {
        return Some(arr);
    }
    if !create {
        return None;
    }
    let arr = js_array_alloc(0);
    crate::map::js_map_set(
        bag.cast(),
        crate::value::js_nanbox_string(event_name_ptr as i64),
        boxed_ptr(arr),
    );
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
        crate::map::js_map_set(
            bag.cast(),
            crate::value::js_nanbox_string(event_name_ptr as i64),
            boxed_ptr(result),
        );
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
    _this: crate::closure::JsThis,
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
/// parent edge. Internal listener state is traced through the instance's symbol properties.
#[no_mangle]
pub extern "C" fn js_event_target_new() -> *mut ObjectHeader {
    let _gc = crate::gc::GcSuppressScope::new();
    note_constructed();
    let target = js_object_alloc(CLASS_ID_EVENT_TARGET, 0);
    state::initialize_target(target, false);
    state::link(target, "EventTarget")
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_raw_mut_ptr(target);
    let name_h = scope.root_string_ptr(event_name_ptr);
    let callback_h = scope.root_raw_mut_ptr(callback_ptr as *mut crate::closure::ClosureHeader);
    let options_h = scope.root_nanbox_f64(options);
    let capture = listener_capture(options_h.get_nanbox_f64());
    let once = listener_option_bool(options_h.get_nanbox_f64(), b"once");
    let signal = listener_signal(options_h.get_nanbox_f64());
    let signal_h = signal.map(|signal| scope.root_raw_mut_ptr(signal));
    if signal_h
        .as_ref()
        .is_some_and(|signal| crate::url::js_abort_signal_is_aborted(signal.get_raw_mut_ptr()) != 0)
    {
        return;
    }
    // Only private storage operations follow; option getters have already run.
    let _gc = crate::gc::GcSuppressScope::new();
    let target = target_h.get_raw_mut_ptr();
    let event_name_ptr = name_h.get_raw_const_ptr();
    let callback_ptr = callback_h.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64;
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
        crate::map::js_map_set(
            bag.cast(),
            crate::value::js_nanbox_string(event_name_ptr as i64),
            boxed_ptr(updated),
        );
    }
    if let Some(signal) = signal_h {
        let signal = signal.get_raw_mut_ptr();
        let abort_listener = crate::closure::js_closure_alloc(
            crate::fn_info!(event_target_abort_remove_listener, 0; with_declared(0)),
            4,
        );
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let target = scope.root_raw_mut_ptr(target);
    let name = scope.root_string_ptr(event_name_ptr);
    let callback = scope.root_raw_mut_ptr(callback_ptr as *mut crate::closure::ClosureHeader);
    let capture = listener_capture(options);
    let _gc = crate::gc::GcSuppressScope::new();
    remove_event_listener_with_capture(
        target.get_raw_mut_ptr(),
        name.get_raw_const_ptr(),
        callback.get_raw_mut_ptr::<crate::closure::ClosureHeader>() as i64,
        capture,
    );
}

fn event_type_ptr(event: f64) -> Option<*const StringHeader> {
    let event_ptr = value_as_ptr::<ObjectHeader>(event)?;
    let type_box = state::get(event_ptr, state::TYPE);
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
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_h = scope.root_raw_mut_ptr(target);
    let event_h = scope.root_nanbox_f64(event);
    let target = || target_h.get_raw_mut_ptr::<ObjectHeader>();
    let event_ptr =
        || crate::value::js_nanbox_get_pointer(event_h.get_nanbox_f64()) as *mut ObjectHeader;
    let Some(event_name) = event_type_ptr(event_h.get_nanbox_f64()) else {
        return bool_value(true);
    };
    let name_h = scope.root_string_ptr(event_name);
    set_event_field(event_ptr(), state::TARGET, boxed_ptr(target()));
    set_event_field(event_ptr(), state::DISPATCHED, bool_value(true));
    let callbacks = {
        let _gc = crate::gc::GcSuppressScope::new();
        let mut callbacks = Vec::new();
        if let Some(arr) = listeners_bag(target())
            .and_then(|bag| event_array(bag, name_h.get_raw_const_ptr(), false))
        {
            for i in 0..js_array_length(arr) {
                let record = f64::from_bits(js_array_get(arr, i).bits());
                callbacks.push((
                    scope.root_nanbox_f64(listener_record_callback(record)),
                    listener_record_capture(record),
                    listener_record_once(record),
                ));
            }
        }
        callbacks
    };
    for (callback, capture, once) in callbacks {
        if closure_value_from_listener(callback.get_nanbox_f64()).is_none() {
            continue;
        }
        if once {
            let _gc = crate::gc::GcSuppressScope::new();
            remove_event_listener_value_with_capture(
                target(),
                name_h.get_raw_const_ptr(),
                callback.get_nanbox_f64(),
                capture,
            );
        }
        let args = [event_h.get_nanbox_f64()];
        crate::closure::js_native_call_value(
            callback.get_nanbox_f64(),
            crate::closure::JsThis::from_f64(boxed_ptr(target())),
            args.as_ptr(),
            1,
        );
        if event_bool_field(event_ptr(), state::IMMEDIATE_STOPPED) {
            break;
        }
    }
    set_event_field(event_ptr(), state::DISPATCHED, bool_value(false));
    bool_value(
        !(event_bool_field(event_ptr(), state::CANCELABLE)
            && event_bool_field(event_ptr(), state::DEFAULT_PREVENTED)),
    )
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
    // Only unreachable private records are read; no user callback can run.
    let _gc = crate::gc::GcSuppressScope::new();
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
    let value = state::get(target, state::MAX_LISTENERS);
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
    state::set(target, state::MAX_LISTENERS, n);
    1
}

/// Brand-check the receiver of a shared EventTarget method.
unsafe fn bound_event_target(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> *mut ObjectHeader {
    let receiver = this.as_f64();
    if let Some(target) = value_as_ptr::<ObjectHeader>(receiver) {
        if is_event_target(target) {
            return target;
        }
    }
    prototype::throw_receiver("EventTarget")
}

fn event_proto_receiver(this: crate::closure::JsThis) -> *mut ObjectHeader {
    let receiver = this.as_f64();
    if let Some(event) = value_as_ptr::<ObjectHeader>(receiver) {
        let valid = unsafe {
            crate::value::addr_class::try_read_tracked_gc_header(event as usize)
                .is_some_and(|h| h.as_ref().obj_type == crate::gc::GC_TYPE_OBJECT)
        };
        if valid && is_event_instance(event) {
            return event;
        }
    }
    let msg = b"Value of this must be an Event";
    let text = crate::string::js_string_from_bytes(msg.as_ptr(), msg.len() as u32);
    let error = crate::error::js_typeerror_new(text);
    crate::exception::js_throw(crate::value::js_nanbox_pointer(error as i64))
}

extern "C" fn event_proto_prevent_default_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    event_proto_receiver(this);
    event_prevent_default_thunk(closure, this)
}

extern "C" fn event_proto_stop_propagation_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    event_proto_receiver(this);
    event_stop_propagation_thunk(closure, this)
}

extern "C" fn event_proto_stop_immediate_propagation_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    event_proto_receiver(this);
    event_stop_immediate_propagation_thunk(closure, this)
}

extern "C" fn event_proto_init_event_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    event_type: f64,
    bubbles: f64,
    cancelable: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_raw_mut_ptr(event_proto_receiver(this));
    let event_type = scope.root_nanbox_f64(event_type);
    if event.with_mut_ptr(|ptr| event_bool_field(ptr, state::DISPATCHED)) {
        return undefined_value();
    }
    let type_string = scope.root_string_ptr(string_from_value(event_type.get_nanbox_f64()));
    let type_value =
        type_string.with_const_ptr::<StringHeader, _>(|p| crate::value::js_nanbox_string(p as i64));
    event.with_mut_ptr::<ObjectHeader, _>(|event| set_event_field(event, state::TYPE, type_value));
    event.with_mut_ptr::<ObjectHeader, _>(|event| {
        set_event_field(
            event,
            state::BUBBLES,
            bool_value(crate::value::js_is_truthy(bubbles) != 0),
        )
    });
    event.with_mut_ptr::<ObjectHeader, _>(|event| {
        set_event_field(
            event,
            state::CANCELABLE,
            bool_value(crate::value::js_is_truthy(cancelable) != 0),
        )
    });
    event.with_mut_ptr::<ObjectHeader, _>(|event| {
        set_event_field(event, state::DEFAULT_PREVENTED, bool_value(false))
    });
    undefined_value()
}

extern "C" fn event_proto_composed_path_thunk(
    _closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let event = scope.root_raw_mut_ptr(event_proto_receiver(this));
    let current = event.with_mut_ptr(|ptr| {
        if event_bool_field(ptr, state::DISPATCHED) {
            state::get(ptr, state::TARGET)
        } else {
            null_value()
        }
    });
    let current = scope.root_nanbox_f64(current);
    let result = js_array_alloc(0);
    let result = if JSValue::from_bits(current.get_nanbox_f64().to_bits()).is_null() {
        result
    } else {
        js_array_push_f64(result, current.get_nanbox_f64())
    };
    boxed_ptr(result)
}

/// Install the WebIDL method values on EventTarget and Event prototypes.
/// CustomEvent inherits Event's methods through its prototype link.
pub(crate) fn install_web_event_proto_methods(name: &str, proto_obj: *mut ObjectHeader) {
    prototype::install(name, proto_obj);
}

/// Shared body of the add/remove listener thunks. `string_from_value` can
/// allocate (a non-string `type` is coerced), so the receiver and the value
/// arguments are rooted across it and re-read afterwards.
unsafe fn bound_listener_call(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    event_type: f64,
    listener: f64,
    options: f64,
    add: bool,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let target_handle = scope.root_raw_mut_ptr(bound_event_target(closure, this));
    let listener_value = JSValue::from_bits(listener.to_bits());
    if !listener_value.is_pointer() {
        // Node ignores a nullish listener and rejects a non-object one; a
        // non-pointer here is neither a closure nor a handler object, so there
        // is nothing to register or remove.
        return undefined_value();
    }
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
    this: crate::closure::JsThis,
    event_type: f64,
    listener: f64,
    options: f64,
) -> f64 {
    unsafe { bound_listener_call(closure, this, event_type, listener, options, true) }
}

extern "C" fn event_target_remove_event_listener_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    event_type: f64,
    listener: f64,
    options: f64,
) -> f64 {
    unsafe { bound_listener_call(closure, this, event_type, listener, options, false) }
}

extern "C" fn event_target_dispatch_event_thunk(
    closure: *const crate::closure::ClosureHeader,
    this: crate::closure::JsThis,
    event: f64,
) -> f64 {
    unsafe { js_event_target_dispatch_event(bound_event_target(closure, this), event) }
}

/// Class identity, including declared subclasses; never a property/name probe.
pub(crate) fn native_class_name(mut class: u32) -> Option<&'static str> {
    for _ in 0..64 {
        let name = match class {
            crate::native_class_ids::ABORT_CONTROLLER => Some("AbortController"),
            crate::native_class_ids::ABORT_SIGNAL => Some("AbortSignal"),
            CLASS_ID_EVENT => Some("Event"),
            CLASS_ID_CUSTOM_EVENT => Some("CustomEvent"),
            CLASS_ID_DOM_EXCEPTION => Some("DOMException"),
            CLASS_ID_EVENT_TARGET => Some("EventTarget"),
            _ => None,
        };
        if name.is_some() {
            return name;
        }
        match crate::object::get_parent_class_id(class) {
            Some(parent) if parent != 0 && parent != class => class = parent,
            _ => return None,
        }
    }
    None
}

pub(crate) fn note_constructed() {
    if crate::hot_diag::receiver_repr_on() {
        crate::hot_diag::receiver_repr_note_constructed(
            crate::hot_diag::ReceiverReprFamily::EventTarget,
        );
    }
}

#[cfg(test)]
pub(crate) mod honest_tests;

/// Initialize the native base through super(), retaining the derived shape
/// and prototype. Merely creating a class host does not run this constructor.
#[no_mangle]
pub extern "C" fn js_event_target_subclass_init(this: f64, kind: u32) -> f64 {
    if kind == 2 {
        prototype::throw_receiver("AbortSignal");
    }
    let scope = crate::gc::RuntimeHandleScope::new();
    let owner =
        scope.root_raw_mut_ptr(value_as_ptr::<ObjectHeader>(this).expect("native super receiver"));
    let class = owner.with_const_ptr::<ObjectHeader, _>(|ptr| unsafe { (*ptr).class_id });
    let proto = crate::object::class_decl_prototype_value(class);
    owner.with_mut_ptr::<ObjectHeader, _>(|ptr| {
        crate::object::prototype_chain::object_link_class_default_prototype(
            ptr as usize,
            proto.to_bits(),
        )
    });
    owner.with_mut_ptr(|owner| {
        if kind == 0 {
            state::initialize_target(owner, false);
        } else {
            state::set(owner, state::CONTROLLER_SIGNAL, undefined_value());
        }
    });
    undefined_value()
}

#[used(compiler)]
static KEEP_JS_EVENT_TARGET_SUBCLASS_INIT: extern "C" fn(f64, u32) -> f64 =
    js_event_target_subclass_init;
