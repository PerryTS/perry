// A Number whose IEEE bits look like a heap address (a denormal double), handed
// as an ARGUMENT where the runtime expects an object, an iterable, a callable or
// a collection. The runtime's shared pointer decoder used to return such a
// word's bits as an address ("raw pointer" arm), so `new Set(e)`, `new Map(e)`,
// `new WeakMap(e)`, `new WeakSet(e)`, `Map.groupBy(e, f)`, `Object.groupBy(e, f)`,
// `Object.groupBy([1], e)` and `String.raw(e)` dereferenced the Number
// (SIGSEGV). A Number is a primitive in every one of them: these must all
// match Node (a TypeError, or the result for a non-iterable primitive).

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^21", Number.MIN_VALUE * Math.pow(2, 21)], // first word above the handle band
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)], // low heap window
  ["2^40", Number.MIN_VALUE * Math.pow(2, 40)],
  ["odd", bitsDouble(0, 0x10000003)], // unaligned
  ["mmap", bitsDouble(0x7f12, 0x34567890)], // mmap-band address shape
  ["brk", bitsDouble(0x5555, 0x55560010)], // brk-band address shape
  ["maxden", bitsDouble(0x000fffff, 0xffffffff)],
  ["-2^28", -(Number.MIN_VALUE * Math.pow(2, 28))],
  ["+0", 0],
  ["1.5", 1.5], // control: an ordinary Number
];

function id(x: any): any {
  return x;
}
function thisType(this: any): any {
  return typeof this;
}

const OPS: [string, (e: any) => any][] = [
  ["new Set", (e) => new Set(e).size],
  ["new Map", (e) => new Map(e).size],
  ["new WeakMap", (e) => typeof new WeakMap(e)],
  ["new WeakSet", (e) => typeof new WeakSet(e)],
  ["Map.groupBy", (e) => typeof (Map as any).groupBy(e, id)],
  ["Object.groupBy", (e) => typeof (Object as any).groupBy(e, id)],
  ["Object.groupBy cb", (e) => typeof (Object as any).groupBy([1], e)],
  ["String.raw", (e) => String.raw(e)],
  // Already Number-correct before; kept so the decoder's other callers stay so.
  ["Array.from", (e) => Array.from(e).length],
  ["Array.from map", (e) => Array.from([1, 2], e).length],
  ["concat", (e) => [1].concat(e).length],
  ["slice.call", (e) => Array.prototype.slice.call(e).length],
  ["WeakMap set", (e) => new WeakMap<any, any>().set(e, 1)],
  ["WeakMap get", (e) => {
    const m = new WeakMap<any, any>();
    return String(m.get(e)) + m.has(e) + m.delete(e);
  }],
  ["WeakSet add", (e) => new WeakSet<any>().add(e)],
  ["WeakRef", (e) => typeof new WeakRef(e)],
  ["registry", (e) => new FinalizationRegistry<any>(id).register(e, 1)],
  ["for-of", (e) => {
    let n = 0;
    for (const _x of e) n++;
    return n;
  }],
  ["spread", (e) => [...e].length],
  ["destructure", (e) => {
    const [a] = e;
    return a;
  }],
  ["apply args", (e) => thisType.apply(null, e)],
  ["Reflect.apply args", (e) => Reflect.apply(id, null, e)],
  ["fromEntries", (e) => Object.keys(Object.fromEntries(e)).length],
  ["entries", (e) => Object.entries(e).length + Object.values(e).length],
  ["assign", (e) => Object.keys(Object.assign({ a: 1 }, e)).join()],
  ["structuredClone", (e) => Object.is(structuredClone(e), e)],
  ["typed set", (e) => {
    const t = new Uint8Array(4);
    t.set(e);
    return t[0];
  }],
  ["flat", (e) => [e, [e]].flat().length],
];

for (const [name, op] of OPS) {
  let line = name + ":";
  for (const [label, e] of VALUES) {
    let r: string;
    try {
      r = String(op(e));
    } catch (x: any) {
      r = "THROW " + (x && x.name);
    }
    line += " " + label + "=" + r;
  }
  console.log(line);
}
