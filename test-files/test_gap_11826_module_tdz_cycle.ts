// #11826, import cycle: cyc_a imports cyc_b, which imports cyc_a back, so
// cyc_b's body runs before cyc_a's. Calling cyc_a's exported functions (which
// read cyc_a's `const`/`let`) and reading those bindings directly must throw
// ReferenceError there; after cyc_a's body has run they read the values.
import { readCA, readLA } from "./fixtures/issue_11826_module_tdz/cyc_a.ts";

console.log("main", readCA(), readLA());
