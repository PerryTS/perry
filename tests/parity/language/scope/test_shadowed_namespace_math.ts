// A local binding named `Math` shadows the global namespace: a call
// `Math.abs(...)` and the constant `Math.PI` use the binding, not the builtin.
function viaConst(): string {
  const Math = { abs: (..._a: unknown[]) => "local-Math", PI: "local-PI" };
  return "" + Math.abs(-3) + " " + Math.PI;
}
function viaParam(Math: any): string {
  return "" + Math.abs(-3);
}
console.log(viaConst());
console.log(viaParam({ abs: () => "param-Math" }));
