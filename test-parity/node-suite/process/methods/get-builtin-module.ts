// process.getBuiltinModule(id) — Node 22.3+ accessor. Returns the named
// built-in module namespace for a known builtin (prefixless or `node:`-
// prefixed); returns undefined for unknown ids and for npm packages, without
// throwing. Regression cover for #1398 / #2482.
console.log("is function:", typeof process.getBuiltinModule === "function");
const fs = process.getBuiltinModule("fs");
console.log("fs typeof:", typeof fs);
console.log("fs.readFileSync:", typeof fs.readFileSync);
console.log("node:fs existsSync:", typeof process.getBuiltinModule("node:fs").existsSync);
console.log("os.platform:", typeof process.getBuiltinModule("os").platform);
const path = process.getBuiltinModule("node:path");
console.log("path.join:", path.join("a", "b"));
console.log("npm pkg:", process.getBuiltinModule("axios"));
console.log("unknown:", process.getBuiltinModule("not-a-real-module"));
console.log("empty:", process.getBuiltinModule(""));
