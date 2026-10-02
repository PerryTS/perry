// Same-named classes in sibling / nested scopes are distinct classes.

import * as ma from "./mod_a.ts";
import * as mb from "./mod_b.ts";

// expected.txt is node 26.5.1 output: `node --experimental-strip-types main.ts`.

// 1. The reported shape: prototype write on the second block's base class,
//    read through the second block's subclass.
let firstB: any;
let firstC: any;
{
  class B {}
  class C extends B {}
  void new C();
  firstB = B;
  firstC = C;
}
{
  class B {}
  class C extends B {}
  (B.prototype as any).j = 6;
  console.log("proto-write", (new C() as any).j, (new B() as any).j, (new firstC() as any).j);
  console.log("instanceof", new C() instanceof B, new firstC() instanceof B, new C() instanceof firstB);
  console.log("identity", B === firstB, C === firstC, Object.getPrototypeOf(C) === B);
}

// 2. Methods and fields on same-named classes in sibling blocks.
{
  class K { f = "f1"; m() { return "m1"; } }
  class S extends K { n() { return "n1:" + this.m(); } }
  const s = new S();
  console.log("block1", s.f, s.m(), s.n());
}
{
  class K { f = "f2"; m() { return "m2"; } }
  class S extends K { n() { return "n2:" + this.m(); } }
  const s = new S();
  console.log("block2", s.f, s.m(), s.n());
  (K.prototype as any).m = function () { return "patched2"; };
  console.log("patched", s.m(), s.n());
}
{
  class K { f = "f3"; m() { return "m3"; } }
  console.log("block3", new K().f, new K().m());
}

// 3. Prototype method writes (method-shaped values) on a renamed class.
{
  class P {}
  (P.prototype as any).hello = function () { return "p1"; };
  console.log("pm1", (new P() as any).hello());
}
{
  class P {}
  (P.prototype as any).hello = function () { return "p2"; };
  console.log("pm2", (new P() as any).hello());
}
{
  class P {}
  console.log("pm3", typeof (new P() as any).hello);
}

// 4. Static members.
{
  class St { static v = 1; static get() { return "s1"; } }
  console.log("static1", St.v, St.get());
  St.v = 10;
}
{
  class St { static v = 2; static get() { return "s2"; } }
  console.log("static2", St.v, St.get());
  (St as any).w = 7;
}
{
  class St { static v = 3; }
  console.log("static3", St.v, (St as any).w);
}

// 5. A function called twice: each call creates a fresh class.
function make(tag: string) {
  class F { t() { return tag; } }
  class G extends F {}
  (F.prototype as any).extra = "x" + tag;
  return { F, G };
}
const m1 = make("a");
const m2 = make("b");
console.log("fn", new m1.G().t(), new m2.G().t(), (new m1.G() as any).extra, (new m2.G() as any).extra);
console.log("fn-instanceof", new m1.G() instanceof m2.F, new m2.G() instanceof m2.F, m1.F === m2.F);

// 6. A loop body declaring a class that shadows an outer same-named class.
class LoopK { v() { return "outer"; } }
const seen: string[] = [];
for (let i = 0; i < 3; i++) {
  class LoopK { v() { return "inner"; } }
  (LoopK.prototype as any).w = "w" + i;
  seen.push(new LoopK().v() + ":" + (new LoopK() as any).w);
}
console.log("loop", seen.join(","), new LoopK().v(), (new LoopK() as any).w);

// 7. Nested blocks shadowing an outer class of the same name.
class N { who() { return "outer"; } }
{
  class N { who() { return "mid"; } }
  (N.prototype as any).q = "midq";
  {
    class N { who() { return "inner"; } }
    (N.prototype as any).q = "innerq";
    console.log("nested-inner", new N().who(), (new N() as any).q);
  }
  console.log("nested-mid", new N().who(), (new N() as any).q);
}
console.log("nested-outer", new N().who(), (new N() as any).q);

// 8. Sibling blocks inside a function body.
function inFn() {
  const out: string[] = [];
  {
    class B {}
    class C extends B {}
    out.push(String((new C() as any).k));
  }
  {
    class B {}
    class C extends B {}
    (B.prototype as any).k = "fk";
    out.push(String((new C() as any).k));
  }
  return out.join(",");
}
console.log("fn-blocks", inFn(), inFn());

// 9. Class expressions with the same name.
const E1 = class E { e() { return "e1"; } };
const E2 = class E { e() { return "e2"; } };
(E2.prototype as any).z = "z2";
console.log("classexpr", new E1().e(), new E2().e(), (new E1() as any).z, (new E2() as any).z, new E1() instanceof E2);

// 10. Same-named classes in two modules.
(mb.Base.prototype as any).tag = "tb";
console.log(
  "modules",
  new ma.Kid().who(),
  new mb.Kid().who(),
  (new ma.Kid() as any).tag,
  (new mb.Kid() as any).tag,
  new ma.Kid() instanceof mb.Base,
  ma.Base === mb.Base,
);
