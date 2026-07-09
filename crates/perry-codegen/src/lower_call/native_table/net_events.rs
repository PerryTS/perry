use super::*;

pub(super) const NET_EVENTS_ROWS: &[NativeModSig] = &[
    // ========== WebSocket (ws) ==========
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "Server",
        class_filter: None,
        runtime: "js_ws_server_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "WebSocket",
        class_filter: None,
        runtime: "js_ws_connect",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_ws_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_I32,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "send",
        class_filter: None,
        runtime: "js_ws_send",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_ws_close",
        args: &[],
        ret: NR_VOID,
    },
    // #6117 — `ws.readyState` data getter (CONNECTING=0 / OPEN=1 /
    // CLOSING=2 / CLOSED=3). Reached as a 0-arg NativeMethodCall via the
    // bare-member-read reroute in perry-hir's `is_native_dispatch_member`.
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "readyState",
        class_filter: None,
        runtime: "js_ws_ready_state",
        args: &[],
        ret: NR_F64,
    },
    // Issue #577 Phase 4 — `("ws", "Client")` instance methods.
    // The wsId delivered to `Server.on('upgrade', (req, wsId, head) => …)`
    // is NaN-boxed POINTER_TAG so unbox_to_i64 (called by the dispatch
    // helper) extracts the original integer ws_id; user code writing
    // `wsId.send("…")` / `wsId.on("message", cb)` / `wsId.close()`
    // dispatches via these class-filtered entries to the dedicated
    // i64-taking Client variants.
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "send",
        class_filter: Some("Client"),
        runtime: "js_ws_send_client_i64",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "close",
        class_filter: Some("Client"),
        runtime: "js_ws_close_client_i64",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "on",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "ws",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Client"),
        runtime: "js_ws_on_client_i64",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    // Server-side helpers — the user receives a client handle as a plain
    // f64 number from `wss.on('connection', (handle) => …)`, then passes
    // it back to these free functions to write/close that specific peer.
    // Without these entries the receiver-less call falls through to the
    // silent stub a few hundred lines down, evaluates the args for side
    // effects, and returns TAG_UNDEFINED — so frames silently never ship
    // (issue #136).
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "sendToClient",
        class_filter: None,
        runtime: "js_ws_send_to_client",
        args: &[NA_F64, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "ws",
        has_receiver: false,
        method: "closeClient",
        class_filter: None,
        runtime: "js_ws_close_client",
        args: &[NA_F64],
        ret: NR_VOID,
    },
    // ========== Raw TCP sockets (net) + TLS ==========
    // Factory: `net.createConnection(...)` / `net.connect(...)` returns
    // a Socket handle. Supports both Node overloads:
    //   - `net.connect(port, host)` — positional
    //   - `net.connect({ host, port }, cb?)` — options object (issue #770)
    // Both args are passed through as `NA_F64` so the runtime sees the
    // raw NaN-boxed bits and can discriminate the overload by tag.
    // Pre-#770 the second arg was `NA_STR`, which silently corrupted the
    // options-object call site: codegen tried to coerce the callback
    // function to a string pointer, the runtime read garbage bytes as
    // the host name, and `getaddrinfo`'s internal `CString::new()`
    // panicked with "file name contained an unexpected NUL byte".
    //
    // HIR lowering at crates/perry-hir/src/lower.rs registers the
    // return value as class "Socket" so subsequent methods dispatch via
    // the class_filter entries below.
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "createConnection",
        class_filter: None,
        runtime: "js_net_socket_connect",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // Factory alias: `net.connect(...)` is the spec'd alias for
    // `net.createConnection(...)`. Pre-issue-#422 only the
    // `createConnection` form was wired; `net.connect(...)` fell through
    // to the receiver-less unknown-method path which returns
    // TAG_UNDEFINED, so user code reading `typeof net.connect(...)`
    // saw `"undefined"` (issue #422 reproducer 3).
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_net_socket_connect",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // `net.createServer` and callable `net.Server` are normally rewritten to
    // `Expr::NetCreateServer` so the one-arg listener shorthand is preserved.
    // These rows cover less-static call sites that still reach the generic
    // native-module dispatcher.
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "createServer",
        class_filter: None,
        runtime: "js_net_create_server",
        args: &[NA_PTR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "Server",
        class_filter: None,
        runtime: "js_net_create_server",
        args: &[NA_PTR, NA_PTR],
        ret: NR_PTR,
    },
    // Constructor: `new net.Socket()` allocates an unconnected socket
    // handle whose TCP connection is deferred until `sock.connect(port,
    // host)` runs. The HIR's `lower_new` arm rewrites `new net.Socket()`
    // (Member callee) to a receiver-less `Expr::NativeMethodCall` so it
    // reaches this dispatch entry; the matching let-stmt registration in
    // `lower.rs` tags the binding as a `("net", "Socket")` native instance
    // so subsequent `sock.connect/.write/.on/.end/.destroy` calls find
    // the class-filtered entries below (issue #422 reproducer 1 + 2).
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "Socket",
        class_filter: None,
        runtime: "js_net_socket_alloc",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "_normalizeArgs",
        class_filter: None,
        runtime: "js_net_normalize_args",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "_createServerHandle",
        class_filter: None,
        runtime: "js_net_create_server_handle_stub",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    // Issue #810/#811 — IP classification helpers + Happy-Eyeballs default
    // accessors. Pure string/global-flag functions, no sockets or I/O.
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isIP",
        class_filter: None,
        runtime: "js_net_is_ip",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isIPv4",
        class_filter: None,
        runtime: "js_net_is_ipv4",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "isIPv6",
        class_filter: None,
        runtime: "js_net_is_ipv6",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "getDefaultAutoSelectFamily",
        class_filter: None,
        runtime: "js_net_get_default_auto_select_family",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "setDefaultAutoSelectFamily",
        class_filter: None,
        runtime: "js_net_set_default_auto_select_family",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "getDefaultAutoSelectFamilyAttemptTimeout",
        class_filter: None,
        runtime: "js_net_get_default_auto_select_family_attempt_timeout",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: false,
        method: "setDefaultAutoSelectFamilyAttemptTimeout",
        class_filter: None,
        runtime: "js_net_set_default_auto_select_family_attempt_timeout",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // Instance method: `sock.connect(port, host)` initiates the deferred
    // TCP connection on a `new net.Socket()`-allocated handle. Twin of
    // the `createConnection` factory above — both end up in the same
    // tokio task body via `run_socket_task`.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "connect",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_method_connect",
        args: &[NA_F64, NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "write",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_write",
        // Issue #1131 — pass the full NaN-boxed JS value (NA_JSV) so
        // the runtime can probe Buffer-vs-string-vs-number and read
        // through the correct header layout. NA_PTR pre-stripped the
        // tag, so `sock.write("ping")` handed the runtime a bare
        // StringHeader pointer that it reinterpreted as a
        // BufferHeader → garbage on the wire.
        args: &[NA_JSV],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "end",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_end",
        // Issue #1852 — `socket.end([data])` writes the optional final
        // chunk before half-closing. NA_JSV carries the full NaN-boxed
        // value so the runtime can probe Buffer/string/number; the
        // no-arg `socket.end()` form pads this slot with `undefined`.
        args: &[NA_JSV],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "destroy",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_destroy",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "on",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    // Issue #1852 — chainable no-op `net.Socket` option setters. Perry's
    // TCP transport doesn't model Nagle/keep-alive/idle-timeout or read
    // back-pressure yet, but the methods must exist + be callable (pre-fix
    // they threw "x is not a function" — the radar's "value() missing"
    // cluster). Each returns the socket handle so chained forms keep
    // dispatching. `args: &[]` ignores the option arguments.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "setNoDelay",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "setKeepAlive",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "setTimeout",
        class_filter: Some("Socket"),
        // #2013: validate `msecs` (number, non-negative finite); the optional
        // callback is passed through but ignored. Returns the socket handle.
        runtime: "js_net_socket_set_timeout",
        args: &[NA_F64, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "setEncoding",
        class_filter: Some("Socket"),
        // #4973: real setEncoding — switches 'data' delivery to strings.
        runtime: "js_net_socket_set_encoding",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "pause",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "resume",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "ref",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "unref",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "cork",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "uncork",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "setDefaultEncoding",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    // Issue #2131 — `socket.address()` returns the local bind address
    // (`{ address, family, port }`). Captured at connect/accept time and
    // emitted as JSON through `NR_OBJ_FROM_JSON_STR` so user code reads
    // a real object whose `.port` is a number — closes the "undefined.address"
    // cluster on the radar.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "address",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_address",
        args: &[],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    // #2549 — `net.Socket` state / counter / metadata property getters.
    // Socket rows remain generic so they still match in the fallback pass when
    // the HIR preserves a more specific net class filter for nearby accessors
    // such as `Server.listening` and `SocketAddress.address`.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "pending",
        class_filter: None,
        runtime: "js_net_socket_get_pending",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "connecting",
        class_filter: None,
        runtime: "js_net_socket_get_connecting",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "destroyed",
        class_filter: None,
        runtime: "js_net_socket_get_destroyed",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "readyState",
        class_filter: None,
        runtime: "js_net_socket_get_ready_state",
        args: &[],
        ret: NR_STR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "bytesRead",
        class_filter: None,
        runtime: "js_net_socket_get_bytes_read",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "bytesWritten",
        class_filter: None,
        runtime: "js_net_socket_get_bytes_written",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "timeout",
        class_filter: None,
        runtime: "js_net_socket_get_timeout",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "localAddress",
        class_filter: None,
        runtime: "js_net_socket_get_local_address",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "localPort",
        class_filter: None,
        runtime: "js_net_socket_get_local_port",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "localFamily",
        class_filter: None,
        runtime: "js_net_socket_get_local_family",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "remoteAddress",
        class_filter: None,
        runtime: "js_net_socket_get_remote_address",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "remotePort",
        class_filter: None,
        runtime: "js_net_socket_get_remote_port",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "remoteFamily",
        class_filter: None,
        runtime: "js_net_socket_get_remote_family",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "bufferSize",
        class_filter: None,
        runtime: "js_net_socket_get_buffer_size",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "autoSelectFamilyAttemptedAddresses",
        class_filter: None,
        runtime: "js_net_socket_get_auto_select_family_attempted_addresses",
        args: &[],
        ret: NR_F64,
    },
    // Issue #2131 — EventEmitter surface beyond `on`/`addListener`.
    // `once` flags the listener in a side-table so the pump removes it
    // after the next event fires. `off`/`removeListener` delete a
    // specific callback; `removeAllListeners(event?)` drains the event
    // (or every event when called bare). `listenerCount` and
    // `eventNames` round out the introspection surface — Perry
    // pre-#2131 returned "x is not a function" for all of these
    // (radar's "function should not have been called" + "undefined.on"
    // clusters).
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "once",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_once",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "off",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "removeListener",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "removeAllListeners",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_remove_all_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "listenerCount",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_listener_count",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "eventNames",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_event_names",
        args: &[],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    // Issue #2211 — `socket.listeners(event)` / `socket.rawListeners(event)`.
    // Returns a real JS array of registered callbacks; consumers do
    // `socket.listeners('timeout').length` etc. Returned as NR_PTR so
    // the runtime ArrayHeader is NaN-boxed POINTER_TAG. Perry collapses
    // `once` into the listener vector + a removal side-table, so
    // `listeners` and `rawListeners` share an impl — the onceWrapper
    // distinction is unobservable to callers that read the array before
    // any event has fired (which is the shape the radar tests use).
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "listeners",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "rawListeners",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_raw_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // Issue #2131 — `socket.resetAndDestroy()` is the "send RST then
    // destroy" variant; we alias to `destroy()` (FIN-then-close) for
    // now since the connected peer treats both as an abrupt close in
    // the cases the parity radar exercises.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "resetAndDestroy",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_reset_and_destroy",
        args: &[],
        ret: NR_PTR,
    },
    // upgradeToTLS returns a Promise (handle pointer) — await it to wait
    // for the TLS handshake before sending anything over the upgraded stream.
    // upgradeToTLS(servername, verify): verify is 0/1 (number, not bool).
    // verify=1 uses the system trust store + hostname check (sslmode=verify-full);
    // verify=0 accepts any cert (sslmode=require, for local self-signed DBs).
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "upgradeToTLS",
        class_filter: Some("Socket"),
        runtime: "js_net_socket_upgrade_tls",
        args: &[NA_STR, NA_F64],
        ret: NR_PROMISE,
    },
    // Factory: `tls.connect(...)` opens plain TCP then runs a full TLS
    // handshake before firing 'connect'. Returns a Socket handle that behaves
    // identically to one produced by net.createConnection (same
    // write/end/destroy/on surface). All four args pass through as raw
    // NaN-boxed values so the runtime can resolve Node's overloads —
    // `connect(options[, cb])`, `connect(port[, host][, options][, cb])` —
    // plus Perry's legacy positional `connect(host, port, servername, verify)`
    // (#4971: the old NA_STR first arg string-coerced the options object, so
    // `tls.connect({ port })` returned a null handle).
    NativeModSig {
        module: "tls",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_tls_connect",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    // ========== net.Server (issue #1123 followup) ==========
    // Server-side TCP via `net.createServer(...).listen(port, cb)`. The
    // factory itself is wired through `Expr::NetCreateServer` in
    // perry-codegen/src/expr.rs (not this table); the instance methods
    // dispatch here once the let-binding gets registered as
    // `("net", "Server")` in HIR lowering. Shape mirrors
    // `js_node_http_server_*` from perry-ext-http-server (signatures
    // are deliberately parallel so the codegen side reads the same).
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "listen",
        class_filter: Some("Server"),
        runtime: "js_net_server_listen",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "close",
        class_filter: Some("Server"),
        runtime: "js_net_server_close",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "address",
        class_filter: Some("Server"),
        runtime: "js_net_server_address",
        args: &[],
        // Issue #1852 — `js_net_server_address` returns a JSON string
        // (`{"port":…,"address":…,"family":…}` or `"null"`).
        // NR_OBJ_FROM_JSON_STR pipes it through `js_json_parse_or_null`
        // so `server.address().port` reads a real number. Pre-fix the
        // NR_PTR kind NaN-boxed the StringHeader as a POINTER_TAG object,
        // so `.port` came back `undefined` (the radar's "undefined.address"
        // cluster).
        ret: NR_OBJ_FROM_JSON_STR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "on",
        class_filter: Some("Server"),
        runtime: "js_net_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "addListener",
        class_filter: Some("Server"),
        runtime: "js_net_server_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_VOID,
    },
    // Issue #1852 — chainable no-op `net.Server` option setters
    // (`ref`/`unref`/`setTimeout`). Same rationale as the Socket stubs
    // above: callable + chainable, options ignored.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "ref",
        class_filter: Some("Server"),
        runtime: "js_net_server_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "unref",
        class_filter: Some("Server"),
        runtime: "js_net_server_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "setTimeout",
        class_filter: Some("Server"),
        runtime: "js_net_server_noop_self",
        args: &[],
        ret: NR_PTR,
    },
    // Issue #2131 — `net.Server` EventEmitter surface beyond
    // `on`/`addListener`. Same shape as the Socket entries above; the
    // FFI implementations share the underlying `statics::listeners()`
    // map and `statics::once_flags()` side-table.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "once",
        class_filter: Some("Server"),
        runtime: "js_net_server_once",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "off",
        class_filter: Some("Server"),
        runtime: "js_net_server_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "removeListener",
        class_filter: Some("Server"),
        runtime: "js_net_server_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "removeAllListeners",
        class_filter: Some("Server"),
        runtime: "js_net_server_remove_all_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "listenerCount",
        class_filter: Some("Server"),
        runtime: "js_net_server_listener_count",
        args: &[NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "eventNames",
        class_filter: Some("Server"),
        runtime: "js_net_server_event_names",
        args: &[],
        ret: NR_OBJ_FROM_JSON_STR,
    },
    // Issue #2211 — mirror of the Socket listeners/rawListeners surface
    // for net.Server. Same impl since socket and server handles share
    // the `statics::listeners()` map keyed by id.
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "listeners",
        class_filter: Some("Server"),
        runtime: "js_net_server_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "net",
        has_receiver: true,
        method: "rawListeners",
        class_filter: Some("Server"),
        runtime: "js_net_server_raw_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // ========== node:stream — Readable.from / Duplex.from (#631/#1532) ==========
    // The other stream constructors (`new Readable(opts)` etc.) are wired
    // via `lower_builtin_new` so the codegen can carry the closure-fields
    // ObjectHeader with NaN-boxed POINTER_TAG; they never reach this
    // table. Static factory calls surface as `stream.from(...)` with a
    // class filter for constructor imports like `Duplex.from(...)`.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "from",
        class_filter: Some("Duplex"),
        runtime: "js_node_stream_duplex_from_options",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "from",
        class_filter: None,
        runtime: "js_node_stream_readable_from_options",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    // #1534: static introspection helpers `isDisturbed` and
    // `isErrored`. Node exposes them on every stream class (Readable /
    // Writable / Duplex inherit from Stream). For a freshly-constructed
    // stream both return `false`, which matches Perry's stub state
    // (we don't track disturbed/errored bits yet). Consumers that
    // branch on `if (Readable.isErrored(s)) cleanup()` typecheck,
    // don't throw, and skip the error-cleanup arm — which is the
    // honest answer for a stream we never let actually transfer data.
    //
    // The directional helpers `isReadable` / `isWritable` are NOT here:
    // Node's answer depends on the stream type (Readable returns
    // `true` for isReadable + `null` for isWritable; Writable swaps;
    // Duplex says `true` for both). Perry's stub doesn't carry that
    // information at runtime, so a uniform return would lie for at
    // least one case — kept as a follow-up under #1534.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "isDisturbed",
        class_filter: None,
        runtime: "js_node_stream_is_disturbed",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "isErrored",
        class_filter: None,
        runtime: "js_node_stream_is_errored",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // #1534: `Readable.isReadable(s)` / module-level `isReadable(s)`.
    // Now backed by a per-stream readable-direction flag (set at
    // construction) plus the ended/errored bits, so a fresh Readable
    // answers `true` and a Writable answers `false`.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "isReadable",
        class_filter: None,
        runtime: "js_node_stream_is_readable",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // #1746: `stream.isWritable(s)` — mirror of `isReadable` for the
    // writable side. `null` for a stream with no writable side, `false`
    // once it has ended/errored, `true` otherwise.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "isWritable",
        class_filter: None,
        runtime: "js_node_stream_is_writable",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // #2685: top-level stream byte-view helpers and destroyed-state predicate.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "_isArrayBufferView",
        class_filter: None,
        runtime: "js_node_stream_is_array_buffer_view",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "_isUint8Array",
        class_filter: None,
        runtime: "js_node_stream_is_uint8_array",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "_uint8ArrayToBuffer",
        class_filter: None,
        runtime: "js_node_stream_uint8_array_to_buffer",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "isDestroyed",
        class_filter: None,
        runtime: "js_node_stream_is_destroyed",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // #1537: `stream.getDefaultHighWaterMark(objectMode)` /
    // `setDefaultHighWaterMark(objectMode, value)` — the per-mode platform
    // default highWaterMark (65536 byte / 16 objectMode), mutable at runtime.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "getDefaultHighWaterMark",
        class_filter: None,
        runtime: "js_node_stream_get_default_hwm",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "setDefaultHighWaterMark",
        class_filter: None,
        runtime: "js_node_stream_set_default_hwm",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    // #1541: `stream.addAbortSignal(signal, stream)` wires the
    // AbortSignal so that aborting it destroys the stream — and
    // returns the stream for chaining. Perry's stream stubs don't
    // implement destroy/abort propagation yet, but the identity
    // return shape (`r = addAbortSignal(s, r)`) needs to work so
    // feature-detect-and-call sites don't crash. The signal is
    // accepted and ignored; the stream is returned verbatim.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "addAbortSignal",
        class_filter: None,
        runtime: "js_node_stream_add_abort_signal",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    // #1539: `stream.compose(...streams)` chains streams into a
    // composite Duplex; `stream.duplexPair([opts])` returns a paired
    // `[Duplex, Duplex]`. Both return fresh Duplex stubs today
    // (real composition/pairing isn't propagated yet) so consumers
    // that branch on `instanceof Duplex` / typeof get the right
    // shape and don't crash. Variadic args list for `compose` is
    // accepted and ignored.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "compose",
        class_filter: None,
        runtime: "js_node_stream_compose",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "pipeline",
        class_filter: None,
        runtime: "js_node_stream_pipeline",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "finished",
        class_filter: None,
        runtime: "js_node_stream_finished",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "duplexPair",
        class_filter: None,
        runtime: "js_node_stream_duplex_pair",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // #2521: Web-stream interop. Class-specific rows route to real
    // Node/WHATWG adapters; generic rows below remain as shape fallbacks when
    // HIR did not preserve the stream class receiver.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "toWeb",
        class_filter: Some("Readable"),
        runtime: "js_node_stream_readable_to_web",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "toWeb",
        class_filter: Some("Writable"),
        runtime: "js_node_stream_writable_to_web",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "toWeb",
        class_filter: Some("Duplex"),
        runtime: "js_node_stream_duplex_to_web",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "toWeb",
        class_filter: None,
        runtime: "js_node_stream_to_web",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "fromWeb",
        class_filter: Some("Readable"),
        runtime: "js_node_stream_readable_from_web",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "fromWeb",
        class_filter: Some("Writable"),
        runtime: "js_node_stream_writable_from_web",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "fromWeb",
        class_filter: Some("Duplex"),
        runtime: "js_node_stream_duplex_from_web",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "fromWeb",
        class_filter: None,
        runtime: "js_node_stream_from_web",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // Narrow node:stream instance-method wiring used by the current
    // stream/promises stubs. These keep Perry's hidden stream state in sync
    // when typed `new PassThrough()` / `new Writable()` instances call the
    // methods directly so Perry's hidden stream state and EventEmitter
    // listener registry stay in sync on typed stream instances.
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_node_stream_method_on",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "addListener",
        class_filter: None,
        runtime: "js_node_stream_method_on",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "once",
        class_filter: None,
        runtime: "js_node_stream_method_once",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "emit",
        class_filter: None,
        runtime: "js_node_stream_method_emit_args",
        args: &[NA_F64, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "read",
        class_filter: None,
        runtime: "js_node_stream_method_read",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "pipe",
        class_filter: None,
        runtime: "js_node_stream_method_pipe",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "unpipe",
        class_filter: None,
        runtime: "js_node_stream_method_unpipe",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "pause",
        class_filter: None,
        runtime: "js_node_stream_method_pause",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "isPaused",
        class_filter: None,
        runtime: "js_node_stream_method_is_paused",
        args: &[],
        ret: NR_F64,
    },
    // #1539: readable.push(chunk) returns the backpressure signal
    // (`true` below highWaterMark, `false` at/above it).
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "push",
        class_filter: None,
        runtime: "js_node_stream_method_push",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "unshift",
        class_filter: None,
        runtime: "js_node_stream_method_unshift",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // #1539: readableHighWaterMark / writableHighWaterMark property
    // getters (no-arg, lowered as a property read on the instance).
    // Transform can carry distinct readable/writable marks.
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableHighWaterMark",
        class_filter: None,
        runtime: "js_node_stream_method_readable_hwm",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableLength",
        class_filter: None,
        runtime: "js_node_stream_method_readable_length",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableObjectMode",
        class_filter: None,
        runtime: "js_node_stream_method_readable_object_mode",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readable",
        class_filter: None,
        runtime: "js_node_stream_method_readable",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableFlowing",
        class_filter: None,
        runtime: "js_node_stream_method_readable_flowing",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableEnded",
        class_filter: None,
        runtime: "js_node_stream_method_readable_ended",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableEncoding",
        class_filter: None,
        runtime: "js_node_stream_method_readable_encoding",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableHighWaterMark",
        class_filter: None,
        runtime: "js_node_stream_method_writable_hwm",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableLength",
        class_filter: None,
        runtime: "js_node_stream_method_writable_length",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableNeedDrain",
        class_filter: None,
        runtime: "js_node_stream_method_writable_need_drain",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableObjectMode",
        class_filter: None,
        runtime: "js_node_stream_method_writable_object_mode",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableAborted",
        class_filter: None,
        runtime: "js_node_stream_method_readable_aborted",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "closed",
        class_filter: None,
        runtime: "js_node_stream_method_closed",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "errored",
        class_filter: None,
        runtime: "js_node_stream_method_errored",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "readableDidRead",
        class_filter: None,
        runtime: "js_node_stream_method_readable_did_read",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writable",
        class_filter: None,
        runtime: "js_node_stream_method_writable",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableCorked",
        class_filter: None,
        runtime: "js_node_stream_method_writable_corked",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableEnded",
        class_filter: None,
        runtime: "js_node_stream_method_writable_ended",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "writableFinished",
        class_filter: None,
        runtime: "js_node_stream_method_writable_finished",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "allowHalfOpen",
        class_filter: None,
        runtime: "js_node_stream_method_allow_half_open",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "destroyed",
        class_filter: None,
        runtime: "js_node_stream_method_destroyed",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "pause",
        class_filter: None,
        runtime: "js_node_stream_method_pause",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "resume",
        class_filter: None,
        runtime: "js_node_stream_method_resume",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "setEncoding",
        class_filter: None,
        runtime: "js_node_stream_method_set_encoding",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "destroy",
        class_filter: None,
        runtime: "js_node_stream_method_destroy",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "write",
        class_filter: None,
        runtime: "js_node_stream_method_write3",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "end",
        class_filter: None,
        runtime: "js_node_stream_method_end3",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "cork",
        class_filter: None,
        runtime: "js_node_stream_method_cork",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "uncork",
        class_filter: None,
        runtime: "js_node_stream_method_uncork",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "setMaxListeners",
        class_filter: None,
        runtime: "js_node_stream_method_set_max_listeners",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "getMaxListeners",
        class_filter: None,
        runtime: "js_node_stream_method_get_max_listeners",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "prependListener",
        class_filter: None,
        runtime: "js_node_stream_method_prepend_listener",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "prependOnceListener",
        class_filter: None,
        runtime: "js_node_stream_method_prepend_once_listener",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "off",
        class_filter: None,
        runtime: "js_node_stream_method_off",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "removeListener",
        class_filter: None,
        runtime: "js_node_stream_method_remove_listener",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "removeAllListeners",
        class_filter: None,
        runtime: "js_node_stream_method_remove_all_listeners",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "eventNames",
        class_filter: None,
        runtime: "js_node_stream_method_event_names",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_node_stream_method_listener_count",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "listeners",
        class_filter: None,
        runtime: "js_node_stream_method_listeners",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "stream",
        has_receiver: true,
        method: "rawListeners",
        class_filter: None,
        runtime: "js_node_stream_method_raw_listeners",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    // ========== Events ==========
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "EventEmitter",
        class_filter: None,
        runtime: "js_event_emitter_new",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "EventEmitterAsyncResource",
        class_filter: None,
        runtime: "js_event_emitter_async_resource_call",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_event_emitter_on",
        // NA_JSV for event names preserves Node's ToString coercion path.
        // The listener also stays NA_JSV so runtime validation can throw
        // ERR_INVALID_ARG_TYPE for non-functions (#3072).
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "emit",
        class_filter: None,
        runtime: "js_event_emitter_emit",
        args: &[NA_JSV, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "removeListener",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener",
        // NA_JSV (#3072): validate the listener is callable before removal.
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "removeAllListeners",
        class_filter: None,
        runtime: "js_event_emitter_remove_all_listeners",
        args: &[NA_VARARGS],
        ret: NR_PTR,
    },
    // EventEmitter additions (#850) — `once` / `addListener` (alias for
    // `on`) / `prependListener` / `prependOnceListener` / `listenerCount`
    // / `listeners` / `rawListeners` / `eventNames` / `setMaxListeners` /
    // `getMaxListeners`. Pre-fix `.once(...)` and the prepend variants
    // silently no-op'd and the read-only accessors returned undefined.
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "once",
        class_filter: None,
        runtime: "js_event_emitter_once",
        // NA_JSV (#3072): validate the listener is callable.
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "addListener",
        class_filter: None,
        runtime: "js_event_emitter_on",
        // NA_JSV (#3072): validate the listener is callable.
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "prependListener",
        class_filter: None,
        runtime: "js_event_emitter_prepend_listener",
        // NA_JSV (#3072): validate the listener is callable.
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "prependOnceListener",
        class_filter: None,
        runtime: "js_event_emitter_prepend_once_listener",
        // NA_JSV (#3072): validate the listener is callable.
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "off",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener",
        // NA_JSV (#3072): validate the listener is callable.
        args: &[NA_JSV, NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_event_emitter_listener_count",
        args: &[NA_JSV, NA_JSV],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "listeners",
        class_filter: None,
        runtime: "js_event_emitter_listeners",
        args: &[NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "rawListeners",
        class_filter: None,
        runtime: "js_event_emitter_raw_listeners",
        args: &[NA_JSV],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "eventNames",
        class_filter: None,
        runtime: "js_event_emitter_event_names",
        args: &[],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "setMaxListeners",
        class_filter: None,
        runtime: "js_event_emitter_set_max_listeners",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "getMaxListeners",
        class_filter: None,
        runtime: "js_event_emitter_get_max_listeners",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "domain",
        class_filter: None,
        runtime: "js_event_emitter_domain_value",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "asyncId",
        class_filter: Some("EventEmitterAsyncResource"),
        runtime: "js_event_emitter_async_resource_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "triggerAsyncId",
        class_filter: Some("EventEmitterAsyncResource"),
        runtime: "js_event_emitter_async_resource_trigger_async_id",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "asyncResource",
        class_filter: Some("EventEmitterAsyncResource"),
        runtime: "js_event_emitter_async_resource_async_resource",
        args: &[],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "emitDestroy",
        class_filter: Some("EventEmitterAsyncResource"),
        runtime: "js_event_emitter_async_resource_emit_destroy",
        args: &[],
        ret: NR_F64,
    },
    // Module-level helpers (`events.once` / `events.getEventListeners` /
    // `events.listenerCount` / `events.getMaxListeners` /
    // `events.setMaxListeners`). All take the emitter handle as a
    // positional arg, so `has_receiver: false`.
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "once",
        class_filter: None,
        runtime: "js_events_once",
        args: &[NA_F64, NA_STR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "on",
        class_filter: None,
        runtime: "js_events_on",
        args: &[NA_F64, NA_STR, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "addAbortListener",
        class_filter: None,
        runtime: "js_events_add_abort_listener",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "getEventListeners",
        class_filter: None,
        runtime: "js_events_get_event_listeners",
        args: &[NA_F64, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_events_listener_count",
        args: &[NA_F64, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "getMaxListeners",
        class_filter: None,
        runtime: "js_events_get_max_listeners",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "setMaxListeners",
        class_filter: None,
        runtime: "js_events_set_max_listeners",
        args: &[NA_F64, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "init",
        class_filter: None,
        runtime: "js_events_init",
        args: &[],
        ret: NR_F64,
    },
    // ========== StringDecoder (issue #848) ==========
    // The typed-receiver path: `const d = new StringDecoder("utf8");
    // d.write(buf)` enters here because `d` is registered as a native
    // instance in HIR (`("string_decoder", "StringDecoder")`). The
    // any-typed receiver path (`(d as any).write(buf)` /
    // `Map.get("d").write(...)`) goes through HANDLE_METHOD_DISPATCH
    // instead — both routes call the same underlying handle dispatch,
    // so behavior is identical. `NR_F64` because we return a STRING_TAG-
    // NaN-boxed value directly from the FFI (NR_STR would re-NaN-box a
    // raw pointer and produce nonsense).
    NativeModSig {
        module: "string_decoder",
        has_receiver: true,
        method: "write",
        class_filter: Some("StringDecoder"),
        runtime: "js_string_decoder_write",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "string_decoder",
        has_receiver: true,
        method: "end",
        class_filter: Some("StringDecoder"),
        runtime: "js_string_decoder_end",
        args: &[NA_F64],
        ret: NR_F64,
    },
];
