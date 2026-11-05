// Issue #838 follow-up (b) — JS-classic prototype-method dispatch when
// the receiver is a function declaration, not a `class` block. dayjs's
// minified bundle (also Babel's class-from-function emit pattern)
// wraps a `function M(){…}` inside an IIFE and attaches methods via
// `var m = M.prototype; m.format = function(){…}` — the original #838
// fix bailed because `lookup_class("M")` returned None for function
// declarations, so the registration fell through to a generic
// PropertySet that nothing in dispatch consulted. After the follow-up:
// the HIR recognises the shape, allocates a synthetic class id keyed
// by the closure's NaN-boxed bits, registers the method on it, and
// the matching `new M(args)` site stamps the same id on the instance.
//
// Note: this test deliberately avoids the TypeScript `function f(this:
// X, …)` typed-this annotation. That syntax is type-only in TS, but a
// separate Perry HIR bug treats it as a real first parameter, which
// shifts every subsequent positional arg by one slot and produces
// `parse:undefined` instead of `parse:foo`. The dayjs minified bundle
// never uses the typed-this shape, so the bug is orthogonal to this
// fix and is left as a follow-up.

// 1. The dayjs minified shape: IIFE returns a function declaration that
// also serves as the constructor. Aliased prototype-method assignment
// inside the IIFE body.
var Klass = (function () {
  function M(t: any) {
    (this as any).val = t;
  }
  var m = (M as any).prototype;
  m.parse = function () {
    return "parse:" + (this as any).val;
  };
  m.init = function () {
    return "init:" + (this as any).val;
  };
  return M;
})();

const a = new (Klass as any)("foo");
console.log("typeof a.parse:", typeof (a as any).parse);
console.log("a.parse():", (a as any).parse());
console.log("a.init():", (a as any).init());

// 2. The direct shape on a function declaration:
// `function M(){}; M.prototype.x = fn`.
function K(n: number) {
  (this as any).n = n;
}
(K as any).prototype.double = function () {
  return (this as any).n * 2;
};
(K as any).prototype.triple = function () {
  return (this as any).n * 3;
};

const b = new (K as any)(7);
console.log("b.double():", (b as any).double());
console.log("b.triple():", (b as any).triple());

// 3. Babel's class-from-function emit shape: `var Foo = function(){
// function Foo(){…}; var _proto = Foo.prototype; _proto.x = fn;
// return Foo; }();` — same as (1) but the outer binding is `var`
// rather than the IIFE result being immediately new'd. The IIFE
// returns the inner function declaration, the outer name aliases to
// that closure, and `new <outer>(args)` then routes through the same
// synthetic-class-id machinery.
var Babel: any = (function () {
  function Inner(label: string) {
    (this as any).label = label;
  }
  var p = (Inner as any).prototype;
  p.greet = function () {
    return "hello, " + (this as any).label;
  };
  return Inner;
})();

const c = new (Babel as any)("world");
console.log("c.greet():", (c as any).greet());
