// #10515 / #10694: Uint8Array and Buffer element reads and writes may be
// answered from the byte-view admission cache (inline, and at the top of the
// runtime accessors). Every shape the cache must NOT answer, or must answer
// exactly like the full path, is exercised here through typed parameters,
// untyped (`any`) parameters and closure captures.

function getT(b: Uint8Array, i: number): number { return b[i]; }
function setT(b: Uint8Array, i: number, v: number): void { b[i] = v; }
function getB(b: Buffer, i: number): any { return b[i]; }
function setB(b: Buffer, i: number, v: number): void { b[i] = v; }
function getA(b: any, i: any): any { return b[i]; }
function setA(b: any, i: any, v: any): void { b[i] = v; }
function sumT(b: Uint8Array): number { let s = 0; for (let i = 0; i < b.length; i++) s += b[i]; return s; }
function sumA(b: any): number { let s = 0; for (let i = 0; i < b.length; i++) s += b[i]; return s; }
function fillT(b: Uint8Array, k: number): void { for (let i = 0; i < b.length; i++) b[i] = (i * k) & 0xff; }
function hex(b: any): string { return Array.from(b as Uint8Array, (x: number) => x.toString(16).padStart(2, "0")).join(""); }

// 1. owning Uint8Array / Buffer, typed and untyped, repeated so the cache primes
{
  const u = new Uint8Array(8); const b = Buffer.alloc(8);
  for (let r = 0; r < 3; r++) {
    setT(u, r, 250 + r); setB(b, r, 300 + r); setA(u, r + 3, -1 - r); setA(b, r + 3, r + 0.75);
  }
  console.log("owning", hex(u), hex(b), getT(u, 1), getB(b, 1), getA(u, 4), getA(b, 4));
  console.log("sums", sumT(u), sumA(b), sumT(b), sumA(u));
}

// 2. value conversion on the store: wrapping, fractions, NaN, and non-Numbers
{
  const u = new Uint8Array(10);
  const vals: any[] = [256, -1, 1.5, -1.5, NaN, Infinity, 4294967295, "7", true, null];
  for (let i = 0; i < vals.length; i++) setA(u, i, vals[i]);
  console.log("convert-any", Array.from(u).join(","));
  const w = new Uint8Array(4);
  setT(w, 0, 511); setT(w, 1, -129); setT(w, 2, 2.9); setT(w, 3, 1e10);
  console.log("convert-typed", Array.from(w).join(","));
  let calls = 0;
  const obj = { valueOf() { calls++; return 66; } };
  setA(w, 0, obj); setA(w, 9, obj);
  console.log("valueOf", w[0], calls);
}

// 3. out-of-bounds and non-canonical keys
{
  const u = new Uint8Array([1, 2, 3]); const b = Buffer.from([4, 5, 6]);
  console.log("oob-read", getT(u, 3), getA(u, 3), getA(u, -1), getA(u, 1.5), getB(b, 7), getA(b, "01"));
  setT(u, 3, 9); setA(u, 5, 9); setA(u, -1, 9); setA(u, 1.5, 9); setA(b, 3, 9);
  console.log("oob-write", Array.from(u).join(","), u.length, Object.keys(u).join("|"), hex(b), b.length);
}

// 4. views: subarray and Uint8Array-over-ArrayBuffer alias their backing
{
  const base = new Uint8Array(16); fillT(base, 3);
  const sub = base.subarray(4, 8);
  setT(sub, 0, 200); setA(sub, 1, 201); setT(base, 6, 202); setA(base, 7, 203);
  console.log("subarray", getT(sub, 0), getA(sub, 1), getT(sub, 2), getA(sub, 3), getT(base, 4), getA(base, 5), sumT(sub));
  const ab = new ArrayBuffer(12);
  const v1 = new Uint8Array(ab); const v2 = new Uint8Array(ab, 3, 5);
  for (let r = 0; r < 3; r++) { setT(v1, 3 + r, 10 + r); setA(v2, 3, 90 + r); }
  console.log("ab-views", hex(v1), hex(v2), getT(v2, 0), getA(v1, 6));
  const bsl = Buffer.alloc(10); const bview = bsl.subarray(2, 6);
  setB(bview, 1, 77); setA(bsl, 4, 88);
  console.log("buffer-subarray", hex(bsl), hex(bview));
  const slice = base.slice(0, 4); setT(slice, 0, 1);
  console.log("slice-copy", getT(slice, 0), getT(base, 0));
}

// 5. ArrayBuffer, SharedArrayBuffer and DataView are not integer-indexed
{
  const ab: any = new ArrayBuffer(4); const dv: any = new DataView(ab);
  setA(ab, 0, 5); setA(dv, 1, 6);
  console.log("non-indexed", getA(ab, 0), getA(dv, 1), new Uint8Array(ab).join(","), Object.keys(ab).join("|"), Object.keys(dv).join("|"));
  const sab = new SharedArrayBuffer(4); const s1 = new Uint8Array(sab); const s2: any = new Uint8Array(sab);
  setT(s1, 2, 33); setA(s2, 3, 44);
  console.log("shared", getA(s2, 2), getT(s1, 3));
}

// 6. detach / transfer and resizable backings
{
  const ab = new ArrayBuffer(8); const u = new Uint8Array(ab);
  setT(u, 0, 1); getT(u, 0); getT(u, 0);
  const moved = ab.transfer();
  setT(u, 0, 9); setA(u, 1, 9);
  console.log("detached", u.length, getT(u, 0), getA(u, 1), new Uint8Array(moved)[0]);
  const rab = new ArrayBuffer(4, { maxByteLength: 16 }); const ru = new Uint8Array(rab);
  setT(ru, 3, 7); rab.resize(8); setT(ru, 6, 8); setA(ru, 7, 9);
  console.log("resizable", ru.length, Array.from(ru).join(","));
  rab.resize(2);
  console.log("shrunk", ru.length, getT(ru, 3), getA(ru, 1));
}

// 7. many live buffers hitting the same cache sets, alternating access
{
  const pool: Uint8Array[] = [];
  for (let k = 0; k < 40; k++) { const p = k % 2 ? Buffer.alloc(32 + k) : new Uint8Array(24 + k); pool.push(p); }
  let acc = 0;
  for (let r = 0; r < 20; r++) for (let k = 0; k < pool.length; k++) { const p = pool[k]; setT(p, r % p.length, r + k); acc = (acc + getT(p, (r * 7) % p.length) + getA(pool[(k * 13) % pool.length], r)) % 1000003; }
  console.log("pool", acc, pool.map((p) => sumT(p)).join(","));
}

// 8. buffers created and dropped in a loop (address reuse after collection)
{
  let acc = 0;
  for (let r = 0; r < 300; r++) {
    const t = r % 3 === 0 ? Buffer.alloc(64) : new Uint8Array(48);
    for (let i = 0; i < t.length; i++) setT(t, i, i + r);
    acc = (acc + sumT(t) + getA(t, 5)) % 1000003;
    if (r % 100 === 50 && typeof (globalThis as any).gc === "function") (globalThis as any).gc();
  }
  console.log("churn", acc);
}

// 9. closure captures (the nanoid pool shape) and named expandos
{
  const ALPHA = "useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";
  const mk = () => { const cc = Uint8Array.from(ALPHA, (s: string) => s.charCodeAt(0)); let mask = 63; return (b: Uint8Array) => { for (let i = 0; i < b.length; i++) b[i] = cc[b[i] & mask]; return b; }; };
  const refill = mk(); const pool = Buffer.alloc(32); for (let i = 0; i < 32; i++) pool[i] = i * 11;
  console.log("nanoid", refill(pool).toString("latin1"), refill(pool).toString("latin1"));
  const e: any = new Uint8Array(3); e.tag = "x"; setA(e, "name", "n"); setA(e, 1, 5);
  console.log("expando", e.tag, e.name, e[1], Object.keys(e).join("|"));
}
