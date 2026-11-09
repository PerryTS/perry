// A local binding named `String` shadows the global namespace: a call
// `String.fromCharCode(...)` use the binding, not the builtin.
function viaConst(): string {
  const String = { fromCharCode: (..._a: unknown[]) => "local-String" };
  return "" + String.fromCharCode(65);
}
function viaParam(String: any): string {
  return "" + String.fromCharCode(65);
}
console.log(viaConst());
console.log(viaParam({ fromCharCode: () => "param-String" }));
