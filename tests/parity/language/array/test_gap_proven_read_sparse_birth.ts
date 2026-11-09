// A fresh `new Array(k)` longer than the runtime's dense bound (1,000,000)
// records length `k` over an empty store. Its length never moves, but an
// element read of it must not address slot `i` of the store as if it held
// `k` slots: holes past the store read `undefined`, writes give it slots.

const big = new Array(2_000_000);
let defined = 0;
for (let i = 0; i < big.length; i++) {
  if (big[i] !== undefined) defined++;
}
console.log("module holes", defined, big.length);

function local(): string {
  const a = new Array(1_500_000);
  let seen = 0;
  for (let i = 0; i < a.length; i += 1000) {
    if (a[i] === undefined) seen++;
  }
  return `local holes ${seen} ${a.length} ${a[1_499_999]}`;
}
console.log(local());

const filled = new Array(1_000_001).fill(3);
let sum = 0;
for (let i = 0; i < filled.length; i++) sum += filled[i];
console.log("filled", sum, filled[1_000_000]);

const written = new Array(1_200_000);
for (let i = 0; i < written.length; i += 7) written[i] = i & 15;
let acc = 0;
let holes = 0;
for (let i = 0; i < written.length; i++) {
  const v = written[i];
  if (v === undefined) holes++;
  else acc += v;
}
console.log("written", acc, holes, written[1_199_996], written[1_199_999]);

// At the dense bound itself the store holds every slot.
const edge = new Array(1_000_000);
edge[999_999] = 42;
let edgeDefined = 0;
for (let i = 0; i < edge.length; i++) if (edge[i] !== undefined) edgeDefined++;
console.log("edge", edgeDefined, edge[999_999], edge[0]);
