// An enum reached through a namespace (`import * as` or a named `export * as`
// re-export) must not replace a directly imported enum that shares its
// declaration name. Node cannot strip `enum`, so the expected output is stored
// in test-parity/expected/namespace-enum-direct-import.txt.
import * as schema from "./fixtures/namespace-enum/schema.ts";
import { nested } from "./fixtures/namespace-enum/barrel.ts";
import { Kind } from "./fixtures/namespace-enum/local.ts";

console.log("direct import:", Kind.Value);
console.log("namespace import:", schema.Kind.Value);
console.log("named namespace re-export:", nested.Kind.Value);
