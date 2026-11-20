// A typed-array element store converts its value first: ToNumber for the
// Number kinds (an object's valueOf / toString / Symbol.toPrimitive runs, a
// BigInt or a Symbol throws a TypeError and nothing is stored), ToBigInt for
// the BigInt kinds; then the modular (or clamping) narrowing; then the store,
// if the index is still valid after the conversion (it can detach or shrink
// the buffer). Every kind, `u[i] = v` in every loop shape, `set`, `fill`,
// `from`, `of`, the constructor, `with`, an out-of-range and an invalid index,
// DataView setters, Atomics.store, Buffer. Negative controls: Number values.
const log: string[] = [];
function obj(tag: string, v: any): any { return { valueOf() { log.push(tag + ".valueOf"); return v; } }; }
function strObj(tag: string, v: any): any { return { valueOf: undefined, toString() { log.push(tag + ".toString"); return v; } }; }
function primObj(tag: string, v: any): any { return { [Symbol.toPrimitive](h: string) { log.push(tag + ".toPrim:" + h); return v; } }; }
function thrower(tag: string): any { return { valueOf() { log.push(tag + ".throw"); throw new RangeError("boom-" + tag); } }; }
function show(label: string, f: () => any) {
  log.length = 0;
  let r: any;
  try { r = f(); } catch (e: any) { r = "THROW " + (e && e.constructor && e.constructor.name); }
  let s: string;
  if (r && typeof r === "object" && "length" in r && typeof r.length === "number") { s = "[" + Array.prototype.join.call(r, ",") + "]"; }
  else s = typeof r === "bigint" ? r + "n" : String(r);
  console.log(label + " -> " + s + " log=" + log.join("|"));
}
const kinds: any[] = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array];
const vals: [string, () => any][] = [
  ["obj7", () => obj("o", 7)], ["obj300", () => obj("o", 300.7)], ["objStr", () => obj("o", "12")],
  ["toStr", () => strObj("s", "5")], ["toPrim", () => primObj("p", 9)], ["null", () => null], ["undef", () => undefined],
  ["str", () => "42"], ["strx", () => "x"], ["true", () => true], ["false", () => false], ["neg", () => -1.5], ["nan", () => NaN],
  ["arr", () => [3]], ["date0", () => new Date(5)],
];
for (const K of kinds) {
  for (const [vn, mk] of vals) {
    show(K.name + " idx " + vn, () => { const u = new K(2); const v = mk(); u[0] = v; return u; });
  }
  show(K.name + " idx bigint", () => { const u = new K(2); (u as any)[0] = 1n; return u; });
  show(K.name + " throw", () => { const u = new K(2); u[0] = 3; u[0] = thrower("t"); return u; });
  show(K.name + " set", () => { const u = new K(3); u.set([obj("a", 1), obj("b", 2)], 1); return u; });
  show(K.name + " fill", () => { const u = new K(3); u.fill(obj("f", 4)); return u; });
  show(K.name + " from", () => K.from([obj("a", 1), "2", null]));
  show(K.name + " of", () => K.of(obj("a", 1), true, undefined));
  show(K.name + " ctor", () => new K([obj("a", 1), primObj("p", 2)]));
  show(K.name + " oob", () => { const u = new K(2); u[5] = obj("x", 1); return u; });
  show(K.name + " neg", () => { const u = new K(2); (u as any)[-1] = obj("x", 1); return u; });
  show(K.name + " frac", () => { const u = new K(2); (u as any)[1.5] = obj("x", 1); return u; });
  show(K.name + " with", () => { const u = new K(2); return (u as any).with(0, obj("w", 6)); });
}
for (const K of [BigInt64Array, BigUint64Array] as any[]) {
  show(K.name + " big", () => { const u = new K(2); u[0] = 5n; return u; });
  show(K.name + " num", () => { const u = new K(2); u[0] = 5; return u; });
  show(K.name + " obj", () => { const u = new K(2); u[0] = obj("o", 6n); return u; });
  show(K.name + " objnum", () => { const u = new K(2); u[0] = obj("o", 6); return u; });
  show(K.name + " str", () => { const u = new K(2); u[0] = "7"; return u; });
  show(K.name + " true", () => { const u = new K(2); u[0] = true; return u; });
  show(K.name + " null", () => { const u = new K(2); u[0] = null; return u; });
  show(K.name + " undef", () => { const u = new K(2); u[0] = undefined; return u; });
  show(K.name + " fill", () => { const u = new K(2); u.fill(obj("f", 3n)); return u; });
  show(K.name + " fillnum", () => { const u = new K(2); u.fill(3); return u; });
  show(K.name + " set", () => { const u = new K(2); u.set([obj("a", 1n), 2n]); return u; });
  show(K.name + " setnum", () => { const u = new K(2); u.set([1]); return u; });
  show(K.name + " from", () => K.from([obj("a", 1n)]));
  show(K.name + " ctor", () => new K([obj("a", 1n), 2n]));
  show(K.name + " ctornum", () => new K([1]));
  show(K.name + " oobnum", () => { const u = new K(2); u[5] = 1; return u; });
}
// DataView
for (const m of ["setInt8", "setUint8", "setInt16", "setUint16", "setInt32", "setUint32", "setFloat32", "setFloat64"]) {
  show("dv " + m + " obj", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv[m](0, obj("d", 7)); return new Uint8Array(dv.buffer); });
  show("dv " + m + " offobj", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv[m](obj("off", 1), 2); return new Uint8Array(dv.buffer); });
  show("dv " + m + " throw", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv[m](0, thrower("t")); return new Uint8Array(dv.buffer); });
  show("dv " + m + " str", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv[m](0, "3"); return new Uint8Array(dv.buffer); });
  show("dv " + m + " bigint", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv[m](0, 1n); return new Uint8Array(dv.buffer); });
}
show("dv setBigInt64 obj", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv.setBigInt64(0, obj("d", 7n)); return new Uint8Array(dv.buffer); });
show("dv setBigInt64 num", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv.setBigInt64(0, 7); return new Uint8Array(dv.buffer); });
show("dv setBigUint64 str", () => { const dv: any = new DataView(new ArrayBuffer(8)); dv.setBigUint64(0, "7"); return new Uint8Array(dv.buffer); });
// Atomics
show("atomics store obj", () => { const u = new Int32Array(2); const r = Atomics.store(u, 0, obj("a", 5)); return [r, u[0]]; });
show("atomics store str", () => { const u = new Int32Array(2); const r = Atomics.store(u, 0, "6" as any); return [r, u[0]]; });
show("atomics add obj", () => { const u = new Int32Array(2); Atomics.add(u, 0, obj("a", 5)); return u; });
show("atomics store big", () => { const u = new BigInt64Array(2); Atomics.store(u, 0, obj("a", 5n)); return u; });
show("atomics store bignum", () => { const u = new BigInt64Array(2); Atomics.store(u, 0, 5 as any); return u; });
// detach / resize during conversion
show("detach idx", () => { const b = new ArrayBuffer(8); const u = new Uint8Array(b); u[0] = { valueOf() { log.push("det"); (b as any).transfer(); return 9; } } as any; return [u.length, b.byteLength]; });
show("resize idx", () => { const b = new (ArrayBuffer as any)(8, { maxByteLength: 16 }); const u = new Uint8Array(b); u[6] = { valueOf() { log.push("rs"); b.resize(4); return 9; } } as any; return [u.length, Array.from(u).join("")]; });
show("resize grow idx", () => { const b = new (ArrayBuffer as any)(4, { maxByteLength: 16 }); const u = new Uint8Array(b); const w = new Uint8Array(b, 0, 4); w[3] = { valueOf() { b.resize(2); return 9; } } as any; return [u.length, w.length]; });
show("detach fill", () => { const b = new ArrayBuffer(8); const u = new Uint8Array(b); u.fill({ valueOf() { (b as any).transfer(); return 9; } } as any); return [u.length]; });
show("resize fill", () => { const b = new (ArrayBuffer as any)(8, { maxByteLength: 16 }); const u = new Uint8Array(b); u.fill({ valueOf() { b.resize(3); return 9; } } as any); return [u.length, Array.from(u).join("")]; });
show("detach dv", () => { const b = new ArrayBuffer(8); const dv = new DataView(b); dv.setUint8(0, { valueOf() { (b as any).transfer(); return 9; } } as any); return [b.byteLength]; });
// loop shapes
{ log.length = 0; const u = new Uint8Array(4); let k = 0; const q: any = { valueOf() { k++; return k; } };
  for (let i = 0; i < 4; i++) u[i] = q; console.log("loop for", Array.from(u).join(","), k);
  for (const _ of [1, 2]) u[0] = q; console.log("loop forof", u[0], k);
  let j = 0; while (j < 2) { u[1] = q; j++; } console.log("loop while", u[1], k);
  const f = new Float64Array(2); f[0] = q; f[1] = q as any + 0; console.log("f64", f[0], f[1], k);
  const n = new Uint8Array(2); n[0] = 257; n[1] = 3.9; console.log("num", n[0], n[1]);
  const cl = new Uint8ClampedArray(3); cl[0] = 300; cl[1] = 2.5; cl[2] = -3; console.log("clamp", cl.join(","));
}
// Values Rust's float parser and a saturating cast got wrong, and the
// assignment's own result (the value, not the stored byte).
{
  const u = new Uint8Array(8);
  const vals: any[] = ["0x10", " 12 ", "inf", "1e3", "", 2 ** 31 + 5, 1e10, -1, 4294967295, -0.5];
  vals.forEach((v, i) => { if (i < 8) u[i] = v; });
  console.log("u8 strings", u.join(","));
  const b = Buffer.alloc(4);
  const q: any = { valueOf() { return 300; } };
  b[0] = q; b[1] = "7" as any; b[2] = 1e10; b[3] = true as any;
  console.log("buffer", b.join(","));
  let r: any = (u[0] = 1.5); console.log("result", r, u[0]);
  r = (u[1] = q); console.log("result obj", typeof r, u[1]);
  try { u[2] = Symbol("s") as any; } catch (e: any) { console.log("symbol", e.constructor.name, u[2]); }
  try { u[3] = 5n as any; } catch (e: any) { console.log("bigint", e.constructor.name, u[3]); }
}
