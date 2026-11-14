// A read `a[k]` whose key a length guard bounds (`k <= i < a.length`) is
// not past the end of an Array that never shrinks: the whole-program facts
// leave the past-the-end `undefined` out of it, and the read needs no bound
// test (and no hole test when the Array has no hole). Every case prints
// what node prints; the negative controls are the reads that are past the
// end, under a key that is no index, of an Array that shrinks, or of a hole.
const many = process.argv.length > 100;

function gen(n: number, seed: number): number[] {
  const out: number[] = [];
  for (let i = 0; i < n; i++) out.push(((i * 37 + seed) & 127) | (((i * 11 + seed * 3) & 127) << 7));
  return out;
}
// The consumer: a typed call that takes the element as a Number.
function use(m: number): number {
  return ((m & 127) * 3 + ((m >> 7) & 127)) | 0;
}
function show(v: unknown): string {
  return v === undefined ? "U" : typeof v === "number" ? String(v) : typeof v + ":" + String(v);
}

// (a) Reads at the counter, with and without a default.
function sum(xs: number[]): number {
  let s = 0;
  for (let i = 0; i < xs.length; i++) s = (s + use(xs[i])) | 0;
  for (let i = 0; i < xs.length; i++) s = (s + use(xs[i] ?? 7)) | 0;
  return s;
}
console.log("sum", sum(gen(0, 1)), sum(gen(1, 2)), sum(gen(33, 3)));

// (b) The insertion sort of a copy: `result[i]`, `result[j]` and the stores.
function order(moves: readonly number[]): number[] {
  const result = moves.slice();
  for (let i = 1; i < result.length; i++) {
    const move = result[i];
    let j = i - 1;
    while (j >= 0 && (result[j] ?? 0) < move) {
      result[j + 1] = result[j];
      j--;
    }
    result[j + 1] = move;
  }
  return result;
}
let acc = 0;
for (let r = 0; r < 120; r++) {
  const sorted = order(gen(3 + (r % 19), r));
  for (const m of sorted) acc = (acc + use(m)) | 0;
  if (r < 3) console.log("sort", sorted.join(","));
}
console.log("sort many", acc, order([]).length, order(gen(1, 4)).join(","));

// (c) Keys below the counter, guards by `while`, `if`, `&&` and `?:`.
function below(n: number): string {
  const xs = gen(n, 5);
  let s = 0;
  for (let i = 2; i < xs.length; i++) s = (s + use(xs[i - 2]) + use(xs[i - 1])) | 0;
  let k = 0;
  while (k < xs.length) {
    s = (s + use(xs[k])) | 0;
    k++;
  }
  const m = n >> 1;
  if (m < xs.length) s = (s + use(xs[m])) | 0;
  const t = m < xs.length ? use(xs[m]) : -1;
  const u = n > 3 && 3 < xs.length && use(xs[3]) > 0 ? 1 : 0;
  for (let i = xs.length - 1; i >= 0; i--) {
    if (i < xs.length) s = (s + use(xs[i])) | 0;
  }
  return s + "/" + t + "/" + u;
}
console.log("below", below(0), below(1), below(4), below(29));

// (d) Boxed elements: the read is in bounds, the element keeps its kind.
function words(n: number): string {
  const ws: (string | number)[] = [];
  for (let i = 0; i < n; i++) ws.push(i % 3 === 0 ? "w" + i : i);
  let out = "";
  for (let i = 0; i < ws.length; i++) out += show(ws[i]) + ";";
  return out;
}
console.log("words", words(0), words(7));

// Negative controls.

// (n1) The Array shrinks between the guard and the read.
function popped(n: number): string {
  const xs = gen(n, 7);
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    xs.pop();
    out += show(xs[i]) + ",";
  }
  return out;
}
console.log("popped", popped(5));
function spliced(n: number): string {
  const xs = gen(n, 8);
  const al = xs;
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    if (i === 2) al.splice(0, 3);
    out += show(xs[i]) + ",";
  }
  return out;
}
console.log("spliced", spliced(5));
function drop(a: number[]): void {
  a.length = a.length >> 1;
}
function callee(n: number): string {
  const xs = gen(n, 9);
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    if (i === 3) drop(xs);
    out += show(xs[i]) + ",";
  }
  return out;
}
console.log("callee", callee(8));
function shifted(n: number): string {
  const xs = gen(n, 10);
  let out = "";
  let i = xs.length - 1;
  if (i >= 0 && i < xs.length) {
    xs.shift();
    out += show(xs[i]);
  }
  return out;
}
console.log("shifted", shifted(4));

// (n2) Keys that are no index, or past the counter.
function keys(n: number): string {
  const xs = gen(n, 11);
  let out = "";
  for (let i = 0; i < xs.length; i++) out += show(xs[i - 2]) + ",";
  for (let i = 0; i < xs.length; i++) out += show(xs[i + 1]) + ",";
  for (let i = 0.5; i < xs.length; i++) out += show(xs[i]) + ",";
  for (let i = -1; i < xs.length; i += 2) out += show(xs[i]) + ",";
  let f = many ? 1 : NaN;
  if (!(f >= xs.length)) out += show(xs[f]) + ",";
  for (let i = 0; i <= xs.length; i++) out += show(xs[i]) + ",";
  return out;
}
console.log("keys", keys(3));

// (n3) The counter or the Array is written after the guard.
function written(n: number): string {
  const xs = gen(n, 12);
  const ys = gen(n + 4, 13);
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    i += 2;
    out += show(xs[i]) + ",";
  }
  for (let i = 0; i < ys.length; i++) out += show(xs[i]) + ",";
  let a = ys;
  for (let i = 0; i < a.length; i++) {
    if (i === 2) a = xs;
    out += show(a[i]) + ",";
  }
  let c = 0;
  const bump = () => {
    c += 5;
  };
  if (c < xs.length) {
    bump();
    out += show(xs[c]);
  }
  return out;
}
console.log("written", written(3));

// (n4) Holes: the read is in bounds and reads the hole.
function holes(n: number): string {
  const xs = gen(n, 14);
  xs[n + 2] = 9;
  let out = "";
  for (let i = 0; i < xs.length; i++) out += show(xs[i]) + ",";
  const ys = gen(n, 15);
  delete ys[1];
  for (let i = 0; i < ys.length; i++) out += show(ys[i]) + ",";
  const zs = new Array<number>(n);
  zs[0] = 4;
  for (let i = 0; i < zs.length; i++) out += show(zs[i]) + ",";
  const us: (number | undefined)[] = [];
  for (let i = 0; i < n; i++) us.push(i === 1 ? undefined : i);
  for (let i = 0; i < us.length; i++) out += show(us[i]) + ",";
  return out;
}
console.log("holes", holes(3));

// (n5) Receivers that are no plain Array of this program.
function others(n: number): string {
  let out = "";
  const str = "abc".repeat(n);
  for (let i = 0; i < str.length; i++) out += show(str[i]);
  const ta = new Uint8Array(n + 2);
  for (let i = 0; i < ta.length; i++) out += show(ta[i]);
  const parsed = JSON.parse("[1,2,3]") as number[];
  for (let i = 0; i < parsed.length; i++) {
    if (i === 1) parsed.length = 1;
    out += show(parsed[i]) + ",";
  }
  const like: { length: number; [k: number]: number } = { length: 3, 0: 5 };
  for (let i = 0; i < like.length; i++) out += show(like[i]) + ",";
  return out;
}
console.log("others", others(2));

// (n6) One callee's shrink un-proves the reads of every Array it may get.
function trim(a: number[], on: boolean): number[] {
  if (on) a.pop();
  return a;
}
function shared(n: number): string {
  const xs = trim(gen(n, 16), false);
  const ys = trim(gen(n, 17), true);
  let out = "";
  for (let i = 0; i < xs.length; i++) {
    trim(ys, i === 0);
    out += show(xs[i]) + "|" + show(ys[i]) + ",";
  }
  return out;
}
console.log("shared", shared(4));
