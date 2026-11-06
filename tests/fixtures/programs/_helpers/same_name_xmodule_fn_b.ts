// Issue #35 / #321: a SECOND module exporting a function named `equals`
// (distinct from `same_name_xmodule_fn_a.ts`'s `equals`). This one has two
// normal parameters and a different body. Imported as `{ equals as eqB }`,
// the call must dispatch into THIS body, not module A's.
export function equals(x: number, y: number): any {
  return "B:" + (x + y);
}
