// `o.f++` on any receiver: one [[Get]], ToNumeric, one [[Set]], the base
// evaluated once.
const log: string[] = [];
class Tex {
  version = 0;
  name = "";
}
const texs = [new Tex(), new Tex()];
let k = 0;
const get = () => (log.push("base"), texs[k++ & 1]);
const r1 = get().version++;
const r2 = ++get().version;
console.log("base", log.join(","), k, r1, r2, texs.map((t) => t.version).join(","));

class Acc {
  _v = 1;
  get v() { log.push("get"); return this._v; }
  set v(x) { log.push("set:" + x); this._v = x; }
}
log.length = 0;
const a = new Acc();
const r3 = a.v++;
const r4 = --a.v;
console.log("accessor", log.join(","), r3, r4, a._v);

const o: any = { s: "5", b: 10n, obj: { valueOf() { log.push("valueOf"); return 2; } }, u: undefined };
log.length = 0;
const r5 = o.s++;
const r6 = o.b++;
const r7 = o.obj++;
const r8 = o.u++;
console.log("coerce", r5, typeof r5, o.s, r6, o.b, r7, o.obj, r8, o.u, log.join(","));

function inc(this: any) { return this.version++; }
const t = new Tex();
for (let i = 0; i < 3; i++) inc.call(t);
console.log("this", t.version);
const frozen = Object.freeze({ n: 1 });
try {
  (frozen as any).n++;
} catch (e) {
  console.log("frozen", (e as Error).constructor.name, frozen.n);
}
