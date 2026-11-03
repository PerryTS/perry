//! Installer-selected Web events, abort objects and DOMException operations.
use crate::array::ArrayHeader;
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;
use crate::string::StringHeader;
static ABORT_DOM_EXCEPTION_VALUE: Hook<fn() -> f64> = Hook::empty();
pub(crate) fn abort_dom_exception_value() -> f64 {
    ABORT_DOM_EXCEPTION_VALUE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static INSTALL_WEB_EVENT_PROTO_METHODS: Hook<fn(&str, *mut ObjectHeader) -> ()> = Hook::empty();
pub(crate) fn install_web_event_proto_methods(name: &str, proto_obj: *mut ObjectHeader) -> () {
    INSTALL_WEB_EVENT_PROTO_METHODS
        .get()
        .map_or_else(|| (), |run| run(name, proto_obj))
}
static IS_DOM_EXCEPTION_ERROR: Hook<fn(*const crate::error::ErrorHeader) -> bool> = Hook::empty();
pub(crate) fn is_dom_exception_error(err: *const crate::error::ErrorHeader) -> bool {
    IS_DOM_EXCEPTION_ERROR
        .get()
        .map_or_else(|| false, |run| run(err))
}
static IS_DOM_EXCEPTION_OBJECT: Hook<fn(*const ObjectHeader) -> bool> = Hook::empty();
pub(crate) fn is_dom_exception_object(obj: *const ObjectHeader) -> bool {
    IS_DOM_EXCEPTION_OBJECT
        .get()
        .map_or_else(|| false, |run| run(obj))
}
static JS_CUSTOM_EVENT_NEW: Hook<extern "C" fn(f64, f64, u32) -> *mut ObjectHeader> = Hook::empty();
pub(crate) extern "C" fn js_custom_event_new(
    type_value: f64,
    options: f64,
    argc: u32,
) -> *mut ObjectHeader {
    JS_CUSTOM_EVENT_NEW.get().map_or_else(
        || std::ptr::null_mut(),
        |run| run(type_value, options, argc),
    )
}
static JS_DOM_EXCEPTION_NEW: Hook<extern "C" fn(f64, f64) -> *mut crate::error::ErrorHeader> =
    Hook::empty();
pub extern "C" fn js_dom_exception_new(message: f64, name: f64) -> *mut crate::error::ErrorHeader {
    JS_DOM_EXCEPTION_NEW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(message, name))
}
static JS_DOM_EXCEPTION_SUBCLASS_INIT: Hook<extern "C" fn(f64, f64, f64) -> f64> = Hook::empty();
pub(crate) extern "C" fn js_dom_exception_subclass_init(
    this_value: f64,
    message: f64,
    name: f64,
) -> f64 {
    JS_DOM_EXCEPTION_SUBCLASS_INIT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(this_value, message, name),
    )
}
static JS_EVENT_NEW: Hook<extern "C" fn(f64, f64, u32) -> *mut ObjectHeader> = Hook::empty();
pub(crate) extern "C" fn js_event_new(
    type_value: f64,
    options: f64,
    argc: u32,
) -> *mut ObjectHeader {
    JS_EVENT_NEW.get().map_or_else(
        || std::ptr::null_mut(),
        |run| run(type_value, options, argc),
    )
}
static JS_EVENT_SUBCLASS_INIT: Hook<extern "C" fn(f64, f64, f64, u32, u32) -> f64> = Hook::empty();
pub(crate) extern "C" fn js_event_subclass_init(
    this_value: f64,
    type_value: f64,
    options: f64,
    argc: u32,
    is_custom: u32,
) -> f64 {
    JS_EVENT_SUBCLASS_INIT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(this_value, type_value, options, argc, is_custom),
    )
}
static JS_EVENT_TARGET_ADD_EVENT_LISTENER: Hook<
    unsafe extern "C" fn(*mut ObjectHeader, *const StringHeader, i64) -> (),
> = Hook::empty();
pub unsafe extern "C" fn js_event_target_add_event_listener(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> () {
    JS_EVENT_TARGET_ADD_EVENT_LISTENER.get().map_or_else(
        || (),
        |run| unsafe { run(target, event_name_ptr, callback_ptr) },
    )
}
static JS_EVENT_TARGET_GET_EVENT_LISTENERS: Hook<
    unsafe extern "C" fn(*mut ObjectHeader, *const StringHeader) -> *mut ArrayHeader,
> = Hook::empty();
pub unsafe extern "C" fn js_event_target_get_event_listeners(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
) -> *mut ArrayHeader {
    JS_EVENT_TARGET_GET_EVENT_LISTENERS.get().map_or_else(
        || std::ptr::null_mut(),
        |run| unsafe { run(target, event_name_ptr) },
    )
}
static JS_EVENT_TARGET_GET_MAX_LISTENERS: Hook<unsafe extern "C" fn(*mut ObjectHeader) -> f64> =
    Hook::empty();
pub unsafe extern "C" fn js_event_target_get_max_listeners(target: *mut ObjectHeader) -> f64 {
    JS_EVENT_TARGET_GET_MAX_LISTENERS.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| unsafe { run(target) },
    )
}
static JS_EVENT_TARGET_IS_EVENT_TARGET: Hook<unsafe extern "C" fn(*const ObjectHeader) -> i32> =
    Hook::empty();
pub unsafe extern "C" fn js_event_target_is_event_target(target: *const ObjectHeader) -> i32 {
    JS_EVENT_TARGET_IS_EVENT_TARGET
        .get()
        .map_or_else(|| 0, |run| unsafe { run(target) })
}
static JS_EVENT_TARGET_NEW: Hook<extern "C" fn() -> *mut ObjectHeader> = Hook::empty();
pub(crate) extern "C" fn js_event_target_new() -> *mut ObjectHeader {
    JS_EVENT_TARGET_NEW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run())
}
static JS_EVENT_TARGET_REMOVE_EVENT_LISTENER: Hook<
    unsafe extern "C" fn(*mut ObjectHeader, *const StringHeader, i64) -> (),
> = Hook::empty();
pub unsafe extern "C" fn js_event_target_remove_event_listener(
    target: *mut ObjectHeader,
    event_name_ptr: *const StringHeader,
    callback_ptr: i64,
) -> () {
    JS_EVENT_TARGET_REMOVE_EVENT_LISTENER.get().map_or_else(
        || (),
        |run| unsafe { run(target, event_name_ptr, callback_ptr) },
    )
}
static JS_EVENT_TARGET_SET_MAX_LISTENERS: Hook<
    unsafe extern "C" fn(*mut ObjectHeader, f64) -> i32,
> = Hook::empty();
pub unsafe extern "C" fn js_event_target_set_max_listeners(
    target: *mut ObjectHeader,
    n: f64,
) -> i32 {
    JS_EVENT_TARGET_SET_MAX_LISTENERS
        .get()
        .map_or_else(|| 0, |run| unsafe { run(target, n) })
}
static JS_EVENT_TARGET_SUBCLASS_INIT: Hook<extern "C" fn(f64, u32) -> f64> = Hook::empty();
pub(crate) extern "C" fn js_event_target_subclass_init(this: f64, kind: u32) -> f64 {
    JS_EVENT_TARGET_SUBCLASS_INIT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(this, kind),
    )
}
static NATIVE_CLASS_NAME: Hook<fn(u32) -> Option<&'static str>> = Hook::empty();
pub(crate) fn native_class_name(class: u32) -> Option<&'static str> {
    NATIVE_CLASS_NAME
        .get()
        .map_or_else(|| None, |run| run(class))
}
static ABORT_SIGNAL_PTR_FROM_VALUE: Hook<fn(f64) -> Option<*mut ObjectHeader>> = Hook::empty();
pub(crate) fn abort_signal_ptr_from_value(value: f64) -> Option<*mut ObjectHeader> {
    ABORT_SIGNAL_PTR_FROM_VALUE
        .get()
        .map_or_else(|| None, |run| run(value))
}
static IS_ABORT_SIGNAL_VALUE: Hook<fn(f64) -> bool> = Hook::empty();
pub(crate) fn is_abort_signal_value(value: f64) -> bool {
    IS_ABORT_SIGNAL_VALUE
        .get()
        .map_or_else(|| false, |run| run(value))
}
static JS_ABORT_CONTROLLER_NEW: Hook<extern "C" fn() -> *mut ObjectHeader> = Hook::empty();
pub extern "C" fn js_abort_controller_new() -> *mut ObjectHeader {
    JS_ABORT_CONTROLLER_NEW
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run())
}
static JS_ABORT_CONTROLLER_SIGNAL: Hook<extern "C" fn(*mut ObjectHeader) -> *mut ObjectHeader> =
    Hook::empty();
pub extern "C" fn js_abort_controller_signal(controller: *mut ObjectHeader) -> *mut ObjectHeader {
    JS_ABORT_CONTROLLER_SIGNAL
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(controller))
}
static JS_ABORT_ERROR_VALUE: Hook<extern "C" fn() -> f64> = Hook::empty();
pub extern "C" fn js_abort_error_value() -> f64 {
    JS_ABORT_ERROR_VALUE
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_ABORT_SIGNAL_ABORT: Hook<extern "C" fn(f64) -> *mut ObjectHeader> = Hook::empty();
pub(crate) extern "C" fn js_abort_signal_abort(reason: f64) -> *mut ObjectHeader {
    JS_ABORT_SIGNAL_ABORT
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(reason))
}
static JS_ABORT_SIGNAL_ADD_LISTENER: Hook<extern "C" fn(*mut ObjectHeader, f64, f64) -> ()> =
    Hook::empty();
pub extern "C" fn js_abort_signal_add_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) -> () {
    JS_ABORT_SIGNAL_ADD_LISTENER
        .get()
        .map_or_else(|| (), |run| run(signal, event_type, listener))
}
static JS_ABORT_SIGNAL_ANY: Hook<
    extern "C" fn(*mut crate::array::ArrayHeader) -> *mut ObjectHeader,
> = Hook::empty();
pub(crate) extern "C" fn js_abort_signal_any(
    signals_arr: *mut crate::array::ArrayHeader,
) -> *mut ObjectHeader {
    JS_ABORT_SIGNAL_ANY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(signals_arr))
}
static JS_ABORT_SIGNAL_IS_ABORTED: Hook<extern "C" fn(*mut ObjectHeader) -> i32> = Hook::empty();
pub extern "C" fn js_abort_signal_is_aborted(signal: *mut ObjectHeader) -> i32 {
    JS_ABORT_SIGNAL_IS_ABORTED
        .get()
        .map_or_else(|| 0, |run| run(signal))
}
static JS_ABORT_SIGNAL_LISTENER_COUNT: Hook<extern "C" fn(*mut ObjectHeader) -> f64> =
    Hook::empty();
pub extern "C" fn js_abort_signal_listener_count(signal: *mut ObjectHeader) -> f64 {
    JS_ABORT_SIGNAL_LISTENER_COUNT.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(signal),
    )
}
static JS_ABORT_SIGNAL_LISTENERS_COPY: Hook<
    extern "C" fn(*mut ObjectHeader) -> *mut crate::array::ArrayHeader,
> = Hook::empty();
pub extern "C" fn js_abort_signal_listeners_copy(
    signal: *mut ObjectHeader,
) -> *mut crate::array::ArrayHeader {
    JS_ABORT_SIGNAL_LISTENERS_COPY
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(signal))
}
static JS_ABORT_SIGNAL_REMOVE_LISTENER: Hook<extern "C" fn(*mut ObjectHeader, f64, f64) -> ()> =
    Hook::empty();
pub extern "C" fn js_abort_signal_remove_listener(
    signal: *mut ObjectHeader,
    event_type: f64,
    listener: f64,
) -> () {
    JS_ABORT_SIGNAL_REMOVE_LISTENER
        .get()
        .map_or_else(|| (), |run| run(signal, event_type, listener))
}
static JS_ABORT_SIGNAL_RESOLVE_PTR: Hook<extern "C" fn(f64) -> *mut ObjectHeader> = Hook::empty();
pub extern "C" fn js_abort_signal_resolve_ptr(value: f64) -> *mut ObjectHeader {
    JS_ABORT_SIGNAL_RESOLVE_PTR
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(value))
}
static JS_ABORT_SIGNAL_TIMEOUT: Hook<extern "C" fn(f64) -> *mut ObjectHeader> = Hook::empty();
pub(crate) extern "C" fn js_abort_signal_timeout(ms: f64) -> *mut ObjectHeader {
    JS_ABORT_SIGNAL_TIMEOUT
        .get()
        .map_or_else(|| std::ptr::null_mut(), |run| run(ms))
}
static PROTOTYPE_INSTALL_CONSTRUCTOR_CONSTANTS: Hook<fn(&str, *mut ObjectHeader) -> ()> =
    Hook::empty();
pub(crate) fn prototype_install_constructor_constants(
    name: &str,
    constructor: *mut ObjectHeader,
) -> () {
    PROTOTYPE_INSTALL_CONSTRUCTOR_CONSTANTS
        .get()
        .map_or_else(|| (), |run| run(name, constructor))
}
static STATE_TRANSFER_FAMILY: Hook<fn(*mut ObjectHeader) -> Option<&'static str>> = Hook::empty();
pub(crate) fn state_transfer_family(owner: *mut ObjectHeader) -> Option<&'static str> {
    STATE_TRANSFER_FAMILY
        .get()
        .map_or_else(|| None, |run| run(owner))
}
static REGISTER_ROOTS: Hook<fn()> = Hook::empty();
pub(crate) fn register_installed_roots() {
    if let Some(register) = REGISTER_ROOTS.peek() {
        register();
    }
}
fn register_roots() {
    crate::gc::gc_register_mutable_root_scanner_named_once(
        "crate::event_target::state::scan_roots",
        crate::event_target::state::scan_roots,
    );
}

pub(crate) fn install() {
    crate::perry_install_once!({
        ABORT_DOM_EXCEPTION_VALUE.set(crate::event_target::abort_dom_exception_value);
        INSTALL_WEB_EVENT_PROTO_METHODS.set(crate::event_target::install_web_event_proto_methods);
        IS_DOM_EXCEPTION_ERROR.set(crate::event_target::is_dom_exception_error);
        IS_DOM_EXCEPTION_OBJECT.set(crate::event_target::is_dom_exception_object);
        JS_CUSTOM_EVENT_NEW.set(crate::event_target::js_custom_event_new);
        JS_DOM_EXCEPTION_NEW.set(crate::event_target::js_dom_exception_new);
        JS_DOM_EXCEPTION_SUBCLASS_INIT.set(crate::event_target::js_dom_exception_subclass_init);
        JS_EVENT_NEW.set(crate::event_target::js_event_new);
        JS_EVENT_SUBCLASS_INIT.set(crate::event_target::js_event_subclass_init);
        JS_EVENT_TARGET_ADD_EVENT_LISTENER
            .set(crate::event_target::js_event_target_add_event_listener);
        JS_EVENT_TARGET_GET_EVENT_LISTENERS
            .set(crate::event_target::js_event_target_get_event_listeners);
        JS_EVENT_TARGET_GET_MAX_LISTENERS
            .set(crate::event_target::js_event_target_get_max_listeners);
        JS_EVENT_TARGET_IS_EVENT_TARGET.set(crate::event_target::js_event_target_is_event_target);
        JS_EVENT_TARGET_NEW.set(crate::event_target::js_event_target_new);
        JS_EVENT_TARGET_REMOVE_EVENT_LISTENER
            .set(crate::event_target::js_event_target_remove_event_listener);
        JS_EVENT_TARGET_SET_MAX_LISTENERS
            .set(crate::event_target::js_event_target_set_max_listeners);
        JS_EVENT_TARGET_SUBCLASS_INIT.set(crate::event_target::js_event_target_subclass_init);
        NATIVE_CLASS_NAME.set(crate::event_target::native_class_name);
        ABORT_SIGNAL_PTR_FROM_VALUE.set(crate::event_target::abort::abort_signal_ptr_from_value);
        IS_ABORT_SIGNAL_VALUE.set(crate::event_target::abort::is_abort_signal_value);
        JS_ABORT_CONTROLLER_NEW.set(crate::event_target::abort::js_abort_controller_new);
        JS_ABORT_CONTROLLER_SIGNAL.set(crate::event_target::abort::js_abort_controller_signal);
        JS_ABORT_ERROR_VALUE.set(crate::event_target::abort::js_abort_error_value);
        JS_ABORT_SIGNAL_ABORT.set(crate::event_target::abort::js_abort_signal_abort);
        JS_ABORT_SIGNAL_ADD_LISTENER.set(crate::event_target::abort::js_abort_signal_add_listener);
        JS_ABORT_SIGNAL_ANY.set(crate::event_target::abort::js_abort_signal_any);
        JS_ABORT_SIGNAL_IS_ABORTED.set(crate::event_target::abort::js_abort_signal_is_aborted);
        JS_ABORT_SIGNAL_LISTENER_COUNT
            .set(crate::event_target::abort::js_abort_signal_listener_count);
        JS_ABORT_SIGNAL_LISTENERS_COPY
            .set(crate::event_target::abort::js_abort_signal_listeners_copy);
        JS_ABORT_SIGNAL_REMOVE_LISTENER
            .set(crate::event_target::abort::js_abort_signal_remove_listener);
        JS_ABORT_SIGNAL_RESOLVE_PTR.set(crate::event_target::abort::js_abort_signal_resolve_ptr);
        JS_ABORT_SIGNAL_TIMEOUT.set(crate::event_target::abort::js_abort_signal_timeout);
        PROTOTYPE_INSTALL_CONSTRUCTOR_CONSTANTS
            .set(crate::event_target::prototype::install_constructor_constants);
        STATE_TRANSFER_FAMILY.set(crate::event_target::state::transfer_family);
        REGISTER_ROOTS.set(register_roots);
    });
    register_installed_roots();
}
