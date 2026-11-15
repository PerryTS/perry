// A Number whose IEEE bits look like a heap address (a denormal double), as an
// ELEMENT of the list handed to Buffer.concat. The runtime kept a list element
// without a NaN-box tag as the buffer's address and read a header there
// (SIGSEGV); a negative denormal lost its sign bit to the tag mask and crashed
// the same way. A Number element is not a Buffer: ERR_INVALID_ARG_TYPE, as in
// Node. Real Buffers and a Uint8Array are the positive controls.

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
  ["+0", 0],
  ["1.5", 1.5],
  ["str", "ab"],
  ["buf", Buffer.from("xyz")],
  ["u8", new Uint8Array([1, 2])],
];

const OPS: [string, (e: any) => any][] = [
  ["concat [e]", (e) => Buffer.concat([e]).length],
  ["concat [buf, e]", (e) => Buffer.concat([Buffer.from("ab"), e]).length],
  ["concat [e] len", (e) => Buffer.concat([e], 2).length],
  ["concat [e, e]", (e) => Buffer.concat([e, e]).toString("hex")],
];

for (const [name, op] of OPS) {
  let line = name + ":";
  for (const [label, e] of VALUES) {
    let r: string;
    try {
      r = String(op(e));
    } catch (x: any) {
      r = "THROW " + (x && x.name) + " " + (x && x.code);
    }
    line += " " + label + "=" + r;
  }
  console.log(line);
}
