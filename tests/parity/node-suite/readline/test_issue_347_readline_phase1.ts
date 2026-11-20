// Regression test for #347 Phase 1: readline.createInterface +
// rl.on('close') + rl.close(). Verifies that:
//   1. `import * as readline from 'readline'` resolves to the native
//      stdlib module (NATIVE_MODULES entry in perry-hir/src/ir.rs).
//   2. `readline.createInterface(opts)` returns a handle whose
//      .on/.close methods dispatch via the ("readline", true, METHOD)
//      entries in perry-codegen's native dispatch table.
//   3. `rl.close()` triggers the 'close' event asynchronously through
//      the event-loop pump (js_readline_process_pending) — matches
//      Node's behavior where 'close' fires on next tick after the
//      synchronous code completes.
//
// Live stdin tests (rl.question, rl.on('line', ...)) need an
// interactive terminal or a piped input fixture which the parity
// runner doesn't currently support. Those shapes are unit-tested in
// crates/perry-stdlib/src/readline.rs::tests instead.

import * as readline from "readline";

const rl = readline.createInterface({
    input: process.stdin,
    output: process.stdout,
});

rl.on("close", () => {
    console.log("closed");
});

rl.close();
console.log("done");
