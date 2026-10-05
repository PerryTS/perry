// A read site's class entry over a DEEP prototype chain (deeper than the
// site's own holder words): babel's parser reads `this.match` through eight
// layers of subclasses and mixins. Every way the chain can change after the
// entry is primed must be seen by the next read.
const out: string[] = [];
const N = 3000;

class L0 {
  base = 0;
  m(): string { return "L0.m"; }
  get g(): string { return "L0.g"; }
}
L0.prototype["v"] = "L0.v";
class L1 extends L0 { a1 = 1; }
class L2 extends L1 { a2 = 2; }
class L3 extends L2 { a3 = 3; }
class L4 extends L3 { a4 = 4; }
class L5 extends L4 { a5 = 5; }
class L6 extends L5 { a6 = 6; }
class L7 extends L6 { a7 = 7; }
class L8 extends L7 { a8 = 8; }

function rv(o: any): any { return o.v; }
function rm(o: any): any { return o.m; }
function rg(o: any): any { return o.g; }
function rabs(o: any): any { return o.missing; }
function fn(f: any): string { return typeof f === "function" ? "fn:" + f.call(null) : String(f); }

function loop(label: string, read: (o: any) => any, o: any, mutate: () => void, show: (v: any) => string = String): void {
  const seen: string[] = [];
  let last = "";
  for (let i = 0; i < N; i++) {
    if (i === N / 2) mutate();
    const s = show(read(o));
    if (s !== last) { seen.push(i + "=" + s); last = s; }
  }
  out.push(label + " " + seen.join(" "));
}

// 1. data on the deep holder: value overwrite, shadow on a middle prototype,
//    own shadow, delete from the holder.
const a = new L8();
loop("overwrite", rv, a, () => { L0.prototype["v"] = "L0.v2"; });
loop("shadow-mid", rv, a, () => { (L4.prototype as any).v = "L4.v"; });
loop("own-shadow", rv, a, () => { (a as any).v = "own"; });
loop("delete-own", rv, a, () => { delete (a as any).v; });
loop("delete-mid", rv, a, () => { delete (L4.prototype as any).v; });
loop("delete-holder", rv, a, () => { delete (L0.prototype as any).v; });
loop("re-add-holder", rv, a, () => { L0.prototype["v"] = "back"; });

// 2. methods read as values through the deep chain.
const b = new L8();
loop("method", rm, b, () => { (L6.prototype as any).m = function () { return "L6.m"; }; }, fn);
loop("method-del", rm, b, () => { delete (L6.prototype as any).m; }, fn);
loop("method-replace", rm, b, () => { L0.prototype.m = function () { return "L0.m2"; }; }, fn);

// 3. getters through the deep chain; a data property turned into a getter.
const c = new L8();
loop("getter", rg, c, () => {
  Object.defineProperty(L2.prototype, "g", { get() { return "L2.g"; }, configurable: true });
});
loop("data-to-getter", rv, c, () => {
  Object.defineProperty(L5.prototype, "v", { get() { return "L5.get"; }, configurable: true });
});
delete (L5.prototype as any).v;
delete (L2.prototype as any).g;

// 4. absent reads through the deep chain: added later anywhere, including
//    Object.prototype; holder value set to undefined / null.
const d = new L8();
loop("absent-then-mid", rabs, d, () => { (L3.prototype as any).missing = "L3.missing"; });
loop("absent-holder-undef", rabs, d, () => { (L3.prototype as any).missing = undefined; });
loop("absent-holder-null", rabs, d, () => { (L3.prototype as any).missing = null; });
delete (L3.prototype as any).missing;
loop("absent-then-objproto", rabs, d, () => { (Object.prototype as any).missing = "OP.missing"; });
delete (Object.prototype as any).missing;

// 5. the chain relinked under the entry: setPrototypeOf on a middle prototype.
class Alt { v = "unused"; }
(Alt.prototype as any).v = "Alt.v";
const e = new L8();
L0.prototype["v"] = "L0.v";
loop("relink-mid", rv, e, () => { Object.setPrototypeOf(L5.prototype, Alt.prototype); });
loop("relink-back", rv, e, () => { Object.setPrototypeOf(L5.prototype, L4.prototype); });

// 6. a Proxy spliced into the chain.
const f = new L8();
loop("proxy-in-chain", rv, f, () => {
  const p = new Proxy(Object.create(L3.prototype), {
    get(t, k, r) { return k === "v" ? "proxy.v" : Reflect.get(t, k, r); },
  });
  Object.setPrototypeOf(L6.prototype, p);
});
Object.setPrototypeOf(L6.prototype, L5.prototype);

// 7. the chain cut with a null prototype at its root.
const g = new L8();
loop("null-root", rabs, g, () => { Object.setPrototypeOf(L0.prototype, null); });
loop("null-root-v", rv, g, () => { (L1.prototype as any).v = "L1.v"; });
Object.setPrototypeOf(L0.prototype, Object.prototype);
delete (L1.prototype as any).v;

// 8. per-evaluation (mixin) layers, as babel composes its parser.
const Mix = (B: any, tag: string) => class extends B { ["t_" + tag]() { return tag; } };
let C: any = class Root { root() { return "root"; } };
const Root0: any = C.prototype;
Root0.k = "Root.k";
for (let i = 0; i < 9; i++) C = Mix(C, "x" + i);
const h1 = new C();
const h2 = new C();
function rk(o: any): any { return o.k; }
// (Only the root is mutated: a mixin layer's own prototype identity is #11769.)
loop("mixin", rk, h1, () => { Root0.k = "Root.k2"; });
loop("mixin-sibling", rk, h2, () => { (h2 as any).k = "own.k"; });
// a second evaluation of the same mixin stack: distinct prototypes
let D: any = class Root2 {};
(D.prototype as any).k = "Root2.k";
for (let i = 0; i < 9; i++) D = Mix(D, "y" + i);
const objs = [new C(), new D(), new C(), new D()];
const ks: string[] = [];
for (let i = 0; i < N; i++) {
  const s = String(rk(objs[i & 3]));
  if (i < 4 || i === N - 1) ks.push(s);
}
out.push("mixin-evals " + ks.join(","));

// 9. one site, many receiver classes at different depths.
class S1 extends L2 {}
class S2 extends L5 {}
class S3 extends L8 {}
const many = [new S1(), new S2(), new S3(), new L1(), new L8(), {}];
const ms: string[] = [];
for (let i = 0; i < N; i++) {
  const s = String(rv(many[i % many.length]));
  if (i < many.length) ms.push(s);
  if (i === N / 2) (L1.prototype as any).v = "L1.late";
}
for (const o of many) ms.push(String(rv(o)));
out.push("many " + ms.join(","));

// 10. a site that met a receiver its entries cannot describe (a Proxy in the
//     chain) still serves class receivers, and still sees a later change.
const odd: any = Object.create(new Proxy({}, {}));
function rq(o: any): any { return o.q; }
const qs: string[] = [];
const recvs = [odd, new L8(), new S2(), new L8()];
for (let i = 0; i < N; i++) {
  const s = String(rq(recvs[i & 3]));
  if (i === N / 2) (L3.prototype as any).q = "L3.q";
  if (i < 4 || i >= N - 4) qs.push(s);
}
out.push("latched-site " + qs.join(","));

for (const line of out) console.log(line);
