// Mirrors remeda's dist/chunk-WIMGWYZL.js shape (arity-dispatching purry
// core). Imports "a" from chunk_d6fck2ga (renamed to "t" here) and
// exports its own "u" as "a" — this file's EXPORTED name "a" is the one
// that, pre-fix, clobbered an unrelated LOCAL alias "a" in a sibling
// chunk that also imports from here.
import { a as t } from "./chunk_d6fck2ga.ts"

function u(r: (...args: any[]) => any, n: any[], o: unknown): any {
  const a = r.length - n.length
  if (a === 0) return r(...n)
  if (a === 1) return t(r, n, o)
  throw new Error("Wrong number of arguments")
}
export { u as a }
