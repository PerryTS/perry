// A local binding named `Map` shadows the global namespace: a call
// `Map.groupBy(...)` uses the binding, not the builtin.
function viaConst(): string {
  const Map = { groupBy: (..._a: unknown[]) => "local-Map" };
  return "" + Map.groupBy([1], (x: any) => x);
}
function viaParam(Map: any): string {
  return "" + Map.groupBy([1], (x: any) => x);
}
console.log(viaConst());
console.log(viaParam({ groupBy: () => "param-Map" }));
