// Shared helpers for #10554: a function referenced inside its own module
// must be the SAME object an importer sees.
export function fnDecl() {
  return 1;
}
export const fnExpr = function fnExprNamed() {
  return 2;
};
export const arrowFn = () => 3;

// Value comparisons made FROM WITHIN this module -- the defect's exact
// shape: `isSame*` is itself exported, so it is a candidate for
// cross-module inlining, and its body's reference to the sibling export is
// a plain in-module reference, not an import.
export function isSameDecl(x: unknown) {
  return x === fnDecl;
}
export function isSameExpr(x: unknown) {
  return x === fnExpr;
}
export function isSameArrow(x: unknown) {
  return x === arrowFn;
}
export function bothSame(a: unknown, b: unknown) {
  return a === b;
}
export function makeSet() {
  return new Set<unknown>([fnDecl, fnExpr, arrowFn]);
}

// A default export that ALSO references an exported sibling by value --
// #10548 fixed the export-ROW identity for `export default F`; this checks
// the (distinct) cross-module-inliner defect #10554 fixes doesn't resurface
// under a default export.
export default function useDecl(x: unknown) {
  return x === fnDecl;
}
