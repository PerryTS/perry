// THR6 commit 2: a Number key on a typed array reached through a receiver
// whose static type is not a proven typed array (union / late-init local).
type AnyTA = Int8Array | Uint8Array | Uint8ClampedArray | Int16Array | Uint16Array | Int32Array | Uint32Array | Float32Array | Float64Array | number[];
const KEYS: number[] = [0, 3, 1.5, -0, NaN, 2 ** 32, 4, -1, 4294967295, Infinity, -Infinity, 1e21, 0.5, -0.5, 2.0000000000000004];
function store(o: AnyTA, k: number, v: number) { o[k] = v; }
function storeAny(o: AnyTA, k: number, v: any) { (o as any)[k] = v; }
function view(o: any): string { return Array.from(o as ArrayLike<number>).join(",") + "|" + Object.keys(o).join(";"); }
function keys(make: () => AnyTA, v: number): string {
  const out: string[] = [];
  for (const k of KEYS) { const o = make(); store(o, k, v); out.push(view(o)); }
  return out.join(" ");
}
function late(): string {
  let buf: Float32Array;
  buf = new Float32Array(4);
  const out: string[] = [];
  for (const k of KEYS) buf[k] = 7.25;
  out.push(Array.from(buf).join(","), Object.keys(buf).join(";"));
  return out.join("|");
}
function values(): string {
  const out: string[] = [];
  const vals: any[] = [1.5, -1, 300, "12", "x", true, null, undefined, 2 ** 40, -0, NaN];
  for (const ctor of [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array] as any[]) {
    const o = new ctor(vals.length);
    for (let i = 0; i < vals.length; i++) storeAny(o, i, vals[i]);
    out.push(ctor.name + ":" + Array.from(o).map((x: number) => Object.is(x, -0) ? "-0" : String(x)).join(","));
  }
  return out.join(" ");
}
function coerce(): string {
  const o = new Int16Array(2); let log = "";
  const mk = (n: number) => ({ valueOf() { log += n; return n; } });
  for (const k of [1, 1.5, -0, 5, NaN, -1]) storeAny(o, k, mk(k === 1 ? 9 : 3));
  return log + "|" + Array.from(o).join(",");
}
function strictMode(): string {
  "use strict";
  const o = new Uint8Array(2); const r: string[] = [];
  for (const k of [1.5, 9, -1, NaN, 1]) { try { store(o, k, 4); r.push("ok"); } catch (e) { r.push((e as Error).constructor.name); } }
  return r.join(",") + "|" + Array.from(o).join(",");
}
function bigint(): string {
  const o: any = new BigInt64Array(2); const r: string[] = [];
  for (const k of [0, 1.5, 9]) { try { storeAny(o as any, k, 5); r.push("ok"); } catch (e) { r.push((e as Error).constructor.name); } }
  for (const k of [0, 1.5, 9]) { try { storeAny(o as any, k, 5n as any); r.push("ok"); } catch (e) { r.push((e as Error).constructor.name); } }
  return r.join(",") + "|" + Array.from(o as BigInt64Array).join(",");
}
function detached(): string {
  const ab = new ArrayBuffer(8); const o = new Uint8Array(ab); let log = 0;
  store(o, 0, 1);
  const ab2 = (ab as any).transfer();
  store(o, 0, 2); storeAny(o, 1, { valueOf() { log++; return 3; } });
  const ab3 = new ArrayBuffer(4); const p = new Uint8Array(ab3);
  storeAny(p, 0, { valueOf() { (ab3 as any).transfer(); log += 10; return 5; } });
  return log + "|" + o.length + "|" + p.length + "|" + new Uint8Array(ab2).join(",") + "|" + (o as any)[0] + "|" + (p as any)[0];
}
function protoSetter(): string {
  let hits = 0;
  Object.defineProperty(Object.prototype, "6", { set(_v) { hits++; }, configurable: true });
  const o = new Float64Array(2); store(o, 6, 1); store(o, 1, 2);
  const plain: number[] = [0]; store(plain, 6, 1);
  delete (Object.prototype as any)["6"];
  return hits + "|" + Array.from(o).join(",") + "|" + Object.keys(o).join(";");
}
class Sub extends Float32Array { tag = "s"; }
function subclass(): string { const o = new Sub(3); for (const k of [2, 1.5, 3]) store(o, k, 8); return Array.from(o).join(",") + "|" + Object.keys(o).join(";"); }
function arrays(): string { const out: string[] = []; for (const k of KEYS) { const a: number[] = [0, 0]; store(a, k, 1); out.push(Object.keys(a).join(";") + "/" + a.length); } return out.join(" "); }
console.log("f32", keys(() => new Float32Array(4), 7.5));
console.log("u8c", keys(() => new Uint8ClampedArray(4), 300.5));
console.log("i32", keys(() => new Int32Array(4), -(2 ** 31) - 1));
console.log("late", late());
console.log("values", values());
console.log("coerce", coerce());
console.log("strict", strictMode());
console.log("bigint", bigint());
console.log("detached", detached());
console.log("proto", protoSetter());
console.log("subclass", subclass());
console.log("arrays", arrays());
