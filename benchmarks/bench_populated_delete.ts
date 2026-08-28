// Benchmark: delete/re-add churn on a POPULATED object — the cache/dictionary
// pattern (`delete obj[k]; obj[k] = v` with k rotating over resident keys).
//
// Measured 2026-08-29 (16-core Linux host, node v26.8.1, N = 200_000,
// 500 resident keys):
//
//   node        37 ms
//   perry   ~8_000 ms        (~200x)
//
// This is EIGHT TIMES worse than the 0<->1-key delete oscillation
// (`bench_dynamic_property_keys`' delete loop, ~25x after #8936), because the
// churn cost scales with the RESIDENT key count: every iteration pays a
// 500-element keys-array clone on the delete, a second clone on the re-add
// (appending to a shared array), two layout rebuilds, two shape-descriptor
// mints (with `ShapeFacts` hashing and reverse-index maintenance), and a
// 500-slot value shift. O(resident keys) per delete, with large constants.
//
// A delete-transition memo (V8-style back-transitions keyed on the keys
// array's ADDRESS) was built and measured against this benchmark: it was a
// WASH, twice, in drift-cancelling interleaved A/B pairs — with rotating keys
// each delete+re-add changes the shape, so the next delete is a fresh
// (shape, key) pair and address-keyed memoisation never converges. It was
// deleted rather than shipped. The structural fix needs a content-stable
// shape identity (facts independent of the keys array's address), so that
// equal key-sets reuse one canonical shape regardless of which clone produced
// them — that is a shape-interning change, not a cache.
const o: Record<string, number> = {};
for (let i = 0; i < 500; i++) o["k" + i] = i;
let s = 0;
const N = 200000;
const t = Date.now();
for (let i = 0; i < N; i++) {
  const k = "k" + (i % 500);
  delete o[k];
  o[k] = i;
  s += o[k];
}
console.log("popdel_ms=" + (Date.now() - t) + " chk=" + (s % 7));
