use super::*;

pub(super) const NODE_MISC_ROWS: &[NativeModSig] = &[
    // ========== node:cluster ==========
    // Primary lifecycle methods mutate runtime state, so ordinary
    // `cluster.setupPrimary(...)` / `.fork(...)` call syntax must route to
    // the same helpers as captured callable exports.
    NativeModSig {
        module: "cluster",
        has_receiver: false,
        method: "setupPrimary",
        class_filter: None,
        runtime: "js_cluster_setup_primary",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "cluster",
        has_receiver: false,
        method: "setupMaster",
        class_filter: None,
        runtime: "js_cluster_setup_primary",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "cluster",
        has_receiver: false,
        method: "fork",
        class_filter: None,
        runtime: "js_cluster_fork",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "cluster",
        has_receiver: false,
        method: "disconnect",
        class_filter: None,
        runtime: "js_cluster_disconnect",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== node:vm ==========
    // Minimal contextification surface for APIs that require a vm context
    // object but do not execute code inside it yet.
    NativeModSig {
        module: "vm",
        has_receiver: false,
        method: "createContext",
        class_filter: None,
        runtime: "js_vm_create_context",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== node:repl ==========
    NativeModSig {
        module: "repl",
        has_receiver: false,
        method: "start",
        class_filter: None,
        runtime: "js_repl_start",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "repl",
        has_receiver: false,
        method: "REPLServer",
        class_filter: None,
        runtime: "js_repl_repl_server_new",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "repl",
        has_receiver: false,
        method: "Recoverable",
        class_filter: None,
        runtime: "js_repl_recoverable_new",
        args: &[NA_F64],
        ret: NR_F64,
    },
    // ========== node:querystring ==========
    // Module-level functions. `decode` / `encode` route to the same
    // runtime symbols as `parse` / `stringify` so the test's
    // `decode === parse` identity-equality check passes.
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "escape",
        class_filter: None,
        runtime: "js_querystring_escape",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "unescape",
        class_filter: None,
        runtime: "js_querystring_unescape",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "unescapeBuffer",
        class_filter: None,
        runtime: "js_querystring_unescape_buffer",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "parse",
        class_filter: None,
        runtime: "js_querystring_parse",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "decode",
        class_filter: None,
        runtime: "js_querystring_parse",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "stringify",
        class_filter: None,
        runtime: "js_querystring_stringify",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "querystring",
        has_receiver: false,
        method: "encode",
        class_filter: None,
        runtime: "js_querystring_stringify",
        args: &[NA_F64, NA_F64, NA_F64, NA_F64],
        ret: NR_F64,
    },
    // ========== LRU Cache ==========
    NativeModSig {
        module: "lru-cache",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_lru_cache_new",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "get",
        class_filter: None,
        runtime: "js_lru_cache_get",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "set",
        class_filter: None,
        runtime: "js_lru_cache_set",
        args: &[NA_F64, NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "has",
        class_filter: None,
        runtime: "js_lru_cache_has",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "delete",
        class_filter: None,
        runtime: "js_lru_cache_delete",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "clear",
        class_filter: None,
        runtime: "js_lru_cache_clear",
        args: &[],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "lru-cache",
        has_receiver: true,
        method: "size",
        class_filter: None,
        runtime: "js_lru_cache_size",
        args: &[],
        ret: NR_F64,
    },
    // ========== commander (CLI parsing) ==========
    // `new Command()` is dispatched separately by `lower_builtin_new` so it
    // produces a real CommanderHandle instead of an empty placeholder. The
    // entries below cover the fluent chain methods + the parse() entry that
    // actually reads argv and fires the registered .action() callback.
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "name",
        class_filter: None,
        runtime: "js_commander_name",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "description",
        class_filter: None,
        runtime: "js_commander_description",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "version",
        class_filter: None,
        runtime: "js_commander_version",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "command",
        class_filter: None,
        runtime: "js_commander_command",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "option",
        class_filter: None,
        runtime: "js_commander_option",
        args: &[NA_STR, NA_STR, NA_STR],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "requiredOption",
        class_filter: None,
        runtime: "js_commander_required_option",
        args: &[NA_STR, NA_STR, NA_STR],
        ret: NR_PTR,
    },
    // .action(cb) — NA_PTR coerces the NaN-boxed closure to its raw i64
    // pointer so the runtime can call back through `js_closure_call1`.
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "action",
        class_filter: None,
        runtime: "js_commander_action",
        args: &[NA_PTR],
        ret: NR_PTR,
    },
    // .parse(argv) — runtime reads std::env::args() directly; user-provided
    // argv expression evaluates for side effects but is not forwarded.
    // NA_F64 keeps the LLVM call signature aligned with the runtime decl
    // (`(I64, DOUBLE) -> I64`).
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "parse",
        class_filter: None,
        runtime: "js_commander_parse",
        args: &[NA_F64],
        ret: NR_PTR,
    },
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "opts",
        class_filter: None,
        runtime: "js_commander_opts",
        args: &[],
        ret: NR_PTR,
    },
    // `.argument("<file>")` declares a positional; returns the same handle so
    // the fluent chain continues (#5137).
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "argument",
        class_filter: None,
        runtime: "js_commander_argument",
        args: &[NA_STR],
        ret: NR_PTR,
    },
    // `program.args` — a bare member read lowers to this 0-arg getter, which
    // returns a JS array of the parsed positional arguments (#5137).
    NativeModSig {
        module: "commander",
        has_receiver: true,
        method: "args",
        class_filter: None,
        runtime: "js_commander_args_array",
        args: &[],
        ret: NR_PTR,
    },
];
