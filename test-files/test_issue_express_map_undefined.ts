// Regression test for the express `Cannot read properties of undefined
// (reading 'map')` crash at module init.
//
// Downstream of #911 (the express `value is not a function` fix), express's
// `lib/utils.js` IIFE now runs `var { METHODS } = require('node:http')` in
// the correct order — but `METHODS` was undefined because Perry's
// `node:http` shim (and its perry-jsruntime sibling) didn't expose
// `METHODS`. The next line, `METHODS.map((method) => method.toLowerCase())`,
// then threw the spec TypeError and express died before its main entry's
// `console.log` ran.
//
// Node's `require('node:http').METHODS` is a sorted array of HTTP verb
// strings sourced from llhttp. Perry now ships:
//
//   1. `perry-runtime::object::http_methods_array()` — long-lived cached
//      JS array, surfaced via `get_native_module_constant` for the
//      `("http"|"https"|"http2", "METHODS")` triples that fire when a
//      Perry-native module reads the namespace (e.g. CJS-wrapped express).
//   2. `perry-jsruntime::modules.rs` stub — `export const METHODS = [...]`
//      so the same read from a jsruntime-hosted module (e.g. `router`
//      pulled in transitively through express but not in
//      `compilePackages`) also resolves.
//
// Both surfaces share the same Node 22 snapshot. This regression test
// covers the Perry-native surface (the jsruntime side has no direct
// gap-test entry but is exercised via the express e2e smoke).

import http from "node:http";

// Spot-check on the http namespace.
const m = (http as any).METHODS as string[];
console.log("typeof:", typeof m);
console.log("is array:", Array.isArray(m));
console.log("length:", m.length);
console.log("has GET:", m.includes("GET"));
console.log("has POST:", m.includes("POST"));
console.log("has DELETE:", m.includes("DELETE"));

// Express-shape: `.map((s) => s.toLowerCase())` is the exact call that
// threw pre-fix.
const lower = m.map((method) => method.toLowerCase());
console.log("first lower:", lower[0]);
console.log("get index:", lower.indexOf("get"));

// Destructuring pattern (matches express's `var { METHODS } = require(...)`).
const { METHODS } = http as any;
console.log("destructured length:", (METHODS as string[]).length);

console.log("OK");
