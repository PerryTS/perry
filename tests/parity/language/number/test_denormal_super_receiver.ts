// `super.x = v` with a primitive `this` (a method called with `.call(n)`): the
// write's Receiver is that Number. The runtime "normalized" a Receiver with
// clear top 16 bits (a denormal) and an integral Number above 65536 into a heap
// pointer at those bits before running the inherited setter, so the setter saw
// an object instead of the Number. A primitive Receiver stays the primitive: the
// setter's `this` is the Number, and a write with no setter fails (TypeError in
// strict code), as in Node. An instance is the positive control.

function bitsDouble(hi: number, lo: number): number {
  const f = new Float64Array(1);
  const u = new Uint32Array(f.buffer);
  u[0] = lo;
  u[1] = hi;
  return f[0];
}

let seen = "";

class A {
  set x(v: any) {
    seen = typeof this + ":" + (typeof this === "number" ? String(this === v) : "obj");
  }
}

class B extends A {
  viaSetter(v: any): string {
    seen = "";
    super.x = v;
    return seen;
  }
  noSetter(v: any): string {
    super["y" as "x"] = v;
    return "ok";
  }
}

const VALUES: [string, any][] = [
  ["min", Number.MIN_VALUE],
  ["2^28", Number.MIN_VALUE * Math.pow(2, 28)],
  ["mmap", bitsDouble(0x7f12, 0x34567890)],
  ["70000", 70000],
  ["2^40", Math.pow(2, 40)],
  ["1.5", 1.5],
  ["inst", new B()],
];

const OPS: [string, (e: any) => any][] = [
  ["super setter", (e) => B.prototype.viaSetter.call(e, e)],
  ["super no setter", (e) => B.prototype.noSetter.call(e, 1)],
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
