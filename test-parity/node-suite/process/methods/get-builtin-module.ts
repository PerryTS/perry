// process.getBuiltinModule(id) — Node 22.3+ accessor. Returns the named
// built-in module namespace for a known builtin (prefixless or `node:`-
// prefixed); returns undefined for unknown ids and for npm packages, without
// throwing. Regression cover for #1398 / #2482.
console.log("is function:", typeof process.getBuiltinModule === "function");
console.log("length:", process.getBuiltinModule.length);
const fs = process.getBuiltinModule("fs");
console.log("fs typeof:", typeof fs);
console.log("fs.readFileSync:", typeof fs.readFileSync);
console.log("node:fs existsSync:", typeof process.getBuiltinModule("node:fs").existsSync);
console.log("os.platform:", typeof process.getBuiltinModule("os").platform);
const path = process.getBuiltinModule("node:path");
console.log("path.join:", path.join("a", "b"));
const getBuiltinModule = process.getBuiltinModule;
const dynamicId = "fs";
const dynamicFs = getBuiltinModule(dynamicId);
console.log("captured dynamic fs typeof:", typeof dynamicFs);
console.log("captured dynamic fs.readFileSync:", dynamicFs ? typeof dynamicFs.readFileSync : "missing");
const timersId = "timers";
const timers = getBuiltinModule(timersId);
console.log("captured timers.setTimeout:", timers ? typeof timers.setTimeout : "missing");
console.log("npm pkg:", process.getBuiltinModule("axios"));
console.log("unknown:", process.getBuiltinModule("not-a-real-module"));
console.log("empty:", process.getBuiltinModule(""));

function invalid(label: string, value: any) {
  try {
    getBuiltinModule(value);
    console.log(label, "no throw");
  } catch (err: any) {
    console.log(label, err.name, err.code, String(err.message).includes("\"id\""));
  }
}

invalid("invalid number:", 123);
invalid("invalid null:", null);
