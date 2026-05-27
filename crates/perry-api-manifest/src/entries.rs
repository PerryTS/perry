//! The actual manifest data — the source of truth.
//!
//! Two categories of entry feed this table:
//!
//! 1. **Methods dispatched through `NATIVE_MODULE_TABLE`** in
//!    `crates/perry-codegen/src/lower_call.rs`. These are extracted
//!    mechanically and a CI test in `perry-codegen` asserts that every
//!    `NATIVE_MODULE_TABLE` entry has a counterpart here so drift can't
//!    ship.
//! 2. **Methods/properties dispatched via custom `Expr::*` variants**
//!    in `perry-hir`'s lowering — `crypto.randomUUID` lowers to
//!    `Expr::CryptoRandomUUID` directly, never touching
//!    `NATIVE_MODULE_TABLE`. Same for `os.platform` → `Expr::OsPlatform`,
//!    `path.join` → `Expr::PathJoin`, etc. These are listed manually
//!    below; coverage of a module is what promotes it to "strict mode"
//!    in the unimplemented-API check (#463) — modules with at least
//!    one entry have all references gated against the manifest, modules
//!    with zero entries fall through to existing permissive behavior.
//!
//! Adding a new method/property to a module here automatically lifts
//! the corresponding compile error.

use crate::{ApiEntry, ApiKind, ApiSource, ParamSpec, TypeSpec};

/// Module specifiers Perry recognizes as native (i.e. resolvable
/// without going through the V8 fallback). Migrated from
/// `crates/perry-hir/src/ir.rs::NATIVE_MODULES` so the manifest can
/// answer module-resolution questions without depending on
/// `perry-hir`. Order matches the original list to keep diffs minimal.
pub const NATIVE_MODULES: &[&str] = &[
    "mysql2",
    "mysql2/promise",
    "pg",
    "uuid",
    "bcrypt",
    "argon2",
    "ioredis",
    "axios",
    "node-fetch",
    "ws",
    "zlib",
    "crypto",
    "dotenv",
    "dotenv/config",
    "jsonwebtoken",
    "nanoid",
    "slugify",
    "validator",
    "ethers",
    "mongodb",
    "better-sqlite3",
    "tursodb",
    "iroh",
    "node-cron",
    "nodemailer",
    "http",
    "https",
    "http2",
    "events",
    "os",
    "buffer",
    "assert",
    "assert/strict",
    "child_process",
    "net",
    "tls",
    "stream",
    "streams",
    "fs",
    "path",
    "console",
    "util",
    "util/types",
    "url",
    "lru-cache",
    "commander",
    "decimal.js",
    "bignumber.js",
    "exponential-backoff",
    "lodash",
    "dayjs",
    "date-fns",
    "moment",
    "sharp",
    "cheerio",
    "cron",
    "fastify",
    "async_hooks",
    "readline",
    "string_decoder",
    "querystring",
    "cluster",
    "tty",
    "perf_hooks",
    "process",
    "perry/tui",
    "perry/ui",
    "perry/system",
    "perry/plugin",
    "perry/widget",
    "perry/i18n",
    "worker_threads",
    "perry/thread",
    "perry/updater",
    "perry/media",
    "perry/audio",
    "perry/background",
    "redis",
    "rate-limiter-flexible",
    "fetch",
    // `@perryts/pdf` — official PDF creation package (#516).
    // Bundled wrapper lives in `crates/perry-ext-pdf`; the producer
    // side companion to the existing PdfView widget. d.ts at
    // `types/perry/pdf/index.d.ts`.
    "@perryts/pdf",
    // `perry/ads` — official in-app advertising package (#867).
    // MVP scaffold: bundled wrapper at `crates/perry-ext-ads`
    // returns structured `{ error: "no-sdk-linked" }` placeholders
    // until real Google Mobile Ads SDK integration lands. d.ts at
    // `types/perry/ads/index.d.ts`.
    "perry/ads",
];

/// Node built-in submodules that Perry routes through the
/// `node_submodules` runtime table rather than `NATIVE_MODULES`.
/// Keeping these separate preserves the compiler's submodule import
/// lowering while still allowing manifest/docs entries for the subpath.
pub const NODE_SUBMODULES: &[&str] = &["stream/promises"];

/// Modules handled entirely by `perry-runtime` — the linker doesn't
/// need to pull in `perry-stdlib` for these. Migrated from
/// `crates/perry-hir/src/ir.rs::RUNTIME_ONLY_MODULES`.
pub const RUNTIME_ONLY_MODULES: &[&str] = &[
    "fs",
    "path",
    "os",
    "buffer",
    "assert",
    "assert/strict",
    "child_process",
    "stream",
    "url",
    "console",
    "util",
    "util/types",
    "process",
    "perry/ui",
    "perry/system",
    "perry/widget",
    "perry/i18n",
    "perry/thread",
    "perry/media",
    "perry/audio",
    "perry/tui",
    "perry/background",
    "tty",
    "perf_hooks",
];

const fn method(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Method {
            has_receiver,
            class_filter,
        },
        source: ApiSource::Stdlib,
        stub: false,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

/// Method entry with declared `params` and `returns`. Used to backfill
/// auto-derivable rows from the codegen dispatch table so the
/// generated `.d.ts` carries real signatures (#512).
const fn method_sig(
    module: &'static str,
    name: &'static str,
    has_receiver: bool,
    class_filter: Option<&'static str>,
    params: &'static [ParamSpec],
    returns: TypeSpec,
) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Method {
            has_receiver,
            class_filter,
        },
        source: ApiSource::Stdlib,
        stub: false,
        abi_version: None,
        params,
        returns,
    }
}

const fn property(module: &'static str, name: &'static str) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Property,
        source: ApiSource::Stdlib,
        stub: false,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

const fn class(module: &'static str, name: &'static str) -> ApiEntry {
    ApiEntry {
        module,
        name,
        kind: ApiKind::Class,
        source: ApiSource::Stdlib,
        stub: false,
        abi_version: None,
        params: &[],
        returns: TypeSpec::Any,
    }
}

// -----------------------------------------------------------------------------
// Param shorthand consts. Auto-derived rows cite these to keep the
// table compact. Names are `p0`/`p1`/... — the codegen dispatch table
// doesn't carry user-facing names, and the manifest-v1 spec doesn't
// require them.
// -----------------------------------------------------------------------------

const fn p_str(name: &'static str) -> ParamSpec {
    ParamSpec::Named {
        name,
        ty: TypeSpec::String,
        optional: false,
    }
}
const fn p_any(name: &'static str) -> ParamSpec {
    ParamSpec::Named {
        name,
        ty: TypeSpec::Any,
        optional: false,
    }
}

/// #1843 — every `zlib.create*` Transform-stream factory shares the same
/// shape: an optional `options` object in, a stream handle (`Any`) out.
const ZLIB_STREAM_OPTS: &[ParamSpec] = &[ParamSpec::Named {
    name: "options",
    ty: TypeSpec::Any,
    optional: true,
}];
const fn zlib_stream_factory(name: &'static str) -> ApiEntry {
    method_sig("zlib", name, false, None, ZLIB_STREAM_OPTS, TypeSpec::Any)
}

/// Source-of-truth manifest. See module-level docs for what feeds it.
pub static API_MANIFEST: &[ApiEntry] = &[
    // ===========================================================
    // Methods dispatched via NATIVE_MODULE_TABLE
    // (extracted from crates/perry-codegen/src/lower_call.rs;
    //  drift guarded by perry-codegen's manifest_consistency test)
    // ===========================================================
    method_sig(
        "fastify",
        "default",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("fastify", "get", true, None),
    method("fastify", "post", true, None),
    method("fastify", "put", true, None),
    method("fastify", "delete", true, None),
    method("fastify", "patch", true, None),
    method("fastify", "head", true, None),
    method("fastify", "options", true, None),
    method("fastify", "all", true, None),
    method("fastify", "route", true, None),
    method("fastify", "addHook", true, None),
    method("fastify", "setErrorHandler", true, None),
    method("fastify", "register", true, None),
    method("fastify", "listen", true, None),
    method("fastify", "close", true, None),
    // #1113 — `app.server` is a Node-compatible getter returning the
    // FastifyApp handle (pointer-tagged) so `typeof app.server ===
    // "object"`. Lowered as a zero-arg NativeMethodCall by the HIR
    // property-as-method path; the runtime side is
    // `js_fastify_app_server`. `app.server.on(event, cb)` then
    // dispatches against the same handle (the `"on"` arm below).
    // Today only `"upgrade"` is stored; bidirectional WebSocket
    // upgrade through hyper is the tracked follow-up.
    method("fastify", "server", true, None),
    method("fastify", "on", true, None),
    method("fastify", "method", true, None),
    method("fastify", "url", true, None),
    // Manifest-consistency catch-up (release-sweep gate).
    method("fastify", "type", true, None),
    method("fastify", "params", true, None),
    method("fastify", "param", true, None),
    method("fastify", "query", true, None),
    method("fastify", "rawBody", true, None),
    method("fastify", "headers", true, None),
    method("fastify", "header", true, None),
    method("fastify", "user", true, None),
    method("fastify", "status", true, None),
    method("fastify", "code", true, None),
    method("fastify", "send", true, None),
    method("fastify", "text", true, None),
    method("fastify", "html", true, None),
    method("fastify", "redirect", true, None),
    method("fastify", "json", true, None),
    method("fastify", "body", true, None),
    method_sig(
        "mysql2",
        "createConnection",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "mysql2",
        "createPool",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "mysql2/promise",
        "createConnection",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "mysql2/promise",
        "createPool",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("mysql2", "query", true, Some("Pool")),
    method("mysql2", "execute", true, Some("Pool")),
    method("mysql2", "end", true, Some("Pool")),
    method("mysql2/promise", "query", true, Some("Pool")),
    method("mysql2/promise", "execute", true, Some("Pool")),
    method("mysql2/promise", "end", true, Some("Pool")),
    method("mysql2", "query", true, Some("PoolConnection")),
    method("mysql2", "execute", true, Some("PoolConnection")),
    method("mysql2/promise", "query", true, Some("PoolConnection")),
    method("mysql2/promise", "execute", true, Some("PoolConnection")),
    method("mysql2", "query", true, None),
    method("mysql2", "execute", true, None),
    method("mysql2", "end", true, None),
    method("mysql2", "getConnection", true, None),
    method("mysql2", "release", true, None),
    method("mysql2", "beginTransaction", true, None),
    method("mysql2", "commit", true, None),
    method("mysql2", "rollback", true, None),
    method("mysql2/promise", "query", true, None),
    method("mysql2/promise", "execute", true, None),
    method("mysql2/promise", "end", true, None),
    method("mysql2/promise", "getConnection", true, None),
    method("mysql2/promise", "release", true, None),
    method("mysql2/promise", "beginTransaction", true, None),
    method("mysql2/promise", "commit", true, None),
    method("mysql2/promise", "rollback", true, None),
    method_sig("pg", "connect", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("pg", "Pool", false, None, &[p_any("p0")], TypeSpec::Any),
    method("pg", "connect", true, Some("Client")),
    method("pg", "query", true, Some("Pool")),
    method("pg", "end", true, Some("Pool")),
    method("pg", "query", true, None),
    method("pg", "end", true, None),
    method_sig(
        "ioredis",
        "createClient",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("ioredis", "set", true, None),
    method("ioredis", "get", true, None),
    method("ioredis", "del", true, None),
    method("ioredis", "exists", true, None),
    method("ioredis", "incr", true, None),
    method("ioredis", "decr", true, None),
    method("ioredis", "expire", true, None),
    method("ioredis", "quit", true, None),
    // v0.5.707 closes-#605: NATIVE_MODULE_TABLE added connect/disconnect rows
    // when normalizing the `redis` npm package alias to ioredis dispatch.
    // Manifest must mirror or `every_dispatch_entry_has_manifest_counterpart`
    // fails the workspace test build.
    method("ioredis", "connect", true, None),
    method("ioredis", "disconnect", true, None),
    method_sig(
        "mongodb",
        "connect",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("mongodb", "connect", true, None),
    method("mongodb", "db", true, None),
    method("mongodb", "collection", true, None),
    method("mongodb", "insertOne", true, None),
    method("mongodb", "insertMany", true, None),
    method("mongodb", "find", true, None),
    method("mongodb", "findOne", true, None),
    method("mongodb", "updateOne", true, None),
    method("mongodb", "updateMany", true, None),
    method("mongodb", "deleteOne", true, None),
    method("mongodb", "deleteMany", true, None),
    method("mongodb", "countDocuments", true, None),
    method("mongodb", "close", true, None),
    method_sig(
        "better-sqlite3",
        "default",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method("better-sqlite3", "prepare", true, None),
    method("better-sqlite3", "run", true, None),
    method("better-sqlite3", "get", true, None),
    method("better-sqlite3", "all", true, None),
    method("better-sqlite3", "exec", true, None),
    method("better-sqlite3", "close", true, None),
    // Manifest-consistency catch-up (release-sweep gate): NATIVE_MODULE_TABLE
    // had a `raw` row that wasn't mirrored here.
    method("better-sqlite3", "raw", true, None),
    // #1022 — surface the rest of the v8-proxy-materialized methods so
    // the api-docs drift check stays green. `pragma` / `iterate` /
    // `pluck` / `columns` / `transaction` are wired through
    // `perry-jsruntime::bridge::materialize_sqlite_*_proxy` for the V8
    // fallback path (drizzle on better-sqlite3); the native-side
    // codegen lowering already routes the same names through
    // `NATIVE_MODULE_TABLE`.
    method("better-sqlite3", "pragma", true, None),
    method("better-sqlite3", "iterate", true, None),
    method("better-sqlite3", "pluck", true, None),
    method("better-sqlite3", "columns", true, None),
    method("better-sqlite3", "transaction", true, None),
    // tursodb (#424). open / exec / execBatch / close /
    // lastInsertRowid / isAutocommit shipped in v0.5.543; queryAll /
    // queryOne shipped in v0.5.553 (close the row-as-object gap by
    // building shapes inside spawn_blocking and resolving with
    // POINTER_TAG'd JsValues).
    method("tursodb", "open", false, None),
    method("tursodb", "exec", true, None),
    method("tursodb", "execBatch", true, None),
    method("tursodb", "queryAll", true, None),
    method("tursodb", "queryOne", true, None),
    method("tursodb", "close", true, None),
    method("tursodb", "lastInsertRowid", true, None),
    method("tursodb", "isAutocommit", true, None),
    // iroh (#425). bind / nodeId / close shipped in v0.5.544; the
    // peer connection + stream surface (connect / acceptOne /
    // openBi / acceptBi / streamWrite / streamFinish /
    // streamReadToEnd / connClose) shipped in v0.5.554. ALPN is
    // hardcoded to `b"perry-iroh/0"` for v0.
    method("iroh", "bind", false, None),
    method("iroh", "nodeId", true, None),
    method("iroh", "close", true, None),
    method("iroh", "connect", true, None),
    method("iroh", "acceptOne", true, None),
    method("iroh", "openBi", true, None),
    method("iroh", "acceptBi", true, None),
    method("iroh", "streamWrite", true, None),
    method("iroh", "streamFinish", true, None),
    method("iroh", "streamReadToEnd", true, None),
    method("iroh", "connClose", true, None),
    method_sig("ws", "Server", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "ws",
        "WebSocket",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method("ws", "on", true, None),
    method("ws", "send", true, None),
    method("ws", "close", true, None),
    // #1113 — `wss.handleUpgrade(req, socket, head, cb)` for a
    // `new WebSocketServer({ noServer: true })`.
    method("ws", "handleUpgrade", true, None),
    // Issue #577 Phase 4 — Client-class methods for the upgrade-path wsId.
    method("ws", "on", true, Some("Client")),
    method("ws", "addListener", true, Some("Client")),
    method("ws", "send", true, Some("Client")),
    method("ws", "close", true, Some("Client")),
    class("ws", "Client"),
    method_sig(
        "ws",
        "sendToClient",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "ws",
        "closeClient",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "net",
        "createConnection",
        false,
        None,
        // p0 = port (number) or options object; p1 = host (string) or
        // connectListener; p2 = connectListener in positional form.
        // Issue #770 widened to accept the options-object overload.
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "net",
        "connect",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig("net", "Socket", false, None, &[], TypeSpec::Any),
    method("net", "connect", true, Some("Socket")),
    method("net", "write", true, Some("Socket")),
    method("net", "end", true, Some("Socket")),
    method("net", "destroy", true, Some("Socket")),
    method("net", "on", true, Some("Socket")),
    method("net", "upgradeToTLS", true, Some("Socket")),
    // Issue #1852 — chainable no-op `net.Socket` option setters. Perry's
    // TCP transport doesn't model Nagle/keep-alive/idle-timeout or read
    // back-pressure yet, but the methods must be callable (and return the
    // socket for chaining) instead of throwing "not a function". These
    // names also cover the `net.Server` `ref`/`unref`/`setTimeout` rows
    // below (`module_has_symbol` is name-based), so they unblock the
    // strict-API gate for both classes.
    method("net", "setNoDelay", true, Some("Socket")),
    method("net", "setKeepAlive", true, Some("Socket")),
    method("net", "setTimeout", true, Some("Socket")),
    method("net", "setEncoding", true, Some("Socket")),
    method("net", "setDefaultEncoding", true, Some("Socket")),
    method("net", "pause", true, Some("Socket")),
    method("net", "resume", true, Some("Socket")),
    method("net", "ref", true, Some("Socket")),
    method("net", "unref", true, Some("Socket")),
    method("net", "cork", true, Some("Socket")),
    method("net", "uncork", true, Some("Socket")),
    // Issue #1123 followup — `net.Server` instance methods backing
    // `createServer(...).listen/.close/.address/.on`. Mirrors the
    // shape of the http-server rows at entries.rs:2298. The
    // factory `createServer(...)` itself doesn't show up in the
    // dispatch table because it lowers to `Expr::NetCreateServer`
    // (handled in `crates/perry-codegen/src/expr.rs`), not a
    // NativeMethodCall — same reason `("http", "createServer")`
    // appears here but not as a dispatch-table row.
    method("net", "listen", true, Some("Server")),
    method("net", "close", true, Some("Server")),
    method("net", "address", true, Some("Server")),
    method("net", "addListener", true, Some("Server")),
    // Issue #811 — IP classification helpers + Happy-Eyeballs default
    // accessors. Pure string/global-flag functions.
    method("net", "isIP", false, None),
    method("net", "isIPv4", false, None),
    method("net", "isIPv6", false, None),
    method("net", "getDefaultAutoSelectFamily", false, None),
    method("net", "setDefaultAutoSelectFamily", false, None),
    method(
        "net",
        "getDefaultAutoSelectFamilyAttemptTimeout",
        false,
        None,
    ),
    method(
        "net",
        "setDefaultAutoSelectFamilyAttemptTimeout",
        false,
        None,
    ),
    method_sig(
        "tls",
        "connect",
        false,
        None,
        &[p_str("p0"), p_any("p1"), p_str("p2"), p_any("p3")],
        TypeSpec::Any,
    ),
    method_sig("events", "EventEmitter", false, None, &[], TypeSpec::Any),
    method("events", "on", true, None),
    method("events", "emit", true, None),
    method("events", "removeListener", true, None),
    method("events", "removeAllListeners", true, None),
    // EventEmitter additions wired in v0.5.922 (issue #850).
    property("events", "defaultMaxListeners"),
    property("events", "errorMonitor"),
    property("events", "captureRejections"),
    property("events", "captureRejectionSymbol"),
    method("events", "once", true, None),
    method("events", "addListener", true, None),
    method("events", "prependListener", true, None),
    method("events", "prependOnceListener", true, None),
    method("events", "off", true, None),
    method("events", "listenerCount", true, None),
    method("events", "listeners", true, None),
    method("events", "rawListeners", true, None),
    method("events", "eventNames", true, None),
    method("events", "setMaxListeners", true, None),
    method("events", "getMaxListeners", true, None),
    // Module-level helpers (`events.once` / `events.getEventListeners` /
    // `events.listenerCount` / `events.getMaxListeners` /
    // `events.setMaxListeners`).
    method("events", "once", false, None),
    method("events", "addAbortListener", false, None),
    method("events", "getEventListeners", false, None),
    method("events", "listenerCount", false, None),
    method("events", "getMaxListeners", false, None),
    method("events", "setMaxListeners", false, None),
    // Module-level `events.on(emitter, name)` — async-iterable queue,
    // PR #1257.
    method("events", "on", false, None),
    method_sig(
        "lru-cache",
        "default",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("lru-cache", "get", true, None),
    method("lru-cache", "set", true, None),
    method("lru-cache", "has", true, None),
    method("lru-cache", "delete", true, None),
    method("lru-cache", "clear", true, None),
    method("lru-cache", "size", true, None),
    method("commander", "name", true, None),
    method("commander", "description", true, None),
    method("commander", "version", true, None),
    method("commander", "command", true, None),
    method("commander", "option", true, None),
    method("commander", "requiredOption", true, None),
    method("commander", "action", true, None),
    method("commander", "parse", true, None),
    method("commander", "opts", true, None),
    method("async_hooks", "createHook", false, None),
    method("async_hooks", "executionAsyncId", false, None),
    method("async_hooks", "triggerAsyncId", false, None),
    method("async_hooks", "enable", true, Some("AsyncHook")),
    method("async_hooks", "run", true, None),
    method("async_hooks", "getStore", true, None),
    method("async_hooks", "enterWith", true, None),
    method("async_hooks", "exit", true, None),
    method("async_hooks", "disable", true, None),
    method("async_hooks", "asyncId", true, Some("AsyncResource")),
    method("async_hooks", "triggerAsyncId", true, Some("AsyncResource")),
    method("async_hooks", "emitDestroy", true, Some("AsyncResource")),
    method(
        "async_hooks",
        "runInAsyncScope",
        true,
        Some("AsyncResource"),
    ),
    method("async_hooks", "bind", true, Some("AsyncResource")),
    // AsyncResource — Nest's `@nestjs/core` request-scoped DI uses
    // this to bind a callback to a synthetic async resource. The
    // stub in `node:async_hooks` JS module satisfies callers that
    // only need the `runInAsyncScope` shape.
    class("async_hooks", "AsyncResource"),
    class("async_hooks", "AsyncLocalStorage"),
    method("decimal.js", "plus", true, None),
    method("decimal.js", "minus", true, None),
    method("decimal.js", "times", true, None),
    method("decimal.js", "div", true, None),
    method("decimal.js", "mod", true, None),
    method("decimal.js", "pow", true, None),
    method("decimal.js", "sqrt", true, None),
    method("decimal.js", "abs", true, None),
    method("decimal.js", "neg", true, None),
    method("decimal.js", "round", true, None),
    method("decimal.js", "floor", true, None),
    method("decimal.js", "ceil", true, None),
    method("decimal.js", "toFixed", true, None),
    method("decimal.js", "toString", true, None),
    method("decimal.js", "toNumber", true, None),
    method("decimal.js", "valueOf", true, None),
    method("decimal.js", "eq", true, None),
    method("decimal.js", "lt", true, None),
    method("decimal.js", "lte", true, None),
    method("decimal.js", "gt", true, None),
    method("decimal.js", "gte", true, None),
    method("decimal.js", "cmp", true, None),
    method("decimal.js", "isZero", true, None),
    method("decimal.js", "isPositive", true, None),
    method("decimal.js", "isNegative", true, None),
    method_sig("uuid", "v4", false, None, &[], TypeSpec::String),
    method_sig("uuid", "v1", false, None, &[], TypeSpec::String),
    method_sig("uuid", "v7", false, None, &[], TypeSpec::String),
    method_sig(
        "uuid",
        "validate",
        false,
        None,
        &[ParamSpec::Named {
            name: "id",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "jsonwebtoken",
        "sign",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "payload",
                ty: TypeSpec::Any,
                optional: false,
            },
            ParamSpec::Named {
                name: "secret",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "options",
                ty: TypeSpec::Any,
                optional: true,
            },
            // #915: FFI's 4th arg is `kid_ptr: *const StringHeader` — the
            // dispatch table padding zeroes it when the user doesn't pass
            // it. Surfacing the slot in the manifest keeps the
            // #512 arity-drift assertion happy without forcing every
            // caller to write a 4th positional arg.
            ParamSpec::Named {
                name: "kid",
                ty: TypeSpec::String,
                optional: true,
            },
        ],
        TypeSpec::String,
    ),
    method_sig(
        "jsonwebtoken",
        "verify",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "token",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "secret",
                ty: TypeSpec::String,
                optional: false,
            },
        ],
        TypeSpec::Any,
    ),
    method_sig(
        "jsonwebtoken",
        "decode",
        false,
        None,
        &[ParamSpec::Named {
            name: "token",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Any,
    ),
    method_sig(
        "nodemailer",
        "createTransport",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("nodemailer", "sendMail", true, None),
    method("nodemailer", "verify", true, None),
    method_sig("dotenv", "config", false, None, &[], TypeSpec::Any),
    method_sig(
        "nanoid",
        "nanoid",
        false,
        None,
        &[ParamSpec::Named {
            name: "size",
            ty: TypeSpec::Number,
            optional: false,
        }],
        TypeSpec::String,
    ),
    method_sig(
        "slugify",
        "default",
        false,
        None,
        &[p_str("p0"), p_str("p1"), p_str("p2")],
        TypeSpec::String,
    ),
    method_sig(
        "slugify",
        "slugify",
        false,
        None,
        &[p_str("p0"), p_str("p1"), p_str("p2")],
        TypeSpec::String,
    ),
    method_sig(
        "validator",
        "isEmail",
        false,
        None,
        &[ParamSpec::Named {
            name: "s",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "validator",
        "isURL",
        false,
        None,
        &[ParamSpec::Named {
            name: "s",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "validator",
        "isUUID",
        false,
        None,
        &[ParamSpec::Named {
            name: "s",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "validator",
        "isJSON",
        false,
        None,
        &[ParamSpec::Named {
            name: "s",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "validator",
        "isEmpty",
        false,
        None,
        &[ParamSpec::Named {
            name: "s",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "exponential-backoff",
        "backOff",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "argon2",
        "hash",
        false,
        None,
        &[ParamSpec::Named {
            name: "password",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Any,
    ),
    method_sig(
        "argon2",
        "verify",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "hash",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "password",
                ty: TypeSpec::String,
                optional: false,
            },
        ],
        TypeSpec::Any,
    ),
    method_sig(
        "bcrypt",
        "hash",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "password",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "saltOrRounds",
                ty: TypeSpec::Any,
                optional: false,
            },
        ],
        TypeSpec::Any,
    ),
    method_sig(
        "bcrypt",
        "compare",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "plaintext",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "hash",
                ty: TypeSpec::String,
                optional: false,
            },
        ],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/thread",
        "parallelMap",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/thread",
        "parallelFilter",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/thread",
        "spawn",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "chunk",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "compact",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "drop",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "first",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("lodash", "head", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("lodash", "last", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "flatten",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("lodash", "uniq", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "reverse",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "take",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "camelCase",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "lodash",
        "kebabCase",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "lodash",
        "snakeCase",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "lodash",
        "clamp",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "range",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "times",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("lodash", "size", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "sum",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "mean",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "sumBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "meanBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Number,
    ),
    method_sig("lodash", "tail", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("lodash", "max", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig("lodash", "min", false, None, &[p_any("p0")], TypeSpec::Any),
    method_sig(
        "lodash",
        "maxBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "minBy",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "lodash",
        "clamp",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Number,
    ),
    method_sig(
        "lodash",
        "inRange",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Bool,
    ),
    method_sig(
        "lodash",
        "random",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Number,
    ),
    method_sig("dayjs", "default", false, None, &[], TypeSpec::Any),
    method_sig("dayjs", "dayjs", false, None, &[], TypeSpec::Any),
    method("dayjs", "format", true, None),
    method("dayjs", "year", true, None),
    method("dayjs", "month", true, None),
    method("dayjs", "date", true, None),
    method("dayjs", "day", true, None),
    method("dayjs", "hour", true, None),
    method("dayjs", "minute", true, None),
    method("dayjs", "second", true, None),
    method("dayjs", "millisecond", true, None),
    method("dayjs", "valueOf", true, None),
    method("dayjs", "unix", true, None),
    method("dayjs", "toISOString", true, None),
    method("dayjs", "add", true, None),
    method("dayjs", "subtract", true, None),
    method("dayjs", "startOf", true, None),
    method("dayjs", "endOf", true, None),
    method("dayjs", "isBefore", true, None),
    method("dayjs", "isAfter", true, None),
    method("dayjs", "isSame", true, None),
    method("dayjs", "isValid", true, None),
    method("dayjs", "diff", true, None),
    method("dayjs", "clone", true, None),
    method_sig("moment", "default", false, None, &[], TypeSpec::Any),
    method_sig("moment", "moment", false, None, &[], TypeSpec::Any),
    method_sig(
        "sharp",
        "default",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig("sharp", "sharp", false, None, &[p_str("p0")], TypeSpec::Any),
    method("sharp", "resize", true, None),
    method("sharp", "rotate", true, None),
    method("sharp", "flip", true, None),
    method("sharp", "flop", true, None),
    method("sharp", "grayscale", true, None),
    method("sharp", "blur", true, None),
    method("sharp", "jpeg", true, None),
    method("sharp", "png", true, None),
    method("sharp", "webp", true, None),
    method("sharp", "toFile", true, None),
    method("sharp", "toBuffer", true, None),
    method("sharp", "metadata", true, None),
    method("sharp", "width", true, None),
    method("sharp", "height", true, None),
    method_sig(
        "cheerio",
        "load",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method("cheerio", "select", true, None),
    method("cheerio", "text", true, None),
    method("cheerio", "html", true, None),
    method("cheerio", "attr", true, None),
    method("cheerio", "length", true, None),
    method("cheerio", "first", true, None),
    method("cheerio", "last", true, None),
    method("cheerio", "eq", true, None),
    method("cheerio", "find", true, None),
    method("cheerio", "children", true, None),
    method("cheerio", "parent", true, None),
    method("cheerio", "hasClass", true, None),
    method_sig(
        "zlib",
        "gzipSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "zlib",
        "gunzipSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "zlib",
        "deflateSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "zlib",
        "inflateSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig("zlib", "gzip", false, None, &[p_str("p0")], TypeSpec::Any),
    method_sig("zlib", "gunzip", false, None, &[p_str("p0")], TypeSpec::Any),
    // One-shot sync codecs that round out the #1843 set: raw deflate/inflate
    // (no zlib wrapper), auto-detect unzip, and CRC32.
    method_sig(
        "zlib",
        "deflateRawSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "zlib",
        "inflateRawSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "zlib",
        "unzipSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    // `crc32(data, seed?)` — `seed` is the running CRC from a prior chunk
    // so callers can stream a long input. Dispatch declares 2 args; mirror
    // that arity here so manifest_consistency stays green.
    method_sig(
        "zlib",
        "crc32",
        false,
        None,
        &[
            p_str("p0"),
            ParamSpec::Named {
                name: "seed",
                ty: TypeSpec::Number,
                optional: true,
            },
        ],
        TypeSpec::Number,
    ),
    // Callback-form variants that #1843 didn't surface. `gzip`/`gunzip` and
    // `brotliCompress`/`brotliDecompress` already exist above as method_sig
    // entries; these stub the rest so `typeof zlib.deflate === "function"`
    // resolves true. Actual callback dispatch piggy-backs off the existing
    // native_table sync routes when used with `util.promisify`.
    method("zlib", "deflate", false, None),
    method("zlib", "deflateRaw", false, None),
    method("zlib", "inflate", false, None),
    method("zlib", "inflateRaw", false, None),
    method("zlib", "unzip", false, None),
    // Stream classes — registered as classes so `typeof zlib.Gzip` reads
    // "function". #1843 exposed the `create*` factories but not the
    // constructor names themselves.
    class("zlib", "Deflate"),
    class("zlib", "DeflateRaw"),
    class("zlib", "Gzip"),
    class("zlib", "Gunzip"),
    class("zlib", "Inflate"),
    class("zlib", "InflateRaw"),
    class("zlib", "Unzip"),
    class("zlib", "BrotliCompress"),
    class("zlib", "BrotliDecompress"),
    // `zlib.constants` — the ~50 Z_*/DEFLATE/INFLATE/GZIP/BROTLI_*/ZSTD_*
    // constants Node exposes on `require('node:zlib').constants`. Required
    // by axios for stream wiring. Values are resolved at runtime by
    // `get_native_module_constant` in `perry-runtime/src/object.rs`.
    property("zlib", "constants"),
    class("zlib", "Deflate"),
    class("zlib", "DeflateRaw"),
    class("zlib", "Gzip"),
    class("zlib", "Gunzip"),
    class("zlib", "Inflate"),
    class("zlib", "InflateRaw"),
    class("zlib", "Unzip"),
    class("zlib", "BrotliCompress"),
    class("zlib", "BrotliDecompress"),
    class("zlib", "ZstdCompress"),
    class("zlib", "ZstdDecompress"),
    // #1843 — Brotli one-shot compress/decompress (sync + async).
    method_sig(
        "zlib",
        "brotliCompressSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "zlib",
        "brotliDecompressSync",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "zlib",
        "brotliCompress",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "zlib",
        "brotliDecompress",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    // #1843 — Transform-stream factories. Each returns a stream handle
    // supporting `.write`/`.end`/`.on('data'|'end'|'error')`/`.pipe`.
    zlib_stream_factory("createGzip"),
    zlib_stream_factory("createGunzip"),
    zlib_stream_factory("createDeflate"),
    zlib_stream_factory("createInflate"),
    zlib_stream_factory("createDeflateRaw"),
    zlib_stream_factory("createInflateRaw"),
    zlib_stream_factory("createUnzip"),
    zlib_stream_factory("createBrotliCompress"),
    // `zlib.createBrotliDecompress(options?)` — now a real Transform stream
    // (still passes axios's `typeof === 'function'` module-init gate).
    zlib_stream_factory("createBrotliDecompress"),
    zlib_stream_factory("createZstdCompress"),
    zlib_stream_factory("createZstdDecompress"),
    method_sig(
        "cron",
        "validate",
        false,
        None,
        &[ParamSpec::Named {
            name: "expr",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::Bool,
    ),
    method_sig(
        "cron",
        "schedule",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "expr",
                ty: TypeSpec::String,
                optional: false,
            },
            ParamSpec::Named {
                name: "handler",
                ty: TypeSpec::Any,
                optional: false,
            },
        ],
        TypeSpec::Any,
    ),
    method_sig(
        "cron",
        "describe",
        false,
        None,
        &[ParamSpec::Named {
            name: "expr",
            ty: TypeSpec::String,
            optional: false,
        }],
        TypeSpec::String,
    ),
    method("cron", "start", true, None),
    method("cron", "stop", true, None),
    method("cron", "isRunning", true, None),
    method("cron", "nextDate", true, None),
    method_sig(
        "perry/tui",
        "Text",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "Box", false, None, &[], TypeSpec::Any),
    method_sig(
        "perry/tui",
        "render",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig("perry/tui", "enter", false, None, &[], TypeSpec::Void),
    method_sig(
        "perry/tui",
        "state",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("perry/tui", "get", true, Some("State")),
    method("perry/tui", "set", true, Some("State")),
    method_sig(
        "perry/tui",
        "useInput",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "run",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig("perry/tui", "exit", false, None, &[], TypeSpec::Void),
    method_sig(
        "perry/tui",
        "boxSetFlexDirection",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetJustifyContent",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetAlignItems",
        false,
        None,
        &[p_any("p0"), p_str("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetGap",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetPadding",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetWidth",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetHeight",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexGrow",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    // Manifest-consistency catch-up (release-sweep gate, v0.5.823):
    // NATIVE_MODULE_TABLE accumulated 12 perry/tui entries during the
    // #679 ink-API ergonomics work (v0.5.810) and follow-ups that
    // weren't mirrored here. Restoring drift-free state.
    method_sig(
        "perry/tui",
        "boxSetPaddingEach",
        false,
        None,
        &[
            p_any("p0"),
            p_any("p1"),
            p_any("p2"),
            p_any("p3"),
            p_any("p4"),
        ],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexShrink",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexBasis",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetFlexBasisPct",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetWidthPct",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "boxSetHeightPct",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "TextStyled",
        false,
        None,
        &[p_str("p0"), p_str("p1"), p_str("p2"), p_any("p3")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Table",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Tabs",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "InputAt",
        false,
        None,
        &[p_str("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "AnimatedSpinner",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "useStateTuple",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "Spacer", false, None, &[], TypeSpec::Any),
    method_sig(
        "perry/tui",
        "ProgressBar",
        false,
        None,
        &[p_any("p0"), p_any("p1"), p_any("p2")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Spinner",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Input",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "List",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "Select",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "TextArea",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::Any,
    ),
    // ---- perry/tui ink-shape hooks (#679 Phase 1) ----
    method_sig(
        "perry/tui",
        "useState",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "useStateSet",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "useEffect",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "useMemo",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig(
        "perry/tui",
        "useRef",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "useApp", false, None, &[], TypeSpec::Any),
    method_sig("perry/tui", "useStdout", false, None, &[], TypeSpec::Any),
    method_sig(
        "perry/tui",
        "waitUntilExit",
        false,
        None,
        &[],
        TypeSpec::Void,
    ),
    method("perry/tui", "exit", true, Some("TuiApp")),
    method("perry/tui", "waitUntilExit", true, Some("TuiApp")),
    method("perry/tui", "write", true, Some("TuiStdout")),
    method("perry/tui", "columns", true, Some("TuiStdout")),
    method("perry/tui", "rows", true, Some("TuiStdout")),
    method("perry/tui", "get", true, Some("RefBox")),
    method("perry/tui", "set", true, Some("RefBox")),
    // ---- perry/tui Phase 3 — focus management (#679) ----
    method_sig(
        "perry/tui",
        "useFocus",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Any,
    ),
    method_sig("perry/tui", "focusNext", false, None, &[], TypeSpec::Void),
    method_sig(
        "perry/tui",
        "focusPrevious",
        false,
        None,
        &[],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "focus",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Void,
    ),
    method_sig(
        "perry/tui",
        "useFocusManager",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    method("perry/tui", "focusNext", true, Some("FocusManager")),
    method("perry/tui", "focusPrevious", true, Some("FocusManager")),
    method("perry/tui", "focus", true, Some("FocusManager")),
    method_sig(
        "readline",
        "createInterface",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method("readline", "question", true, None),
    method("readline", "on", true, None),
    method("readline", "close", true, None),
    method_sig(
        "worker_threads",
        "getEnvironmentData",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "setEnvironmentData",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::Void,
    ),
    method_sig(
        "worker_threads",
        "getWorkerData",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "workerData",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    method_sig(
        "worker_threads",
        "parentPort",
        false,
        None,
        &[],
        TypeSpec::Any,
    ),
    method("worker_threads", "postMessage", true, None),
    method_sig(
        "ethers",
        "getAddress",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "ethers",
        "formatEther",
        false,
        None,
        &[p_any("p0")],
        TypeSpec::String,
    ),
    method_sig(
        "ethers",
        "formatUnits",
        false,
        None,
        &[p_any("p0"), p_any("p1")],
        TypeSpec::String,
    ),
    method_sig(
        "ethers",
        "parseEther",
        false,
        None,
        &[p_str("p0")],
        TypeSpec::BigInt,
    ),
    method_sig(
        "ethers",
        "parseUnits",
        false,
        None,
        &[p_str("p0"), p_any("p1")],
        TypeSpec::BigInt,
    ),
    method("ethers", "createRandom", false, Some("Wallet")),
    // ===========================================================
    // Methods dispatched via custom Expr::* variants
    // (perry-hir/src/lower/expr_call.rs and expr_member.rs)
    // ===========================================================

    // crypto — issue #463 calls out crypto.subtle.encrypt as the
    // motivating example. Some entries below are dispatched via
    // codegen-level chain pattern matching (createHash/createHmac via
    // expr.rs:8475+, pbkdf2Sync via expr.rs:8677+) rather than through
    // NATIVE_MODULE_TABLE — they do work, even though they don't show
    // up in the dispatch-table extraction.
    method("crypto", "randomBytes", false, None),
    method("crypto", "randomUUID", false, None),
    method("crypto", "randomInt", false, None),
    method("crypto", "hash", false, None),
    method("crypto", "sha256", false, None),
    method("crypto", "md5", false, None),
    method("crypto", "getRandomValues", false, None),
    // crypto.randomFillSync(buffer, offset?, size?) — fills the
    // typed-array / Buffer with cryptographically strong random
    // bytes in-place and returns the same object. Required by
    // axios (Uint32Array) for ID generation.
    method("crypto", "randomFillSync", false, None),
    method("crypto", "createHash", false, None),
    method("crypto", "createSign", false, None),
    method("crypto", "createVerify", false, None),
    class("crypto", "ECDH"),
    // #1367: X509Certificate — `new X509Certificate(pem|der)` + read-only
    // subject/issuer/validFrom/validTo/serialNumber/fingerprint/ca props.
    class("crypto", "X509Certificate"),
    // Legacy Netscape SPKAC helper namespace:
    // crypto.Certificate.{verifySpkac,exportPublicKey,exportChallenge}.
    property("crypto", "Certificate"),
    method("crypto", "createECDH", false, None),
    method("crypto", "createDiffieHellman", false, None),
    method("crypto", "createDiffieHellmanGroup", false, None),
    method("crypto", "getDiffieHellman", false, None),
    method("crypto", "createPrivateKey", false, None),
    method("crypto", "createPublicKey", false, None),
    method("crypto", "generateKeyPairSync", false, None),
    method("crypto", "createHmac", false, None),
    // `crypto.createCipheriv(alg, key, iv)` / `createDecipheriv(...)` —
    // issue #1075. Registers a CipherHandle dispatched via the
    // small-pointer-handle method route. Supports aes-128-cbc,
    // aes-256-cbc, aes-128-gcm, aes-256-gcm. Wired in `expr.rs`
    // (no NATIVE_MODULE_TABLE entry — direct dispatch like createHash).
    method("crypto", "createCipheriv", false, None),
    method("crypto", "createDecipheriv", false, None),
    // `crypto.createSign(alg)` / `createVerify(alg)` — RSA PKCS#1 v1.5 sign /
    // verify over the SHA family (#1364). SignHandle dispatched like createHash
    // (no NATIVE_MODULE_TABLE entry — direct codegen dispatch in expr/calls.rs).
    method("crypto", "createSign", false, None),
    method("crypto", "createVerify", false, None),
    // `crypto.createSecretKey(key, encoding?)` — required by jose for the
    // JWT signing path; returns a Uint8Array-marked Buffer of the key
    // bytes that `instanceof Uint8Array` accepts on both sides of the
    // V8 boundary. Wired through codegen in `expr.rs` (no NATIVE_MODULE_TABLE
    // entry — direct dispatch matches the createHash/createHmac pattern).
    method("crypto", "createSecretKey", false, None),
    method("crypto", "pbkdf2Sync", false, None),
    method("crypto", "pbkdf2", false, None),
    // crypto.scryptSync(password, salt, keylen, options?) -> Buffer. Wired in
    // codegen `expr/calls.rs`; HIR types the result as Uint8Array.
    method("crypto", "scryptSync", false, None),
    // crypto.hkdfSync(digest, ikm, salt, info, keylen) -> ArrayBuffer.
    method("crypto", "hkdfSync", false, None),
    // crypto.generateKeyPairSync(type, options) -> { publicKey, privateKey }
    // PEM strings (RSA / EC P-256). Wired in codegen `expr/calls.rs`.
    method("crypto", "generateKeyPairSync", false, None),
    // crypto.randomInt([min,] max) — uniform integer in [min, max).
    // crypto.timingSafeEqual(a, b) — constant-time byte comparison.
    // crypto.getHashes() / getCiphers() / getCurves() — supported-algorithm name lists.
    // crypto.getFips() — FIPS mode flag.
    // crypto.sign/verify/publicEncrypt/privateDecrypt/privateEncrypt/publicDecrypt —
    // asymmetric one-shot helpers. All wired in codegen `expr/calls.rs`
    // (direct dispatch, like createHash).
    method("crypto", "randomInt", false, None),
    method("crypto", "timingSafeEqual", false, None),
    method("crypto", "sign", false, None),
    method("crypto", "verify", false, None),
    method("crypto", "publicEncrypt", false, None),
    method("crypto", "privateDecrypt", false, None),
    method("crypto", "privateEncrypt", false, None),
    method("crypto", "publicDecrypt", false, None),
    method("crypto", "getHashes", false, None),
    method("crypto", "getCiphers", false, None),
    method("crypto", "getCurves", false, None),
    method("crypto", "getFips", false, None),
    // Web Crypto API (issue #561) — `crypto.subtle.*`. The HIR
    // lowering at `crates/perry-hir/src/lower/expr_call.rs` recognizes
    // the `crypto.subtle.<method>(args)` chain and emits a
    // `WebCrypto*` HIR variant. Listing `subtle` here flips the strict
    // strict-API gate (#463) so unimported `crypto.subtle` reads inside
    // an import-style binding don't silently return undefined.
    property("crypto", "subtle"),
    // os — methods mapped to Expr::Os* in expr_call.rs.
    method("os", "platform", false, None),
    method("os", "availableParallelism", false, None),
    method("os", "arch", false, None),
    method("os", "endianness", false, None),
    method("os", "hostname", false, None),
    method("os", "homedir", false, None),
    method("os", "loadavg", false, None),
    method("os", "machine", false, None),
    method("os", "tmpdir", false, None),
    method("os", "totalmem", false, None),
    method("os", "freemem", false, None),
    method("os", "uptime", false, None),
    method("os", "type", false, None),
    method("os", "release", false, None),
    method("os", "cpus", false, None),
    method("os", "networkInterfaces", false, None),
    method("os", "userInfo", false, None),
    method("os", "version", false, None),
    method_sig(
        "os",
        "getPriority",
        false,
        None,
        &[ParamSpec::Named {
            name: "pid",
            ty: TypeSpec::Number,
            optional: true,
        }],
        TypeSpec::Number,
    ),
    method_sig(
        "os",
        "setPriority",
        false,
        None,
        &[
            ParamSpec::Named {
                name: "pidOrPriority",
                ty: TypeSpec::Number,
                optional: false,
            },
            ParamSpec::Named {
                name: "priority",
                ty: TypeSpec::Number,
                optional: true,
            },
        ],
        TypeSpec::Void,
    ),
    property("os", "EOL"),
    property("os", "devNull"),
    // Issue #649: os/crypto.constants tables — see
    // get_native_module_constant in perry-runtime/src/object.rs.
    property("os", "constants"),
    property("crypto", "constants"),
    // path — methods mapped to Expr::Path* in expr_call.rs.
    method("path", "join", false, None),
    method("path", "dirname", false, None),
    method("path", "basename", false, None),
    method("path", "extname", false, None),
    method("path", "resolve", false, None),
    method("path", "isAbsolute", false, None),
    method("path", "relative", false, None),
    method("path", "normalize", false, None),
    method("path", "parse", false, None),
    method("path", "format", false, None),
    method("path", "toNamespacedPath", false, None),
    method("path", "matchesGlob", false, None),
    property("path", "sep"),
    property("path", "delimiter"),
    property("path", "posix"),
    property("path", "win32"),
    // process — properties mapped to Expr::Process* / Expr::Os* in expr_member.rs.
    method("process", "abort", false, None),
    method("process", "cwd", false, None),
    method("process", "uptime", false, None),
    method("process", "memoryUsage", false, None),
    method("process", "nextTick", false, None),
    method("process", "chdir", false, None),
    method("process", "kill", false, None),
    method("process", "exit", false, None),
    method("process", "umask", false, None),
    method("process", "threadCpuUsage", false, None),
    method("process", "availableMemory", false, None),
    method("process", "constrainedMemory", false, None),
    method("process", "getuid", false, None),
    method("process", "geteuid", false, None),
    method("process", "getgid", false, None),
    method("process", "getegid", false, None),
    method("process", "emitWarning", false, None),
    method("process", "on", false, None),
    method("process", "addListener", false, None),
    method("process", "once", false, None),
    method("process", "prependListener", false, None),
    method("process", "prependOnceListener", false, None),
    method("process", "emit", false, None),
    method("process", "listeners", false, None),
    method("process", "rawListeners", false, None),
    method("process", "eventNames", false, None),
    method("process", "listenerCount", false, None),
    method("process", "removeListener", false, None),
    method("process", "off", false, None),
    method("process", "removeAllListeners", false, None),
    method("process", "setMaxListeners", false, None),
    method("process", "getMaxListeners", false, None),
    method("process", "cpuUsage", false, None),
    method("process", "resourceUsage", false, None),
    method("process", "getActiveResourcesInfo", false, None),
    method("process", "hrtime", false, None),
    property("process", "argv"),
    property("process", "platform"),
    property("process", "arch"),
    property("process", "pid"),
    property("process", "ppid"),
    property("process", "version"),
    property("process", "versions"),
    property("process", "stdin"),
    property("process", "stdout"),
    property("process", "stderr"),
    property("process", "env"),
    // ===========================================================
    // Class exports (constructors `new Foo(...)` from a module).
    // ===========================================================
    class("buffer", "Buffer"),
    class("events", "EventEmitter"),
    class("ws", "WebSocketServer"),
    class("ws", "WebSocket"),
    class("net", "Socket"),
    class("net", "Server"),
    class("ioredis", "Redis"),
    class("mysql2/promise", "Pool"),
    class("mysql2", "Pool"),
    class("pg", "Pool"),
    class("pg", "Client"),
    class("url", "URL"),
    class("url", "URLSearchParams"),
    // Issue #848: string_decoder.StringDecoder — handle-based dispatch
    // for `write` / `end` + `lastNeed` / `lastTotal` / `lastChar` getters.
    class("string_decoder", "StringDecoder"),
    method("string_decoder", "write", true, Some("StringDecoder")),
    method("string_decoder", "end", true, Some("StringDecoder")),
    property("string_decoder", "lastNeed"),
    property("string_decoder", "lastTotal"),
    property("string_decoder", "lastChar"),
    property("string_decoder", "encoding"),
    // node:querystring — legacy URL-encoded form parser. Greenfield
    // (deprecated since Node 11 but still imported by many npm pkgs).
    method("querystring", "escape", false, None),
    method("querystring", "unescape", false, None),
    method("querystring", "parse", false, None),
    method("querystring", "stringify", false, None),
    // `decode` / `encode` are aliases the test_parity_querystring fixture
    // verifies are *identity-equal* to parse/stringify. Native dispatch
    // routes both names to the same runtime symbol so the closures live
    // at the same address.
    method("querystring", "decode", false, None),
    method("querystring", "encode", false, None),
    // node:cluster — shape-only surface. The fixture probes
    // typeof properties + reads constants; we never actually fork.
    // Methods are wired through `is_native_module_callable_export`
    // (bound-method closure path) so `typeof cluster.fork === "function"`
    // holds without us implementing a real fork.
    method("cluster", "fork", false, None),
    method("cluster", "disconnect", false, None),
    method("cluster", "setupPrimary", false, None),
    method("cluster", "setupMaster", false, None),
    class("cluster", "Worker"),
    property("cluster", "isPrimary"),
    property("cluster", "isMaster"),
    property("cluster", "isWorker"),
    property("cluster", "worker"),
    property("cluster", "workers"),
    property("cluster", "settings"),
    property("cluster", "schedulingPolicy"),
    property("cluster", "SCHED_RR"),
    property("cluster", "SCHED_NONE"),
    // `cluster.on` / `cluster.addListener` exist as EventEmitter
    // prototype methods on the cluster module ITSELF in Node, but
    // `import * as cluster from "node:cluster"` reads them as named
    // exports — and there is no `on` / `addListener` named export.
    // Node's parity fixture prints "undefined" for both. Register them
    // as properties so the #463 strict gate doesn't bail out at compile
    // time; `get_native_module_constant` returns `undefined` at
    // runtime.
    property("cluster", "on"),
    property("cluster", "addListener"),
    // ===========================================================
    // #513 Phase A: backfill receiver-less surface for modules that
    // previously had zero entries. Without these, `module_has_any_entries`
    // returned false and the unimplemented-API gate (#463) silently
    // fell through to the old permissive behavior. One entry is enough
    // to flip strictness on for the module — the entries below cover
    // the most common surface so legitimate calls continue to compile.
    // ===========================================================

    // --- fs (sync surface lowered to Expr::Fs* in expr_call.rs;
    //     async + stream + extra sync helpers route through runtime
    //     externs declared by perry-runtime/src/fs.rs). ---
    method("fs", "readFileSync", false, None),
    method("fs", "writeFileSync", false, None),
    method("fs", "appendFileSync", false, None),
    method("fs", "existsSync", false, None),
    method("fs", "exists", false, None),
    method("fs", "mkdirSync", false, None),
    method("fs", "unlinkSync", false, None),
    method("fs", "openSync", false, None),
    method("fs", "open", false, None),
    method("fs", "closeSync", false, None),
    method("fs", "close", false, None),
    method("fs", "fstatSync", false, None),
    method("fs", "fstat", false, None),
    method("fs", "fsyncSync", false, None),
    method("fs", "fsync", false, None),
    method("fs", "fdatasyncSync", false, None),
    method("fs", "fdatasync", false, None),
    method("fs", "fchmodSync", false, None),
    method("fs", "fchmod", false, None),
    method("fs", "fchownSync", false, None),
    method("fs", "fchown", false, None),
    method("fs", "futimesSync", false, None),
    method("fs", "futimes", false, None),
    method("fs", "ftruncateSync", false, None),
    method("fs", "ftruncate", false, None),
    method("fs", "readSync", false, None),
    method("fs", "writeSync", false, None),
    method("fs", "read", false, None),
    method("fs", "write", false, None),
    method("fs", "readvSync", false, None),
    method("fs", "writevSync", false, None),
    method("fs", "readv", false, None),
    method("fs", "writev", false, None),
    method("fs", "rmSync", false, None),
    method("fs", "rmdirSync", false, None),
    method("fs", "readdirSync", false, None),
    method("fs", "statSync", false, None),
    method("fs", "lstat", false, None),
    method("fs", "statfsSync", false, None),
    method("fs", "statfs", false, None),
    method("fs", "opendirSync", false, None),
    method("fs", "opendir", false, None),
    method("fs", "globSync", false, None),
    method("fs", "glob", false, None),
    method("fs", "lstatSync", false, None),
    method("fs", "utimesSync", false, None),
    method("fs", "utimes", false, None),
    method("fs", "lutimesSync", false, None),
    method("fs", "lutimes", false, None),
    method("fs", "renameSync", false, None),
    method("fs", "copyFileSync", false, None),
    method("fs", "cpSync", false, None),
    method("fs", "cp", false, None),
    method("fs", "accessSync", false, None),
    method("fs", "realpathSync", false, None),
    method("fs", "realpath", false, None),
    method("fs", "mkdtempSync", false, None),
    method("fs", "mkdtemp", false, None),
    method("fs", "chmodSync", false, None),
    method("fs", "chmod", false, None),
    method("fs", "chownSync", false, None),
    method("fs", "chown", false, None),
    method("fs", "lchownSync", false, None),
    method("fs", "lchown", false, None),
    method("fs", "lchmodSync", false, None),
    method("fs", "lchmod", false, None),
    method("fs", "truncateSync", false, None),
    method("fs", "truncate", false, None),
    method("fs", "linkSync", false, None),
    method("fs", "link", false, None),
    method("fs", "symlinkSync", false, None),
    method("fs", "symlink", false, None),
    method("fs", "readlinkSync", false, None),
    method("fs", "readlink", false, None),
    method("fs", "readFile", false, None),
    method("fs", "writeFile", false, None),
    method("fs", "appendFile", false, None),
    method("fs", "access", false, None),
    method("fs", "rename", false, None),
    method("fs", "copyFile", false, None),
    method("fs", "mkdir", false, None),
    method("fs", "unlink", false, None),
    method("fs", "rm", false, None),
    method("fs", "rmdir", false, None),
    method("fs", "readdir", false, None),
    method("fs", "stat", false, None),
    method("fs", "createReadStream", false, None),
    method("fs", "createWriteStream", false, None),
    method("fs", "watchFile", false, None),
    method("fs", "unwatchFile", false, None),
    method("fs", "watch", false, None),
    property("fs", "promises"),
    property("fs", "constants"),
    // --- console (Node global console exposed as node:console too). ---
    class("console", "Console"),
    method("console", "log", false, None),
    method("console", "info", false, None),
    method("console", "debug", false, None),
    method("console", "error", false, None),
    method("console", "warn", false, None),
    method("console", "assert", false, None),
    method("console", "dir", false, None),
    method("console", "dirxml", false, None),
    method("console", "trace", false, None),
    method("console", "table", false, None),
    method("console", "clear", false, None),
    method("console", "count", false, None),
    method("console", "countReset", false, None),
    method("console", "time", false, None),
    method("console", "timeEnd", false, None),
    method("console", "timeLog", false, None),
    method("console", "group", false, None),
    method("console", "groupCollapsed", false, None),
    method("console", "groupEnd", false, None),
    method("console", "profile", false, None),
    method("console", "profileEnd", false, None),
    method("console", "timeStamp", false, None),
    // --- util (a small surface — Perry implements util.inspect /
    //     util.format / util.promisify shapes through builtins.rs;
    //     the rest are documented stubs) ---
    method("util", "inspect", false, None),
    method("util", "format", false, None),
    // `util.formatWithOptions(options, format[, ...args])` — identical to
    // `util.format` except the first arg is an `util.inspect` options bag
    // applied to any `%o`/`%O` placeholders. Required by the `debug` npm
    // package (top-1k downloads, transitive dep of express/socket.io). Our
    // stub ignores the options bag and delegates to `util.format`; full
    // options-passthrough is a follow-up.
    method("util", "formatWithOptions", false, None),
    method("util", "promisify", false, None),
    method("util", "callbackify", false, None),
    method("util", "deprecate", false, None),
    method("util", "inherits", false, None),
    method("util", "isDeepStrictEqual", false, None),
    method("util", "stripVTControlCharacters", false, None),
    class("util", "TextEncoder"),
    class("util", "TextDecoder"),
    // util.types — Node's runtime type-introspection namespace. Required
    // for `@nestjs/core` / rxjs internal dispatch (PR #754 fixture). The
    // backing object lives in the `node:util` stub in
    // perry-jsruntime/src/modules.rs and answers every is* probe with
    // `false` (a safe default — no Perry value type matches Node's
    // privileged BoxedPrimitive/Proxy/external introspection cases).
    property("util", "types"),
    method("util/types", "isPromise", false, None),
    method("util/types", "isArrayBuffer", false, None),
    method("util/types", "isSharedArrayBuffer", false, None),
    method("util/types", "isAnyArrayBuffer", false, None),
    method("util/types", "isArrayBufferView", false, None),
    method("util/types", "isTypedArray", false, None),
    method("util/types", "isUint8Array", false, None),
    method("util/types", "isUint16Array", false, None),
    method("util/types", "isInt32Array", false, None),
    method("util/types", "isFloat64Array", false, None),
    method("util/types", "isMap", false, None),
    method("util/types", "isSet", false, None),
    method("util/types", "isDate", false, None),
    method("util/types", "isRegExp", false, None),
    // Boxed primitive introspection (PR #1257). The `util/types` import form
    // and the `util.types` namespace-access form both lower to this canonical
    // module key.
    method("util/types", "isNumberObject", false, None),
    method("util/types", "isStringObject", false, None),
    method("util/types", "isBooleanObject", false, None),
    method("util/types", "isBoxedPrimitive", false, None),
    // node:assert — assertion helpers used by tests and many npm packages.
    method("assert", "ok", false, None),
    method("assert", "fail", false, None),
    method("assert", "equal", false, None),
    method("assert", "notEqual", false, None),
    method("assert", "strictEqual", false, None),
    method("assert", "notStrictEqual", false, None),
    method("assert", "deepEqual", false, None),
    method("assert", "notDeepEqual", false, None),
    method("assert", "deepStrictEqual", false, None),
    method("assert", "notDeepStrictEqual", false, None),
    method("assert", "match", false, None),
    method("assert", "doesNotMatch", false, None),
    method("assert", "throws", false, None),
    method("assert", "doesNotThrow", false, None),
    method("assert", "ifError", false, None),
    method("assert", "default", false, None),
    method("assert", "strict", false, None),
    property("assert", "strict"),
    class("assert", "AssertionError"),
    method("assert/strict", "ok", false, None),
    method("assert/strict", "fail", false, None),
    method("assert/strict", "equal", false, None),
    method("assert/strict", "notEqual", false, None),
    method("assert/strict", "strictEqual", false, None),
    method("assert/strict", "notStrictEqual", false, None),
    method("assert/strict", "deepEqual", false, None),
    method("assert/strict", "notDeepEqual", false, None),
    method("assert/strict", "deepStrictEqual", false, None),
    method("assert/strict", "notDeepStrictEqual", false, None),
    method("assert/strict", "match", false, None),
    method("assert/strict", "doesNotMatch", false, None),
    method("assert/strict", "throws", false, None),
    method("assert/strict", "doesNotThrow", false, None),
    method("assert/strict", "ifError", false, None),
    method("assert/strict", "default", false, None),
    class("assert/strict", "AssertionError"),
    // --- stream (Web Streams API + Node stream classes — see
    //     perry-stdlib/src/streams.rs and perry-ext-streams) ---
    class("stream", "Readable"),
    class("stream", "Writable"),
    class("stream", "Duplex"),
    class("stream", "Transform"),
    class("stream", "PassThrough"),
    // Legacy base class (extends EventEmitter); modern classes hang off it as
    // statics. `stream.Stream === stream.default`. #1966.
    class("stream", "Stream"),
    // `node:stream`'s default export is the legacy `Stream` class itself.
    method("stream", "default", false, None),
    method("stream", "pipeline", false, None),
    method("stream", "finished", false, None),
    property("stream", "promises"),
    method("stream/promises", "pipeline", false, None),
    method("stream/promises", "finished", false, None),
    // `require('stream')` returns the legacy `Stream` constructor itself,
    // which has its own `.prototype` (it extends EventEmitter). The
    // `node_modules/send` package (express's static-file backend) does
    // `util.inherits(SendStream, require('stream'))`, which reads
    // `Stream.prototype` — the gate rejects the access without this entry.
    property("stream", "prototype"),
    // #1533: `stream.promises` namespace (`await pipeline(...)` /
    // `finished(...)`). The read resolves to a `stream/promises`-tagged
    // namespace object; its members are gated under that submodule name.
    property("stream", "promises"),
    method("stream/promises", "pipeline", false, None),
    method("stream/promises", "finished", false, None),
    // `Readable.from(iterable)` — Node's static factory. Resolves
    // through the `Readable.foo` -> `stream.foo` route in
    // `lower_call.rs`, so the gate keys off `stream.from`.
    method("stream", "from", false, None),
    // #1534/#1746: static introspection helpers — `Readable.isDisturbed(s)`,
    // `Readable.isErrored(s)`, `Readable.isReadable(s)`, and
    // `stream.isWritable(s)` (also re-exported module-level). Perry tracks
    // per-stream disturbed/errored bits and readable/writable direction
    // flags, so these answer per-instance (`null` for the wrong direction,
    // `false` once ended/errored, `true` otherwise).
    method("stream", "isDisturbed", false, None),
    method("stream", "isErrored", false, None),
    method("stream", "isReadable", false, None),
    method("stream", "isWritable", false, None),
    // #1537: `stream.getDefaultHighWaterMark(objectMode)` /
    // `setDefaultHighWaterMark(objectMode, value)` — the per-mode platform
    // default highWaterMark (65536 byte / 16 objectMode), mutable at runtime.
    method("stream", "getDefaultHighWaterMark", false, None),
    method("stream", "setDefaultHighWaterMark", false, None),
    // #1541: `stream.addAbortSignal(signal, stream)` — Node wires
    // the AbortSignal so aborting it destroys the stream. Stub
    // ignores the signal and returns the stream verbatim so chain
    // patterns (`r = addAbortSignal(s, r)`) keep working.
    method("stream", "addAbortSignal", false, None),
    // #1539: `stream.compose(...streams)` chains streams into a
    // composite Duplex; `stream.duplexPair([opts])` returns a paired
    // `[Duplex, Duplex]`. Both return fresh Duplex stubs today.
    method("stream", "compose", false, None),
    method("stream", "duplexPair", false, None),
    // #1540: Web-stream interop helpers — Readable/Writable .toWeb /
    // .fromWeb. Stubs return a fresh Duplex (data isn't propagated
    // between Node and WHATWG universes yet).
    method("stream", "toWeb", false, None),
    method("stream", "fromWeb", false, None),
    // EventEmitter methods on stream instances. node:stream extends
    // EventEmitter — every Readable/Writable/Duplex/Transform/PassThrough
    // exposes the full `.on('data'|'end'|'error'|'close'|...)` /
    // `.once` / `.off` / `.removeListener` / `.emit` /
    // `.removeAllListeners` / `.addListener` / `.prependListener` /
    // `.prependOnceListener` / `.listenerCount` / `.listeners` /
    // `.eventNames` / `.setMaxListeners` / `.getMaxListeners` surface.
    // The runtime closures are built by `js_node_stream_*_new` (see
    // `crates/perry-runtime/src/node_stream.rs`); these entries exist so
    // the #463 unimplemented-API gate accepts `stream.on(...)` /
    // `stream.once(...)` / etc. in user code (e.g. axios's
    // `AxiosTransformStream extends stream.Transform` + downstream
    // event wiring). Has_receiver=true because every call site reads
    // `<instance>.on(...)`, not `stream.on(...)` as a module-level
    // helper.
    method("stream", "on", true, None),
    method("stream", "once", true, None),
    method("stream", "off", true, None),
    method("stream", "addListener", true, None),
    method("stream", "removeListener", true, None),
    method("stream", "removeAllListeners", true, None),
    method("stream", "emit", true, None),
    method("stream", "prependListener", true, None),
    method("stream", "prependOnceListener", true, None),
    method("stream", "listenerCount", true, None),
    method("stream", "listeners", true, None),
    method("stream", "rawListeners", true, None),
    method("stream", "eventNames", true, None),
    method("stream", "setMaxListeners", true, None),
    method("stream", "getMaxListeners", true, None),
    // Core stream instance stubs used by stream/promises and the
    // Readable/Writable/Duplex/Transform/PassThrough constructor surface.
    method("stream", "read", true, None),
    method("stream", "resume", true, None),
    method("stream", "destroy", true, None),
    method("stream", "write", true, None),
    method("stream", "end", true, None),
    method("stream", "cork", true, None),
    method("stream", "uncork", true, None),
    // #1539: push() backpressure return + readable/writableHighWaterMark
    // property getters on typed stream instances.
    method("stream", "push", true, None),
    method("stream", "readableHighWaterMark", true, None),
    method("stream", "readable", true, None),
    method("stream", "readableEnded", true, None),
    method("stream", "writableHighWaterMark", true, None),
    method("stream", "readableAborted", true, None),
    method("stream", "writableCorked", true, None),
    method("stream", "writable", true, None),
    method("stream", "writableEnded", true, None),
    method("stream", "writableFinished", true, None),
    method("stream", "destroyed", true, None),
    // --- child_process (synchronous + async exec surface;
    //     spawn/fork are documented but not yet codegen'd) ---
    method("child_process", "exec", false, None),
    method("child_process", "execSync", false, None),
    method("child_process", "execFile", false, None),
    method("child_process", "execFileSync", false, None),
    method("child_process", "spawn", false, None),
    method("child_process", "spawnSync", false, None),
    method("child_process", "fork", false, None),
    // #1856: `ChildProcess` is the streaming-subprocess constructor; reading
    // it as a value yields `[Function: ChildProcess]`. `Stream` is not a real
    // `child_process` export (Node returns `undefined`) — registered so the
    // value-read passes the #463 surface gate and resolves to `undefined`.
    class("child_process", "ChildProcess"),
    property("child_process", "Stream"),
    // --- tty (only `isatty` is implemented; ReadStream / WriteStream
    //     are wrapped via process.stdin / process.stdout) ---
    method("tty", "isatty", false, None),
    class("tty", "ReadStream"),
    class("tty", "WriteStream"),
    // --- perf_hooks (W3C User Timing on `performance` + PerformanceObserver) ---
    method("perf_hooks", "now", false, None),
    method("perf_hooks", "mark", false, None),
    method("perf_hooks", "measure", false, None),
    method("perf_hooks", "getEntries", false, None),
    method("perf_hooks", "getEntriesByName", false, None),
    method("perf_hooks", "getEntriesByType", false, None),
    method("perf_hooks", "clearMarks", false, None),
    method("perf_hooks", "clearMeasures", false, None),
    method("perf_hooks", "eventLoopUtilization", false, None),
    method("perf_hooks", "toJSON", false, None),
    method("perf_hooks", "clearResourceTimings", false, None),
    method("perf_hooks", "setResourceTimingBufferSize", false, None),
    // #1478: stub — records the entry (no-op today, see codegen).
    method("perf_hooks", "markResourceTiming", false, None),
    // #1335: returns `fn` unchanged today; the spec'd "wraps fn to
    // record a 'function' timeline entry" piece isn't recorded yet.
    method("perf_hooks", "timerify", false, None),
    // #1336: monitorEventLoopDelay() / createHistogram() return a
    // Histogram-shaped object whose method/property reads route
    // through the internal `perf_histogram` namespace (not listed in
    // NATIVE_MODULES because users never import it — they receive the
    // object as a return value, same pattern as `perf_observer`).
    // Stub — every stat reads 0 and the mutators are no-ops.
    method("perf_hooks", "monitorEventLoopDelay", false, None),
    method("perf_hooks", "createHistogram", false, None),
    property("perf_hooks", "timeOrigin"),
    property("perf_hooks", "nodeTiming"),
    property("perf_hooks", "performance"),
    property("perf_hooks", "constants"),
    class("perf_hooks", "PerformanceObserver"),
    // PerformanceObserver.supportedEntryTypes — static array of entry-type
    // names. Read inline (`PerformanceObserver.supportedEntryTypes.includes(...)`)
    // it resolves as a perf_hooks property; declare it so the read isn't gated.
    property("perf_hooks", "supportedEntryTypes"),
    class("perf_hooks", "PerformanceEntry"),
    class("perf_hooks", "PerformanceMark"),
    class("perf_hooks", "PerformanceMeasure"),
    method("perf_hooks", "observe", true, Some("PerformanceObserver")),
    method(
        "perf_hooks",
        "disconnect",
        true,
        Some("PerformanceObserver"),
    ),
    method(
        "perf_hooks",
        "takeRecords",
        true,
        Some("PerformanceObserver"),
    ),
    // --- buffer (module-level helpers in addition to the Buffer class
    //     already registered above) ---
    method("buffer", "alloc", false, None),
    method("buffer", "allocUnsafe", false, None),
    method("buffer", "allocUnsafeSlow", false, None),
    method("buffer", "from", false, None),
    method("buffer", "of", false, None),
    method("buffer", "concat", false, None),
    method("buffer", "isBuffer", false, None),
    method("buffer", "isEncoding", false, None),
    method("buffer", "byteLength", false, None),
    // Buffer module-level encoding probes added in PR #1257.
    method("buffer", "isAscii", false, None),
    method("buffer", "isUtf8", false, None),
    // Issue #1210: re-encode bytes between supported encodings.
    method("buffer", "transcode", false, None),
    // Issue #1211: Blob / File constructors + object-URL helpers
    // exposed from node:buffer.  Blob/File constructors are recognized
    // by the codegen builtin path, so they only need to appear here
    // as class exports.
    class("buffer", "Blob"),
    class("buffer", "File"),
    method("buffer", "resolveObjectURL", false, None),
    property("buffer", "constants"),
    property("buffer", "kMaxLength"),
    property("buffer", "kStringMaxLength"),
    // --- url (additional helpers) ---
    method("url", "fileURLToPath", false, None),
    method("url", "pathToFileURL", false, None),
    method("url", "domainToASCII", false, None),
    method("url", "domainToUnicode", false, None),
    method("url", "urlToHttpOptions", false, None),
    method("url", "format", false, None),
    method("url", "parse", false, None),
    method("url", "resolve", false, None),
    // Issue #1211: Blob/File object-URL registry — paired with the
    // `resolveObjectURL` export on `node:buffer`.
    method("url", "createObjectURL", false, None),
    method("url", "revokeObjectURL", false, None),
    // --- http (perry-ext-http surface + classes the framework spec
    //     exposes). Both http and https route through the same crate. ---
    method("http", "createServer", false, None),
    method("http", "request", false, None),
    method("http", "get", false, None),
    property("http", "METHODS"),
    property("http", "STATUS_CODES"),
    class("http", "Server"),
    class("http", "ClientRequest"),
    class("http", "IncomingMessage"),
    class("http", "ServerResponse"),
    method("https", "createServer", false, None),
    method("https", "request", false, None),
    method("https", "get", false, None),
    class("https", "Server"),
    class("https", "ClientRequest"),
    class("https", "IncomingMessage"),
    class("https", "ServerResponse"),
    // --- axios (perry-ext-axios) — the npm `axios` HTTP client surface.
    //     The default export is callable (`axios(config)`); both flow
    //     through perry-ext-axios's `js_axios_*` symbols. ---
    method("axios", "default", false, None),
    method("axios", "get", false, None),
    method("axios", "post", false, None),
    method("axios", "put", false, None),
    method("axios", "delete", false, None),
    method("axios", "patch", false, None),
    method("axios", "head", false, None),
    method("axios", "options", false, None),
    method("axios", "request", false, None),
    method("axios", "create", false, None),
    method("axios", "all", false, None),
    // --- node-fetch (perry-ext-fetch) — also exposes the Web Fetch
    //     API classes (Headers, Request, Response, Blob). ---
    method("node-fetch", "default", false, None),
    class("node-fetch", "Headers"),
    class("node-fetch", "Request"),
    class("node-fetch", "Response"),
    class("node-fetch", "Blob"),
    // --- bignumber.js — alias surface for decimal.js. The wrapper
    //     dispatches to the same perry-ext-decimal implementation. ---
    class("bignumber.js", "BigNumber"),
    // --- node-cron — alias for the cron wrapper.
    method("node-cron", "schedule", false, None),
    method("node-cron", "validate", false, None),
    // --- perry/ui constructors + setters. Auto-derivable from
    //     PERRY_UI_TABLE in crates/perry-dispatch/src/lib.rs. The
    //     reverse drift test enforces parity in both directions. ---
    method("perry/ui", "App", false, None),
    method("perry/ui", "Window", false, None),
    method("perry/ui", "VStack", false, None),
    method("perry/ui", "HStack", false, None),
    method("perry/ui", "ZStack", false, None),
    method("perry/ui", "Section", false, None),
    method("perry/ui", "Spacer", false, None),
    method("perry/ui", "Divider", false, None),
    method("perry/ui", "ScrollView", false, None),
    method("perry/ui", "Text", false, None),
    // Issue #710 — AttributedText (per-range styling)
    method("perry/ui", "AttributedText", false, None),
    method("perry/ui", "attributedTextAppend", false, None),
    method("perry/ui", "attributedTextClear", false, None),
    method("perry/ui", "TextField", false, None),
    method("perry/ui", "TextArea", false, None),
    method("perry/ui", "SecureField", false, None),
    method("perry/ui", "Button", false, None),
    method("perry/ui", "Toggle", false, None),
    method("perry/ui", "Slider", false, None),
    method("perry/ui", "ProgressView", false, None),
    method("perry/ui", "Picker", false, None),
    method("perry/ui", "ImageFile", false, None),
    method("perry/ui", "ImageSymbol", false, None),
    method("perry/ui", "Image", false, None),
    method("perry/ui", "LazyVStack", false, None),
    method("perry/ui", "NavStack", false, None),
    method("perry/ui", "TabBar", false, None),
    // Issue #553 — production-mobile widgets
    method("perry/ui", "BottomNavigation", false, None),
    method("perry/ui", "bottomNavAddItem", false, None),
    method("perry/ui", "bottomNavSetBadge", false, None),
    method("perry/ui", "bottomNavSetSelected", false, None),
    method("perry/ui", "bottomNavSetTintColor", false, None),
    method("perry/ui", "bottomNavSetUnselectedTintColor", false, None),
    method("perry/ui", "ImageGallery", false, None),
    method("perry/ui", "imageGalleryAddImage", false, None),
    method("perry/ui", "imageGallerySetIndex", false, None),
    // Issue #658 — WebView (auth flows / payments / embedded HTML)
    method("perry/ui", "WebView", false, None),
    method("perry/ui", "webviewLoadUrl", false, None),
    method("perry/ui", "webviewReload", false, None),
    method("perry/ui", "webviewGoBack", false, None),
    method("perry/ui", "webviewGoForward", false, None),
    method("perry/ui", "webviewCanGoBack", false, None),
    method("perry/ui", "webviewEvaluateJs", false, None),
    method("perry/ui", "webviewClearCookies", false, None),
    method("perry/ui", "scrollviewSetScrollEndCallback", false, None),
    method("perry/ui", "scrollViewSetScrollEndCallback", false, None),
    method("perry/ui", "lazyvstackSetRefreshControl", false, None),
    method("perry/ui", "lazyvstackEndRefreshing", false, None),
    method("perry/ui", "lazyvstackSetScrollEndCallback", false, None),
    method("perry/ui", "Table", false, None),
    method("perry/ui", "Canvas", false, None),
    method("perry/ui", "CameraView", false, None),
    method("perry/ui", "cameraStart", false, None),
    method("perry/ui", "cameraStop", false, None),
    method("perry/ui", "cameraFreeze", false, None),
    method("perry/ui", "cameraUnfreeze", false, None),
    method("perry/ui", "cameraSampleColor", false, None),
    method("perry/ui", "cameraSetOnTap", false, None),
    method("perry/ui", "cameraRegisterFrameCallback", false, None),
    method("perry/ui", "cameraUnregisterFrameCallback", false, None),
    method("perry/ui", "SplitView", false, None),
    method("perry/ui", "ForEach", false, None),
    method("perry/ui", "State", false, None),
    method("perry/ui", "VStackWithInsets", false, None),
    method("perry/ui", "HStackWithInsets", false, None),
    method("perry/ui", "showToast", false, None),
    method("perry/ui", "setText", false, None),
    method("perry/ui", "alert", false, None),
    method("perry/ui", "alertWithButtons", false, None),
    method("perry/ui", "menuCreate", false, None),
    method("perry/ui", "menuAddItem", false, None),
    method("perry/ui", "menuAddSeparator", false, None),
    method("perry/ui", "menuAddSubmenu", false, None),
    method("perry/ui", "menuAddStandardAction", false, None),
    method("perry/ui", "menuAddItemWithShortcut", false, None),
    method("perry/ui", "menuClear", false, None),
    method("perry/ui", "menuBarCreate", false, None),
    method("perry/ui", "menuBarAddMenu", false, None),
    method("perry/ui", "menuBarAttach", false, None),
    method("perry/ui", "trayCreate", false, None),
    method("perry/ui", "traySetIcon", false, None),
    method("perry/ui", "traySetTooltip", false, None),
    method("perry/ui", "trayAttachMenu", false, None),
    method("perry/ui", "trayOnClick", false, None),
    method("perry/ui", "trayDestroy", false, None),
    method("perry/ui", "toolbarCreate", false, None),
    method("perry/ui", "toolbarAddItem", false, None),
    method("perry/ui", "toolbarAttach", false, None),
    method("perry/ui", "openFileDialog", false, None),
    method("perry/ui", "openFolderDialog", false, None),
    method("perry/ui", "saveFileDialog", false, None),
    method("perry/ui", "pollOpenFile", false, None),
    method("perry/ui", "clipboardRead", false, None),
    method("perry/ui", "clipboardWrite", false, None),
    method("perry/ui", "addKeyboardShortcut", false, None),
    method("perry/ui", "registerGlobalHotkey", false, None),
    // Continuous keyboard events (issue #1864).
    method("perry/ui", "onKeyDown", false, None),
    method("perry/ui", "onKeyUp", false, None),
    method("perry/ui", "onAppKeyDown", false, None),
    method("perry/ui", "onAppKeyUp", false, None),
    method("perry/ui", "focus", false, None),
    method("perry/ui", "blur", false, None),
    method("perry/ui", "isKeyDown", false, None),
    method("perry/ui", "currentModifiers", false, None),
    method("perry/ui", "onTerminate", false, None),
    method("perry/ui", "onActivate", false, None),
    method("perry/ui", "appSetTimer", false, None),
    method("perry/ui", "appSetMinSize", false, None),
    method("perry/ui", "appSetMaxSize", false, None),
    method("perry/ui", "embedNSView", false, None),
    method("perry/ui", "sheetCreate", false, None),
    method("perry/ui", "sheetPresent", false, None),
    method("perry/ui", "sheetDismiss", false, None),
    method("perry/ui", "frameSplitCreate", false, None),
    method("perry/ui", "frameSplitAddChild", false, None),
    // --- perry/system — auto-derivable from PERRY_SYSTEM_TABLE. ---
    method("perry/system", "isDarkMode", false, None),
    method("perry/system", "getDeviceIdiom", false, None),
    method("perry/system", "getDeviceModel", false, None),
    // Bug-report-flow utility: stable OS-version string per
    // platform (e.g. `"15.2"`, `"macOS 14.5"`, `"Android 14"`).
    // Common need for crash reports and telemetry; pairs with
    // getDeviceModel / getAppVersion.
    method("perry/system", "getOSVersion", false, None),
    method("perry/system", "getLocale", false, None),
    method("perry/system", "getAppVersion", false, None),
    method("perry/system", "getAppBuildNumber", false, None),
    method("perry/system", "getBundleId", false, None),
    method("perry/system", "getAppIcon", false, None),
    method("perry/system", "openURL", false, None),
    // #917 — system share sheet (UIActivityViewController on iOS,
    // NSSharingServicePicker on macOS, Intent.ACTION_SEND on
    // Android). Two convenience entry points cover the common
    // shapes: plain text + URL.
    method("perry/system", "shareText", false, None),
    method("perry/system", "shareUrl", false, None),
    // #675 — App Group / cross-process shared storage. Widget
    // extensions, share extensions, watchOS targets, etc. all need
    // a way to share key/value data with the host app. macOS/iOS:
    // `UserDefaults(suiteName:)`. Android: scoped SharedPreferences
    // (follow-up). Every other platform: an in-process HashMap
    // fallback so the API surface is exercisable in dev/tests; not
    // actually cross-process there. Follow-up tracker: #675.
    method("perry/system", "appGroupSet", false, None),
    method("perry/system", "appGroupGet", false, None),
    method("perry/system", "appGroupDelete", false, None),
    method("perry/system", "keychainSave", false, None),
    method("perry/system", "keychainGet", false, None),
    method("perry/system", "keychainDelete", false, None),
    method("perry/system", "preferencesGet", false, None),
    method("perry/system", "preferencesSet", false, None),
    method("perry/system", "notificationSend", false, None),
    method("perry/system", "notificationCancel", false, None),
    method("perry/system", "notificationOnTap", false, None),
    method("perry/system", "notificationOnReceive", false, None),
    method(
        "perry/system",
        "notificationOnBackgroundReceive",
        false,
        None,
    ),
    method("perry/system", "notificationRegisterRemote", false, None),
    method("perry/system", "audioStart", false, None),
    method("perry/system", "audioStop", false, None),
    method("perry/system", "audioGetLevel", false, None),
    method("perry/system", "audioGetPeak", false, None),
    method("perry/system", "audioGetWaveform", false, None),
    method("perry/system", "audioSetOutputFilename", false, None),
    method("perry/system", "audioRegisterCallback", false, None),
    method("perry/system", "audioUnregisterCallback", false, None),
    method("perry/system", "audioStartRecording", false, None),
    method("perry/system", "audioStopRecording", false, None),
    // --- perry/system geolocation + image picker (issue #552). ---
    method("perry/system", "geolocationGetCurrent", false, None),
    method("perry/system", "geolocationWatch", false, None),
    method("perry/system", "geolocationStopWatch", false, None),
    method("perry/system", "geolocationRequestPermission", false, None),
    method("perry/system", "imagePickerPick", false, None),
    // --- perry/system in-app screen capture (issue #918). ---
    method("perry/system", "takeScreenshot", false, None),
    // --- perry/system network reachability (issue #582). ---
    method("perry/system", "networkGetStatus", false, None),
    method("perry/system", "networkOnChange", false, None),
    method("perry/system", "networkStopOnChange", false, None),
    // --- perry/system deep links (issue #583). ---
    method("perry/system", "appOnOpenUrl", false, None),
    method("perry/system", "appGetLaunchUrl", false, None),
    // --- perry/background (issue #538) — BGTaskScheduler / WorkManager. ---
    method("perry/background", "registerTask", false, None),
    method("perry/background", "schedule", false, None),
    method("perry/background", "cancel", false, None),
    // --- perry/i18n — auto-derivable from PERRY_I18N_TABLE. ---
    method("perry/i18n", "t", false, None),
    method("perry/i18n", "Currency", false, None),
    method("perry/i18n", "Percent", false, None),
    method("perry/i18n", "FormatNumber", false, None),
    method("perry/i18n", "FormatTime", false, None),
    method("perry/i18n", "ShortDate", false, None),
    method("perry/i18n", "LongDate", false, None),
    method("perry/i18n", "Raw", false, None),
    // --- perry/updater — auto-derivable from PERRY_UPDATER_TABLE. ---
    method("perry/updater", "compareVersions", false, None),
    method("perry/updater", "verifyHash", false, None),
    method("perry/updater", "verifySignature", false, None),
    method("perry/updater", "verifySignatureV2", false, None),
    method("perry/updater", "computeFileSha256", false, None),
    method("perry/updater", "writeSentinel", false, None),
    method("perry/updater", "readSentinel", false, None),
    method("perry/updater", "clearSentinel", false, None),
    method("perry/updater", "getExePath", false, None),
    method("perry/updater", "getBackupPath", false, None),
    method("perry/updater", "getSentinelPath", false, None),
    method("perry/updater", "installUpdate", false, None),
    method("perry/updater", "performRollback", false, None),
    method("perry/updater", "relaunch", false, None),
    // --- perry/media — auto-derivable from PERRY_MEDIA_TABLE. ---
    method("perry/media", "createPlayer", false, None),
    method("perry/media", "play", false, None),
    method("perry/media", "pause", false, None),
    method("perry/media", "stop", false, None),
    method("perry/media", "seek", false, None),
    method("perry/media", "setVolume", false, None),
    method("perry/media", "setRate", false, None),
    method("perry/media", "getCurrentTime", false, None),
    method("perry/media", "getDuration", false, None),
    method("perry/media", "getState", false, None),
    method("perry/media", "isPlaying", false, None),
    method("perry/media", "onStateChange", false, None),
    method("perry/media", "onTimeUpdate", false, None),
    method("perry/media", "setNowPlaying", false, None),
    method("perry/media", "destroy", false, None),
    // --- perry/audio (issue #1867) — auto-derivable from PERRY_AUDIO_TABLE. ---
    method("perry/audio", "loadSound", false, None),
    method("perry/audio", "unload", false, None),
    method("perry/audio", "play", false, None),
    method("perry/audio", "stop", false, None),
    method("perry/audio", "pause", false, None),
    method("perry/audio", "resume", false, None),
    method("perry/audio", "setVolume", false, None),
    method("perry/audio", "setRate", false, None),
    method("perry/audio", "setPan", false, None),
    method("perry/audio", "fadeIn", false, None),
    method("perry/audio", "fadeOut", false, None),
    method("perry/audio", "crossfade", false, None),
    method("perry/audio", "createBus", false, None),
    method("perry/audio", "destroyBus", false, None),
    method("perry/audio", "muteBus", false, None),
    method("perry/audio", "soloBus", false, None),
    method("perry/audio", "setMasterVolume", false, None),
    method("perry/audio", "suspend", false, None),
    method("perry/audio", "resumeAll", false, None),
    method("perry/audio", "isPlaying", false, None),
    method("perry/audio", "getDuration", false, None),
    method("perry/audio", "getPosition", false, None),
    method("perry/audio", "onEnded", false, None),
    method("perry/audio", "onLoaded", false, None),
    // --- perry/plugin — host-side functions (PERRY_PLUGIN_TABLE in
    //     lower_call.rs). Instance methods on PluginApi are tracked on
    //     class_filter rows — see perry/plugin's PluginApi class. ---
    method("perry/plugin", "loadPlugin", false, None),
    method("perry/plugin", "unloadPlugin", false, None),
    method("perry/plugin", "emitHook", false, None),
    method("perry/plugin", "emitEvent", false, None),
    method("perry/plugin", "invokeTool", false, None),
    method("perry/plugin", "setPluginConfig", false, None),
    method("perry/plugin", "discoverPlugins", false, None),
    method("perry/plugin", "listPlugins", false, None),
    method("perry/plugin", "listHooks", false, None),
    method("perry/plugin", "listTools", false, None),
    method("perry/plugin", "pluginCount", false, None),
    method("perry/plugin", "initPlugins", false, None),
    class("perry/plugin", "PluginApi"),
    // --- perry/widget — declarative widget-extension entrypoint
    //     (iOS WidgetKit / Android home-screen widgets). One callable
    //     export `Widget(config)` produces a WidgetDecl in HIR; see
    //     try_lower_widget_decl in perry-hir/src/lower.rs. ---
    method("perry/widget", "Widget", false, None),
    // --- redis — alias for ioredis (well-known table routes both to
    //     perry-ext-ioredis). The Redis class instance methods come
    //     from the ioredis class entries. ---
    class("redis", "Redis"),
    method("redis", "createClient", false, None),
    // --- date-fns — alias for dayjs (well-known routes both to
    //     perry-ext-dayjs). Surface methods are the date-fns
    //     functional API exposed by the wrapper. ---
    method("date-fns", "format", false, None),
    method("date-fns", "parseISO", false, None),
    method("date-fns", "addDays", false, None),
    method("date-fns", "addMonths", false, None),
    method("date-fns", "addYears", false, None),
    method("date-fns", "differenceInDays", false, None),
    method("date-fns", "differenceInHours", false, None),
    method("date-fns", "differenceInMinutes", false, None),
    method("date-fns", "isAfter", false, None),
    method("date-fns", "isBefore", false, None),
    method("date-fns", "startOfDay", false, None),
    method("date-fns", "endOfDay", false, None),
    // --- rate-limiter-flexible — perry-ext-ratelimit. Surface mirrors
    //     the npm package's RateLimiterMemory class. ---
    class("rate-limiter-flexible", "RateLimiterMemory"),
    class("rate-limiter-flexible", "RateLimiterAbstract"),
    // --- fetch — well-known alias for perry-ext-fetch. Same surface
    //     as node-fetch (the more common alias above). ---
    method("fetch", "default", false, None),
    class("fetch", "Headers"),
    class("fetch", "Request"),
    class("fetch", "Response"),
    class("fetch", "Blob"),
    // --- streams — Web Streams API umbrella (perry-ext-streams). ---
    class("streams", "ReadableStream"),
    class("streams", "WritableStream"),
    class("streams", "TransformStream"),
    class("streams", "TextEncoder"),
    class("streams", "TextDecoder"),
    class("streams", "DecompressionStream"),
    // node:stream/web QueuingStrategy classes (#1545).
    class("streams", "ByteLengthQueuingStrategy"),
    class("streams", "CountQueuingStrategy"),
    // --- node:http server (issue #577) ---
    method("http", "createServer", false, None),
    method("http", "listen", true, Some("HttpServer")),
    method("http", "close", true, Some("HttpServer")),
    method("http", "closeAllConnections", true, Some("HttpServer")),
    method("http", "closeIdleConnections", true, Some("HttpServer")),
    method("http", "on", true, Some("HttpServer")),
    method("http", "addListener", true, Some("HttpServer")),
    method("http", "on", true, Some("IncomingMessage")),
    method("http", "addListener", true, Some("IncomingMessage")),
    method("http", "pause", true, Some("IncomingMessage")),
    method("http", "resume", true, Some("IncomingMessage")),
    method("http", "destroy", true, Some("IncomingMessage")),
    method("http", "read", true, Some("IncomingMessage")),
    // Issue #769 — `ClientRequest.setTimeout(ms)` for `http.request` /
    // `http.get` returns. Class filter differs from any existing http
    // method, so the manifest-consistency drift guard requires a row
    // here even though the test collapses class_filter variants.
    method("http", "setTimeout", true, Some("ClientRequest")),
    method("http", "setHeader", true, Some("ServerResponse")),
    method("http", "getHeader", true, Some("ServerResponse")),
    method("http", "removeHeader", true, Some("ServerResponse")),
    method("http", "hasHeader", true, Some("ServerResponse")),
    method("http", "writeHead", true, Some("ServerResponse")),
    method("http", "write", true, Some("ServerResponse")),
    method("http", "addTrailers", true, Some("ServerResponse")),
    method("http", "end", true, Some("ServerResponse")),
    method("http", "flushHeaders", true, Some("ServerResponse")),
    method("http", "writeContinue", true, Some("ServerResponse")),
    method("http", "writeProcessing", true, Some("ServerResponse")),
    method("http", "on", true, Some("ServerResponse")),
    method("http", "addListener", true, Some("ServerResponse")),
    method("http", "method", true, Some("IncomingMessage")),
    method("http", "url", true, Some("IncomingMessage")),
    method("http", "httpVersion", true, Some("IncomingMessage")),
    method("http", "statusCode", true, Some("IncomingMessage")),
    method("http", "statusMessage", true, Some("IncomingMessage")),
    method("http", "headers", true, Some("IncomingMessage")),
    method("http", "trailers", true, Some("IncomingMessage")),
    method("http", "setStatus", true, Some("ServerResponse")),
    method("http", "getStatus", true, Some("ServerResponse")),
    method("http", "__get_method", true, Some("IncomingMessage")),
    method("http", "__get_url", true, Some("IncomingMessage")),
    method("http", "__get_httpVersion", true, Some("IncomingMessage")),
    method("http", "__get_complete", true, Some("IncomingMessage")),
    method("http", "__get_aborted", true, Some("IncomingMessage")),
    method("http", "__get_destroyed", true, Some("IncomingMessage")),
    method("http", "__get_statusCode", true, Some("IncomingMessage")),
    method("http", "__get_statusMessage", true, Some("IncomingMessage")),
    method("http", "__get_headers", true, Some("IncomingMessage")),
    method("http", "__get_trailers", true, Some("IncomingMessage")),
    method("http", "__get_statusCode", true, Some("ServerResponse")),
    method("http", "__set_statusCode", true, Some("ServerResponse")),
    method("http", "__set_statusMessage", true, Some("ServerResponse")),
    method("http", "__get_headersSent", true, Some("ServerResponse")),
    method("http", "__get_writableEnded", true, Some("ServerResponse")),
    method(
        "http",
        "__get_writableFinished",
        true,
        Some("ServerResponse"),
    ),
    class("http", "Server"),
    class("http", "IncomingMessage"),
    class("http", "ServerResponse"),
    // --- node:https server (issue #577 Phase 2) ---
    method("https", "createServer", false, None),
    method("https", "listen", true, Some("HttpsServer")),
    method("https", "close", true, Some("HttpsServer")),
    method("https", "on", true, Some("HttpsServer")),
    class("https", "Server"),
    // --- node:http2 server (issue #577 Phase 3) ---
    method("http2", "createSecureServer", false, None),
    method("http2", "listen", true, Some("Http2SecureServer")),
    method("http2", "close", true, Some("Http2SecureServer")),
    method("http2", "on", true, Some("Http2SecureServer")),
    class("http2", "Http2SecureServer"),
    class("http2", "Http2ServerRequest"),
    class("http2", "Http2ServerResponse"),
    // `http2.constants` — the frozen object of HTTP2_HEADER_* / NGHTTP2_* /
    // HTTP_STATUS_* values. `@hono/node-server` imports it by name (#1651).
    property("http2", "constants"),
    // `@perryts/google-auth` no longer ships in the bundled manifest —
    // since v0.5.1015 it lives at https://github.com/PerryTS/google-auth
    // and is installed via `npm install @perryts/google-auth`. The
    // package's own `perry.nativeLibrary.functions` declares the FFI
    // surface; the manifest's unimplemented-API check resolves the
    // import via the standard external-nativeLibrary lookup.
    // --- @perryts/pdf (issue #516) ---
    // Minimal PDF creation API. The five FFI entry points exported
    // by crates/perry-ext-pdf. Param shapes intentionally loose
    // here (mostly `p_any`) — codegen's NATIVE_MODULE_TABLE rows
    // tighten them. createPdf takes a single options object and
    // returns a numeric handle; pdfAddText/pdfAddLine accept
    // positional args.
    method_sig(
        "@perryts/pdf",
        "createPdf",
        false,
        None,
        &[p_any("opts")],
        TypeSpec::Number,
    ),
    method("@perryts/pdf", "pdfAddText", false, None),
    method("@perryts/pdf", "pdfAddLine", false, None),
    method("@perryts/pdf", "pdfNewPage", false, None),
    method("@perryts/pdf", "pdfSave", false, None),
    // --- perry/ads (issue #867) ---
    // Six FFI entry points exported by crates/perry-ext-ads.
    // Promise-returning load / show pairs for interstitial and
    // rewarded ads; sync handle-returning create + destroy pair
    // for the banner widget. Listed here so the manifest's
    // unimplemented-API check (#463) accepts them when a user
    // writes `import { js_ads_interstitial_show } from "perry/ads"`.
    // The MVP returns structured `{ error: "no-sdk-linked" }`
    // placeholders; real Google Mobile Ads SDK integration is
    // tracked under the same issue.
    method("perry/ads", "js_ads_interstitial_load", false, None),
    method("perry/ads", "js_ads_interstitial_show", false, None),
    method("perry/ads", "js_ads_rewarded_load", false, None),
    method("perry/ads", "js_ads_rewarded_show", false, None),
    method("perry/ads", "js_ads_banner_create", false, None),
    method("perry/ads", "js_ads_banner_destroy", false, None),
];
