// A local binding named `AbortSignal` shadows the global namespace: a call
// `AbortSignal.timeout(...)` use the binding, not the builtin.
function viaConst(): string {
  const AbortSignal = { timeout: (..._a: unknown[]) => "local-AbortSignal" };
  return "" + AbortSignal.timeout(5);
}
function viaParam(AbortSignal: any): string {
  return "" + AbortSignal.timeout(5);
}
console.log(viaConst());
console.log(viaParam({ timeout: () => "param-AbortSignal" }));
