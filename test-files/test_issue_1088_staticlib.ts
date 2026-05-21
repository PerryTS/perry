// Issue #1088 — `--output-type staticlib` + unified Event Loop FFI.
//
// Minimal TS surface to validate the staticlib link path. The companion
// `run_test_issue_1088.sh` script:
//   1. Compiles this with `--output-type staticlib`, producing a
//      `libperry_tslib.a` whose only exported entry point is
//      `perry_module_init` (no `main` symbol — that would collide with
//      the host's own main).
//   2. Links a small C smoke host against the archive + libperry_runtime.a,
//      calls `perry_module_init`, then walks `perry_poll` / `perry_has_work`
//      / `perry_next_wake_ms` / `perry_set_wake_callback` to prove the host
//      embedding FFI is reachable and behaves.
//
// The TS body itself is intentionally trivial — the whole point of the
// staticlib mode is that the *host* drives the event loop, so the TS
// surface only needs to print on init to confirm `perry_module_init`
// actually ran.
console.log("[ts] module init: hello from staticlib");
