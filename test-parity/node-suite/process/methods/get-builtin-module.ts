// process.getBuiltinModule(id) — Node 22.3+ accessor returning the
// named built-in module if loaded into the interpreter, undefined
// otherwise (no throw on unknown ids). Perry AOT-compiles every
// imported module, so there's no observable runtime "is this loaded?"
// query — undefined for every id is the spec-compatible answer.
// Regression cover for #1398.
console.log("is function:", typeof process.getBuiltinModule === "function");
console.log("unknown:", process.getBuiltinModule("not-a-real-module"));
console.log("empty:", process.getBuiltinModule(""));
