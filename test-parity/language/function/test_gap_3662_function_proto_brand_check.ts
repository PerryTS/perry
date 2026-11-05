// #3662: Function.prototype.{apply,call,bind} must brand-check `this` and throw
// a TypeError when invoked (reflectively) on a non-callable receiver — mirrors
// test262 built-ins/Function/prototype/{apply,bind}/this-not-callable*.js, which
// are plain `Function.prototype.apply.call(thisValue, ...)` forms. The direct
// fast path and reflective calls on a real function must still work.
function name(e: unknown): string {
  return (e as any)?.constructor?.name ?? String(e);
}
function expectThrow(label: string, fn: () => void) {
  try {
    fn();
    console.log(label + ": NO THROW");
  } catch (e) {
    console.log(label + ": " + name(e));
  }
}
const f = function (a: number, b: number) {
  return a + b;
};
const apply = Function.prototype.apply;
const call = Function.prototype.call;
const bind = Function.prototype.bind;

// Non-callable `this` → TypeError.
expectThrow("apply/undefined", () => apply.call(undefined, null, []));
expectThrow("apply/null", () => apply.call(null, null, []));
expectThrow("apply/object", () => apply.call({}, null, []));
expectThrow("apply/number", () => apply.call(42, null, []));
expectThrow("call/undefined", () => call.call(undefined));
expectThrow("call/null", () => call.call(null));
expectThrow("call/object", () => call.call({}));
expectThrow("bind/undefined", () => bind.call(undefined));
expectThrow("bind/null", () => bind.call(null));
expectThrow("bind/object", () => bind.call({}));

// Positive: direct + reflective calls on a real function still work.
console.log("apply-direct:", f.apply(null, [2, 3]));
console.log("call-direct:", f.call(null, 4, 5));
console.log("bind-direct:", f.bind(null, 10)(20));
console.log("apply-reflect:", apply.call(f, null, [6, 7]));
console.log("call-reflect:", call.call(f, null, 8, 9));
console.log("bind-reflect:", bind.call(f, null, 100)(200));
