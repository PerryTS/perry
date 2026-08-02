// Helper for test_gap_imported_member_array_method_names.ts (#7154).
//
// A module that exports (a) a plain object whose members happen to be named
// like Array.prototype methods and (b) real arrays. This is the shape of
// zod's top-level export: `zod/src/index.ts` does
// `import * as z from "./v4/classic/external.js"; export { z };`, so a
// consumer writing `import { z } from "zod"` holds a NAMED import of an
// object with `export function map(keyType, valueType)` on it.

export const ns = {
  map: (a: unknown, b: unknown) => `map:${a},${b}`,
  filter: (a: unknown, b: unknown) => `filter:${a},${b}`,
  find: (a: unknown) => `find:${a}`,
  forEach: (a: unknown) => `forEach:${a}`,
  reduce: (a: unknown, b: unknown) => `reduce:${a},${b}`,
  reduceRight: (a: unknown, b: unknown) => `reduceRight:${a},${b}`,
  slice: (a: unknown, b: unknown) => `slice:${a},${b}`,
  includes: (a: unknown) => `includes:${a}`,
  indexOf: (a: unknown) => `indexOf:${a}`,
  join: (a: unknown) => `join:${a}`,
  sort: (a: unknown) => `sort:${a}`,
  keys: () => "keys",
  values: () => "values",
  entries: () => "entries",
  flat: () => "flat",
  with: (a: unknown, b: unknown) => `with:${a},${b}`,
  toSorted: (a: unknown) => `toSorted:${a}`,
  toReversed: () => "toReversed",
  toSpliced: (a: unknown, b: unknown) => `toSpliced:${a},${b}`,
};

// Real exported arrays — the fold's original beneficiaries. Their methods must
// keep producing Array.prototype results.
export const arr = [3, 1, 2];
export const nums: number[] = [10, 20, 30];
