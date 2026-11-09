// A local binding named `Number` shadows the global namespace: a call
// `Number.isInteger(...)` and the constant `Number.MAX_SAFE_INTEGER` use the binding, not the builtin.
function viaConst(): string {
  const Number = { isInteger: (..._a: unknown[]) => "local-Number", MAX_SAFE_INTEGER: "local-MAX_SAFE_INTEGER" };
  return "" + Number.isInteger(1.5) + " " + Number.MAX_SAFE_INTEGER;
}
function viaParam(Number: any): string {
  return "" + Number.isInteger(1.5);
}
console.log(viaConst());
console.log(viaParam({ isInteger: () => "param-Number" }));
