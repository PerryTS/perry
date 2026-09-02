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
