// CJL: an ES module importing a CommonJS module sees its namespace keys
// (default, the detected names, module.exports) and no compiler plumbing.
import * as ns from "./cjl_ns/lib.cjs";

console.log(Object.keys(ns).sort().join(","));
const dynamic = await import("./cjl_ns/lib.cjs");
console.log(Object.keys(dynamic).sort().join(","));
console.log(ns.alpha, dynamic.beta());
