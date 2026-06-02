// Issue #557: zero-arg `console.log()` emitted nothing — should emit
// a newline (matches Node/bun). The codegen catch-all in
// `crates/perry-codegen/src/lower_call.rs` for the entire console.*
// surface returned undefined immediately when args was empty for any
// non-special method, so log/info/warn/error/debug/etc. all silently
// no-op'd at zero arity.
//
// Fix routes log/info/debug → js_console_log_spread(0),
// warn → js_console_warn_spread(0), error → js_console_error_spread(0).
// All three runtime fns already print a single newline to the right
// stream when their arg is null.

console.log("a");
console.log();
console.log("b");
console.log("");
console.log("c");

// info / debug share the log path.
console.info("info-a");
console.info();
console.info("info-b");

console.debug("debug-a");
console.debug();
console.debug("debug-b");

// `console.log` with multiple args still works (regression check for
// the multi-arg path).
console.log("x", "y", "z");
