// Full-iteration + random-access patterns. The lazy path is
// exercised by the regression sweep under `PERRY_JSON_TAPE=1`;
// here we just verify access patterns are correctness-equivalent
// to Node regardless of representation.

const blob = JSON.stringify([
  { id: 0, v: 10 },
  { id: 1, v: 20 },
  { id: 2, v: 30 },
  { id: 3, v: 40 },
  { id: 4, v: 50 },
  { id: 5, v: 60 },
  { id: 6, v: 70 },
  { id: 7, v: 80 },
]);

// Sequential for-loop — walk cursor keeps this O(n) overall.
const a = JSON.parse(blob);
let seqSum = 0;
for (let i = 0; i < a.length; i++) {
  seqSum += a[i].v;
}
console.log("seq-sum:" + seqSum);

// Reverse iteration — cursor resets on i < walk_idx, each step
// walks from root but distance shrinks monotonically.
const b = JSON.parse(blob);
let revSum = 0;
for (let i = b.length - 1; i >= 0; i--) {
  revSum += b[i].v;
}
console.log("rev-sum:" + revSum);

// Random-order access — deterministic permutation.
const c = JSON.parse(blob);
const perm = [3, 7, 1, 5, 0, 6, 2, 4];
let rndSum = 0;
for (let i = 0; i < perm.length; i++) {
  rndSum += c[perm[i]].v;
}
console.log("rnd-sum:" + rndSum);

// Stringify after iteration — bitmap has bits set, must go through
// force-materialize-via-cache to produce byte-correct output.
const d = JSON.parse(blob);
for (let i = 0; i < d.length; i++) {
  d[i].v;
}
console.log("stringify-after-iter-len:" + JSON.stringify(d).length);

// Repeated identity check — cache retains identity across loops.
const e = JSON.parse(blob);
const first = e[0];
for (let j = 0; j < 3; j++) {
  if (e[0] !== first) {
    console.log("identity-broken-at-" + j);
  }
}
console.log("identity-ok");

// Threshold trip: cumulative walk > 2 × cached_length triggers
// force-materialize. With cached_length=8, we need walks > 16.
// Random accesses each cost ~n/2 = 4 walks, so after ~5 random
// accesses the threshold trips. Subsequent stringify still works.
const f = JSON.parse(blob);
let tripSum = 0;
for (let k = 0; k < 10; k++) {
  tripSum += f[(k * 3) % f.length].v;
}
console.log("trip-sum:" + tripSum);
console.log("trip-stringify-len:" + JSON.stringify(f).length);
