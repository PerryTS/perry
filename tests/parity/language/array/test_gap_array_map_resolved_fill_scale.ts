// `Array.prototype.map`'s plain-array fill used to resolve the result
// array's head once per element (avoiding a re-classification of the same
// pointer through `clean_arr_ptr`/`array_numeric_layout`) only for a source
// of at most 64 elements; longer sources fell back to the fully
// re-classifying `note_array_slot`. This fixture pins the fast path across
// that former boundary: sources both under and well over 64 (and over the
// ~2048-element / 16KB born-old allocation threshold), with callbacks
// designed to attack the specific risk of resolving the result header once
// per element instead of proving it fresh every store — a callback that
// allocates (forcing a collection between the resolve and the store), that
// returns non-numeric values (retiring the raw-f64 numeric claim mid-fill),
// that mutates the SOURCE by growing or truncating it out from under the
// still-running loop, and a sparse/holey source (skips must still land at
// the right index in the result).

function range(n: number): number[] {
  const a: number[] = [];
  for (let i = 0; i < n; i++) a.push(i);
  return a;
}

// ---- control: plain numeric map, both sides of the old 64 cap -----------

const small = range(16);
console.log("ctrl-small", JSON.stringify(small.map((v) => v + 1)));

const mid = range(65); // one past the old cap
console.log("ctrl-mid-sum", mid.map((v) => v * 2).reduce((a, b) => a + b, 0));
console.log("ctrl-mid-ends", JSON.stringify([mid.map((v) => v * 2)[0], mid.map((v) => v * 2)[64]]));

const big = range(500);
const bigMapped = big.map((v) => v * 3 + 1);
console.log("ctrl-big", bigMapped.length, bigMapped[0], bigMapped[250], bigMapped[499]);
console.log("ctrl-big-sum", bigMapped.reduce((a, b) => a + b, 0));

// Past the born-old allocation threshold (~2048 elements / 16KB of f64s):
// the RESULT array itself starts life in the old generation.
const huge = range(5000);
const hugeMapped = huge.map((v) => v + 0.25);
console.log(
  "ctrl-huge",
  hugeMapped.length,
  hugeMapped[0],
  hugeMapped[2048],
  hugeMapped[4999],
  hugeMapped.reduce((a, b) => a + b, 0),
);

// ---- non-numeric return kinds: retires the raw-f64 numeric claim --------

const kindsSrc = range(200);
const kindsMapped = kindsSrc.map((v) => {
  if (v % 5 === 0) return `s${v}`;
  if (v % 5 === 1) return { v };
  if (v % 5 === 2) return undefined;
  if (v % 5 === 3) return v % 2 === 0;
  return v * 1.5;
});
console.log(
  "kinds",
  kindsMapped.length,
  typeof kindsMapped[0],
  typeof kindsMapped[1],
  typeof kindsMapped[2],
  kindsMapped[2],
  typeof kindsMapped[3],
  typeof kindsMapped[4],
  kindsMapped[4],
);
console.log("kinds-json", JSON.stringify(kindsMapped));

// -0 / NaN survive the fast path exactly.
const zeroNan = range(100).map((v) => (v === 0 ? -0 : v === 1 ? NaN : v));
console.log("zero-nan", Object.is(zeroNan[0], -0), zeroNan[1] !== zeroNan[1], zeroNan[99]);

// ---- callback allocates heavily: forces collections mid-fill ------------

const allocSrc = range(300);
const allocMapped = allocSrc.map((v) => {
  const junk = new Array(48).fill({ v, pad: [v, v, v] });
  let s = 0;
  for (const j of junk) s += j.v;
  return s + v;
});
console.log("alloc-len", allocMapped.length, allocMapped[0], allocMapped[149], allocMapped[299]);
console.log("alloc-sum", allocMapped.reduce((a, b) => a + b, 0));

// ---- callback pushes to the source mid-fill ------------------------------

const pushSrc = range(120);
const pushMapped = pushSrc.map((v, i) => {
  if (i < 10) pushSrc.push(1000 + i);
  return v;
});
console.log("push-mapped-len", pushMapped.length, JSON.stringify(pushMapped.slice(0, 5)));
console.log("push-mapped-tail", pushMapped[119]);
console.log("push-src-len", pushSrc.length, pushSrc[120], pushSrc[129]);

// ---- callback truncates the source mid-fill ------------------------------

const truncSrc = range(150);
const truncMapped = truncSrc.map((v, i) => {
  if (i === 20) truncSrc.length = 60;
  return v;
});
console.log("trunc-mapped-len", truncMapped.length);
console.log("trunc-mapped-json-head", JSON.stringify(truncMapped.slice(0, 25)));
console.log("trunc-mapped-holes", 100 in truncMapped, 61 in truncMapped, 59 in truncMapped);
console.log("trunc-src-len", truncSrc.length);

// ---- sparse / holey source, well past the old 64-element cap ------------

const holey: number[] = [];
holey.length = 200;
for (let i = 0; i < 200; i++) {
  if (i % 7 !== 0) holey[i] = i;
}
const holeyMapped = holey.map((v) => v * 10);
console.log(
  "holey-len",
  holeyMapped.length,
  0 in holeyMapped,
  7 in holeyMapped,
  14 in holeyMapped,
  1 in holeyMapped,
  holeyMapped[1],
  holeyMapped[199],
);
console.log("holey-json", JSON.stringify(holeyMapped.slice(0, 16)));

// A fully empty-but-long holey source: every index skipped.
const allHoles: number[] = new Array(90);
const allHolesMapped = allHoles.map((v) => v + 1);
console.log("all-holes-len", allHolesMapped.length, JSON.stringify(Object.keys(allHolesMapped)));

// ---- frozen source, past the old cap -------------------------------------

const frozenBig = Object.freeze(range(90));
console.log("frozen-big", JSON.stringify(frozenBig.map((v) => v + 1)).length, frozenBig.map((v) => v + 1)[89]);

// ---- object-identity payloads mixed with numbers, past the old cap ------

const tag = { name: "shared" };
const identitySrc = range(80);
const identityMapped = identitySrc.map((v) => (v === 40 ? tag : v));
console.log("identity", identityMapped[40] === tag, identityMapped[39], identityMapped[41]);
