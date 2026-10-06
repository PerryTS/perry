use super::*;

pub(super) const DATABASES_ROWS: &[NativeModSig] = &[
    // ========== better-sqlite3 ==========
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: false,
        method: "default",
        class_filter: None,
        runtime: "js_sqlite_open",
        args: &[NA_STR],
        ret: NR_HANDLE_ID,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "prepare",
        class_filter: None,
        runtime: "js_sqlite_prepare",
        args: &[NA_STR],
        ret: NR_HANDLE_ID,
    },
    // stmt.run/get/all/iterate take JS-side variadic params. The runtime
    // consumes them as a single `*const ArrayHeader`, so VarArgsAsArray
    // packs every user-supplied arg into a real JS array before the call.
    // Pre-#339 these used `NA_F64` and the runtime had to defensively
    // bail when the high-16 bits looked like a NaN-box tag — fine for
    // the no-arg case (TAG_UNDEFINED), but `.all('a')` passed a
    // STRING-tagged f64 that also tripped the bail and the params were
    // silently dropped.
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "run",
        class_filter: None,
        runtime: "js_sqlite_stmt_run",
        args: &[NA_VARARGS],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "get",
        class_filter: None,
        runtime: "js_sqlite_stmt_get",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "all",
        class_filter: None,
        runtime: "js_sqlite_stmt_all",
        args: &[NA_VARARGS],
        ret: NR_GCPTR,
    },
    // `stmt.raw([toggle])` — flips the statement into raw mode and
    // returns the same handle so `stmt.raw().all(...)` chains. drizzle's
    // PreparedQuery.values() relies on this; without it `stmt.raw` is
    // undefined and the call surfaces as `(number).all is not a
    // function` deeper in the chain. Refs #643. The optional `toggle`
    // arg isn't threaded through the dispatch yet (always enables);
    // extend `args` if a real downstream needs `.raw(false)`.
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "raw",
        class_filter: None,
        runtime: "js_sqlite_stmt_raw",
        args: &[],
        ret: NR_HANDLE_ID,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "exec",
        class_filter: None,
        runtime: "js_sqlite_exec",
        args: &[NA_STR],
        ret: NR_VOID,
    },
    NativeModSig {
        module: "better-sqlite3",
        has_receiver: true,
        method: "close",
        class_filter: None,
        runtime: "js_sqlite_close",
        args: &[],
        ret: NR_VOID,
    },
    // ========== bun:sqlite ==========
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: false,
        method: "Database",
        class_filter: None,
        runtime: "js_bun_sqlite_database_call",
        args: &[NA_F64, NA_F64],
        ret: NR_HANDLE_ID,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "query",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_query",
        args: &[NA_F64],
        ret: NR_HANDLE_ID,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "prepare",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_query",
        args: &[NA_F64],
        ret: NR_HANDLE_ID,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "run",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_run",
        args: &[NA_F64, NA_VARARGS],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "close",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_close",
        args: &[],
        ret: NR_I32,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "serialize",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_serialize",
        args: &[NA_F64],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "loadExtension",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_load_extension",
        args: &[NA_F64],
        ret: NR_I32,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "transaction",
        class_filter: Some("Database"),
        runtime: "js_bun_sqlite_database_transaction",
        args: &[NA_F64],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "run",
        class_filter: Some("Statement"),
        runtime: "js_bun_sqlite_statement_run",
        args: &[NA_VARARGS],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "get",
        class_filter: Some("Statement"),
        runtime: "js_bun_sqlite_statement_get",
        args: &[NA_VARARGS],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "all",
        class_filter: Some("Statement"),
        runtime: "js_bun_sqlite_statement_all",
        args: &[NA_VARARGS],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "values",
        class_filter: Some("Statement"),
        runtime: "js_bun_sqlite_statement_values",
        args: &[NA_VARARGS],
        ret: NR_GCPTR,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "safeIntegers",
        class_filter: Some("Statement"),
        runtime: "js_bun_sqlite_statement_safe_integers",
        args: &[NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "bun:sqlite",
        has_receiver: true,
        method: "finalize",
        class_filter: Some("Statement"),
        runtime: "js_bun_sqlite_statement_finalize",
        args: &[],
        ret: NR_VOID,
    },
    // ========== node:sqlite ==========
    // DatabaseSync, StatementSync, StatementSyncIterator, SQLTagStore and
    // Session are ordinary objects (#11919): every receiver method resolves
    // from the family prototype. Only the call forms and `backup` stay here.
    NativeModSig {
        module: "sqlite",
        has_receiver: false,
        method: "DatabaseSync",
        class_filter: None,
        runtime: "js_node_sqlite_database_sync_call",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "sqlite",
        has_receiver: false,
        method: "Session",
        class_filter: None,
        runtime: "js_node_sqlite_session_call",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "sqlite",
        has_receiver: false,
        method: "StatementSync",
        class_filter: None,
        runtime: "js_node_sqlite_statement_sync_call",
        args: &[NA_F64, NA_F64],
        ret: NR_F64,
    },
    NativeModSig {
        module: "sqlite",
        has_receiver: false,
        method: "backup",
        class_filter: None,
        runtime: "js_node_sqlite_backup",
        args: &[NA_F64, NA_F64, NA_F64],
        ret: NR_PROMISE,
    },
];
