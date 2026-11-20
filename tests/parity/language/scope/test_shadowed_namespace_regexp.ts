// A local binding named `RegExp` shadows the global namespace: a call
// `RegExp.escape(...)` uses the binding, not the builtin.
function viaConst(): string {
  const RegExp = { escape: (..._a: unknown[]) => "local-RegExp" };
  return "" + RegExp.escape('a');
}
function viaParam(RegExp: any): string {
  return "" + RegExp.escape('a');
}
console.log(viaConst());
console.log(viaParam({ escape: () => "param-RegExp" }));
