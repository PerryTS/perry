// Sloppy-mode `this` is bound ONCE per activation (OrdinaryCallBindThis):
// undefined/null become globalThis, a primitive becomes ONE wrapper object,
// objects pass unchanged. Every `this` in one activation — and every arrow
// that inherits it — must name that same value: `this === this` holds for a
// primitive receiver. Covers function declarations, nested declarations,
// function expressions, object-literal methods, generators, async functions,
// class references, and a "use strict" function in a sloppy file.
function kind(this: any) { return typeof this; }
function same(this: any) { return this === this; }
function viaArrow(this: any) { const a = () => this; return a() === this && a() === a(); }
function tagOf(this: any) { return Object.prototype.toString.call(this); }
function isGlobal(this: any) { return this === globalThis; }
function keep(this: any) { return this; }
function twice(this: any) { const x = this; const y = this; return x === y; }
function mutate(this: any) { this.extra = 1; return this.extra; }

const receivers: any[] = [1, 2.5, -0, NaN, true, false, "s", "", 10n];
for (const r of receivers) {
  const label = typeof r === "bigint" ? "bigint" : JSON.stringify(r);
  console.log(
    "decl", label, kind.call(r), same.call(r), viaArrow.call(r), tagOf.call(r),
    twice.call(r), mutate.call(r), keep.call(r) === keep.call(r),
  );
}
console.log("nullish", isGlobal.call(undefined), isGlobal.call(null), isGlobal(), kind.call(undefined));
const o = { a: 1 };
console.log("object", keep.call(o) === o, same.call(o), kind.call(o));
const fnRecv = function () { return 3; };
console.log("function", keep.call(fnRecv) === fnRecv, kind.call(fnRecv));

// Nested declaration and a function expression.
function outer(this: any) {
  function inner(this: any) { return [typeof this, this === this, this instanceof Number]; }
  return inner.call(this);
}
console.log("nested", JSON.stringify(outer.call(7)), JSON.stringify(outer.call(o)));
const expr = function (this: any) { return [typeof this, this === this, this instanceof String]; };
console.log("expr", JSON.stringify(expr.call("x")), JSON.stringify(expr.call(4)));

// Object-literal method called with a primitive.
const lit: any = { m() { return [typeof this, this === this, this instanceof Boolean]; } };
console.log("literal", JSON.stringify(lit.m.call(true)), JSON.stringify(lit.m.call(9)));

// A method call ON a primitive: the receiver reaches the body unboxed, and
// the body binds it once.
(Number.prototype as any).kindOf = function (this: any) { return [typeof this, this === this, this instanceof Number]; };
(String.prototype as any).kindOf = function (this: any) { const a = () => this; return [typeof this, this === a(), this.length]; };
(Boolean.prototype as any).kindOf = function (this: any) { return [typeof this, this === this, this.valueOf()]; };
const five: any = 5;
const str: any = "abc";
const yes: any = true;
console.log("proto-method", JSON.stringify(five.kindOf()), JSON.stringify(str.kindOf()), JSON.stringify(yes.kindOf()));
let hot = 0;
for (let i = 0; i < 3000; i++) { const r = (i as any).kindOf(); if (r[0] === "object" && r[1] && r[2]) hot++; }
console.log("proto-method-hot", hot);

// A detached function expression / method called plainly binds globalThis.
const detachedExpr = function (this: any) { return this === globalThis; };
const detachedLit: any = { m() { return this === globalThis; } };
const dm = detachedLit.m;
console.log("detached", detachedExpr(), dm(), typeof (function (this: any) { return this; })());

// A primitive receiver's wrapper is a fresh object per ACTIVATION.
console.log("per-activation", keep.call(5) === keep.call(5), keep.call(5) == keep.call(5));
console.log("value", keep.call(5).valueOf(), keep.call("ab").length, keep.call(true).valueOf());

// Class reference receiver stays the class.
class C { static tag = "C"; }
function readTag(this: any) { return this === C ? this.tag : "boxed"; }
console.log("classref", readTag.call(C), keep.call(C) === C);

// Generators and async functions bind their receiver the same way.
function* gen(this: any) { yield typeof this; yield this === this; const a = () => this; yield a() === this; }
console.log("generator", JSON.stringify([...gen.call(3)]), JSON.stringify([...gen.call("q")]));
async function af(this: any) { const before = this; await null; return [typeof this, this === before, this === this]; }
af.call(8).then((v) => console.log("async", JSON.stringify(v)));

// A "use strict" function in a sloppy file sees the primitive itself.
function strictOne(this: any) { "use strict"; return [typeof this, this === this, this === 6]; }
function strictKeep(this: any) { "use strict"; return this; }
console.log("strict", JSON.stringify(strictOne.call(6)), strictKeep.call(undefined) === undefined, strictKeep.call("z") === "z");

// Many activations under allocation pressure keep their own wrappers.
let ok = 0;
for (let i = 0; i < 20000; i++) {
  const w = keep.call(i);
  const junk = { i, s: "x" + i };
  if (typeof w === "object" && w.valueOf() === i && same.call(i) && junk.i === i) ok++;
}
console.log("pressure", ok);
