// Regression test for #360 item #2: `import process from 'node:process'`
// and `import { cwd } from 'node:process'` were rejected at module
// resolution with `Warning: Could not resolve import 'process' from <file>`,
// and the destructured `cwd` link-failed with `Undefined symbols: _cwd`.
// Surfaced when compiling ink (8 sites across ink/build/*.js do one of
// the two import shapes).
//
// Fix in two places:
//   1. perry-hir/src/ir.rs: add "process" to NATIVE_MODULES (and to
//      RUNTIME_ONLY_MODULES — process surface lives in perry-runtime,
//      not perry-stdlib, so a process-only program shouldn't pull stdlib
//      in for nothing).
//   2. perry-codegen/src/lower_call/native.rs: route receiver-less
//      NativeMethodCall { module: "process", method: "cwd" | "uptime" |
//      "memoryUsage" } to the same runtime helpers (js_process_cwd /
//      js_process_uptime / js_process_memory_usage) that
//      Expr::ProcessCwd / Expr::ProcessUptime / Expr::ProcessMemoryUsage
//      already use for the implicit-global `process.cwd()` form. Without
//      step 2, step 1 alone makes the import resolve cleanly but the
//      destructured `cwd()` call silently returns `undefined` — worse UX
//      than the original "Could not resolve" error.

import process from "node:process";
import { cwd } from "node:process";

// 1. Default import: `process` binding works the same as the implicit
// global. argv access matches Node.
console.log("argv length >= 1:", process.argv.length >= 1);

// 2. Destructured `cwd` is callable; bare call returns the same directory
// as the implicit `process.cwd()` access.
// (Note: `typeof cwd` returns "number" in Perry today vs Node's "function" —
// same cross-module reference typeof bug as #348/createContext, separate
// follow-up under #360 item #1. The destructured binding *works* as a
// callable, which is what this fix delivers.)
console.log("cwd() === process.cwd():", cwd() === process.cwd());

// 3. cwd() returns a non-empty string. Don't pin the actual path —
// that varies by runner CWD — but check the shape.
const dir = cwd();
console.log("cwd() is string:", typeof dir === "string");
console.log("cwd() non-empty:", dir.length > 0);
