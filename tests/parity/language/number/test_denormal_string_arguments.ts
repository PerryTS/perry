// A Number whose IEEE bits look like a heap address (a denormal double) handed
// where the runtime wants a string. The shared string decoder returned a
// 4-byte-aligned word with clear top 16 bits above 0x10000 as a StringHeader
// address ("raw pointer" arm), so `Buffer.isEncoding(e)` read a header at the
// Number's bits (SIGSEGV). A Number there is its string form, as in Node
// ("5e-324" is no encoding). Strings are the positive controls.

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
  ["+0", 0],
  ["1.5", 1.5],
  ["utf8", "utf8"],
  ["hex", "hex"],
];

const OPS: [string, (e: any) => any][] = [
  ["Buffer.isEncoding", (e) => Buffer.isEncoding(e)],
  ["Buffer.isBuffer", (e) => Buffer.isBuffer(e)],
  ["String", (e) => String(e).length],
  ["concat", (e) => ("x" + e).length],
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
