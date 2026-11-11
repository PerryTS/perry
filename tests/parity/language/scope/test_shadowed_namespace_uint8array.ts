// A local binding named `Uint8Array` shadows the global namespace: a call
// `Uint8Array.fromHex(...)` use the binding, not the builtin.
function viaConst(): string {
  const Uint8Array = { fromHex: (..._a: unknown[]) => "local-Uint8Array" };
  return "" + Uint8Array.fromHex('00');
}
function viaParam(Uint8Array: any): string {
  return "" + Uint8Array.fromHex('00');
}
console.log(viaConst());
console.log(viaParam({ fromHex: () => "param-Uint8Array" }));
