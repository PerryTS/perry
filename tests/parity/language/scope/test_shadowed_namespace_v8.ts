// A local binding named `v8` shadows the global namespace: a call
// `v8.getHeapStatistics(...)` uses the binding, not the builtin.
function viaConst(): string {
  const v8 = { getHeapStatistics: (..._a: unknown[]) => "local-v8" };
  return "" + v8.getHeapStatistics();
}
function viaParam(v8: any): string {
  return "" + v8.getHeapStatistics();
}
console.log(viaConst());
console.log(viaParam({ getHeapStatistics: () => "param-v8" }));
