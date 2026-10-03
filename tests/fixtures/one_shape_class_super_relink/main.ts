// `super.name` is a property lookup on the home object's CURRENT
// [[Prototype]] (`C.prototype`'s for an instance method, `C`'s for a static
// one) with `this` as the receiver. A patched, deleted or accessor parent
// member, a relinked `C.prototype` and a relinked `C` all apply, for calls,
// `typeof super.m`, `super.m` as a value and `super.x` reads.
// `inst instanceof K` is OrdinaryHasInstance (or K[Symbol.hasInstance]): it
// walks inst's LIVE chain for K.prototype. And `C.prototype.__proto__ = X`
// is the Object.prototype accessor's setter: a relink exactly like
// `Object.setPrototypeOf(C.prototype, X)`.
//
// Class names are unique per block. Each line is `name value`, compared with
// node 26.5.1. "Primed" cases change the chain mid-loop at a parameter
// receiver with an argv trip count, so the site ran before the change.
import { EventEmitter } from "events";
const N = Number(process.argv[2] ?? "40");
const H = N >> 1;
function show(name: string, v: any): void {
  console.log(name + " " + String(v));
}
function tryIt(f: () => any): string {
  try {
    return "ok:" + String(f());
  } catch (e) {
    return (e instanceof TypeError) ? "TypeError" : "other:" + String(e);
  }
}
// Runs `f(o)` n times, applying `change` before iteration `at`; returns the
// first and last results.
function primed(o: any, n: number, at: number, change: () => void, f: (o: any) => any): string {
  let before = "";
  let after = "";
  for (let i = 0; i < n; i++) {
    if (i === at) change();
    const v = tryIt(() => f(o));
    if (i < at) before = v; else after = v;
  }
  return before + " / " + after;
}

// ---- super.m() calls -------------------------------------------------------
{
  class PB { m() { return "PB.m"; } }
  class PC extends PB { m() { return "PC.m"; } callM() { return super.m(); } }
  const c = new PC();
  show("call-base", tryIt(() => c.callM()));
  (PB.prototype as any).m = function (this: any) { return "patched:" + (this === c); };
  show("call-after-patch", tryIt(() => c.callM()));
  show("own-m-unchanged", c.m());
}
{
  class QB { m() { return "QB.m"; } }
  class QC extends QB { callM() { return super.m(); } }
  show("call-primed-patch", primed(new QC(), N, H,
    () => { (QB.prototype as any).m = () => "QB.patched"; }, (o) => o.callM()));
}
{
  class GB { m() { return "GB.m"; } }
  class GC extends GB { callM() { return super.m(); } typeM() { return typeof super.m; } }
  const g = new GC();
  Object.defineProperty(GB.prototype, "m", {
    get(this: any) { const self = this; return () => "getter:" + (self === g); },
    configurable: true,
  });
  show("call-via-getter", tryIt(() => g.callM()));
  show("typeof-via-getter", g.typeM());
}
{
  class DB { m() { return "DB.m"; } }
  class DC extends DB { callM() { return super.m(); } typeM() { return typeof super.m; } }
  const d = new DC();
  show("call-before-delete", tryIt(() => d.callM()));
  delete (DB.prototype as any).m;
  show("call-after-delete", tryIt(() => d.callM()));
  show("typeof-after-delete", d.typeM());
}
{
  class RB { m() { return "RB.m"; } }
  class RX { m() { return "RX.m"; } }
  class RC extends RB { callM() { return super.m(); } }
  show("call-primed-relink", primed(new RC(), N, H,
    () => { Object.setPrototypeOf(RC.prototype, RX.prototype); }, (o) => o.callM()));
}
{
  class MB { m() { return "MB.m"; } }
  class MC extends MB { nope() { return super.zz(); } }
  show("call-missing", tryIt(() => new MC().nope()));
}
{
  // A patch of the same name on an unrelated class trips the per-name guard;
  // the parent's private state must still work through the runtime path.
  class VB { #v = 7; m() { return "VB.m" + this.#v; } }
  class VC extends VB { m() { return "VC"; } callM() { return super.m(); } }
  class Unrelated { m() { return 0; } }
  (Unrelated.prototype as any).m = () => 1;
  show("call-private-tripped", tryIt(() => new VC().callM()));
}

// ---- super property gets ---------------------------------------------------
{
  class TB { m() { return 1; } }
  class TC extends TB { typeM() { return typeof super.m; } valueM() { return super.m; } }
  const t = new TC();
  show("typeof-base", t.typeM());
  show("value-is-proto-m", t.valueM() === TB.prototype.m);
  show("typeof-primed-patch", primed(t, N, H,
    () => { (TB.prototype as any).m = 5; }, (o) => o.typeM()));
  show("value-after-patch", String(t.valueM()));
}
{
  class XB { get g() { return "XB.g:" + (this as any).tag; } }
  class XC extends XB { tag = "c"; readX() { return super.x; } readG() { return super.g; } }
  const x = new XC();
  show("data-primed", primed(x, N, H, () => { (XB.prototype as any).x = 42; }, (o) => o.readX()));
  show("getter-receiver", x.readG());
  Object.defineProperty(XB.prototype, "g", { get(this: any) { return "XB.g2:" + this.tag; }, configurable: true });
  show("getter-redefined", x.readG());
  class XY { get g() { return "XY.g:" + (this as any).tag; } }
  Object.setPrototypeOf(XC.prototype, XY.prototype);
  show("getter-after-relink", x.readG());
  show("data-after-relink", String(x.readX()));
  Object.setPrototypeOf(XC.prototype, null);
  show("get-on-null-home", tryIt(() => x.readG()));
}

// ---- static super ----------------------------------------------------------
{
  class SB { static s() { return "SB.s:" + (this as any).name; } s() { return "SB.inst-s"; } }
  class SC extends SB {
    static callS() { return super.s(); }
    static typeS() { return typeof super.s; }
    static readK() { return super.k; }
  }
  show("static-base", tryIt(() => SC.callS()));
  (SB as any).s = function (this: any) { return "SB.s-patched:" + this.name; };
  show("static-after-patch", tryIt(() => SC.callS()));
  (SB as any).k = "SB.k";
  show("static-data", SC.readK());
  class SD { static s() { return "SD.s"; } static k = "SD.k"; }
  Object.setPrototypeOf(SC, SD);
  show("static-after-relink", tryIt(() => SC.callS()));
  show("static-typeof-after-relink", SC.typeS());
  show("static-data-after-relink", SC.readK());
}

{
  // The parent has an INSTANCE method of the name and no static one: in a
  // static member, super.m reads the parent constructor, so it is absent.
  class StB { onlyInst() { return "StB.inst-m"; } static both() { return "StB.static-s"; } both() { return "StB.inst-s"; } }
  class StC extends StB {
    static callOnly() { return super.onlyInst(); }
    static callBoth() { return super.both(); }
    static typeOnly() { return typeof super.onlyInst; }
    static valueOnly() { return super.onlyInst; }
  }
  show("static-inst-only-call", tryIt(() => StC.callOnly()));
  show("static-inst-only-typeof", StC.typeOnly());
  show("static-inst-only-value", StC.valueOnly());
  show("static-prefers-static", tryIt(() => StC.callBoth()));
}

// ---- instanceof ------------------------------------------------------------
{
  class IB { m() { return "IB"; } }
  class ID { m() { return "ID"; } }
  class IE extends ID {}
  class IC extends IB {}
  class ISub extends IC {}
  const c = new IC();
  const s = new ISub();
  show("inst-before", [c instanceof IB, c instanceof ID].join(","));
  Object.setPrototypeOf(IC.prototype, IE.prototype);
  show("inst-old-parent", c instanceof IB);
  show("inst-new-chain", [c instanceof IE, c instanceof ID, c instanceof IC, c instanceof Object].join(","));
  show("inst-subclass", [s instanceof ISub, s instanceof IC, s instanceof IB, s instanceof ID].join(","));
  const K: any = Math.random() < 2 ? IB : ID;
  show("inst-dynamic-rhs", c instanceof K);
  show("inst-fresh-instance", [new IC() instanceof IB, new IC() instanceof IE].join(","));
}
{
  class HB {}
  class HC extends HB {}
  const h = new HC();
  class HK { static [Symbol.hasInstance](v: any) { return v === h; } }
  Object.setPrototypeOf(HC.prototype, HK.prototype);
  show("inst-hasInstance", [h instanceof HK, new HC() instanceof HK].join(","));
}
{
  class NB {}
  class NC extends NB {}
  const n = new NC();
  Object.setPrototypeOf(NC.prototype, null);
  show("inst-null", [n instanceof NB, n instanceof NC, n instanceof Object].join(","));
  const plain = { tag: 1 };
  class OC extends NB {}
  const o = new OC();
  Object.setPrototypeOf(OC.prototype, plain);
  show("inst-plain", [o instanceof NB, o instanceof Object].join(","));
  Object.setPrototypeOf(OC.prototype, NB.prototype);
  show("inst-relinked-back", o instanceof NB);
}
{
  class LB {}
  class LD {}
  class LC extends LB {}
  const l = new LC();
  show("inst-primed", primed(l, N, H, () => { Object.setPrototypeOf(LC.prototype, LD.prototype); },
    (o) => [o instanceof LB, o instanceof LD].join(",")));
}
{
  class WB {}
  class WD {}
  class WC extends WB {}
  const w = new WC();
  Object.setPrototypeOf(w, WD.prototype);
  show("inst-own-proto", [w instanceof WC, w instanceof WB, w instanceof WD].join(","));
}

{
  class MA {}
  class MB extends MA {}
  class MD {}
  const mid = Object.create(MD.prototype);
  const m = new MB();
  Object.setPrototypeOf(MB.prototype, mid);
  show("inst-via-plain-mid", [m instanceof MA, m instanceof MD, m instanceof MB, m instanceof Object].join(","));
}
{
  class QA {}
  class QB extends QA {}
  const q = new QB();
  Object.setPrototypeOf(QB.prototype, Object.create(null));
  show("inst-to-null-proto-object", [q instanceof QA, q instanceof QB, q instanceof Object].join(","));
}
{
  class RA {}
  class RB extends RA {}
  class RC extends RB {}
  class RX {}
  const r = new RC();
  Object.setPrototypeOf(RB.prototype, RX.prototype);
  show("inst-mid-relink", [r instanceof RA, r instanceof RB, r instanceof RC, r instanceof RX].join(","));
  const dyn: any = Math.random() < 2 ? RA : RX;
  const dyn2: any = Math.random() < 2 ? RX : RA;
  show("inst-mid-relink-dynamic", [r instanceof dyn, r instanceof dyn2].join(","));
}
{
  class YA {}
  class YB extends YA {}
  const y = new YB();
  Object.setPrototypeOf(y, Object.create(YA.prototype));
  show("inst-own-proto-plain", [y instanceof YB, y instanceof YA, y instanceof Object].join(","));
  const y2 = new YB();
  Object.setPrototypeOf(y2, null);
  show("inst-own-proto-null", [y2 instanceof YB, y2 instanceof YA, y2 instanceof Object].join(","));
}
{
  class ZA {}
  class ZB extends ZA {}
  class ZX {}
  const z = new ZB();
  show("inst-primed-object", primed(z, N, H, () => { Object.setPrototypeOf(ZB.prototype, ZX.prototype); },
    (o) => [o instanceof ZA, o instanceof ZX, o instanceof Object].join(",")));
}

// ---- the __proto__ setter on a class prototype ------------------------------
{
  class UB { m() { return "UB"; } }
  class UE { m() { return "UE"; } }
  class U1 extends UB { callM() { return super.m(); } }
  class U2 extends UB {}
  class U3 extends UB {}
  class U4 extends UB {}
  class U5 extends UB {}
  const u1 = new U1();
  (U1.prototype as any).__proto__ = UE.prototype;
  show("dunder-direct", [Object.getPrototypeOf(U1.prototype) === UE.prototype,
    Object.prototype.hasOwnProperty.call(U1.prototype, "__proto__"), u1.m(), u1.callM(),
    u1 instanceof UB, u1 instanceof UE].join(","));
  const p2: any = U2.prototype;
  p2.__proto__ = UE.prototype;
  show("dunder-alias", [Object.getPrototypeOf(U2.prototype) === UE.prototype, (new U2() as any).m()].join(","));
  (U3.prototype as any)["__proto__"] = UE.prototype;
  show("dunder-computed", Object.getPrototypeOf(U3.prototype) === UE.prototype);
  const u4 = new U4();
  (U4.prototype as any).__proto__ = null;
  show("dunder-null", [Object.getPrototypeOf(U4.prototype), typeof (u4 as any).m, u4 instanceof UB].join(","));
  (U5.prototype as any).__proto__ = 5;
  show("dunder-ignored", [Object.getPrototypeOf(U5.prototype) === UB.prototype, (new U5() as any).m()].join(","));
  show("dunder-primed", primed(new U2(), N, H, () => { (U2.prototype as any).__proto__ = UB.prototype; },
    (o) => o.m() + ":" + (o instanceof UB)));
}

// ---- untouched and native bases still resolve -------------------------------
{
  class AB { m(x: number) { return x + 1; } }
  class AC extends AB { m(x: number) { return super.m(x) * 2; } }
  show("plain-super", new AC().m(3));
  class Bus extends EventEmitter { emit(ev: string, ...a: any[]) { return super.emit(ev, ...a); } }
  class Logged extends Bus { emit(ev: string, x: any) { return super.emit(ev, x); } }
  const l = new Logged();
  let got = "";
  l.on("x", (v: any) => { got = "got:" + v; });
  l.emit("x", 5);
  show("native-base", got);
  class MM extends Map<string, number> { get(k: string) { return (super.get(k) ?? 0) + 100; } }
  const mm = new MM();
  mm.set("a", 1);
  show("map-base", mm.get("a"));
}
