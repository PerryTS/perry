// A local binding named `JSON` shadows the global namespace: a call
// `JSON.stringify(...)` use the binding, not the builtin.
function viaConst(): string {
  const JSON = { stringify: (..._a: unknown[]) => "local-JSON" };
  return "" + JSON.stringify(1);
}
function viaParam(JSON: any): string {
  return "" + JSON.stringify(1);
}
console.log(viaConst());
console.log(viaParam({ stringify: () => "param-JSON" }));
