// #1681 (Phase 3 of #1677) — `precompile(EXPR)` evaluates EXPR at BUILD TIME
// by Perry compiling and running its own output (no node, no V8), captures
// the generated function source, and compiles it natively. The shipped
// binary contains no JS engine. (`precompile` is a Perry build-time
// intrinsic with no Node equivalent — compared against a stored expected
// output, not Node.)
function makeAdder(n: number): string {
  return "(a: number) => a + " + n;
}
function makeGreeter(greeting: string): string {
  return "function (name: string) { return \"" + greeting + ", \" + name; }";
}

const addTen = precompile(makeAdder(10));
const greet = precompile(makeGreeter("Hello"));

console.log("addTen(5):", addTen(5));
console.log("addTen(-3):", addTen(-3));
console.log("greet:", greet("Ada"));
