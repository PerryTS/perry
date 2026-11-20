// A local binding named `Buffer` shadows the global namespace: a call
// `Buffer.from(...)` use the binding, not the builtin.
function viaConst(): string {
  const Buffer = { from: (..._a: unknown[]) => "local-Buffer" };
  return "" + Buffer.from('a');
}
function viaParam(Buffer: any): string {
  return "" + Buffer.from('a');
}
console.log(viaConst());
console.log(viaParam({ from: () => "param-Buffer" }));
