// JSON.stringify with a replacer must surface sparse-array HOLES to the
// replacer (and toJSON) as `undefined` — per spec, SerializeJSONArray does
// Get(holder, index), and a missing index yields undefined. Perry's replacer
// walk read the raw element slot, so a hole leaked its internal sentinel — an
// unrecognized quiet-NaN bit pattern that user code observed as a NaN number.
//
// react-server-dom's flight encoder branches `typeof v === "number"` (with the
// isFinite/NaN chain) BEFORE its undefined check, so Next.js sparse
// flightRouterState tuples (`seg[4] = flags` on a length-2 array → holes at
// 2,3) serialized as "$NaN" instead of "$undefined" — corrupting the RSC
// payload of every App Router dynamic route (#5989).
//
// Validated byte-for-byte against `node --experimental-strip-types`.

// The flight-encoder shape: typeof-number branch first, then undefined.
function flightReplacer(this: any, k: string, v: any): any {
  if (k === "") return v;
  if (typeof v === "number") return Number.isFinite(v) ? v : "$NaN";
  if (v === undefined) return "$undefined";
  return v;
}

// (1) literal holes
console.log(JSON.stringify([1, , 3], flightReplacer));

// (2) the Next.js sparse-tuple shape: length-2 array extended by index-4 write
const seg: any[] = ["", {}];
seg[1] = { children: ["plain", {}] };
let flags = 0;
flags |= 16;
if (flags !== 0) seg[4] = flags;
console.log(JSON.stringify(seg, flightReplacer));

// (3) replacer receives undefined (not a NaN number) for the hole value param
JSON.stringify([, 7], (k, v) => {
  if (k === "0") console.log(typeof v, v === undefined, typeof v === "number" && Number.isNaN(v));
  return v;
});

// (4) holes + replacer that returns the value unchanged → null in output
console.log(JSON.stringify([, 7], (_k, v) => v));

// (5) pretty-print variant walks the same path
console.log(JSON.stringify([1, , 3], flightReplacer, 1));

// (6) array-of-allowed-keys replacer form over an array with holes
console.log(JSON.stringify({ a: [1, , 3] }, ["a"] as any));

// (7) real NaN still round-trips as "$NaN" through the same replacer
console.log(JSON.stringify([NaN, , 2], flightReplacer));

// (8) objects with >8 properties: overflow fields must reach the replacer
// (field_count caps at the inline slot limit; overflow values live in a side
// table — the walk previously dropped every property past the 8th).
const big: any = {
  p1: 1, p2: 2, p3: 3, p4: 4, p5: 5, p6: 6, p7: 7, p8: 8, p9: 9, p10: 10,
  p11: 11, p12: 12, p13: 13, notFound: undefined, forbidden: undefined, unauthorized: undefined,
};
console.log(JSON.stringify(big, flightReplacer));

// (9) exactly 9 props (first overflow slot), last one undefined-valued
const nine: any = { a1: 1, a2: 2, a3: 3, a4: 4, a5: 5, a6: 6, a7: 7, a8: 8, a9: undefined };
console.log(JSON.stringify(nine, flightReplacer));

// (10) overflow props through the sorted/pretty walk
console.log(JSON.stringify(big, flightReplacer, 1).split("\n").length);

// (11) overflow props through the allowed-keys form
console.log(JSON.stringify(big, ["p9", "p13", "forbidden"] as any));
