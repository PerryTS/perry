// A Number whose IEEE bits look like a heap address (a denormal double) as the
// `this` of the reflective Set / Map prototype methods. Their brand checks took
// a word with clear top 16 bits as a candidate Set / Map address (guarded by
// the collection registry, so a Number whose bits equal a live collection's
// address passed as that collection). A Number receiver is a primitive: the
// brand check throws a TypeError, as in Node. Real collections and a subclass
// instance are the positive controls; `forEach` and `add` hand the receiver on.

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

class MyMap extends Map<any, any> {}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^21", Number.MIN_VALUE * Math.pow(2, 21)],
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)],
  ["2^40", Number.MIN_VALUE * Math.pow(2, 40)],
  ["odd", bitsDouble(0, 0x10000003)],
  ["mmap", bitsDouble(0x7f12, 0x34567890)],
  ["brk", bitsDouble(0x5555, 0x55560010)],
  ["-2^28", -(Number.MIN_VALUE * Math.pow(2, 28))],
  ["1.5", 1.5],
  ["set", new Set([1])],
  ["map", new Map([[1, 2]])],
  ["submap", new MyMap([[1, 3]])],
];

const OPS: [string, (e: any) => any][] = [
  ["Set has", (e) => Set.prototype.has.call(e, 1)],
  ["Set add", (e) => Set.prototype.add.call(e, 2) === e],
  ["Set size", (e) => Reflect.get(Set.prototype, "size", e)],
  ["Set forEach", (e) => {
    let seen = 0;
    Set.prototype.forEach.call(e, (_v: any, _k: any, c: any) => {
      seen += c === e ? 1 : 100;
    });
    return seen;
  }],
  ["Map get", (e) => Map.prototype.get.call(e, 1)],
  ["Map set", (e) => Map.prototype.set.call(e, 5, 6) === e],
  ["Map forEach", (e) => {
    let seen = 0;
    Map.prototype.forEach.call(e, (_v: any, _k: any, c: any) => {
      seen += c === e ? 1 : 100;
    });
    return seen;
  }],
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
