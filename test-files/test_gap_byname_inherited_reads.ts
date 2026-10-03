// Inherited and absent static-key reads answered from holder shapes (#10495):
// every phase reads through the same sites long enough for them to cache, then
// changes what the shapes prove and reads again. The output must stay node's.

const N = 40;

// --- class prototype data added with `C.prototype.k = v` (before the
// prototype object exists), read through instances: inline and spilled keys.
class D {
  a: number;
  b: number;
  c: number;
  constructor(v: number) {
    this.a = v;
    this.b = v + 1;
    this.c = v + 2;
  }
  m() {
    return 1;
  }
}
(D.prototype as any).DB = 26;
(D.prototype as any).DM = 67108863;
(D.prototype as any).DV = 67108864;
const ds: any[] = [];
for (let i = 0; i < 8; i++) ds.push(new D(i));

function readD(o: any): string {
  return `${o.DB}|${o.DM}|${o.DV}|${o.missing}|${o.m === D.prototype.m}`;
}
function phase(label: string, f: (o: any) => string, objs: any[]) {
  let last = "";
  let changes = 0;
  for (let i = 0; i < N; i++) {
    const r = f(objs[i % objs.length]);
    if (r !== last) {
      changes++;
      last = r;
    }
  }
  console.log(label, last, changes);
}
phase("D.start", readD, ds);
(D.prototype as any).DB = 27; // value store on the holder
phase("D.value", readD, ds);
ds[3].DB = "own"; // shadowing on one instance (its shape moves)
phase("D.shadow3", (o) => String(o.DB), [ds[3]]);
phase("D.others", readD, [ds[0], ds[1]]);
delete (D.prototype as any).DM; // delete on the prototype
phase("D.deleted", readD, ds);
(Object.prototype as any).missing = "fromObjectProto"; // absent key appears on Object.prototype
phase("D.objproto", readD, ds);
delete (Object.prototype as any).missing;
phase("D.objproto.gone", readD, ds);
Object.defineProperty(D.prototype, "DV", { get() { return "getter"; }, configurable: true });
phase("D.accessor", readD, ds);
(D.prototype as any).DB = undefined; // a spilled undefined is a found value
phase("D.undef", (o) => `${o.DB}|${"DB" in o}`, ds);

// --- three levels of inheritance: data on the root, the middle, the leaf.
class A1 {
  x() {
    return "A1";
  }
}
class B1 extends A1 {}
class C1 extends B1 {}
(A1.prototype as any).depth = "A1";
(B1.prototype as any).mid = "B1";
const cs: any[] = [new C1(), new C1(), new B1()];
phase("chain.start", (o) => `${o.depth}|${o.mid}|${o.nope}`, cs);
(B1.prototype as any).depth = "B1-shadow";
phase("chain.shadowB", (o) => `${o.depth}|${o.mid}|${o.nope}`, cs);
delete (B1.prototype as any).depth;
phase("chain.unshadow", (o) => `${o.depth}|${o.mid}|${o.nope}`, cs);
Object.setPrototypeOf(cs[0], { depth: "custom", mid: "custom" });
phase("chain.setproto", (o) => `${o.depth}|${o.mid}|${o.nope}`, cs);

// --- absent keys on class instances, Object.prototype mutated, null protos.
class E {
  $L = "en";
  $d = 1;
}
const es: any[] = [new E(), new E()];
phase("absent.start", (o) => `${o.$u}|${o.$offset}|${o.$x}`, es);
(Object.prototype as any).$offset = 9;
phase("absent.objproto", (o) => `${o.$u}|${o.$offset}|${o.$x}`, es);
delete (Object.prototype as any).$offset;
phase("absent.restored", (o) => `${o.$u}|${o.$offset}|${o.$x}`, es);
(E.prototype as any).$x = "onE";
phase("absent.onproto", (o) => `${o.$u}|${o.$offset}|${o.$x}`, es);
const np: any = Object.create(null);
np.k = 1;
phase("absent.nullproto", (o) => `${o.k}|${o.toString}|${o.$u}`, [np]);

// --- plain objects of many shapes reading `constructor` and other inherited
// keys from Object.prototype (one site, several receiver shapes).
const plain: any[] = [{ a: 1 }, { b: 2 }, { c: 3, d: 4 }, { e: 5 }, { f: 6 }, { g: 7, h: 8 }];
phase("ctor.start", (o) => `${o.constructor === Object}|${typeof o.hasOwnProperty}|${o.zz}`, plain);
const own: any = { constructor: "mine", a: 1 };
phase("ctor.own", (o) => `${o.constructor === Object}|${o.constructor === "mine"}`, [own, plain[0]]);
phase("ctor.getproto", (o) => `${Object.getPrototypeOf(o) === Object.prototype}|${o instanceof Object}`, [own, ...plain]);
class K {}
phase("ctor.class", (o) => `${o.constructor === K}|${o.constructor === Object}`, [new K(), plain[1]]);

// --- numeric-string and symbol keys are never answered by the holder walk.
const sym = Symbol("s");
(D.prototype as any)[sym] = "sym";
(D.prototype as any)["7"] = "seven";
phase("keys.odd", (o) => `${o[sym]}|${o["7"]}|${o[7]}`, ds);

// --- a prototype with more keys than its inline slots: the late keys live in
// its spill storage and are read from there.
class Wide {
  w0 = 0;
}
const wp: any = Wide.prototype;
const wideKeys = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9", "p10", "p11", "p12", "p13"];
for (let i = 0; i < wideKeys.length; i++) wp[wideKeys[i]] = 100 + i;
const ws: any[] = [new Wide(), new Wide()];
const readWide = (o: any) => `${o.p0}|${o.p7}|${o.p9}|${o.p12}|${o.p13}|${o.pX}`;
phase("wide.start", readWide, ws);
wp.p12 = "changed";
phase("wide.value", readWide, ws);
wp.p13 = undefined;
phase("wide.undef", readWide, ws);
delete wp.p9;
phase("wide.deleted", readWide, ws);
const plainProto: any = {};
for (let i = 0; i < wideKeys.length; i++) plainProto[wideKeys[i]] = 200 + i;
const viaCreate: any[] = [Object.create(plainProto), Object.create(plainProto)];
phase("wide.create", readWide, viaCreate);
plainProto.p13 = "late";
phase("wide.create.value", readWide, viaCreate);
