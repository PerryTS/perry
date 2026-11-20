// #10515: `b[i] = v` through an untyped receiver stores a byte inline when the
// receiver is an owning Uint8Array. Every other operand must keep the full
// [[Set]] semantics: ToUint8's modulo, NaN/Infinity, non-integer and
// out-of-range keys, value objects with valueOf, clamped arrays, Buffers,
// views sharing an ArrayBuffer.
function put(b: any, i: any, v: any): void { b[i] = v; }
function fill(b: any, cc: any): number { for (let i = 0; i < b.length; i++) b[i] = cc[b[i] & 7]; return b[3]; }

const u = new Uint8Array(8);
const vals: any[] = [5, 255, 256, 257, -1, -256, 1.9, -1.9, 2147483647, 2147483648, -2147483648, 4294967297.5, NaN, Infinity, -Infinity, -0, "12", true, null];
const out: string[] = [];
for (const v of vals) { put(u, 0, v); out.push(String(u[0])); }
console.log("values", out.join(","));

put(u, 1.5, 9); put(u, -0, 7); put(u, "2", 6); put(u, 8, 1); put(u, -1, 1); put(u, 1e10, 1);
console.log("keys", Array.from(u).join(","), JSON.stringify(Object.keys(u)), (u as any)["1.5"], (u as any)[8]);

let calls = 0;
put(u, 3, { valueOf() { calls++; return 300; } });
console.log("valueOf", u[3], calls);

const c = new Uint8ClampedArray(4);
put(c, 0, 300); put(c, 1, -5); put(c, 2, 1.5); put(c, 3, 2.5);
console.log("clamped", Array.from(c).join(","));

const buf = Buffer.alloc(4);
put(buf, 0, 513); put(buf, 1, -2); put(buf, 2, 3.99);
console.log("buffer", Array.from(buf).join(","));

const ab = new ArrayBuffer(8);
const whole = new Uint8Array(ab);
const part = new Uint8Array(ab, 4, 4);
put(whole, 5, 77); put(part, 2, 88);
console.log("views", whole[5], part[1], whole[6], part[2]);

const pools: Uint8Array[] = [];
for (let k = 0; k < 6; k++) { const p = new Uint8Array(64); for (let i = 0; i < 64; i++) p[i] = (i * 5 + k) & 255; pools.push(p); }
const cc = new Uint8Array([11, 22, 33, 44, 55, 66, 77, 88]);
let acc = 0;
for (let r = 0; r < 300; r++) acc = (acc + fill(pools[r % 6], cc)) % 1000003;
console.log("loop", acc, Array.from(pools[2].subarray(0, 8)).join(","));

// Reads through a captured receiver (the polymorphic index read): an in-bounds
// integer answers the byte; everything else keeps [[Get]].
function reader(src: any): (i: any) => any { const r = src; return (i: any) => r[i]; }
const ru = reader(new Uint8Array([10, 20, 30, 40]));
const rc = reader(new Uint8ClampedArray([1, 2]));
const rb = reader(Buffer.from([7, 8, 9]));
const rv = reader(new Uint8Array(new Uint8Array([5, 6, 7, 8]).buffer, 1, 2));
console.log("reads", [0, 3, 4, -1, 1.5, NaN, -0].map((i) => String(ru(i))).join(","),
  rc(1), rc(2), rb(2), rb(3), rv(0), rv(1), rv(2));

// The nanoid shape: the alphabet is a Uint8Array captured by a closure, so its
// reads go through the polymorphic index read once the byte cache admits it.
function mkRefill() {
  const alpha = Uint8Array.from("abcdefgh", (s: string) => s.charCodeAt(0)); let mask = 7;
  return (b: Uint8Array) => { for (let i = 0; i < b.length; i++) b[i] = alpha[b[i] & mask]; return b[3]; };
}
const refill = mkRefill();
const pool = new Uint8Array(16); for (let i = 0; i < 16; i++) pool[i] = i * 5;
let total = 0; for (let k = 0; k < 20; k++) total += refill(pool);
console.log("nanoid", total, Array.from(pool).join(","));
