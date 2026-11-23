// A local binding named `Object` shadows the global namespace: a call
// `Object.keys(...)` uses the binding, not the builtin.
function viaConst(): string {
  const Object = { keys: (..._a: unknown[]) => "local-Object" };
  return "" + Object.keys({a:1});
}
function viaParam(Object: any): string {
  return "" + Object.keys({a:1});
}
console.log(viaConst());
console.log(viaParam({ keys: () => "param-Object" }));
