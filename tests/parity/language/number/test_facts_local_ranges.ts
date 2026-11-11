// The whole-program interval of a proven int32 local is a range proof in
// codegen (PERRY_FACTS_LOCAL_RANGES): a key or an integer chain built from
// such `let`s and from a typed body's I32 parameters is computed in i32.
// Exact against node with the rule on, off and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

const table: number[] = [];
for (let i = 0; i < 4096; i++) table.push((i * 2654435761) | 0);

// 1. A `let` read from an int Array and a parameter, both of either sign,
// feed a key: `(old + 6) * 64 + square`.
class Grid {
  cells: number[] = new Array<number>(64).fill(0);
  h = 0;
  put(square: number, piece: number): void {
    const old = this.cells[square] ?? 0;
    if (old !== 0) this.h ^= table[(old + 6) * 64 + square] ?? 0;
    this.cells[square] = piece;
    if (piece !== 0) this.h ^= table[(piece + 6) * 64 + square] ?? 0;
  }
}
const g = new Grid();
for (let i = 0; i < 64 * RUN; i++) g.put(i, (i % 13) - 6);
for (let i = 0; i < 64 * RUN; i += 3) g.put(i, 0);
for (let i = 0; i < 64 * RUN; i += 7) g.put(i, i & 1 ? -6 : 6);
console.log("grid", g.h, g.cells.join(","));

// 2. A key that can be negative: it names a property, and the chain's
// negative value must not wrap into an element.
const sparse: number[] = [10, 20, 30, 40];
function probe(i: number): number {
  const k = (i & 7) - 4;
  return sparse[k * 2 + 1] ?? -1;
}
let s2 = 0;
for (let i = 0; i < 16 * RUN; i++) s2 += probe(i);
(sparse as any)[-3] = 7;
for (let i = 0; i < 16 * RUN; i++) s2 += probe(i);
console.log("negative key", s2);

// 3. A product past int32: the interval is no int32, so the key stays a
// double and names the property "4294967296" (never element 0).
const wide: number[] = [1, 2, 3];
function far(i: number): number {
  const a = (i & 1) * 65536;
  const b = a * 65536;
  (wide as any)[b] = i + 100;
  return ((wide as any)[b] as number) + wide[0];
}
let s3 = 0;
for (let i = 0; i < 4 * RUN; i++) s3 += far(i);
console.log("past int32", s3, wide.length, wide[0], (wide as any)[4294967296]);

// 4. Lets that are no int32: a fraction, -0, NaN, undefined. No interval.
function loose(i: number): string {
  const half = i / 2;
  const negz = i === 0 ? -0 : i;
  const nan = i === 1 ? NaN : i;
  const und = i === 2 ? undefined : i;
  const o: any = { 0: "zero", 1: "one", 2: "two", 0.5: "half", NaN: "nan", undefined: "undef" };
  return [o[half], o[negz], o[nan], o[und as any], Object.is(negz, -0)].join("/");
}
const out4: string[] = [];
for (let i = 0; i < 4 * RUN; i++) out4.push(loose(i));
console.log("loose", out4.join(" "));

// 5. A let a closure captures and writes (boxed), and a let written past
// its first interval: the proof covers every value or it is not used.
function boxed(n: number): number {
  let k = n & 3;
  const bump = () => {
    k = k + 100;
  };
  let r = table[k + 1] ?? -1;
  bump();
  r = (r + (table[k + 1] ?? -1)) | 0;
  return r;
}
function widened(n: number): number {
  let k = n & 3;
  let r = table[k] ?? -1;
  if (n > 5) k = k - 9;
  r = (r + (table[k] ?? -5)) | 0;
  return r;
}
let s5 = 0;
for (let i = 0; i < 12 * RUN; i++) s5 = (s5 + boxed(i) + widened(i)) | 0;
console.log("boxed and widened", s5);

// 6. A typed parameter with a signed interval in a chain, beside an
// indirect call that passes values outside every direct site's range.
class Lookup {
  t: number[] = [];
  constructor() {
    for (let i = 0; i < 64; i++) this.t.push(i * i);
  }
  get(i: number, j: number): number {
    return this.t[i * 8 + j] ?? -1;
  }
}
const lk = new Lookup();
let s6 = 0;
for (let i = 0; i < 9 * RUN; i++) for (let j = -2; j < 9 * RUN; j++) s6 += lk.get(i - 1, j);
const viaCall = lk.get;
console.log("typed chain", s6, viaCall.call(lk, 1000, 5), viaCall.call(lk, 0.5, 1), viaCall.call(lk, -1, 9));

// 7. Loop counters keep their own (narrower) facts beside a facts interval.
function sweep(): number {
  let acc = 0;
  for (let r = 0; r < 8 * RUN; r++) {
    const base = r * 8;
    for (let f = 0; f < 8; f++) {
      const sq = base + f;
      acc = (acc + (table[sq * 3 + 1] ?? 0)) | 0;
    }
  }
  return acc;
}
console.log("sweep", sweep());
