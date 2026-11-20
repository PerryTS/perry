// A local binding named `URL` shadows the global namespace: a call
// `URL.canParse(...)` use the binding, not the builtin.
function viaConst(): string {
  const URL = { canParse: (..._a: unknown[]) => "local-URL" };
  return "" + URL.canParse('x');
}
function viaParam(URL: any): string {
  return "" + URL.canParse('x');
}
console.log(viaConst());
console.log(viaParam({ canParse: () => "param-URL" }));
