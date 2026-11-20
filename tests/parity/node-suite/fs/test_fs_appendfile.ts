// Issue #226: regression for `fs.appendFileSync` codegen.
//
// Two paths needed wiring (only one was in the issue's diagnosis):
//
//   1. SWC→HIR lowerer in `crates/perry-hir/src/lower/expr_call.rs`. The
//      named-import path (`import { appendFileSync } from "fs"`) already
//      had an arm; the namespace-import + property-access path
//      (`import * as fs from "fs"; fs.appendFileSync(...)`) did not, so
//      the call slipped past as a generic `Call { callee: PropertyGet
//      { object: NativeModuleRef("fs"), property: "appendFileSync" } }`
//      and `Expr::FsAppendFileSync` never got emitted.
//
//   2. LLVM codegen in `crates/perry-codegen/src/expr.rs`. Even when
//      the HIR variant was emitted, the backend lacked the arm next to
//      `FsWriteFileSync`, so the call fell through and was silently
//      dropped. JS and WASM backends had had the arm for a while.
//
// Net pre-fix behaviour: every `fs.appendFileSync` wrote 0 bytes, the
// file wasn't even created on a fresh path. This test exercises the
// common namespace-import shape and asserts the file accumulates.

import * as fs from "fs";

const path = "/tmp/perry_appendfile_test.txt";

if (fs.existsSync(path)) {
  fs.unlinkSync(path);
}

// First call — file doesn't exist yet, append-mode open creates it.
fs.appendFileSync(path, "line one\n");
console.log(fs.readFileSync(path, "utf-8"));

// Second call — should grow the file rather than overwrite.
fs.appendFileSync(path, "line two\n");
console.log(fs.readFileSync(path, "utf-8"));

// Third — accumulates.
fs.appendFileSync(path, "line three\n");
console.log(fs.readFileSync(path, "utf-8"));

fs.unlinkSync(path);
