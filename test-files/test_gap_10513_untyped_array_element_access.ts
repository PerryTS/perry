// #10513 / #10514: untyped (`any`) element stores and reads on ordinary
// Arrays take an inline guarded tier; everything the guard cannot prove must
// keep the full [[Set]]/[[Get]] semantics. Each block prints what it observed.

// Module code is strict: a rejected store throws, so report it rather than stop.
function put(d: any, i: any, v: any): any {
  try { return (d[i] = v); } catch (e: any) { console.log("  throw", String(i), e.constructor.name); return undefined; }
}
function get(d: any, i: any): any { return d[i]; }
function fill(d: any, n: number, k: number): void { for (let j = 0; j < n; j++) d[j] = j * k; }
function sum(d: any, n: number): number { let s = 0; for (let j = 0; j < n; j++) s += d[j]; return s; }
function sumTyped(d: number[], n: number): number { let s = 0; for (let j = 0; j < n; j++) s += d[j]; return s; }
function show(label: string, v: any): void { console.log(label, JSON.stringify(v)); }

// 1. plain dense store/read, numbers and non-numbers
{
  const a: any = [1, 2, 3, 4];
  put(a, 0, 10); put(a, 1, "s"); put(a, 2, null); put(a, 3, { x: 1 });
  show("dense", a);
  put(a, 1, 2.5); put(a, 3, -0);
  console.log("dense2", a[1], Object.is(get(a, 3), -0));
}

// 2. holes: an in-range hole store, and reads of holes
{
  const a: any = [1, , 3];
  console.log("hole-read", get(a, 1), 1 in a);
  put(a, 1, 7);
  console.log("hole-store", get(a, 1), 1 in a, a.length);
  const b: any = new Array(5);
  put(b, 3, "x");
  console.log("presized-holes", JSON.stringify(b), 0 in b, 3 in b);
}

// 3. length growth, append, sparse store past the end
{
  const a: any = [];
  for (let i = 0; i < 40; i++) put(a, i, i * 2);
  console.log("grown", a.length, sum(a, 40), sumTyped(a, 40));
  put(a, 45, 1);
  console.log("sparse", a.length, get(a, 44), 44 in a, get(a, 45));
  const holder: any = { data: [] };
  for (let i = 0; i < 100; i++) holder.data[i] = i; // grows through a field
  console.log("grown-field", holder.data.length, sum(holder.data, 100), get(holder.data, 99));
  const pushed: any = [];
  for (let i = 0; i < 64; i++) pushed.push(i + 0.5);
  console.log("pushed", sum(pushed, 64), sumTyped(pushed, 64));
  fill(pushed, 64, 3);
  console.log("refill", sum(pushed, 64), get(pushed, 63));
}

// 4. out-of-bounds and non-canonical keys
{
  const a: any = [1, 2, 3];
  console.log("oob-read", get(a, 3), get(a, -1), get(a, 1.5), get(a, NaN), get(a, 4294967295));
  put(a, -1, "neg"); put(a, 1.5, "frac"); put(a, "2", "str2"); put(a, "k", "named");
  console.log("keys", a.length, JSON.stringify(Object.keys(a)), a[-1], a[1.5], a[2], a.k);
  const sym = Symbol("s");
  put(a, sym, "symval");
  console.log("symbol", a[sym], a.length);
  put(a, 4294967294, "max");
  console.log("maxidx", a.length);
}

// 5. frozen / sealed / non-extensible arrays (strict: rejected stores throw)
{
  const f: any = Object.freeze([1, 2, 3]);
  put(f, 0, 99); put(f, 3, 99);
  console.log("frozen", JSON.stringify(f));
  const s: any = Object.seal([1, 2, 3]);
  put(s, 0, 99); put(s, 3, 99);
  console.log("sealed", JSON.stringify(s));
  const n: any = Object.preventExtensions([1, 2, 3]);
  put(n, 1, 42); put(n, 5, 42);
  console.log("noext", JSON.stringify(n), n.length);
}

// 6. arrays carrying named props, accessors and descriptors
{
  const a: any = [1, 2, 3];
  a.tag = "t";
  put(a, 1, 20);
  console.log("named", JSON.stringify(a), a.tag);
  const log: string[] = [];
  Object.defineProperty(a, 0, { get() { log.push("get0"); return 7; }, set(v) { log.push("set0=" + v); }, configurable: true });
  put(a, 0, 11); const r = get(a, 0);
  console.log("accessor", r, log.join(","));
  Object.defineProperty(a, 2, { value: 3, writable: false });
  put(a, 2, 30);
  console.log("readonly", get(a, 2));
}

// 7. Object.setPrototypeOf on an array, and an indexed setter on the chain
{
  const log: string[] = [];
  const proto = Object.create(Array.prototype);
  Object.defineProperty(proto, 1, { set(v) { log.push("protoset=" + v); }, get() { return "p1"; }, configurable: true });
  const a: any = [0, , 2];
  Object.setPrototypeOf(a, proto);
  put(a, 1, 5); // hole: goes to the inherited setter
  put(a, 0, 9); // own element
  console.log("setproto", get(a, 0), get(a, 1), 1 in a, Object.hasOwn(a, 1), log.join(","));
  const b: any = [1, 2];
  Object.setPrototypeOf(b, null);
  put(b, 0, "np"); put(b, 5, "np5");
  console.log("nullproto", b[0], b[5], b.length);
}

// 8. Array.prototype pollution with an indexed accessor
{
  const log: string[] = [];
  Object.defineProperty(Array.prototype, 3, { set(v) { log.push("AP3=" + v); }, get() { return "ap3"; }, configurable: true });
  const a: any = [0, 1, 2];
  put(a, 3, "x"); // append position: inherited setter wins
  const h: any = [0, 1, 2, , 4];
  put(h, 3, "y"); // hole: inherited setter wins
  console.log("ap-pollute", a.length, get(a, 3), get(h, 3), log.join(","));
  delete (Array.prototype as any)[3];
  put(h, 3, "z");
  console.log("ap-clean", get(h, 3));
}

// 9. typed arrays, Buffers and shared buffers through the same untyped sites
{
  const u16: any = new Uint16Array(4); const f64: any = new Float64Array(3); const c: any = new Uint8ClampedArray(2);
  put(u16, 0, 70000); put(u16, 1, -1); put(u16, 2, 3.9); put(u16, 9, 1);
  put(f64, 0, 1.25); put(f64, 1, "2.5"); put(f64, 2, true);
  put(c, 0, 300); put(c, 1, 1.5);
  console.log("typed", Array.from(u16).join(","), Array.from(f64).join(","), Array.from(c).join(","));
  const sab = new SharedArrayBuffer(8);
  const v1: any = new Int32Array(sab); const v2: any = new Int32Array(sab, 4, 1);
  put(v1, 1, 123); put(v2, 0, get(v2, 0) + 1);
  console.log("shared", get(v1, 1), get(v2, 0));
  const ab = new ArrayBuffer(8); const v3: any = new Uint8Array(ab, 2, 4); const v4: any = new Uint8Array(ab);
  put(v3, 0, 255); put(v4, 3, 7);
  console.log("views", Array.from(v4).join(","), get(v3, 1));
  const buf: any = Buffer.alloc(4);
  put(buf, 0, 257); put(buf, 1, 65); put(buf, 9, 1);
  console.log("buffer", buf.toString("hex"), get(buf, 1), get(buf, 9));
}

// 10. primitive receivers (reads only: a strict store onto a primitive is a
// separate, pre-existing gap)
{
  const s: any = "abc";
  const n: any = 5;
  console.log("string", get(s, 1), get(s, 5), get(n, 0));
}

// 11. arguments objects keep their mapped semantics
{
  function f(a: any, b: any) { put(arguments, 0, "A"); put(arguments, 1, "B"); return a + "|" + b + "|" + get(arguments, 0); }
  console.log("arguments", f(1, 2));
}

// 12. read-modify-write and many writes through a grown, re-read binding
{
  const g: any = [];
  for (let i = 0; i < 200; i++) g[i] = i;
  const ref: any = { data: g };
  for (let r = 0; r < 3; r++) for (let i = 0; i < 200; i++) ref.data[i] = ref.data[i] + 1;
  console.log("rmw", sum(ref.data, 200), sumTyped(g, 200), g === ref.data);
}
