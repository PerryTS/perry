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
