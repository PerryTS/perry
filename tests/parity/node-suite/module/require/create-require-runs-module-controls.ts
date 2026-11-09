// Controls for create-require-runs-module.ts: the same CommonJS file reached
// by a static ESM import (its `require` of the dependency is a plain literal
// `require`), and by `createRequire` with a computed specifier.
import { createRequire } from "node:module";
import viaImport from "./fixtures/create-require-loader.cjs";

console.log(viaImport.load({ k: 2 }), viaImport.dep(4));
const require = createRequire(import.meta.url);
const parts = ["./fixtures", "create-require-dep.cjs"];
const dep = require(parts.join("/"));
console.log(typeof dep.twice, dep.twice(5));
