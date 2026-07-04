// Parity: instance field initializers run on builtin subclasses whose
// constructor is implicit (Error/Map/Set/Array), same as an explicit one.
class E1 extends Error { tag = "e1"; n = 1 + 2; }
class M1 extends Map { tag = "m1"; }
class S1 extends Set { tag = "s1"; }
class A1 extends Array { tag = "a1"; }
class M2 extends Map { tag = "m2"; constructor() { super(); } }
const e = new E1();
console.log(e.tag, (e as any).n, e instanceof Error);
console.log(new M1().tag, new S1().tag, (new A1() as any).tag, new M2().tag);
try { throw new E1("boom"); } catch (x: any) { console.log(x.message, x.tag); }
