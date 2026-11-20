// #8724: constructing ArrayBuffer / SharedArrayBuffer / DataView through a
// CAPTURED value (a local holding the builtin, Reflect.construct, or a
// subclass) must build the object, not throw "Constructor requires 'new'".
// Regression from routing their [[Call]] to the shared construct-only thunk
// without teaching `identify_global_builtin_constructor` about it, so the
// dynamic-`new` path stopped recognizing a captured constructor. Minified
// bundles capture these globals into locals everywhere.

function run(label: string, fn: () => void): void {
  try {
    fn();
    console.log(label, "ok");
  } catch (e: any) {
    console.log(label, "THREW:", e.message);
  }
}

const ab = () => new ArrayBuffer(8);

run("direct DataView", () => { const dv = new DataView(ab()); if (dv.byteLength !== 8) throw new Error("len"); });
run("captured DataView", () => { const D = DataView; const dv = new D(ab()); if (dv.byteLength !== 8) throw new Error("len"); });
run("Reflect.construct DataView", () => { const dv = Reflect.construct(DataView, [ab()]); if ((dv as DataView).byteLength !== 8) throw new Error("len"); });
run("subclass DataView", () => { class MV extends DataView {} const dv = new MV(ab()); if (dv.byteLength !== 8) throw new Error("len"); });
run("captured ArrayBuffer", () => { const AB = ArrayBuffer; const b = new AB(8); if (b.byteLength !== 8) throw new Error("len"); });
run("captured ArrayBuffer resizable", () => { const AB = ArrayBuffer; const b = new AB(8, { maxByteLength: 16 }); if (b.byteLength !== 8) throw new Error("len"); });
run("captured SharedArrayBuffer", () => { const SAB = SharedArrayBuffer; const s = new SAB(8); if (s.byteLength !== 8) throw new Error("len"); });
run("globalThis ArrayBuffer", () => { const b = new (globalThis as any).ArrayBuffer(8); if (b.byteLength !== 8) throw new Error("len"); });

// The captured DataView must be fully functional, not a branded-but-empty stub.
const D2 = DataView;
const dv2 = new D2(ab());
dv2.setInt32(0, 0x41424344);
console.log("readback", dv2.getInt32(0).toString(16));
