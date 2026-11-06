// `export *` forwards only a source module's exports. A private function in
// one star source must not collide with the same-named export of another star
// source, whether the name is read through the barrel namespace, a named
// re-export, or the private function's own caller.
import * as z from "./fixtures/export-star-private/bridge.ts";
import { foo, useFoo } from "./fixtures/export-star-private/barrel.ts";

// Re-exporting the namespace forces every entry to be materialized.
export { z };

console.log("namespace:", typeof z, Object.keys(z).sort().join(","));
console.log("named re-export:", z.foo());
console.log("barrel namespace:", z.coreNamespace().foo());
console.log("barrel keys:", Object.keys(z.coreNamespace()).sort().join(","));
console.log("direct import:", foo());
console.log("private caller:", useFoo());
