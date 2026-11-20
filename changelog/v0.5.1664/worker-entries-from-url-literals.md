Worker files named by `new URL("<literal>", import.meta.url)` anywhere in a
program are compiled in as worker entries.

The compiler used to find a Worker's file only at the `new Worker(...)` call
site, with a small evaluator of module-local helpers. It could not follow a
URL built by a helper imported from another module, `entry ??= helper()`,
`options.entry ?? helper()`, or a Worker constructed through the
`worker_threads` namespace value. upm does all of these: `src/workers.ts`
builds the three worker URLs, and the pools construct their Workers through
`process.getBuiltinModule("node:worker_threads")`.

Now, in a program that uses worker_threads (it imports the module, names it in
a string literal, or constructs a Worker), every `new URL("<literal>",
import.meta.url)` whose literal names a script (`.ts .tsx .mts .cts .js .mjs
.cjs`) is a worker entry candidate. Its module gets a dynamic edge from the
module that holds the literal, so it is compiled but runs only when a Worker
starts it, and it is registered in the run-time worker entry table, where any
Worker construction finds it. A candidate whose module fails to compile (a
template or data file that only looks like a script) is a compile-time warning,
not an error. A program without worker_threads compiles no extra files.

A program whose only worker entries come from URL literals now also gets
per-thread module state (`program_has_worker`): each worker initializes its own
copy of every module it imports, as a program with a `new Worker` site already
did.

New: `perry_hir::worker_url_literals` and `perry_hir::module_uses_worker_threads`
(`dynamic_import/worker_entries.rs`), the driver pass
`collect_modules/worker_url.rs`.

Tests: `test_gap_worker_entry_from_url_helper`,
`test_gap_worker_shared_module_state`, `test_gap_worker_url_not_a_worker`, and
HIR unit tests for discovery and the worker_threads gate.
