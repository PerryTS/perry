// PR-C1 (DESIGN-p4l1 §2.9, fixture 18): a for-of walk's element read has a
// real site key, so its facts (a proven element read, a Number element) are
// per site. Each loop below must still see exactly what the array iterator
// yields: a const literal, one whose sibling grows, an array mutated while
// it is walked, holes and stored `undefined`.
const out: string[] = [];

const K = [-33, -31, -18, -14, 14, 18, 31, 33];
const KING = [-17, -16, -15, -1, 1, 15, 16, 17];
const K2 = [1, 2, 3, 4];
K2.push(5);

function sumOf(xs: number[]): number {
  let s = 0;
  for (const d of xs) s += d;
  return s;
}
let total = 0;
for (let r = 0; r < 3; r++) {
  for (const d of K) total += d * (r + 1);
  for (const d of KING) total -= d;
}
out.push("k" + total + "," + sumOf(K) + "," + sumOf(KING));

// One loop over either array (`const dirs = c ? K : K2`): the walk's
// receiver holds both.
function walkEither(c: boolean): string {
  const dirs = c ? K : K2;
  let s = "";
  for (const d of dirs) s += d + ";";
  return s;
}
out.push("e" + walkEither(true) + "|" + walkEither(false));

// Nested walks: an enclosing rewrite lowers the inner loop twice.
let pairs = 0;
for (const a of K2) {
  for (const b of K) {
    if (a + b > 0) pairs++;
  }
}
out.push("n" + pairs);

// Mutated while walked: pushes are visited, truncation stops the walk,
// a store replaces a later element, splice and shift move them.
const grow = [1, 2, 3];
for (const x of grow) {
  if (x < 5) grow.push(x + 2);
  out.push("g" + x);
}
const cut = [1, 2, 3, 4, 5, 6];
for (const x of cut) {
  if (x === 3) cut.length = 4;
  out.push("c" + x);
}
const swap = [10, 20, 30, 40];
for (const x of swap) {
  if (x === 10) swap[2] = 99;
  out.push("w" + x);
}
const spl = [1, 2, 3, 4, 5];
for (const x of spl) {
  if (x === 2) spl.splice(0, 2);
  out.push("p" + x);
}
const sh = [7, 8, 9];
for (const x of sh) {
  if (x === 7) sh.shift();
  out.push("s" + x);
}
const pop = [1, 2, 3, 4];
for (const x of pop) {
  pop.pop();
  out.push("o" + x);
}

// Holes and stored undefined: the walk yields `undefined` for both.
const holey: (number | undefined)[] = [1, , 3];
for (const x of holey) out.push("h" + String(x));
const sparse: number[] = [];
sparse[3] = 4;
for (const x of sparse) out.push("q" + String(x));
const stored: (number | undefined)[] = [1, 2, 3];
stored[1] = undefined;
for (const x of stored) out.push("u" + String(x) + typeof x);
const grown = [1, 2];
grown[4] = 5;
for (const x of grown) out.push("r" + String(x ?? -1));

// A walk whose element feeds arithmetic and comparisons in a hot loop.
let acc = 0;
for (let i = 0; i < 2000; i++) {
  for (const d of K) {
    const sq = (i & 127) + d;
    if (sq >= 0 && sq < 128 && (sq & 0x88) === 0) acc += sq;
  }
}
out.push("a" + acc);

console.log(out.join(" "));
