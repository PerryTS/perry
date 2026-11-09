// A local binding named `BigInt` shadows the global namespace: a call
// `BigInt.asIntN(...)` use the binding, not the builtin.
function viaConst(): string {
  const BigInt = { asIntN: (..._a: unknown[]) => "local-BigInt" };
  return "" + BigInt.asIntN(8, 255n);
}
function viaParam(BigInt: any): string {
  return "" + BigInt.asIntN(8, 255n);
}
console.log(viaConst());
console.log(viaParam({ asIntN: () => "param-BigInt" }));
