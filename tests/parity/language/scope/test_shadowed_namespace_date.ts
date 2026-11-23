// A local binding named `Date` shadows the global namespace: a call
// `Date.now(...)` use the binding, not the builtin.
function viaConst(): string {
  const Date = { now: (..._a: unknown[]) => "local-Date" };
  return "" + Date.now();
}
function viaParam(Date: any): string {
  return "" + Date.now();
}
console.log(viaConst());
console.log(viaParam({ now: () => "param-Date" }));
