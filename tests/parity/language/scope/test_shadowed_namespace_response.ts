// A local binding named `Response` shadows the global namespace: a call
// `Response.error(...)` use the binding, not the builtin.
function viaConst(): string {
  const Response = { error: (..._a: unknown[]) => "local-Response" };
  return "" + Response.error();
}
function viaParam(Response: any): string {
  return "" + Response.error();
}
console.log(viaConst());
console.log(viaParam({ error: () => "param-Response" }));
