// Origin module for the issue #678 regression fixture.
//
// Three export shapes the symbol-suffix resolver has to round-trip
// through `export { default as X } from "./inner.ts"`:
//
//   1. `const arrow = (x) => x + 1; export default arrow;`
//      Origin emits `perry_fn_<inner>__default` as the zero-arg getter;
//      consumer must classify the import as `imported_vars` so the call
//      site goes through `js_closure_callN` after fetching the value.
//
//   2. `export default function namedFn(x) { return x * 2; }`
//      Origin emits `perry_fn_<inner>__default` AND `perry_fn_<inner>__namedFn`;
//      consumer must call `__default` (not `__namedFn`) per ES spec when
//      the importing module names the binding via the re-export rename.
//
//   3. `export function plain(x) { return x + 100; }`
//      Plain function export — sanity check that non-rename imports still
//      resolve to `perry_fn_<inner>__plain` correctly with the new
//      origin-name lookup in place.

const arrow = (x: number): number => x + 1;
export default arrow;

export function plain(x: number): number {
  return x + 100;
}
