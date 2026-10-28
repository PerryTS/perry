//! Messaging constructors and prototype operations selected by installer.
use crate::closure::ClosureHeader;
use crate::feature_hooks::Hook;
use crate::object::ObjectHeader;
static JS_BROADCAST_CHANNEL_CONSTRUCTOR_CALL_ERROR: Hook<
    extern "C" fn(*const ClosureHeader, crate::closure::JsThis, f64) -> f64,
> = Hook::empty();
pub(crate) extern "C" fn js_broadcast_channel_constructor_call_error(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
    _arg: f64,
) -> f64 {
    JS_BROADCAST_CHANNEL_CONSTRUCTOR_CALL_ERROR
        .get()
        .map_or_else(
            || f64::from_bits(crate::value::TAG_UNDEFINED),
            |run| run(_closure, _this, _arg),
        )
}
static JS_BROADCAST_CHANNEL_NEW: Hook<extern "C" fn(f64) -> f64> = Hook::empty();
pub(crate) extern "C" fn js_broadcast_channel_new(name: f64) -> f64 {
    JS_BROADCAST_CHANNEL_NEW.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(name),
    )
}
static JS_MESSAGE_CHANNEL_CONSTRUCTOR_CALL_ERROR: Hook<
    extern "C" fn(*const ClosureHeader, crate::closure::JsThis) -> f64,
> = Hook::empty();
pub(crate) extern "C" fn js_message_channel_constructor_call_error(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    JS_MESSAGE_CHANNEL_CONSTRUCTOR_CALL_ERROR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(_closure, _this),
    )
}
static JS_MESSAGE_CHANNEL_NEW: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) extern "C" fn js_message_channel_new() -> f64 {
    JS_MESSAGE_CHANNEL_NEW
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static JS_MESSAGE_PORT_CONSTRUCTOR_CALL_ERROR: Hook<
    extern "C" fn(*const ClosureHeader, crate::closure::JsThis) -> f64,
> = Hook::empty();
pub(crate) extern "C" fn js_message_port_constructor_call_error(
    _closure: *const ClosureHeader,
    _this: crate::closure::JsThis,
) -> f64 {
    JS_MESSAGE_PORT_CONSTRUCTOR_CALL_ERROR.get().map_or_else(
        || f64::from_bits(crate::value::TAG_UNDEFINED),
        |run| run(_closure, _this),
    )
}
static JS_MESSAGE_PORT_CONSTRUCTOR_ERROR: Hook<extern "C" fn() -> f64> = Hook::empty();
pub(crate) extern "C" fn js_message_port_constructor_error() -> f64 {
    JS_MESSAGE_PORT_CONSTRUCTOR_ERROR
        .get()
        .map_or_else(|| f64::from_bits(crate::value::TAG_UNDEFINED), |run| run())
}
static POPULATE_MESSAGING_PROTOTYPE: Hook<fn(&str, *mut ObjectHeader, f64) -> ()> = Hook::empty();
pub(crate) fn populate_messaging_prototype(
    builtin_name: &str,
    proto: *mut ObjectHeader,
    ctor: f64,
) -> () {
    POPULATE_MESSAGING_PROTOTYPE
        .get()
        .map_or_else(|| (), |run| run(builtin_name, proto, ctor))
}
pub(crate) fn install() {
    crate::perry_install_once!({
        crate::messaging::install_root_tls_resolver();
        JS_BROADCAST_CHANNEL_CONSTRUCTOR_CALL_ERROR
            .set(crate::messaging::js_broadcast_channel_constructor_call_error);
        JS_BROADCAST_CHANNEL_NEW.set(crate::messaging::js_broadcast_channel_new);
        JS_MESSAGE_CHANNEL_CONSTRUCTOR_CALL_ERROR
            .set(crate::messaging::js_message_channel_constructor_call_error);
        JS_MESSAGE_CHANNEL_NEW.set(crate::messaging::js_message_channel_new);
        JS_MESSAGE_PORT_CONSTRUCTOR_CALL_ERROR
            .set(crate::messaging::js_message_port_constructor_call_error);
        JS_MESSAGE_PORT_CONSTRUCTOR_ERROR.set(crate::messaging::js_message_port_constructor_error);
        POPULATE_MESSAGING_PROTOTYPE.set(crate::messaging::populate_messaging_prototype);
    });
}

type TlsResolver = fn(u32) -> Result<*mut u8, crate::tls_hot::AccessError>;
static ROOT_TLS_RESOLVER: Hook<TlsResolver> = Hook::empty();
pub(crate) fn resolve_root_guard(idx: u32) -> Result<*mut u8, crate::tls_hot::AccessError> {
    ROOT_TLS_RESOLVER
        .get()
        .expect("messaging installer must precede port root storage")(idx)
}
pub(crate) fn install_root_resolver(resolve: TlsResolver) {
    ROOT_TLS_RESOLVER.set(resolve);
}
