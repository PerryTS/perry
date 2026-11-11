// A local binding named `tty` shadows the global namespace: a call
// `tty.isatty(...)` uses the binding, not the builtin.
function viaConst(): string {
  const tty = { isatty: (..._a: unknown[]) => "local-tty" };
  return "" + tty.isatty(1);
}
function viaParam(tty: any): string {
  return "" + tty.isatty(1);
}
console.log(viaConst());
console.log(viaParam({ isatty: () => "param-tty" }));
