// A Number whose IEEE bits look like a heap address (a denormal double), handed
// as the `other` argument of the ES2025 Set methods. The runtime read `other`'s
// bits as a SetHeader address when they were not NaN-boxed as an object, so
// `new Set([1]).union(e)` and the six other methods dereferenced the Number
// (SIGSEGV), and every other primitive `other` was taken as an empty Set. A
// primitive is never set-like: GetSetRecord throws a TypeError for it. A real
// Set and a Set subclass instance as `other` are the positive controls.

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

class MySet<T> extends Set<T> {}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^21", Number.MIN_VALUE * Math.pow(2, 21)],
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)],
  ["2^40", Number.MIN_VALUE * Math.pow(2, 40)],
  ["odd", bitsDouble(0, 0x10000003)],
  ["mmap", bitsDouble(0x7f12, 0x34567890)],
  ["brk", bitsDouble(0x5555, 0x55560010)],
  ["maxden", bitsDouble(0x000fffff, 0xffffffff)],
  ["-2^28", -(Number.MIN_VALUE * Math.pow(2, 28))],
  ["1.1", 1.1],
  ["+0", 0],
  ["1.5", 1.5],
  ["str", "ab"],
  ["undef", undefined],
  ["null", null],
  ["set", new Set([1, 2])],
  ["subset", new MySet([2, 3])],
];

function show(r: any): string {
  return r instanceof Set ? "Set[" + [...r].join(",") + "]" : String(r);
}

const OPS: [string, (e: any) => any][] = [
  ["union", (e) => (new Set([1]) as any).union(e)],
  ["intersection", (e) => (new Set([1, 2]) as any).intersection(e)],
  ["difference", (e) => (new Set([1, 2]) as any).difference(e)],
  ["symmetricDifference", (e) => (new Set([1]) as any).symmetricDifference(e)],
  ["isSubsetOf", (e) => (new Set([1]) as any).isSubsetOf(e)],
  ["isSupersetOf", (e) => (new Set([1, 2, 3]) as any).isSupersetOf(e)],
  ["isDisjointFrom", (e) => (new Set([5]) as any).isDisjointFrom(e)],
  ["union empty this", (e) => (new Set() as any).union(e)],
];

for (const [name, op] of OPS) {
  let line = name + ":";
  for (const [label, e] of VALUES) {
    let r: string;
    try {
      r = show(op(e));
    } catch (x: any) {
      r = "THROW " + (x && x.name);
    }
    line += " " + label + "=" + r;
  }
  console.log(line);
}
