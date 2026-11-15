// `a[k] ?? <int32 literal>` over an Array stored at rest, where the key is a
// Number the facts do not prove an int32 (a fraction reaches it): the read
// is still an i32. An integer key below the floor is the slot's word; every
// other Number (a fraction, a negative, NaN, an infinity, a key past the
// end) reads `undefined`, so the literal (PERRY_REST_DEFAULT_NUMBER_KEY).
// Exact against node with the rule on, off and in check mode.
// A loop bound the compiler cannot fold keeps each loop as written.
const RUN = Math.random() < 2 ? 1 : 0;

// 1. A board read through a method whose key parameter is a Number: half
// of the keys of the loop are fractions.
class Board {
  cells: number[] = new Array<number>(64).fill(0);
  at(square: number): number {
    return this.cells[square] ?? 0;
  }
  other(square: number): number {
    return this.cells[square] ?? -7;
  }
  put(square: number, piece: number): void {
    this.cells[square] = piece;
  }
}
function board(): void {
  const b = new Board();
  for (let i = 0; i < 64 * RUN; i++) b.put(i, (i % 13) - 6);
  let empty = 0;
  let sum = 0;
  let prod = 0;
  let text = "";
  for (let i = -6; i < 140 * RUN; i++) {
    const k = i / 2;
    if (b.at(k) === 0) empty++;
    const p = b.at(k);
    sum = (sum + p * (i + 7)) | 0;
    prod += b.at(k) * 0.5;
    if (b.other(k) < -6) text += k + ":" + b.other(k) + " ";
  }
  console.log("board", empty, sum, prod, text);
  // The keys no int32 holds, and the edges of the floor.
  console.log(
    "edges",
    [
      b.at(-0),
      b.at(NaN),
      b.at(Infinity),
      b.at(-Infinity),
      b.at(4294967296),
      b.at(4294967297),
      b.at(1e300),
      b.at(5e-324),
      b.at(2147483648),
      b.at(-2147483648),
      b.at(63),
      b.at(64),
      b.at(62.99999999999999),
      b.at(63.00000000000001),
      b.at(0.1 + 0.2),
    ].join(","),
  );
  console.log("other", [b.other(-0), b.other(NaN), b.other(1.5), b.other(64), b.other(7)].join(","));
}
board();

// 2. A direct read with a default through a function whose key is a
// Number: integers in range, fractions, keys below 0 and past the end.
const cells: number[] = new Array<number>(16).fill(0);
function put(i: number, v: number): void {
  cells[i] = v;
}
function get(k: number): number {
  return cells[k] ?? 9;
}
function direct(): void {
  for (let i = 0; i < 16 * RUN; i++) put(i, i * i - 100);
  const out: number[] = [];
  for (let i = -6; i < 40 * RUN; i++) out.push(get(i / 2));
  out.push(get(Math.sqrt(2)), get(Math.sqrt(16)), get(0 / (RUN - 1)), get(-1 / RUN));
  console.log("direct", out.join(","));
}
direct();

// 3. The result feeds an int consumer, a compare and a string.
function consumers(): void {
  const hist: number[] = new Array<number>(8).fill(0);
  let bits = 0;
  let text = "";
  for (let i = 0; i < 24 * RUN; i++) {
    const k = i / 3;
    const v = get(k);
    hist[v & 7] = (hist[v & 7] ?? 0) + 1;
    bits = (bits ^ (v << (i & 15))) | 0;
    if (v !== 9) text += v + ";";
  }
  console.log("consumers", hist.join(","), bits, text);
}
consumers();

// 4. Control: a key that may be a string keeps the generic read: `length`
// and an index string see the Array's own values.
const loose: number[] = new Array<number>(8).fill(0);
function lput(i: number, v: number): void {
  loose[i] = v;
}
function lget(k: any): any {
  return loose[k] ?? 5;
}
function control(): void {
  for (let i = 0; i < 8 * RUN; i++) lput(i, i - 3);
  console.log("control", [lget("length"), lget("2"), lget(2), lget(2.5), lget("x"), lget(-1), lget(8)].join(","));
}
control();

// 5. Control: a default that is no int32 literal keeps its double.
function fraction(k: number): number {
  return cells[k] ?? 0.5;
}
function control2(): void {
  console.log("fraction", [fraction(1), fraction(1.5), fraction(-1), fraction(99)].join(","));
}
control2();
