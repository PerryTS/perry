// G8: `fn(...arguments)` forwards the incoming argument list directly when the
// Arguments object is never otherwise observed (perry-codegen
// codegen/arguments.rs, ArgumentsElision::Reads with spread uses). Module
// code is strict, so no parameter is mapped here; the sloppy half is
// test_gap_g8_arguments_forward_sloppy.cts. Compared against Node.

const show = (...xs: any[]) => JSON.stringify(xs.map((x) => (x === undefined ? "u" : x)));

// Receivers of every dispatch kind: an untyped object literal method, a class
// instance, a module function, a closure value, a builtin.
const sink: any = { m(...xs: any[]) { return "m" + show(...xs); } };
class Sink {
  tag = "c";
  m(a?: any, b?: any, c?: any) { return this.tag + show(a, b, c) + arguments.length; }
}
const inst = new Sink();
function modFn(a?: any, b?: any) { return "f" + show(a, b) + arguments.length; }
let closure: any = (...xs: any[]) => "k" + show(...xs);

function toAny() { return sink.m(...arguments); }
function toClass() { return inst.m(...arguments); }
function toModule() { return modFn(...arguments); }
function toClosure() { return closure(...arguments); }
function toBuiltin() { return Math.max(...arguments); }
function toCall(this: any) { return modFn.call(this, ...arguments); }
for (const args of [[], [1], [1, 2], [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18]]) {
  const f: any[] = [toAny, toClass, toModule, toClosure, toBuiltin, toCall];
  console.log(args.length, f.map((g) => String(g(...args))).join(" "));
}

// Fewer and more arguments than declared parameters; arguments.length and
// arguments[k] next to the spread.
function formals(a: any, b: any) {
  return [arguments.length, arguments[0], arguments[3], show(...arguments), sink.m(a, ...arguments, b)].join("|");
}
console.log((formals as any)(), (formals as any)(1), (formals as any)(1, 2, 3, 4, 5));

// Strict: a reassigned parameter does not change what is forwarded.
function reassigned(a: any) { a = "changed"; return show(...arguments) + a; }
console.log((reassigned as any)("orig", "x"));

// Default and rest parameters next to the forwarded list.
function withDefault(a: any = "d", b: any = a + "!") { return show(...arguments) + show(a, b); }
console.log((withDefault as any)(), (withDefault as any)(undefined, 2), (withDefault as any)(7));
function withRest(a: any, ...rest: any[]) { rest.push("r"); return show(...arguments) + show(...rest); }
console.log((withRest as any)(1, 2, 3));

// The callee's own rest array is fresh: mutating it does not reach the
// forwarder's list, which is forwarded again afterwards.
function mutator(...xs: any[]) { xs[0] = "mut"; xs.push("p"); return xs.length; }
function twice() { const n = mutator(...arguments); return n + show(...arguments) + mutator(...arguments); }
console.log((twice as any)(1, 2));

// Several spreads and interleaved regular arguments.
function multi() { return show(0, ...arguments, "mid", ...arguments, 9); }
console.log((multi as any)("a", "b"));

// Arrow functions see the enclosing function's arguments (a capture).
function viaArrow() { const g = () => show(...arguments); return g(); }
console.log((viaArrow as any)(4, 5));
function viaArrowLater() { const g = () => sink.m(...arguments); const r = show(...arguments); return r + g(); }
console.log((viaArrowLater as any)("x"));
// An arrow's own (non-)arguments: a nested regular function has its own.
function outer() { function inner() { return show(...arguments); } return inner(9) + show(...arguments); }
console.log((outer as any)(1, 2));

// The three.js WebGLState shape: forwarding inside try, exceptions crossing it.
const gl: any = { tex(a: any, b: any) { if (a === "bad") throw new TypeError("bad " + b); return show(...arguments); } };
const state: any = {
  tex: function () {
    try {
      return gl.tex(...arguments);
    } catch (error) {
      return "caught " + (error as Error).message;
    }
  },
};
console.log(state.tex(1, 2, 3), state.tex("bad", "x"));

// Other spread forms of arguments next to a call spread.
function others() { return show([...arguments].length, ...arguments) + Math.min(...arguments); }
console.log((others as any)(3, 1, 2));
class Base { v: any[]; constructor(...xs: any[]) { this.v = xs; } }
class Derived extends Base { constructor() { super(...arguments); } }
function newer() { return new (Derived as any)(...arguments).v.join(","); }
console.log((newer as any)(1, 2, 3), new (Derived as any)("p", "q").v.join(","));

// Generators keep their Arguments object (iterator state outlives the call).
function* gen() { yield show(...arguments); yield arguments.length; }
console.log([...(gen as any)(1, 2)].join(" "));

// Forwarding through many calls with allocation in between (collections move
// the argument list while it is forwarded).
function alloc(...xs: any[]) { const junk: any[] = []; for (let i = 0; i < 50; i++) junk.push({ i, s: "s" + i }); return xs.join(",") + junk.length; }
function fwdAlloc() { return alloc(...arguments); }
let acc = 0;
for (let i = 0; i < 20000; i++) { const r = (fwdAlloc as any)({ i }, "v" + i, [i]); acc += r.length; }
console.log("alloc", acc);
