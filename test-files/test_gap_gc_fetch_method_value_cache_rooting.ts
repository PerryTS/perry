// Census follow-up (gc_runtime_root_holders ffi/ext coverage): the fetch
// bound-method value caches — HEADERS_METHOD_VALUE_CACHE
// (crates/perry-stdlib/src/fetch/headers_method_value.rs) and
// FORM_DATA_METHOD_VALUE_CACHE (fetch/dispatch.rs) — park a NaN-boxed
// bound-method ClosureHeader per (handle, method) pair so
// `h.entries === h.entries` holds. Before fetch/gc.rs existed, that cache
// was the closure's ONLY holder once the first property read's transient
// died: the one-shot js_write_barrier_root_nanbox at the insert site only
// shades a mark cycle already in flight, so a later full sweep freed the
// closure and a copying minor moved it while the cache kept the OLD bits.
// The second property read then returned a dangling function value.
//
// The reads below go through the cache twice per object, with an
// allocating loop between them so a seeded schedule has back-edge polls to
// fire on. Run as
//
//   PERRY_GC_SCHEDULE_RATE=1 PERRY_GC_SCHEDULE_SEED=7 \
//   PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_VERIFY_EVACUATION=1 \
//   PERRY_GC_DIAG=1 ./out
//
// (`PERRY_GC_SCHEDULE_SEED` implies forced evacuation.) A green run only
// counts if a `[gc-fromspace-protect] retired_set=#N` line appeared and the
// exit verdict reports copying_minors/moved_objects above zero. Pre-fix the
// second `typeof` dereferences the poisoned from-space copy and dies;
// post-fix the cache slot is a registered root, rewritten on evacuation.

function churn(): number {
  let keep: Array<{ i: number; s: string }> = [];
  for (let i = 0; i < 30000; i++) {
    keep.push({ i, s: "pad-" + i });
    if (keep.length > 64) keep = [];
  }
  return keep.length;
}

const h = new Headers();
h.set("a", "1");
h.append("b", "2");

// First reads populate the cache; their transient results die immediately.
console.log("headers first:", typeof h.get, typeof h.entries);
churn();
// Second reads are CACHE HITS — pre-fix these bits dangle.
console.log("headers second:", typeof h.get, typeof h.entries);
const collected: string[] = [];
for (const [k, v] of h.entries()) {
  collected.push(k + "=" + v);
}
console.log("headers entries:", collected.join(","));
console.log("headers has:", h.has("a"), h.get("b"));

const fd = new FormData();
fd.append("k", "v");
fd.append("k2", "v2");
console.log("formdata first:", typeof fd.get, typeof fd.has);
churn();
console.log("formdata second:", typeof fd.get, typeof fd.has);
console.log("formdata get:", fd.get("k"), fd.get("k2"), fd.has("nope"));
