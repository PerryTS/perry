// A local binding named `ArrayBuffer` shadows the global namespace: a call
// `ArrayBuffer.isView(...)` use the binding, not the builtin.
function viaConst(): string {
  const ArrayBuffer = { isView: (..._a: unknown[]) => "local-ArrayBuffer" };
  return "" + ArrayBuffer.isView(1);
}
function viaParam(ArrayBuffer: any): string {
  return "" + ArrayBuffer.isView(1);
}
console.log(viaConst());
console.log(viaParam({ isView: () => "param-ArrayBuffer" }));
