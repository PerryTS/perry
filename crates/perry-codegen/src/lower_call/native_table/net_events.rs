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
        args: &[],
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
        ret: NR_PTR,
    },
    // Factory: `tls.connect(host, port, servername, verify)` opens plain TCP
    // then runs a full TLS handshake before firing 'connect'. Returns a Socket
    // handle that behaves identically to one produced by net.createConnection
    // (same write/end/destroy/on surface).
    NativeModSig {
        module: "tls",
        has_receiver: false,
        method: "connect",
        class_filter: None,
        runtime: "js_tls_connect",
        args: &[NA_STR, NA_F64, NA_STR, NA_F64],
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
        args: &[NA_F64, NA_PTR],
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
        ret: NR_PTR,
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
    // ========== node:stream — Readable.from(iterable) (#631) ==========
    // The other stream constructors (`new Readable(opts)` etc.) are wired
    // via `lower_builtin_new` so the codegen can carry the closure-fields
    // ObjectHeader with NaN-boxed POINTER_TAG; they never reach this
    // table. `Readable.from` is a static factory call surfaced as
    // `Readable.from(...)` → `stream.from(...)`, so it lives here.
    NativeModSig {
        module: "stream",
        has_receiver: false,
        method: "from",
        class_filter: None,
        runtime: "js_node_stream_readable_from",
        args: &[NA_F64],
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
        method: "duplexPair",
        class_filter: None,
        runtime: "js_node_stream_duplex_pair",
        args: &[NA_F64],
        ret: NR_F64,
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
        has_receiver: true,
        method: "on",
        class_filter: None,
        runtime: "js_event_emitter_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "emit",
        class_filter: None,
        runtime: "js_event_emitter_emit",
        args: &[NA_STR, NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "removeListener",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "removeAllListeners",
        class_filter: None,
        runtime: "js_event_emitter_remove_all_listeners",
        args: &[NA_STR],
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
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "addListener",
        class_filter: None,
        runtime: "js_event_emitter_on",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "prependListener",
        class_filter: None,
        runtime: "js_event_emitter_prepend_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "prependOnceListener",
        class_filter: None,
        runtime: "js_event_emitter_prepend_once_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "off",
        class_filter: None,
        runtime: "js_event_emitter_remove_listener",
        args: &[NA_STR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_event_emitter_listener_count",
        args: &[NA_STR, NA_PTR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "listeners",
        class_filter: None,
        runtime: "js_event_emitter_listeners",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: true,
        method: "rawListeners",
        class_filter: None,
        runtime: "js_event_emitter_raw_listeners",
        args: &[NA_STR],
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
        args: &[NA_PTR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "on",
        class_filter: None,
        runtime: "js_events_on",
        args: &[NA_PTR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "addAbortListener",
        class_filter: None,
        runtime: "js_events_add_abort_listener",
        args: &[NA_PTR, NA_PTR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "getEventListeners",
        class_filter: None,
        runtime: "js_events_get_event_listeners",
        args: &[NA_PTR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "listenerCount",
        class_filter: None,
        runtime: "js_events_listener_count",
        args: &[NA_PTR, NA_STR],
        ret: NR_F64,
    },
    NativeModSig {
        module: "events",
        has_receiver: false,
        method: "getMaxListeners",
        class_filter: None,
        runtime: "js_events_get_max_listeners",
        args: &[NA_PTR],
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
