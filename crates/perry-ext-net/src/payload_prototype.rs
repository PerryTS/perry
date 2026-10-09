//! Ordinary Socket and Server prototype methods. There is no handle dispatcher.

use super::{payload_socket as socket, payload_transport as p};
use perry_ffi::native_payload::{PayloadMiss, PayloadPrototype};
use perry_ffi::{JsThis, JsValue, RawClosureHeader, TransientRootScope};
use std::ffi::c_void;

pub(crate) fn throw_miss<T>(miss: PayloadMiss) -> T {
    extern "C" {
        fn js_typeerror_new(message: *mut perry_ffi::StringHeader) -> *mut u8;
    }
    #[cfg(panic = "abort")]
    extern "C" {
        fn js_throw(value: f64) -> !;
    }
    #[cfg(not(panic = "abort"))]
    extern "C-unwind" {
        fn js_throw(value: f64) -> !;
    }
    let scope = TransientRootScope::enter();
    let message = scope.root_addr(
        perry_ffi::alloc_string(match miss {
            PayloadMiss::Foreign => "Illegal receiver",
            PayloadMiss::Closed => "Native payload is closed",
        })
        .as_raw() as i64,
    );
    unsafe {
        js_throw(f64::from_bits(
            JsValue::from_object_ptr(js_typeerror_new(
                message.get() as *mut perry_ffi::StringHeader
            ))
            .bits(),
        ))
    }
}

extern "C" {
    fn js_node_stream_method_on(owner: i64, event: f64, callback: f64) -> f64;
    fn js_node_stream_method_once(owner: i64, event: f64, callback: f64) -> f64;
    fn js_node_stream_method_prepend_listener(owner: i64, event: f64, callback: f64) -> f64;
    fn js_node_stream_method_prepend_once_listener(owner: i64, event: f64, callback: f64) -> f64;
}

macro_rules! method0 {
    ($name:ident, $body:expr) => {
        unsafe extern "C" fn $name(_: *const RawClosureHeader, this: JsThis) -> f64 {
            $body(this.as_f64())
        }
    };
}
macro_rules! method1 {
    ($name:ident, $body:expr) => {
        unsafe extern "C" fn $name(_: *const RawClosureHeader, this: JsThis, a: f64) -> f64 {
            $body(this.as_f64(), a)
        }
    };
}
macro_rules! method3 {
    ($name:ident, $body:expr) => {
        unsafe extern "C" fn $name(
            _: *const RawClosureHeader,
            this: JsThis,
            a: f64,
            b: f64,
            c: f64,
        ) -> f64 {
            $body(this.as_f64(), a, b, c)
        }
    };
}
method3!(connect, socket::connect);
method3!(write, socket::write);
method3!(end, socket::end);
method1!(destroy, socket::destroy);
method1!(read, socket::read);
method0!(cork, |owner| socket::cork(owner, false));
method0!(uncork, |owner| socket::cork(owner, true));
method0!(reference, |owner| socket::set_ref(owner, true));
method0!(unreference, |owner| socket::set_ref(owner, false));
method0!(pause, |owner| socket::set_paused(owner, true));
method0!(resume, |owner| socket::set_paused(owner, false));
method0!(reset_destroy, |owner| socket::destroy(
    owner,
    p::undefined()
));
method0!(address, socket::address);
method1!(encoding, socket::set_encoding);
method1!(tos, socket::set_tos);
method0!(get_tos, |owner| socket::get(owner, "typeOfService"));
method3!(listen, super::payload_server::listen);
method1!(close_server, super::payload_server::close);
method0!(server_address, super::payload_server::address);
method1!(connections, super::payload_server::get_connections);
method0!(server_reference, |owner| super::payload_server::set_ref(
    owner, true
));
method0!(server_unreference, |owner| super::payload_server::set_ref(
    owner, false
));
method3!(keep_alive, |owner, _, _, _| {
    socket::link(owner);
    owner
});
method1!(no_delay, |owner, _| {
    socket::link(owner);
    owner
});

unsafe extern "C" fn timeout(
    _: *const RawClosureHeader,
    this: JsThis,
    ms: f64,
    callback: f64,
) -> f64 {
    socket::set_timeout(this.as_f64(), ms, callback)
}
unsafe extern "C" fn pipe(
    _: *const RawClosureHeader,
    this: JsThis,
    dest: f64,
    options: f64,
) -> f64 {
    super::payload_pipe::pipe(this.as_f64(), dest, options)
}
unsafe extern "C" fn unpipe(_: *const RawClosureHeader, this: JsThis, dest: f64) -> f64 {
    super::payload_pipe::unpipe(this.as_f64(), dest)
}

macro_rules! add_listener {
    ($name:ident, $runtime:ident) => {
        unsafe extern "C" fn $name(
            _: *const RawClosureHeader,
            this: JsThis,
            event: f64,
            callback: f64,
        ) -> f64 {
            let scope = TransientRootScope::enter();
            let owner = scope.root_nanbox(this.as_f64());
            let event = scope.root_nanbox(event);
            let callback = scope.root_nanbox(callback);
            socket::link(owner.get());
            $runtime(p::raw_owner(owner.get()), event.get(), callback.get());
            if crate::jsvalue_to_owned_string(event.get()).as_deref() == Some("data") {
                socket::flow(owner.get());
            }
            owner.get()
        }
    };
}
add_listener!(on, js_node_stream_method_on);
add_listener!(once, js_node_stream_method_once);
add_listener!(prepend, js_node_stream_method_prepend_listener);
add_listener!(prepend_once, js_node_stream_method_prepend_once_listener);

macro_rules! socket_getters {
    ($($name:ident => $key:literal),+ $(,)?) => { $(method0!($name, |owner| socket::get(owner, $key));)+ };
}
socket_getters!(
    destroyed => "destroyed", connecting => "connecting", pending => "pending",
    writable => "writable", readable => "readable", writable_ended => "writableEnded",
    readable_ended => "readableEnded", writable_finished => "writableFinished",
    need_drain => "writableNeedDrain", bytes_read => "bytesRead", bytes_written => "bytesWritten",
    length => "writableLength", buffer_size => "bufferSize", corked => "writableCorked",
    hwm => "writableHighWaterMark", read_hwm => "readableHighWaterMark", timeout_value => "timeout",
    tos_value => "typeOfService", ready_state => "readyState", readable_encoding => "readableEncoding",
    local_address => "localAddress", local_port => "localPort", local_family => "localFamily",
    remote_address => "remoteAddress", remote_port => "remotePort", remote_family => "remoteFamily",
    readable_state => "_readableState", writable_state => "_writableState",
    flowing => "readableFlowing"
);
method0!(is_paused, |owner| socket::get(owner, "isPaused"));
method0!(listening, |owner| super::payload_server::get(
    owner,
    "listening"
));

method0!(tls_encrypted, |owner| p::own_get(
    socket::state(owner),
    "encrypted"
));
method0!(tls_authorized, |owner| p::own_get(
    socket::state(owner),
    "authorized"
));
method0!(tls_servername, |owner| p::own_get(
    socket::state(owner),
    "servername"
));
method0!(tls_alpn, |owner| p::own_get(
    socket::state(owner),
    "alpnProtocol"
));
method0!(tls_auth_error, |owner| p::own_get(
    socket::state(owner),
    "authorizationError"
));
method0!(tls_cipher, |owner| p::own_get(
    socket::state(owner),
    "cipher"
));
method0!(tls_protocol, |owner| {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(socket::state(owner.get()));
    if JsValue::from_bits(p::own_get(state.get(), "destroyed").to_bits()).to_bool() {
        f64::from_bits(JsValue::NULL.bits())
    } else {
        p::own_get(state.get(), "protocol")
    }
});
method0!(tls_session_reused, |owner| {
    let value = p::own_get(socket::state(owner), "sessionSupplied");
    f64::from_bits(JsValue::from_bool(JsValue::from_bits(value.to_bits()).to_bool()).bits())
});
method0!(tls_session, |owner| {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(owner);
    let state = scope.root_nanbox(socket::state(owner.get()));
    let session = scope.root_nanbox(p::own_get(state.get(), "session"));
    if JsValue::from_bits(session.get().to_bits()).is_pointer() {
        return session.get();
    }
    if !JsValue::from_bits(p::own_get(state.get(), "tlsConnected").to_bits()).to_bool()
        || JsValue::from_bits(p::own_get(state.get(), "destroyed").to_bits()).to_bool()
    {
        return p::undefined();
    }
    let der = p::own_get(state.get(), "peerCertificateDer");
    let bytes = crate::jsvalue_to_socket_bytes(der).unwrap_or_default();
    if bytes.is_empty() {
        p::undefined()
    } else {
        p::boxed_addr(perry_ffi::alloc_buffer(&bytes[..bytes.len().min(64)]) as i64)
    }
});
extern "C" {
    fn js_tls_client_certificate(owner: i64, own: i32, detailed: f64) -> f64;
}
method1!(tls_peer_certificate, |owner, detailed| {
    socket::link(owner);
    unsafe { js_tls_client_certificate(p::raw_owner(owner), 0, detailed) }
});
method0!(tls_own_certificate, |owner| {
    socket::link(owner);
    unsafe { js_tls_client_certificate(p::raw_owner(owner), 1, p::undefined()) }
});
unsafe extern "C" fn tls_upgrade(
    _: *const RawClosureHeader,
    this: JsThis,
    name: f64,
    verify: f64,
) -> f64 {
    let scope = TransientRootScope::enter();
    let owner = scope.root_nanbox(this.as_f64());
    let verify = scope.root_nanbox(verify);
    let name = scope.root_nanbox(name);
    let name = crate::jsvalue_to_owned_string(name.get()).unwrap_or_default();
    let name = scope.root_addr(perry_ffi::alloc_string(&name).as_raw() as i64);
    p::boxed_addr(crate::js_net_socket_upgrade_tls(
        p::raw_owner(owner.get()),
        name.get(),
        if JsValue::from_bits(verify.get().to_bits()).is_undefined() {
            1.0
        } else {
            if JsValue::from_bits(verify.get().to_bits()).to_bool() {
                1.0
            } else {
                0.0
            }
        },
    ) as i64)
}

macro_rules! methods {
    ($proto:ident; $($name:literal => $fn:ident / $arity:tt / $length:literal),+ $(,)?) => {
        $($proto.method($name, perry_ffi::js_function_info!($fn, $arity; with_flags(perry_ffi::FN_BUILTIN)), $length);)+
    };
}
macro_rules! getters {
    ($proto:ident; $($name:literal => $fn:ident),+ $(,)?) => {
        $($proto.getter($name, perry_ffi::js_function_info!($fn, 0; with_flags(perry_ffi::FN_BUILTIN)));)+
    };
}

pub(crate) unsafe extern "C" fn install_socket(raw: *mut c_void) {
    let mut proto = PayloadPrototype::from_raw(raw);
    // The canonical net.Socket constructor already links this prototype to
    // Duplex.prototype. Keep that exact chain and its EventEmitter parent.
    methods!(proto;
        "getProtocol" => tls_protocol/0/0, "getCipher" => tls_cipher/0/0,
        "getPeerCertificate" => tls_peer_certificate/1/0, "getCertificate" => tls_own_certificate/0/0,
        "getSession" => tls_session/0/0, "isSessionReused" => tls_session_reused/0/0,
        "upgradeToTLS" => tls_upgrade/2/2, "connect" => connect/3/0, "write" => write/3/3, "end" => end/3/3,
        "destroy" => destroy/1/1, "resetAndDestroy" => reset_destroy/0/0,
        "read" => read/1/1, "cork" => cork/0/0, "uncork" => uncork/0/0,
        "ref" => reference/0/0, "unref" => unreference/0/0,
        "pause" => pause/0/0, "resume" => resume/0/0, "isPaused" => is_paused/0/0,
        "address" => address/0/0, "setTimeout" => timeout/2/2,
        "setEncoding" => encoding/1/1, "setTypeOfService" => tos/1/1,
        "getTypeOfService" => get_tos/0/0, "setNoDelay" => no_delay/1/0,
        "setKeepAlive" => keep_alive/3/0, "pipe" => pipe/2/2, "unpipe" => unpipe/1/1,
        "on" => on/2/2, "addListener" => on/2/2, "once" => once/2/2,
        "prependListener" => prepend/2/2, "prependOnceListener" => prepend_once/2/2
    );
    getters!(proto;
        "encrypted" => tls_encrypted, "authorized" => tls_authorized,
        "servername" => tls_servername, "alpnProtocol" => tls_alpn,
        "authorizationError" => tls_auth_error, "destroyed" => destroyed, "connecting" => connecting, "pending" => pending,
        "writable" => writable, "readable" => readable, "writableEnded" => writable_ended,
        "readableEnded" => readable_ended, "writableFinished" => writable_finished,
        "writableNeedDrain" => need_drain, "bytesRead" => bytes_read, "bytesWritten" => bytes_written,
        "writableLength" => length, "bufferSize" => buffer_size, "writableCorked" => corked,
        "writableHighWaterMark" => hwm, "readableHighWaterMark" => read_hwm,
        "timeout" => timeout_value, "typeOfService" => tos_value, "readyState" => ready_state,
        "readableEncoding" => readable_encoding, "localAddress" => local_address,
        "localPort" => local_port, "localFamily" => local_family, "remoteAddress" => remote_address,
        "remotePort" => remote_port, "remoteFamily" => remote_family,
        "_readableState" => readable_state, "_writableState" => writable_state,
        "readableFlowing" => flowing
    );
}

pub(crate) unsafe extern "C" fn install_server(raw: *mut c_void) {
    let mut proto = PayloadPrototype::from_raw(raw);
    // net.Server's canonical prototype already inherits EventEmitter.
    methods!(proto; "listen" => listen/3/0, "close" => close_server/1/1,
        "address" => server_address/0/0, "getConnections" => connections/1/1,
        "ref" => server_reference/0/0, "unref" => server_unreference/0/0
    );
    getters!(proto; "listening" => listening);
}
