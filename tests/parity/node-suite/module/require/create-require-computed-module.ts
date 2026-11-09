// A computed specifier can name the `module` builtin: its `Module._cache`
// hands out every loaded module record, and a write through it is a write
// to that module's exports, which the module then reads.
import { createRequire } from "node:module";
import dep from "./fixtures/create-require-cache-dep.cjs";

const require = createRequire(import.meta.url);
const parts = ["mod", "ule"];
const Module = require(parts.join(""));
for (const k of Object.keys(Module._cache)) {
  if (k.endsWith("create-require-cache-dep.cjs")) Module._cache[k].exports.flag = "x";
}
console.log(dep.get());
