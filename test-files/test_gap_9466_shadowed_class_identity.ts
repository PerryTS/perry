// #9466: same-name class declarations at different lexical depths are DISTINCT
// classes. Perry disambiguates them with a compiler-internal registration key
// (`M$0` — see `maybe_rename_colliding_class`), but that key was minted once
// per NAME instead of once per SCOPE: a nested body inherited the enclosing
// body's alias and its `class M` aliased onto the SAME ClassId, so the third
// and every later same-name class silently ran an earlier one's body. Bare
// blocks never ran the disambiguation scan at all, so sibling `{ class X }`
// blocks collided too.
//
// Every arm distinguishes IDENTITY, not just dispatch: the `instanceof` arms
// are the ones that catch a fix that re-splits names without re-splitting
// class ids.

// --- 1. three depths, three bodies ---------------------------------------
class M { v() { return "top"; } }
function h() {
  class M { v() { return "outer"; } }
  function h2() {
    class M { v() { return "inner"; } }
    return new M().v();
  }
  return [new M().v(), h2()].join(",");
}
console.log("depths:", new M().v(), h());

// --- 1b. three depths with NO top-level declaration of the name -----------
// The outermost declarer keeps the raw registration key; every nested one
// must still mint its own.
function deepA() {
  class D { v() { return "d1"; } }
  function deepB() {
    class D { v() { return "d2"; } }
    function deepC() {
      class D { v() { return "d3"; } }
      return new D().v();
    }
    return [new D().v(), deepC()].join(",");
  }
  return [new D().v(), deepB()].join(",");
}
console.log("no-top:", deepA());

// --- 2a. sibling blocks at module top level -------------------------------
{ class Blk { v() { return "b1"; } } console.log("blk1:", new Blk().v()); }
{ class Blk { v() { return "b2"; } } console.log("blk2:", new Blk().v()); }
{ class Blk { v() { return "b3"; } } console.log("blk3:", new Blk().v()); }

// --- 2b. sibling blocks inside a function ---------------------------------
function blocksInFn() {
  const out: string[] = [];
  { class Q { v() { return "q1"; } } out.push(new Q().v()); }
  { class Q { v() { return "q2"; } } out.push(new Q().v()); }
  { class Q { v() { return "q3"; } } out.push(new Q().v()); }
  return out.join(",");
}
console.log("fn-blocks:", blocksInFn());

// --- 2c. same-name classes in sibling functions ---------------------------
function sibA() { class S { v() { return "sA"; } } return new S().v(); }
function sibB() { class S { v() { return "sB"; } } return new S().v(); }
function sibC() { class S { v() { return "sC"; } } return new S().v(); }
console.log("siblings:", sibA(), sibB(), sibC());

// --- 2d. if/else branches and try/catch/finally are lexical scopes too ----
class If1 { v() { return "if-top"; } }
function branches(flag: boolean) {
  if (flag) {
    class If1 { v() { return "then"; } }
    return new If1().v();
  } else {
    class If1 { v() { return "else"; } }
    return new If1().v();
  }
}
console.log("branches:", branches(true), branches(false), new If1().v());

class T1 { v() { return "t-top"; } }
function tryCatchFinally() {
  const out: string[] = [];
  try {
    class T1 { v() { return "try"; } }
    out.push(new T1().v());
    throw new Error("x");
  } catch {
    class T1 { v() { return "catch"; } }
    out.push(new T1().v());
  } finally {
    class T1 { v() { return "finally"; } }
    out.push(new T1().v());
  }
  out.push(new T1().v());
  return out.join(",");
}
console.log("try:", tryCatchFinally());

class L { v() { return "L-top"; } }
function loopBody() {
  const acc: string[] = [];
  for (let i = 0; i < 2; i++) {
    class L { v() { return "L-body"; } }
    acc.push(new L().v());
  }
  acc.push(new L().v());
  return acc.join(",");
}
console.log("loop:", loopBody());

// --- 3. shadowed classes captured in closures, called after the block exits
const closures: Array<() => string> = [];
{
  class Cap { v() { return "cap1"; } }
  closures.push(() => new Cap().v());
}
{
  class Cap { v() { return "cap2"; } }
  closures.push(() => new Cap().v());
}
function capFn() {
  class Cap { v() { return "cap-fn"; } }
  return () => new Cap().v();
}
closures.push(capFn());
class Cap { v() { return "cap-top"; } }
closures.push(() => new Cap().v());
console.log("closures:", closures.map((f) => f()).join(","));

// --- 4. instanceof across the shadowing boundary --------------------------
class P { tag() { return "P-top"; } }
const topP = new P();
function innerP() {
  class P { tag() { return "P-inner"; } }
  const p = new P();
  return {
    inst: p,
    ownIsInner: p instanceof P,
    topIsInner: topP instanceof P,
    cls: P as any,
  };
}
const r = innerP();
console.log("io own-inner:", r.ownIsInner);
console.log("io top-is-inner:", r.topIsInner);
console.log("io inner-is-top:", r.inst instanceof P);
console.log("io top-is-top:", topP instanceof P);
console.log("io ctor-identity:", r.cls === P);
console.log("io proto-identity:", Object.getPrototypeOf(r.inst) === P.prototype);
console.log("io tags:", topP.tag(), r.inst.tag());

// --- 5. `.name` stays the SOURCE name for every one of them (#9413) -------
function nameA() { class N {} return N.name; }
function nameB() { class N {} return N.name; }
function nameC() { function d() { class N {} return N.name; } return d(); }
class N {}
console.log("names:", N.name, nameA(), nameB(), nameC());
console.log("ctor-names:", new M().constructor.name, r.inst.constructor.name);

// --- 6. subclassing a shadowed class inside the inner scope ---------------
class B { who() { return "B-top"; } }
class SubTop extends B {}
function innerSub() {
  class B { who() { return "B-inner"; } }
  class Sub extends B { both() { return this.who() + "/sub"; } }
  const s = new Sub();
  return [
    s.who(),
    s.both(),
    String(s instanceof B),
    String(s instanceof Sub),
    String(s instanceof SubTop),
  ].join(",");
}
console.log("sub-top:", new SubTop().who(), new SubTop() instanceof B);
console.log("sub-inner:", innerSub());

// --- 7. switch: a bare case statement-list shares ONE switch block scope ---
class Sw { v() { return "sw-top"; } }
function switchBare(k: number) {
  switch (k) {
    case 1:
      class Sw { v() { return "sw-case"; } }
      return new Sw().v();
    default:
      return "none";
  }
}
console.log("switch-bare:", switchBare(1), switchBare(2), new Sw().v());

// A braced case is its own block scope on top of the switch's.
class Sw2 { v() { return "sw2-top"; } }
function switchBraced(k: number) {
  switch (k) {
    case 1: { class Sw2 { v() { return "c1"; } } return new Sw2().v(); }
    case 2: { class Sw2 { v() { return "c2"; } } return new Sw2().v(); }
    default: return new Sw2().v();
  }
}
console.log("switch-braced:", switchBraced(1), switchBraced(2), switchBraced(3));

// --- 8. loop body: ONE declaration site, so ONE class for every iteration ---
// (the disambiguation is keyed on the declaration's source span, and every
// iteration shares that span). The closures must still hold the inner class
// after the loop exits, and the post-loop `new Lp()` must get the OUTER one.
class Lp { v() { return "lp-top"; } }
function loopSameClass() {
  const fs: Array<() => string> = [];
  for (let i = 0; i < 3; i++) {
    class Lp { v() { return "lp-body"; } }
    fs.push(() => new Lp().v());
  }
  return fs.map((f) => f()).join(",") + "|" + new Lp().v();
}
console.log("loop-same-class:", loopSameClass());

// NOT covered here: the same loop body where the class CAPTURES the loop
// variable (`class Cp { v() { return "cp" + i; } }`). Node gives one class
// with three environments (`cp0,cp1,cp2`); perry gives `cp3,cp3,cp3` because
// a class carries ONE `RegisterClassCaptures` snapshot, refreshed at
// assignments and returns — neither of which a loop body has. That is the
// class-capture mechanism, not class identity: it reproduces with NO name
// shadowing anywhere (`class Uniq` declared in a loop body, nothing else
// named Uniq in the program) and is byte-identical before and after this fix.
// Filed separately so this fixture keeps discriminating exactly one thing.

// --- 9. instanceof across a BLOCK boundary, and at the THIRD depth ---------
// Arm 4's instanceof rows sit at two-scope depth, which the name-keyed
// disambiguation already handled; these two are where identity actually broke.
class Ib { tag() { return "ib-top"; } }
const ibTop = new Ib();
let ibInnerInst: any = null;
let ibInnerCls: any = null;
{
  class Ib { tag() { return "ib-block"; } }
  ibInnerInst = new Ib();
  ibInnerCls = Ib;
}
console.log("blk-io same-class:", ibInnerCls === Ib);
console.log("blk-io inner-is-top:", ibInnerInst instanceof Ib);
console.log("blk-io top-is-inner:", ibTop instanceof ibInnerCls);
console.log("blk-io proto:", Object.getPrototypeOf(ibInnerInst) === Ib.prototype);
console.log("blk-io tags:", ibTop.tag(), ibInnerInst.tag());

function ioDepth() {
  class Id { tag() { return "id-1"; } }
  function inner() {
    class Id { tag() { return "id-2"; } }
    return { inst: new Id(), cls: Id as any };
  }
  const deep = inner();
  return [
    deep.inst.tag(),
    String(deep.cls === Id),
    String(deep.inst instanceof Id),
    String(new Id() instanceof deep.cls),
  ].join(",");
}
class Id { tag() { return "id-top"; } }
console.log("depth-io:", ioDepth(), new Id().tag());
