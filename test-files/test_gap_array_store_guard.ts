// The array element store guards on ONE header word (an ordinary array, not a
// growth stub, no element descriptors, not frozen / sealed / non-extensible),
// overwrites only a slot that already holds a value, settles the element kind
// BEFORE writing the value, and consults the prototype only when the store
// adds an element. Every case here is a fact one of those steps must keep:
// each must answer as node. Prototype-polluting cases run LAST (sticky).

function put(xs: any[], i: number, v: any): void {
  xs[i] = v;
}
class Holder {
  vals: any[];
  constructor(v: any[]) {
    this.vals = v;
  }
  put(i: number, v: any): void {
    this.vals[i] = v;
  }
}
function sumNum(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) s += xs[i];
  return s;
}
function show(xs: any[]): string {
  let out = "";
  for (let i = 0; i < xs.length; i++) out += typeof xs[i] + ":" + String(xs[i]) + ",";
  return out;
}

// 1. F64 -> Any: a non-Number stored into a numeric array. The kind bit must
//    clear BEFORE the value lands, or a numeric reader returns the pointer bits.
{
  const xs: number[] = [1.5, 2.5, 3.5, 4.5];
  console.log("f64 sum", sumNum(xs));
  put(xs as any[], 1, "str");
  console.log("f64->str", show(xs), String(xs[1]), sumNum(xs));
  const ys: number[] = [0.5, 1.5, 2.5];
  sumNum(ys);
  new Holder(ys as any[]).put(2, { o: 7 });
  console.log("f64->obj", show(ys), (ys[2] as any).o);
  const zs: number[] = [1.25, 2.25];
  sumNum(zs);
  put(zs as any[], 0, undefined);
  put(zs as any[], 1, null);
  console.log("f64->undef/null", show(zs), sumNum(zs));
}

// 2. Numbers into a numeric array keep it numeric: integers, NaN, -0, Infinity.
{
  const xs: number[] = [1.5, 2.5, 3.5, 4.5, 5.5];
  sumNum(xs);
  let k: any = 7;
  put(xs as any[], 0, k | 0);
  put(xs as any[], 1, 0 / 0);
  put(xs as any[], 2, -0);
  put(xs as any[], 3, 1 / 0);
  new Holder(xs as any[]).put(4, Math.sqrt(-1));
  console.log("f64 nums", show(xs), Object.is(xs[2], -0), Number.isNaN(xs[1]), Number.isNaN(xs[4]));
  for (let i = 0; i < 5; i++) xs[i] = i * 0.5;
  console.log("f64 loop", sumNum(xs));
}

// 3. A store past length creates holes: the array must read them as holes.
{
  const xs: number[] = [1.5, 2.5];
  sumNum(xs);
  put(xs as any[], 4, 9.5);
  console.log("past length", xs.length, show(xs), 1 in xs, 3 in xs);
  const ys: any[] = [{ a: 1 }];
  put(ys, 3, "z");
  console.log("past length any", ys.length, 2 in ys, show(ys));
}

// 4. A frozen / sealed / non-extensible array refuses what it must refuse.
{
  const f: any[] = Object.freeze([1, 2, 3]) as any[];
  try {
    put(f, 0, 9);
    console.log("frozen stored?", f[0]);
  } catch (e) {
    console.log("frozen throws", (e as Error).constructor.name, f[0]);
  }
  const s: any[] = Object.seal([1, 2, 3]) as any[];
  put(s, 1, 8);
  try {
    put(s, 3, 7);
  } catch (e) {
    console.log("sealed add throws", (e as Error).constructor.name);
  }
  console.log("sealed", s.length, s[1], s[3]);
  const n: any[] = Object.preventExtensions([1, 2, 3]) as any[];
  put(n, 2, 6);
  try {
    put(n, 3, 5);
  } catch (e) {
    console.log("nonext add throws", (e as Error).constructor.name);
  }
  console.log("nonext", n.length, n[2], n[3]);
  // Spare capacity: `[length, capacity)` holds holes, so the add would fit
  // inline. Only the header word's NO_EXTEND bit refuses it.
  const m: any[] = [1, 2, 3, 4];
  m.length = 3;
  Object.preventExtensions(m);
  try {
    put(m, 3, 5);
  } catch (e) {
    console.log("nonext spare add throws", (e as Error).constructor.name);
  }
  console.log("nonext spare", m.length, m[3], 3 in m);
}

// 5. An accessor element: the store must call the setter.
{
  const a: any[] = [1, 2, 3];
  let seen = "";
  Object.defineProperty(a, 1, {
    get() {
      return 42;
    },
    set(v) {
      seen += "set" + v;
    },
  });
  put(a, 1, 5);
  new Holder(a).put(1, 6);
  console.log("accessor", seen, a[1]);
}

// 6. A stale alias of a grown array: the store must land in the live array.
{
  const a: any[] = [1, 2];
  const h = new Holder(a);
  const alias: any = { v: a };
  for (let i = 0; i < 100; i++) a.push(i);
  h.put(0, "via-holder");
  put(alias.v, 1, "via-alias");
  console.log("grown", a[0], a[1], alias.v[0], a.length);
}

// 7. Holes: a store into a hole below length is an add; with a clean
//    prototype it simply fills the hole.
{
  const a: any[] = [1, , 3];
  put(a, 1, "filled");
  const b: number[] = new Array(4);
  for (let i = 0; i < 4; i++) b[i] = i + 0.5;
  console.log("holes", show(a), 1 in a, sumNum(b));
}

// 8. LAST: an index setter on Array.prototype intercepts adds (a hole, an
//    append), never an overwrite of an existing element.
{
  let log = "";
  Object.defineProperty(Array.prototype, "1", {
    set(v) {
      log += "proto1=" + v + ";";
    },
    configurable: true,
  });
  Object.defineProperty(Array.prototype, "3", {
    set(v) {
      log += "proto3=" + v + ";";
    },
    configurable: true,
  });
  const a: any[] = [1, , 3];
  put(a, 1, "x");
  const b: any[] = [1, 2, 3];
  put(b, 3, "y");
  new Holder(b).put(1, "own");
  const c: any[] = [1, , 3];
  new Holder(c).put(1, "z");
  // An append that fits the spare capacity inline must still reach the setter.
  const e: any[] = [1, 2, 3, 4, 5];
  e.length = 3;
  put(e, 3, "w");
  log += "e.length=" + e.length + ";";
  console.log("proto", log, 1 in a, b.length, b[1], 1 in c);
}
