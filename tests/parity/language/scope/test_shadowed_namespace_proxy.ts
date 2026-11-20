// A local binding named `Proxy` shadows the global namespace: a call
// `Proxy.revocable(...)` uses the binding, not the builtin.
function viaConst(): string {
  const Proxy = { revocable: (..._a: unknown[]) => "local-Proxy" };
  return "" + Proxy.revocable({}, {});
}
function viaParam(Proxy: any): string {
  return "" + Proxy.revocable({}, {});
}
console.log(viaConst());
console.log(viaParam({ revocable: () => "param-Proxy" }));
