// PR-C1 (DESIGN-p4l1 §2.8, fixture 16): a computed-key read of a program
// Array reads `undefined` past its end only. The facts leave that
// `undefined` out when the key is an integer below the length's lower
// bound, and keep it for every other read: keys reaching the end,
// negative and fractional keys, holes, stored `undefined`, and arrays
// whose length moves.
const out: string[] = [];

const A = [1, 2, 3, 4, 5, 6, 7, 8];

// In the floor: `i < 8` on a length-8 literal.
let inside = 0;
for (let i = 0; i < 8; i++) {
  const x = A[i];
  inside += x * 2;
}
out.push("i" + inside);

// Reaching the end: `i <= 8` reads A[8], which is undefined.
let past = "";
for (let i = 0; i <= 8; i++) {
  const x = A[i];
  past += typeof x === "number" ? x : "U";
}
out.push("p" + past);
let nan = 0;
for (let i = 0; i <= 8; i++) {
  const x = A[i];
  nan = nan + (x as number);
}
out.push("n" + String(nan));

// Negative and fractional keys.
let neg = "";
for (let i = -2; i < 3; i++) {
  const x = A[i];
  neg += String(x) + ",";
}
out.push("m" + neg);
let frac = "";
for (let i = 0; i < 3; i += 0.5) {
  const x = A[i];
  frac += String(x) + ",";
}
out.push("f" + frac);

// Stored undefined: the element is present and is undefined.
const S: (number | undefined)[] = [1, 2, 3, 4];
S[1] = undefined;
let st = "";
for (let i = 0; i < 4; i++) {
  const x = S[i];
  st += typeof x + ":" + String(x ?? 5) + ",";
}
out.push("s" + st);
const T = [1, 2, 3];
function clear(k: number): void {
  (T as (number | undefined)[])[k] = undefined;
}
clear(2);
let tt = "";
for (let i = 0; i < 3; i++) {
  const x = T[i];
  tt += String(x) + ",";
}
out.push("t" + tt);

// Holes: a literal with an elision, a store past the end.
const H: (number | undefined)[] = [1, , 3, 4];
let hs = "";
for (let i = 0; i < 4; i++) {
  const x = H[i];
  hs += String(x) + ",";
}
out.push("h" + hs);
const G = [1, 2, 3];
G[5] = 6;
let gs = "";
for (let i = 0; i < 6; i++) {
  const x = G[i];
  gs += String(x ?? "e") + ",";
}
out.push("g" + gs + G.length);

// The length moves: shrink by `length`, by `pop`, by `splice`; grow by push.
const L = [1, 2, 3, 4, 5];
L.length = 2;
let ls = "";
for (let i = 0; i < 5; i++) {
  const x = L[i];
  ls += String(x) + ",";
}
out.push("l" + ls);
const P = [1, 2, 3, 4];
P.pop();
P.pop();
let ps = "";
for (let i = 0; i < 4; i++) {
  const x = P[i];
  ps += String(x) + ",";
}
out.push("o" + ps);
const Q = [1, 2, 3, 4];
Q.splice(1, 2);
let qs = "";
for (let i = 0; i < 4; i++) {
  const x = Q[i];
  qs += String(x) + ",";
}
out.push("q" + qs);
const R: number[] = [];
for (let i = 0; i < 4; i++) R.push(i * 3);
let rs = 0;
for (let i = 0; i < 4; i++) {
  const x = R[i];
  rs += x;
}
out.push("r" + rs);

// delete leaves a hole below the length.
const D = [1, 2, 3];
delete (D as any)[1];
let ds = "";
for (let i = 0; i < 3; i++) {
  const x = D[i];
  ds += String(x) + ",";
}
out.push("d" + ds);

// Two receivers with different lengths behind one read.
const B4 = [1, 2, 3, 4];
const B2 = [9, 8];
function pick(c: boolean, i: number): number | undefined {
  const arr = c ? B4 : B2;
  return arr[i & 3];
}
let bs = "";
for (let i = 0; i < 4; i++) bs += String(pick(true, i)) + "/" + String(pick(false, i)) + ",";
out.push("b" + bs);

// A key from arithmetic that stays inside: `(i * 3) & 7`.
let mask = 0;
for (let i = 0; i < 100; i++) {
  const x = A[(i * 3) & 7];
  mask += x;
}
out.push("k" + mask);

// Two reads at one site start: `rows[i][0]` reads `rows` under `i` (eight
// rows) and each row under 0 (two cells). Check mode re-tests `i` against
// the outer read's floor, never the row's.
const rows: [string, number][] = [
  ["a", 1], ["b", 2], ["c", 3], ["d", 4], ["e", 5], ["f", 6], ["g", 7], ["h", 8],
];
let rowText = "";
for (let i = 0; i < rows.length; i++) rowText += rows[i][0] + rows[i][1];
out.push("w" + rowText);

console.log(out.join(" "));
