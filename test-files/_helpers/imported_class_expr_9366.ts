export class Decl { m() { return 1; } }
export let Expr = class { m() { return 2; } };
export const Anon = class Named { m() { return 3; } };
export function localProbe() {
  return [typeof Decl.prototype, typeof Expr.prototype, typeof Anon.prototype].join(",");
}
export function localExprPrototype() { return Expr.prototype; }
export function replaceExpr() { Expr = class Replacement { m() { return 4; } }; }
export const box = { marker: 8 };
export const text = "hello";
export let accessorReads = 0;
export const accessor = { get value() { accessorReads++; return this.marker; }, marker: 9 };
export let functionCalls = 0;
export function untouchedFunction() { functionCalls++; return box; }
