// #9403 — `process.on("exit", …)` handlers must fire.
//
// Perry emitted `beforeExit` from the generated event-loop epilogue but never
// emitted `exit` from anywhere, so every `exit` listener a program registered
// was dead code. claude-code registers 17 of them and lost most of its session
// transcript as a result.
//
// This file covers the NATURAL-DRAIN exit path. The explicit `process.exit(n)`
// path and the `process.exitCode` interaction are separate files, because each
// pins a different process status.
//
// Node semantics pinned here:
//   * `beforeExit` fires first, then `exit`.
//   * `exit` handlers run in registration order, each with the exit code as
//     their single argument.
//   * SYNCHRONOUS work inside a handler is honoured — a `writeFileSync` lands
//     and can be read back in the next handler.
//   * ASYNCHRONOUS work scheduled from a handler never runs: the loop is over.
//     `setTimeout`, `setImmediate` and `process.nextTick` are all dropped.
//     (Promise jobs are the one exception on this path — V8 runs a microtask
//     checkpoint after the emit returns, so a `.then` queued by a handler does
//     run, AFTER every handler. Pinned below.)
import { appendFileSync, readFileSync, unlinkSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const marker = join(tmpdir(), "perry_gap_9403_natural_marker.txt");
try {
  unlinkSync(marker);
} catch {
  // first run
}

let beforeExitCount = 0;

process.on("beforeExit", (code: number) => {
  beforeExitCount++;
  console.log("beforeExit code=" + code);
});

// Rest args rather than a fixed parameter: this also pins the ARITY node
// emits with (exactly one argument).
process.on("exit", (...args: number[]) => {
  console.log("exit#1 argc=" + args.length + " code=" + args[0]);
  console.log("exit#1 beforeExit-already-ran=" + (beforeExitCount === 1));
  // Synchronous I/O from inside an exit handler must land. This is the whole
  // user-visible point of the bug: claude-code flushes its session transcript
  // here.
  writeFileSync(marker, "written-by-exit#1 code=" + args[0] + "\n");
});

process.on("exit", (code: number) => {
  console.log("exit#2 code=" + code);
  appendFileSync(marker, "appended-by-exit#2\n");
  console.log("exit#2 readback=" + JSON.stringify(readFileSync(marker, "utf8")));
  unlinkSync(marker);
});

process.on("exit", () => {
  // None of these may run: scheduling async work in an exit handler must not
  // resurrect the event loop.
  setTimeout(() => console.log("BUG: setTimeout callback ran"), 0);
  setImmediate(() => console.log("BUG: setImmediate callback ran"));
  process.nextTick(() => console.log("BUG: nextTick callback ran"));
  // A promise job DOES run, after every handler (V8's microtask checkpoint).
  Promise.resolve()
    .then(() => console.log("promise job 1 (after all exit handlers)"))
    .then(() => console.log("promise job 2 (chained)"));
});

console.log("main done");
