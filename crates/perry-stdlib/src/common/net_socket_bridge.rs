#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
#[inline]
fn nanbox_small_handle(handle: i64) -> f64 {
    perry_runtime::value::js_nanbox_pointer(handle as i64)
}

#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
#[inline]
unsafe fn bind_class_method(handle: i64, name_bytes: &'static [u8]) -> f64 {
    extern "C" {
        fn js_class_method_bind(
            instance: f64,
            method_name_ptr: *const u8,
            method_name_len: usize,
        ) -> f64;
    }
    js_class_method_bind(
        nanbox_small_handle(handle),
        name_bytes.as_ptr(),
        name_bytes.len(),
    )
}

#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
fn external_socket_method_name(property_name: &str) -> Option<&'static [u8]> {
    match property_name {
        "connect" => Some(b"connect"),
        "write" => Some(b"write"),
        "end" => Some(b"end"),
        "destroy" => Some(b"destroy"),
        "on" => Some(b"on"),
        "addListener" => Some(b"addListener"),
        "once" => Some(b"once"),
        "off" => Some(b"off"),
        "removeListener" => Some(b"removeListener"),
        "removeAllListeners" => Some(b"removeAllListeners"),
        "listenerCount" => Some(b"listenerCount"),
        "eventNames" => Some(b"eventNames"),
        "listeners" => Some(b"listeners"),
        "rawListeners" => Some(b"rawListeners"),
        "address" => Some(b"address"),
        "resetAndDestroy" => Some(b"resetAndDestroy"),
        "setNoDelay" => Some(b"setNoDelay"),
        "setKeepAlive" => Some(b"setKeepAlive"),
        "setTimeout" => Some(b"setTimeout"),
        "setEncoding" => Some(b"setEncoding"),
        "pause" => Some(b"pause"),
        "read" => Some(b"read"),
        "resume" => Some(b"resume"),
        "ref" => Some(b"ref"),
        "unref" => Some(b"unref"),
        "cork" => Some(b"cork"),
        "uncork" => Some(b"uncork"),
        "setDefaultEncoding" => Some(b"setDefaultEncoding"),
        "upgradeToTLS" => Some(b"upgradeToTLS"),
        _ => None,
    }
}

#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(super) unsafe fn bind_net_socket_property(handle: i64, property_name: &str) -> Option<f64> {
    {
        if js_ext_net_is_socket_handle(handle) != 0 {
            if let Some(name_bytes) = external_socket_method_name(property_name) {
                return Some(bind_class_method(handle, name_bytes));
            }
        }
    }

    None
}

/// Registers perry-ext-net's socket-handle probe with the runtime, from the
/// `external-net-pump` install (perry-ext-net also registers its own probe,
/// `gc_roots.rs`; perry-stdlib's bundled `net` probe was deleted in tokio
/// lane L4).
#[cfg(all(
    feature = "external-net-pump",
    not(target_os = "ios"),
    not(target_os = "android")
))]
pub(super) unsafe fn register_net_socket_handle_probe() {
    {
        extern "C" {
            fn js_register_net_socket_handle_probe(f: unsafe extern "C" fn(i64) -> bool);
        }
        unsafe extern "C" fn external_net_socket_probe(handle: i64) -> bool {
            js_ext_net_is_socket_handle(handle) != 0
        }
        js_register_net_socket_handle_probe(external_net_socket_probe);
    }
}

#[allow(unused_imports)]
use crate::common::wrapper_imports::*;
