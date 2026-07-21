//! Native-instance member dispatch predicates.
//!
//! Split out of `expr_member.rs` (pure code move).

/// Issue #562 — does `prop` name a stream-API method or property on the
/// given stream module? Used to gate the native-instance property
/// rerouting so subclass-declared fields fall through to regular object
/// property access. Mirrors the methods + accessors hardcoded in
/// `crates/perry-codegen/src/lower_call.rs`'s
/// `module == "<stream_kind>"` arms.
/// Native data-property getters exposed by `blob`-module instances (Blob /
/// File). A bare read of one of these must keep the 0-arg NativeMethodCall
/// dispatch so codegen routes it to the FFI getter (`js_blob_size`, …).
/// Everything else read off a Blob instance is a user-assigned own property
/// and must lower to a plain PropertyGet (see the heap-object guard in
/// `lower_member`).
/// #wall (debug `_.colors` / `_.init`): the inverted-default predicate for the
/// native-instance bare-member-READ block in `lower_member`. Returns `true` only
/// when `(module, class, property)` is a *known* native method/getter that must
/// dispatch through the codegen NATIVE_MODULE_TABLE / per-class FFI as a 0-arg
/// `NativeMethodCall`. Everything else (own properties, library bookkeeping
/// fields, and — critically — any value the HIR mis-tagged native under a module
/// NOT covered by the per-module arms, like the bundled `debug` package's
/// `createDebug`) falls through to a plain `PropertyGet` that READS the stored
/// value instead of INVOKING it.
///
/// This is the consolidated set of the genuine native members that legitimately
/// reach the dispatching arm: the data getters whose values come from FFI
/// (`blob.size`, `res.status`, classic/web-stream state getters), the HTTP
/// per-class FFI getters / methods that are rewritten to `__get_<name>` or
/// dispatched by class_filter, and the events/net method sets. Method-VALUE
/// reads that the per-module arms above already lower to `PropertyGet` are NOT
/// listed here — they keep reading as bound-method values, and the call form
/// `x.method(args)` goes through the call-expression path, unaffected.
pub(crate) fn is_native_dispatch_member(module: &str, class: &str, prop: &str) -> bool {
    match module {
        // Data getters resolved by FFI.
        "blob" => is_blob_getter_name(prop),
        "fetch" => is_fetch_response_getter_name(prop),
        // Web Streams: only the getter list reaches dispatch (methods are
        // PropertyGet bound-method reads).
        "readable_stream"
        | "writable_stream"
        | "transform_stream"
        | "readable_stream_reader"
        | "writable_stream_writer" => {
            is_stream_api_member(module, prop)
                && matches!(
                    prop,
                    "locked"
                        | "desiredSize"
                        | "closed"
                        | "ready"
                        | "readable"
                        | "writable"
                        | "byobRequest"
                )
        }
        // Classic Node streams: state getters dispatch; methods read as values.
        "stream" | "node:stream" => is_classic_stream_getter_name(prop),
        // HTTP / HTTPS: the per-class FFI getters (rewritten to `__get_<name>`)
        // and the runtime/method property sets that dispatch through the
        // NATIVE_MODULE_TABLE class_filter path.
        "http" | "https" => match class {
            "IncomingMessage" => {
                is_http_incoming_message_runtime_property_name(prop)
                    || is_http_incoming_message_method_name(prop)
                    || matches!(prop, "statusCode" | "statusMessage" | "headers")
            }
            "ServerResponse" => {
                is_http_server_response_runtime_property_name(prop)
                    || is_http_server_response_method_name(prop)
            }
            "ClientRequest" => {
                is_http_client_request_method_name(prop)
                    || matches!(
                        prop,
                        "method"
                            | "protocol"
                            | "host"
                            | "path"
                            | "aborted"
                            | "connection"
                            | "destroyed"
                            | "finished"
                            | "maxHeadersCount"
                            | "reusedSocket"
                            | "socket"
                            | "writableEnded"
                            | "writableFinished"
                    )
            }
            "HttpServer" | "HttpsServer" => matches!(
                prop,
                "listening"
                    | "headersTimeout"
                    | "keepAliveTimeout"
                    | "keepAliveTimeoutBuffer"
                    | "requestTimeout"
                    | "timeout"
                    | "maxHeadersCount"
                    | "maxRequestsPerSocket"
            ),
            "Agent" => matches!(prop, "createConnection" | "createSocket"),
            _ => true,
        },
        // #6117 — ws client instances: `readyState` is a native data getter
        // (npm ws: CONNECTING=0 / OPEN=1 / CLOSING=2 / CLOSED=3) dispatched
        // to `js_ws_ready_state`. Server instances expose no readyState, and
        // every other bare member read stays a plain PropertyGet (method
        // CALLS like `ws.send(..)` arrive via the call-expression path, not
        // this bare-read block).
        "ws" => prop == "readyState" && !matches!(class, "WebSocketServer" | "Server"),
        // events / net instances dispatch their EventEmitter / socket methods
        // and getters through the class_filter table. These modules expose no
        // user own-property surface in the bundle walls, so keep dispatching
        // for any member to preserve existing behaviour.
        "events" | "net" => true,
        // #6364 — DisposableStack / AsyncDisposableStack: `disposed` is the
        // only native data getter (its value comes from the FFI helper
        // `js_disposable_stack_disposed`), so a bare read must dispatch as a
        // 0-arg `NativeMethodCall` through the `__disposable__` NativeModSig
        // row. Every other member (`use`/`adopt`/`defer`/`dispose`/
        // `disposeAsync`/`move`) is a method: a method CALL arrives via the
        // call-expression path, and a bare method-VALUE read must stay a plain
        // PropertyGet (a bound-method read), never a 0-arg invoking dispatch.
        "__disposable__" => prop == "disposed",
        // Other native modules historically routed every uncovered member to
        // the dispatching fallback. They have no observed user-own-property
        // surface, so preserve that: dispatch any member not handled by the
        // PropertyGet arms above.
        "dns" | "dns/promises" | "dgram" | "inspector" | "inspector/promises" | "sqlite"
        | "url" | "worker_threads" | "util" | "sys" | "console" | "Headers" => true,
        // Any other module (e.g. a mis-tagged `debug` createDebug value): a bare
        // member read is an own-property GET, never an invoking dispatch.
        _ => false,
    }
}

pub(crate) fn is_blob_getter_name(prop: &str) -> bool {
    matches!(prop, "size" | "type" | "name" | "lastModified")
}

/// Native data-property getters exposed by `fetch`-module Response instances.
/// Mirrors the property arms in `perry-codegen` `lower_call/options/fetch.rs`.
pub(crate) fn is_fetch_response_getter_name(prop: &str) -> bool {
    matches!(
        prop,
        "status"
            | "statusText"
            | "ok"
            | "type"
            | "url"
            | "redirected"
            | "bodyUsed"
            | "headers"
            | "body"
    )
}

pub(crate) fn is_stream_api_member(module: &str, prop: &str) -> bool {
    match module {
        "readable_stream" => matches!(
            prop,
            "getReader"
                | "cancel"
                | "tee"
                | "pipeTo"
                | "pipeThrough"
                | "locked"
                | "enqueue"
                | "close"
                | "error"
                | "desiredSize"
                | "byobRequest"
        ),
        "readable_stream_reader" => {
            matches!(prop, "read" | "releaseLock" | "cancel" | "closed")
        }
        "writable_stream" => matches!(prop, "getWriter" | "abort" | "close" | "locked"),
        "writable_stream_writer" => matches!(
            prop,
            "write" | "close" | "abort" | "releaseLock" | "closed" | "ready" | "desiredSize"
        ),
        "transform_stream" => matches!(prop, "readable" | "writable"),
        _ => false,
    }
}

pub(crate) fn is_classic_stream_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "read"
            | "push"
            | "pipe"
            | "unpipe"
            | "pause"
            | "resume"
            | "destroy"
            | "setEncoding"
            | "isPaused"
            | "write"
            | "end"
            | "cork"
            | "uncork"
            | "setDefaultEncoding"
            | "compose"
            | "iterator"
            | "toArray"
            | "map"
            | "filter"
            | "reduce"
            | "forEach"
            | "find"
            | "some"
            | "every"
            | "flatMap"
            | "take"
            | "drop"
            | "on"
            | "addListener"
            | "once"
            | "prependListener"
            | "prependOnceListener"
            | "emit"
            | "listeners"
            | "rawListeners"
            | "eventNames"
            | "listenerCount"
            | "removeListener"
            | "off"
            | "removeAllListeners"
            | "setMaxListeners"
            | "getMaxListeners"
    )
}

/// Classic Node stream (`stream` / `node:stream`) PROPERTY GETTER names —
/// the no-arg state getters that dispatch through the codegen `NativeModSig`
/// table to their `js_node_stream_method_*` FFI (mirrors the `module: "stream"`
/// getter entries in `lower_call/native_table/net_events.rs`). A bare read of
/// any name NOT in this set and NOT a `is_classic_stream_method_name` method is
/// a plain own-property GET on the heap stream object, so user-assigned fields
/// (`_.colors`, library bookkeeping) read back the stored value instead of
/// being invoked as a 0-arg native call.
pub(crate) fn is_classic_stream_getter_name(prop: &str) -> bool {
    matches!(
        prop,
        "readableHighWaterMark"
            | "readableLength"
            | "readableObjectMode"
            | "readable"
            | "readableFlowing"
            | "readableEnded"
            | "readableEncoding"
            | "readableAborted"
            | "readableDidRead"
            | "writableHighWaterMark"
            | "writableLength"
            | "writableNeedDrain"
            | "writableObjectMode"
            | "writable"
            | "writableCorked"
            | "writableEnded"
            | "writableFinished"
            | "closed"
            | "errored"
            | "allowHalfOpen"
            | "destroyed"
    )
}

pub(crate) fn is_http_incoming_message_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "on" | "addListener"
            | "setEncoding"
            | "setTimeout"
            | "pause"
            | "resume"
            | "destroy"
            | "read"
    )
}

pub(crate) fn is_http_client_request_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "on" | "end"
            | "write"
            | "setHeader"
            | "setTimeout"
            | "listenerCount"
            | "getHeader"
            | "hasHeader"
            | "removeHeader"
            | "getHeaderNames"
            | "getHeaders"
            | "getRawHeaderNames"
            | "abort"
            | "destroy"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setNoDelay"
            | "setSocketKeepAlive"
    )
}

pub(crate) fn is_http_incoming_message_runtime_property_name(prop: &str) -> bool {
    matches!(
        prop,
        "method"
            | "url"
            | "httpVersion"
            | "httpVersionMajor"
            | "httpVersionMinor"
            | "headers"
            | "rawHeaders"
            | "headersDistinct"
            | "trailers"
            | "rawTrailers"
            | "trailersDistinct"
            | "complete"
            | "aborted"
            | "destroyed"
            | "socket"
            | "connection"
            | "signal"
            | "remoteAddress"
            | "remotePort"
    )
}

pub(crate) fn is_http_server_response_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "setHeader"
            | "getHeader"
            | "removeHeader"
            | "hasHeader"
            | "getHeaders"
            | "getHeaderNames"
            | "appendHeader"
            | "setHeaders"
            | "writeHead"
            | "write"
            | "addTrailers"
            | "end"
            | "flushHeaders"
            | "cork"
            | "uncork"
            | "setTimeout"
            | "writeEarlyHints"
            | "writeContinue"
            | "writeProcessing"
            | "on"
            | "addListener"
    )
}

pub(crate) fn is_http_server_response_runtime_property_name(prop: &str) -> bool {
    matches!(
        prop,
        "statusCode"
            | "statusMessage"
            | "headersSent"
            | "writableEnded"
            | "writableFinished"
            | "finished"
            | "sendDate"
            | "strictContentLength"
            | "req"
            | "socket"
            | "connection"
    )
}

pub(crate) fn is_dns_resolver_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "cancel"
            | "getServers"
            | "setServers"
            | "setLocalAddress"
            | "resolve"
            | "resolve4"
            | "resolve6"
            | "resolveAny"
            | "resolveCaa"
            | "resolveCname"
            | "resolveMx"
            | "resolveNaptr"
            | "resolveNs"
            | "resolvePtr"
            | "resolveSoa"
            | "resolveSrv"
            | "resolveTlsa"
            | "resolveTxt"
            | "reverse"
    )
}

pub(crate) fn is_console_instance_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "log"
            | "info"
            | "debug"
            | "dir"
            | "dirxml"
            | "error"
            | "warn"
            | "count"
            | "countReset"
            | "group"
            | "groupCollapsed"
            | "groupEnd"
            | "clear"
            | "profile"
            | "profileEnd"
            | "timeStamp"
    )
}

pub(crate) fn is_dgram_socket_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "send"
            | "bind"
            | "close"
            | "address"
            | "connect"
            | "disconnect"
            | "addMembership"
            | "dropMembership"
            | "setBroadcast"
            | "setMulticastTTL"
            | "setMulticastLoopback"
            | "setMulticastInterface"
            | "setTTL"
            | "setRecvBufferSize"
            | "setSendBufferSize"
            | "getRecvBufferSize"
            | "getSendBufferSize"
            | "ref"
            | "unref"
    )
}

pub(crate) fn is_net_socket_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "address"
            | "connect"
            | "destroy"
            | "destroySoon"
            | "end"
            | "pause"
            | "ref"
            | "resetAndDestroy"
            | "resume"
            | "setEncoding"
            | "setKeepAlive"
            | "setNoDelay"
            | "setTimeout"
            | "unref"
            | "write"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "listeners"
            | "rawListeners"
            | "upgradeToTLS"
            | "setDefaultEncoding"
            | "cork"
            | "uncork"
    )
}

pub(crate) fn is_net_server_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "address"
            | "close"
            | "getConnections"
            | "listen"
            | "ref"
            | "unref"
            | "on"
            | "addListener"
            | "once"
            | "off"
            | "removeListener"
            | "removeAllListeners"
            | "listenerCount"
            | "eventNames"
            | "listeners"
            | "rawListeners"
    )
}

pub(crate) fn is_headers_method_name(prop: &str) -> bool {
    matches!(
        prop,
        "append"
            | "delete"
            | "entries"
            | "forEach"
            | "get"
            | "getSetCookie"
            | "has"
            | "keys"
            | "set"
            | "values"
    )
}

pub(crate) fn is_url_pattern_data_property(prop: &str) -> bool {
    matches!(
        prop,
        "protocol"
            | "username"
            | "password"
            | "hostname"
            | "port"
            | "pathname"
            | "search"
            | "hash"
            | "hasRegExpGroups"
    )
}

pub(crate) fn is_worker_instance_value_property(prop: &str) -> bool {
    matches!(
        prop,
        "threadId"
            | "threadName"
            | "resourceLimits"
            | "stdin"
            | "stdout"
            | "stderr"
            | "performance"
            | "getHeapStatistics"
            | "cpuUsage"
            | "getHeapSnapshot"
            | "startCpuProfile"
            | "startHeapProfile"
            | "postMessage"
            | "terminate"
            | "ref"
            | "unref"
            | "on"
            | "once"
            | "off"
    )
}
