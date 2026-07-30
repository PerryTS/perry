//! `undici` dispatch rows (#466) — served by the bundled
//! perry-ext-undici wrapper over perry's native fetch stack.
//!
//! Constructors are receiver-less rows reached through the bare-ident
//! `new ProxyAgent(...)` / `new Agent(...)` arm in
//! `perry-hir/src/lower/expr_new.rs` (mirrors the `http`/`https`
//! `Agent` route). `setGlobalDispatcher(agent)` pushes the agent's
//! proxy config into the shared fetch client state
//! (`js_fetch_set_global_proxy`); `fetch` from `'undici'` never lands
//! here — the name-keyed arm in `expr_call/globals.rs` lowers it to
//! `Expr::FetchWithOptions` like the global fetch. `request` is a
//! clear-error reject (perry serves the dispatcher/fetch subset).

use super::*;

pub(super) const UNDICI_ROWS: &[NativeModSig] = &[
    // ── constructors ───────────────────────────────────────────────
    // `new ProxyAgent(uri | { uri, token? })` — the NA_STR coercion
    // (`js_value_to_str_ptr_for_ffi`) passes a URI string through and
    // JSON-stringifies an options object; the wrapper parses either.
    NativeModSig {
        module: "undici",
        has_receiver: false,
        method: "ProxyAgent",
        class_filter: None,
        runtime: "js_undici_proxy_agent_new",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "undici",
        has_receiver: false,
        method: "Agent",
        class_filter: None,
        runtime: "js_undici_agent_new",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // ── module-level functions ─────────────────────────────────────
    NativeModSig {
        module: "undici",
        has_receiver: false,
        method: "setGlobalDispatcher",
        class_filter: None,
        runtime: "js_undici_set_global_dispatcher",
        args: &[NA_PTR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "undici",
        has_receiver: false,
        method: "getGlobalDispatcher",
        class_filter: None,
        runtime: "js_undici_get_global_dispatcher",
        args: &[],
        ret: NR_PTR,
    },
    // `request(url, options?)` — rejects with a "not implemented, use
    // fetch" error; the row exists so users get that clear message at
    // runtime instead of a silent fall-through.
    NativeModSig {
        module: "undici",
        has_receiver: false,
        method: "request",
        class_filter: None,
        runtime: "js_undici_request",
        args: &[NA_STR, NA_STR],
        ret: NR_PROMISE,
    },
    // ── Agent / ProxyAgent instance methods ────────────────────────
    NativeModSig {
        module: "undici",
        has_receiver: true,
        method: "close",
        class_filter: Some("ProxyAgent"),
        runtime: "js_undici_agent_close",
        args: &[],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "undici",
        has_receiver: true,
        method: "close",
        class_filter: Some("Agent"),
        runtime: "js_undici_agent_close",
        args: &[],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "undici",
        has_receiver: true,
        method: "destroy",
        class_filter: Some("ProxyAgent"),
        runtime: "js_undici_agent_destroy",
        args: &[],
        ret: NR_PROMISE,
    },
    NativeModSig {
        module: "undici",
        has_receiver: true,
        method: "destroy",
        class_filter: Some("Agent"),
        runtime: "js_undici_agent_destroy",
        args: &[],
        ret: NR_PROMISE,
    },
];
