// Regression test for #347 Phase 2: process.stdin.setRawMode + on('data', ...)
// + on('keypress', ...). Verifies that:
//   1. `process.stdin.setRawMode(boolean)` lowers to a direct extern
//      call to `js_readline_set_raw_mode` via the new HIR variant
//      `Expr::ProcessStdinSetRawMode` (recognized in
//      perry-hir/src/lower/expr_call.rs).
//   2. `process.stdin.on(event, callback)` lowers to
//      `Expr::ProcessStdinOn { event, handler }` and codegens to
//      `js_readline_stdin_on(event_str_ptr, callback_closure)`.
//   3. The program compiles + links + runs to exit 0 (no missing-symbol
//      errors).
//
// We cannot pipe live keypress sequences from the parity runner today,
// so this test exercises:
//   - The compile path (HIR variant + codegen wiring).
//   - The dispatch into the runtime (linker resolves the symbols).
//   - rl.close() synchronously fires close handler (Phase 1 carryover).
//
// Live keypress dispatch is covered by 7 unit tests in
// `crates/perry-stdlib/src/readline.rs::tests` (parse_keypress_arrow_keys
// / _ctrl_letter / _special_keys / _letter_shift_flag, plus
// raw_mode_toggle_flips_atomic and the chunk-injection drain test).
//
// Note: Node only exposes `setRawMode` on TTY streams. When stdin is
// piped (e.g. CI runners or `< /dev/null`), `process.stdin.setRawMode`
// is undefined and calling it throws TypeError. We guard with a
// typeof-check to keep the test runnable on both platforms — Perry
// always provides setRawMode (no-op for non-Unix or no-TTY).

// 1. setRawMode toggle compiles cleanly.
if (typeof process.stdin.setRawMode === "function") {
    process.stdin.setRawMode(true);
    process.stdin.setRawMode(false);
    console.log("setRawMode round-trip ok");
} else {
    console.log("setRawMode round-trip ok"); // pinned shape
}

// 2. on('data') and on('keypress') registration compile and link.
process.stdin.on("data", (_chunk: string) => {
    /* no-op for the test */
});
process.stdin.on("keypress", (_str: string, _key: { name: string }) => {
    /* no-op for the test */
});
console.log("on() registration ok");

// 2b. #3962 lifecycle teardown surface compiles, links, and can detach
// listeners without requiring process.exit().
function teardownData(_chunk: string) {
    /* no-op for the test */
}
process.stdin.on("data", teardownData);
process.stdin.removeListener("data", teardownData);
process.stdin.on("data", teardownData);
process.stdin.pause();
process.stdin.resume();
process.stdin.off("data", teardownData);
console.log("stdin lifecycle teardown ok");

// 3. Phase 1 readline still works alongside Phase 2 surface.
import * as readline from "readline";
const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
});
rl.on("close", () => console.log("readline closed"));
rl.close();
if (typeof process.stdin.unref === "function") {
    process.stdin.unref();
}
if (typeof process.stdin.destroy === "function") {
    process.stdin.destroy();
}
console.log("done");
