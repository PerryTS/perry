// A closure that reads a let or class binding of its enclosing function before the
// declaration ran throws a ReferenceError naming the binding; afterwards it sees the value.
function probe(f: () => unknown): string {
  try { return "ok " + String(f()); } catch (e: any) { return e.name + ": " + e.message; }
}
function viaLet(): string {
  const read = () => x;
  const before = probe(read);
  let x = 1;
  return before + " | " + probe(read);
}
function viaClass(): string {
  const read = () => C;
  const before = probe(read);
  class C {}
  return before + " | " + (read() === C);
}
function viaDefault(): string {
  const read = (v = y) => v;
  const before = probe(read);
  let y = 2;
  return before + " | " + probe(read);
}
function noInit(): string {
  const read = () => z;
  const before = probe(read);
  let z: number | undefined;
  return before + " | " + probe(read);
}
console.log(viaLet());
console.log(viaClass());
console.log(viaDefault());
console.log(noInit());
