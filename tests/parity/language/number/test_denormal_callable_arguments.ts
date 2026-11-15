// A Number whose IEEE bits look like a heap address (a denormal double) as the
// CALLABLE argument of `new Promise(executor)` and `JSON.parse(text, reviver)`.
// Codegen masked the argument's tag off unconditionally, so a denormal kept its
// bits and a negative Number or 1.1 kept its low 48 bits: the runtime called
// that "closure" (SIGSEGV). A Number executor throws a TypeError; a reviver
// that is not callable is ignored (also an object or an array). Functions are
// the positive controls.

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

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
  ["-1.1", -1.1],
  ["+0", 0],
  ["1.5", 1.5],
  ["str", "ab"],
  ["undef", undefined],
  ["obj", { a: 1 }],
  ["arr", [1]],
  ["fn", (k: any, v: any) => (typeof v === "number" ? v * 2 : v)],
];

const OPS: [string, (e: any) => any][] = [
  ["new Promise", (e) => typeof new Promise(e)],
  ["JSON.parse reviver", (e) => JSON.stringify(JSON.parse('{"a":1,"b":[2]}', e))],
  ["JSON.parse reviver scalar", (e) => JSON.parse("3", e)],
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
