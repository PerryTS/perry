// A local binding named `Array` shadows the global namespace: a call
// `Array.isArray(...)` uses the binding, not the builtin.
function viaConst(): string {
  const Array = { isArray: (..._a: unknown[]) => "local-Array" };
  return "" + Array.isArray(1);
}
function viaParam(Array: any): string {
  return "" + Array.isArray(1);
}
console.log(viaConst());
console.log(viaParam({ isArray: () => "param-Array" }));
