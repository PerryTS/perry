// P3 B4b: a birth (constructor prologue, object literal) stores a value the
// facts or its structure prove a Number into a raw f64 lane with no number
// test. Every value below must read back as node prints it, non-finite ones
// and -0 included. Negative controls: births whose argument is not a proven
// Number (a string or undefined through `any`, a value of unknown kind) keep
// the test and take the ordinary constructor.
const dyn = (v: any): any => v;
const N = Math.random() < 2 ? 1 : 0;

class Vec {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  len(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
}
class Acc {
  total: number;
  half: number;
  constructor(total: number) {
    this.total = total;
    this.half = total + 0.5;
  }
}
class State {
  ep: number = -1;
  clock: number = 0.5;
  scale: number = 1.25;
}

function mk(i: number, f: number): Vec {
  // a parameter and a local
  const h = f / 2;
  return new Vec(i + 0.25, h);
}
function fromState(s: State): { ep: number; clock: number; twice: number } {
  // field reads of proven Number fields, and arithmetic over them
  return { ep: s.ep, clock: s.clock, twice: s.clock * 2 };
}

const keep: Vec[] = [];
const lits: { ep: number; clock: number; twice: number }[] = [];
const st = new State();
for (let i = 0; i < 6 * N; i++) {
  st.ep = i % 2 === 0 ? -1 : i * 16.5;
  st.clock = st.clock + 0.75;
  keep.push(mk(i, i * 3.5));
  keep.push(new Vec(Math.sqrt(i), Math.abs(-i) / 4));
  lits.push(fromState(st));
}
console.log(keep.map((v) => `${v.x},${v.y},${v.len().toFixed(3)}`).join(" "));
console.log(JSON.stringify(lits));

// Non-finite values and -0 are Numbers: they are born raw and read back.
const odd = [NaN, Infinity, -Infinity, -0, 0, 1e308 * 10 * N, 0 * (N / 0), -(0 * N), 2 ** 53, 5e-324];
const boxes: Vec[] = [];
for (let i = 0; i < odd.length; i++) boxes.push(new Vec(odd[i], -odd[i]));
console.log(boxes.map((b) => `${b.x}/${b.y}/${Object.is(b.x, -0)}/${Object.is(b.y, -0)}`).join(" "));
console.log(boxes.map((b) => typeof b.x + (b.x === b.x ? "" : "!")).join(""), JSON.stringify(boxes[0]), boxes[3]);
const accs: Acc[] = [];
for (let i = 0; i < 4 * N; i++) accs.push(new Acc(i / 0 - i / 0 + i), new Acc(i * 1.5));
console.log(accs.map((a) => `${a.total}:${a.half}`).join(" "));
const olits: { a: number; b: number; c: number }[] = [];
for (let i = 0; i < 3 * N; i++) olits.push({ a: Math.max(i, 0.5), b: -i || -0, c: i === 1 ? NaN : i * 1e300 * 1e10 });
console.log(olits.map((o) => `${o.a}|${Object.is(o.b, -0)}|${o.c}`).join(" "), JSON.stringify(olits));

// Negative controls: the argument is no proven Number.
const mixed: any[] = ["s", undefined, null, true, 7n, { valueOf: () => 3 }, [4], 2.5, dyn(-0)];
const wild: Vec[] = [];
for (const m of mixed) wild.push(new Vec(m, dyn(m)));
console.log(wild.map((w) => `${typeof w.x}:${String(w.x)}:${typeof w.y}`).join(" "));
const wacc: Acc[] = [];
for (const m of ["s", undefined, 2.5, null]) wacc.push(new Acc(m as any));
console.log(wacc.map((a) => `${String(a.total)}|${String(a.half)}`).join(" "));
function unknown(i: number): any {
  return i % 2 === 0 ? i + 0.5 : "odd" + i;
}
const ulits: { v: number; w: number }[] = [];
for (let i = 0; i < 4 * N; i++) ulits.push({ v: unknown(i), w: i + 0.5 });
console.log(ulits.map((u) => `${typeof u.v}:${u.v}:${u.w}`).join(" "));
// A field that later holds a string keeps working after a raw birth.
const late = new Vec(1.5, 2.5);
(late as any).x = "later";
console.log(typeof late.x, late.x, late.y, keep.length, lits.length);
