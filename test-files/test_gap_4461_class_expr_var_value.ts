// Issue #4461: a class EXPRESSION bound to a variable — `var X = class {...}`
// / `const X = class l {...}` — must be a usable value when referenced by
// name, not just a `new X()` target. Perry pre-registered the binding as a
// module-level local that was never assigned (the class-expression lowering
// emits no `Stmt::Let` for it), so a value read of `X` (`typeof X`,
// `X.staticMethod`, passing `X` around) resolved to the undefined local
// instead of the class ref. Common in minified npm dist bundles
// (esbuild/rollup emit `var Cls = class name {...}` then reference `Cls`),
// which blocked `marked` (`g.parser = b.parse` where `b = class l { static
// parse(){} }`).

// Bare value read: typeof and a static-method call off the binding.
var B = class l {
  static parse(e: any) { return "parsed:" + e; }
  parse(e: any) { return e; }
};
console.log("typeof B =", typeof B);
console.log("new B works:", typeof new B());
console.log(B.parse(1));

// marked's actual shape: a static-only method read off the binding as a
// VALUE (not a direct call) and stashed on an object.
var b = class l {
  static parse(e: any) { return "parse:" + e; }
};
const g: any = {};
g.parser = b.parse;
console.log(g.parser(5));

// const class expression with a self-binding name used inside methods.
const Node2 = class _Node {
  v: number;
  constructor(v: number) { this.v = v; }
  clone() { return new _Node(this.v + 1); }
  static make(v: number) { return new _Node(v); }
};
console.log(typeof Node2);
console.log(new Node2(1).clone().v);
console.log(Node2.make(9).v);

// Pass the binding around as a first-class value.
function callNew(C: any) { return new C(); }
var Empty = class { ok() { return "ok"; } };
console.log(callNew(Empty).ok());

// Regression guard: an ordinary `var` initialised with a non-class value is
// still a real, reassignable local.
var n = 42;
n = n + 1;
console.log(n);
