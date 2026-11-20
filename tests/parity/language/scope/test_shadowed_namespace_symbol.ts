// A local binding named `Symbol` shadows the global namespace: a call
// `Symbol.for(...)` uses the binding, not the builtin.
function viaConst(): string {
  const Symbol = { for: (..._a: unknown[]) => "local-Symbol" };
  return "" + Symbol.for('k');
}
function viaParam(Symbol: any): string {
  return "" + Symbol.for('k');
}
console.log(viaConst());
console.log(viaParam({ for: () => "param-Symbol" }));
