// G8: `f.apply(x, arguments)` in a function whose Arguments object is otherwise unobserved at
// that point hands the caller's argument list to the intrinsic Function.prototype.apply without
// building the object (perry-codegen codegen/arguments.rs, deferred_forward_apply_binding). A
// replaced or own `apply` gets the one Arguments object, born at that site; a later read sees the
// same object. Module code is strict, so no parameter is mapped. Compared against Node.
const show = (...xs: any[]) => JSON.stringify(xs.map((x) => (x === undefined ? "u" : x)));
const target = {
  tag: "T",
  m(this: any, a?: any, b?: any, c?: any) { return this.tag + show(a, b, c) + arguments.length; },
};
function plain(this: any) { return target.m.apply(this, arguments as any); }
function viaTarget() { return target.m.apply(target, arguments as any); }
function withParams(this: any, a: any, b: any) { return target.m.apply(this, arguments as any) + a + b; }
function inTry() {
  try { return target.m.apply(target, arguments as any); } catch (e) { return "caught " + (e as Error).message; }
}
function thenRead() { const r = target.m.apply(target, arguments as any); return r + "|" + arguments.length + show(...arguments); }
for (const args of [[], [1], [1, 2, 3], [{ o: 1 }, "s", null, undefined, 5]]) {
  console.log(args.length, (plain as any).apply({ tag: "P" }, args), (viaTarget as any)(...args), (withParams as any).call({ tag: "W" }, ...args), (inTry as any)(...args), (thenRead as any)(...args));
}

// An own `apply` on the target: it receives the Arguments object, born once per activation.
const seen: any[] = [];
const own: any = function (this: any, ...xs: any[]) { return "own" + show(...xs); };
own.apply = function (thisArg: any, list: any) {
  seen.push(list);
  return "ownApply:" + Object.prototype.toString.call(list) + ":" + list.length + ":" + show(...Array.from(list));
};
function toOwn() { const a = own.apply(null, arguments); const b = own.apply(null, arguments); return a + " " + b + " " + (seen[seen.length - 1] === seen[seen.length - 2]) + " " + (seen[seen.length - 1] === arguments); }
console.log((toOwn as any)(1, 2), (toOwn as any)());

// A getter for `apply` runs once per call, before the list is read.
let getterRuns = 0;
const viaGetter: any = function (...xs: any[]) { return "g" + show(...xs); };
Object.defineProperty(viaGetter, "apply", { get() { getterRuns++; return Function.prototype.apply; } });
function toGetter() { return viaGetter.apply(null, arguments); }
console.log((toGetter as any)(7, 8), getterRuns);

// A non-callable target and a non-function `apply` throw the same TypeErrors.
const notFn: any = { apply: 5 };
function toBad() { try { return notFn.apply(null, arguments); } catch (e) { return e instanceof TypeError; } }
console.log((toBad as any)(1));
const fake: any = { apply: Function.prototype.apply };
function toFake() { try { return fake.apply(null, arguments); } catch (e) { return e instanceof TypeError; } }
console.log((toFake as any)(1));

// Arguments read before the apply: the born object is passed.
function readFirst() { const n = arguments.length; return n + target.m.apply(target, arguments as any); }
console.log((readFirst as any)("x", "y"));
// A closure capturing arguments, and the arguments object compared by identity.
function captured() { const g = () => arguments; return target.m.apply(target, arguments as any) + (g() === g()); }
console.log((captured as any)(3));
// Many calls with heap values (the list is the caller's bundle, never retained).
let total = 0;
for (let i = 0; i < 20000; i++) total += (viaTarget as any)({ i }, "v" + i, [i]).length;
console.log("total", total);
