// A local binding named `Reflect` shadows the global namespace: a call
// `Reflect.ownKeys(...)` uses the binding, not the builtin.
function viaConst(): string {
  const Reflect = { ownKeys: (..._a: unknown[]) => "local-Reflect" };
  return "" + Reflect.ownKeys({});
}
function viaParam(Reflect: any): string {
  return "" + Reflect.ownKeys({});
}
console.log(viaConst());
console.log(viaParam({ ownKeys: () => "param-Reflect" }));
