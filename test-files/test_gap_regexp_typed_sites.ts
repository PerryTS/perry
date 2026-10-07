// Typed RegExp operations must observe ordinary method/property semantics.
function row(name: string, action: () => any) {
  try { console.log(name + ": " + JSON.stringify(action())); }
  catch (e: any) { console.log(name + ": throw " + e.constructor.name); }
}
row("test_typed_own", () => { const r: RegExp = /a/; (r as any).test = () => "own"; return r.test("a"); });
row("exec_typed_own", () => { const r: RegExp = /a/; (r as any).exec = () => ["own"]; return r.exec("a"); });
row("test_method_before_argument", () => {
  const old = RegExp.prototype.test;
  RegExp.prototype.test = function () { return "before"; } as any;
  function argument() { RegExp.prototype.test = function () { return "after"; } as any; return "a"; }
  try { const r: RegExp = /a/; return [r.test(argument()), /a/.test(argument())]; }
  finally { RegExp.prototype.test = old; }
});
row("source_typed_getter", () => {
  const old = Object.getOwnPropertyDescriptor(RegExp.prototype, "source")!;
  Object.defineProperty(RegExp.prototype, "source", {get() { return "patched"; }, configurable: true});
  try { const r: RegExp = /a/; return [r.source, /a/.source]; }
  finally { Object.defineProperty(RegExp.prototype, "source", old); }
});
row("flags_typed_getter", () => {
  const old = Object.getOwnPropertyDescriptor(RegExp.prototype, "flags")!;
  Object.defineProperty(RegExp.prototype, "flags", {get() { return "patched"; }, configurable: true});
  try { const r: RegExp = /a/g; return [r.flags, /a/g.flags]; }
  finally { Object.defineProperty(RegExp.prototype, "flags", old); }
});
row("lastIndex_typed_value", () => { const r: RegExp = /a/; const value = {}; r.lastIndex = value as any; return r.lastIndex === value; });
row("lastIndex_typed_assignment_result", () => { const r: RegExp = /a/g; return [r.lastIndex = 3, r.lastIndex]; });
row("lastIndex_typed_strict", () => { "use strict"; const r: RegExp = /a/g; Object.defineProperty(r, "lastIndex", {writable: false}); r.lastIndex = 1; return r.lastIndex; });
// Fresh receivers and coercions cross collecting operand/callback windows.
function churn(): number {
  const keep: any[] = [];
  for (let i = 0; i < 4096; i++) keep.push({i, text: "rooting-" + i});
  return keep[keep.length - 1].i;
}
function coercingInput(): any {
  churn();
  return {toString() { churn(); return "a"; }};
}
row("test_typed_collecting_argument_and_coercion", () => {
  const r: RegExp = /a/g;
  return [r.test(coercingInput()), r.lastIndex];
});
row("exec_typed_collecting_argument_and_coercion", () => {
  const r: RegExp = /a/g;
  return [r.exec(coercingInput()), r.lastIndex];
});
row("test_literal_collecting_argument_and_coercion", () => /a/.test(coercingInput()));
row("lastIndex_typed_collecting_rhs", () => {
  const r: RegExp = /a/;
  function rhs() { churn(); return {tag: "stored"}; }
  const value = r.lastIndex = rhs() as any;
  return [r.lastIndex === value, (r.lastIndex as any).tag];
});
