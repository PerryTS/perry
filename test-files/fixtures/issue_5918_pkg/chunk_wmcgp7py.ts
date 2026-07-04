// Mirrors remeda's dist/chunk-WMCGP7PY.js shape (a `dropFirstBy`-style
// helper built on the two chunks above). This is where issue #5918 fires:
//
//   import { a as n, c as a } from "./chunk_anxbdsui.ts"   <- local "a" -> anxbdsui's "c"
//   import { a as t } from "./chunk_wimgwyzl.ts"           <- EXPORTED name "a" (wimgwyzl's)
//
// Pre-fix, `import_function_prefixes` was keyed by BOTH the local alias
// AND the exported/origin name for every named import. The second
// import's exported name "a" (from chunk_wimgwyzl) overwrote the first
// import's *local* alias "a" (bound to chunk_anxbdsui's "c") in that same
// map, so codegen resolved local `a` — used inside `o` below — against
// chunk_wimgwyzl instead of chunk_anxbdsui, which doesn't export anything
// named "c" there. Link failure ensued.
import { a as n, c as a } from "./chunk_anxbdsui.ts"
import { a as t } from "./chunk_wimgwyzl.ts"

function s(...e: unknown[]) {
  return t(p, e, o)
}
const p = (e: unknown[], r: number) => (r < 0 ? [...e] : e.slice(r))
function o(e: number): any {
  if (e <= 0) return a
  let r = e
  return (i: unknown) => (r > 0 ? ((r -= 1), n) : { done: false, hasNext: true, next: i })
}
export { s as a }
