// Issue #5918 — `import_function_prefixes` was keyed by both the local
// alias AND the exported/origin name for every named import. The origin
// name isn't unique per file (it's whatever the SOURCE module happens to
// call it), so two unrelated imports in the same file whose origin names
// collide (very common in minifier/esbuild chunk-splitting output, where
// nearly everything is named "a"/"b"/"c") silently overwrite each other's
// map entry — even when the collision has nothing to do with the local
// alias the HIR's `ExternFuncRef` actually carries.
//
// This shape is lifted directly from `remeda`'s real (unmodified)
// dist/chunk-*.js build output — found via a real-world source compile of
// `sst/opencode`, where nearly every remeda function transitively imports
// these exact four chunks.
import { a as dropFrom } from "./fixtures/issue_5918_pkg/chunk_wmcgp7py.ts"

console.log(JSON.stringify(dropFrom([1, 2, 3, 4, 5], 2)))
console.log(JSON.stringify(dropFrom([1, 2, 3, 4, 5], -1)))
