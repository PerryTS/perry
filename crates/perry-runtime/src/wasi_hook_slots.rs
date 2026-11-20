//! WASI probes and operations filled by the existing WASI installer.
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;
static ATTACH_WASI_CONSTRUCTOR_PROTOTYPE: Hook<fn(f64) -> ()> = Hook::empty();
pub(crate) fn attach_wasi_constructor_prototype(constructor_value: f64) -> () {
    ATTACH_WASI_CONSTRUCTOR_PROTOTYPE
        .get()
        .map_or_else(|| (), |run| run(constructor_value))
}
static CALL_IMPORT: Hook<fn(f64, &[u8], &[u64], &[u8]) -> Option<i32>> = Hook::empty();
pub(crate) fn call_import(
    callback: f64,
    kinds: &[u8],
    args: &[u64],
    results: &[u8],
) -> Option<i32> {
    CALL_IMPORT
        .get()
        .map_or_else(|| None, |run| run(callback, kinds, args, results))
}
static EMIT_WASI_STATIC_WARNING: Hook<fn() -> ()> = Hook::empty();
pub(crate) fn emit_wasi_static_warning() -> () {
    EMIT_WASI_STATIC_WARNING
        .get()
        .map_or_else(|| (), |run| run())
}
static ENSURE_WASI_PROTOTYPE_FOR_SUBCLASS: Hook<fn() -> ()> = Hook::empty();
pub(crate) fn ensure_wasi_prototype_for_subclass() -> () {
    ENSURE_WASI_PROTOTYPE_FOR_SUBCLASS
        .get()
        .map_or_else(|| (), |run| run())
}
static IS_EXIT_MARKER: Hook<fn(f64) -> bool> = Hook::empty();
pub(crate) fn is_exit_marker(value: f64) -> bool {
    IS_EXIT_MARKER.get().map_or_else(|| false, |run| run(value))
}
static IS_WASI_IMPORT_OBJECT: Hook<fn(*const ObjectHeader) -> bool> = Hook::empty();
pub(crate) fn is_wasi_import_object(obj: *const ObjectHeader) -> bool {
    IS_WASI_IMPORT_OBJECT
        .get()
        .map_or_else(|| false, |run| run(obj))
}
static IS_WASI_INSTANCE: Hook<fn(f64) -> bool> = Hook::empty();
pub(crate) fn is_wasi_instance(value: f64) -> bool {
    IS_WASI_INSTANCE
        .get()
        .map_or_else(|| false, |run| run(value))
}
static JS_WASI_CONSTRUCTOR_CALL: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_wasi_constructor_call(_options: f64) -> f64 {
    JS_WASI_CONSTRUCTOR_CALL.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(_options),
    )
}
static JS_WASI_INIT_SUBCLASS: Hook<unsafe fn(f64, f64) -> ()> = Hook::empty();
pub(crate) unsafe fn js_wasi_init_subclass(this_box: f64, options: f64) -> () {
    JS_WASI_INIT_SUBCLASS
        .get()
        .map_or_else(|| (), |run| unsafe { run(this_box, options) })
}
static JS_WASI_NEW: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) fn js_wasi_new(options: f64) -> f64 {
    JS_WASI_NEW.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(options),
    )
}
static PENDING_EXIT_CODE: Hook<fn() -> Option<i32>> = Hook::empty();
pub(crate) fn pending_exit_code() -> Option<i32> {
    PENDING_EXIT_CODE.get().map_or_else(|| None, |run| run())
}
pub(crate) fn install() {
    crate::perry_install_once!({
        ATTACH_WASI_CONSTRUCTOR_PROTOTYPE.set(crate::wasi::attach_wasi_constructor_prototype);
        CALL_IMPORT.set(crate::wasi::call_import);
        EMIT_WASI_STATIC_WARNING.set(crate::wasi::emit_wasi_static_warning);
        ENSURE_WASI_PROTOTYPE_FOR_SUBCLASS.set(crate::wasi::ensure_wasi_prototype_for_subclass);
        IS_EXIT_MARKER.set(crate::wasi::is_exit_marker);
        IS_WASI_IMPORT_OBJECT.set(crate::wasi::is_wasi_import_object);
        IS_WASI_INSTANCE.set(crate::wasi::is_wasi_instance);
        JS_WASI_CONSTRUCTOR_CALL.set(crate::wasi::js_wasi_constructor_call);
        JS_WASI_INIT_SUBCLASS.set(crate::wasi::js_wasi_init_subclass);
        JS_WASI_NEW.set(crate::wasi::js_wasi_new);
        PENDING_EXIT_CODE.set(crate::wasi::pending_exit_code);
    });
}
