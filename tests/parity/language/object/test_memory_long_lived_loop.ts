// Memory-leak regression: 200,000 iterations of allocate-and-discard.
// Each iteration creates an object + array + string and immediately
// drops every reference. Total bytes allocated: many GB. RSS must
// plateau, NOT grow linearly with iteration count.
//
// Catches regressions where:
//   - GC threshold doesn't keep up with allocation rate (RSS climbs)
//   - Block-persist cascade pins dead working-set blocks
//   - PARSE_KEY_CACHE / shape-cache interns leak
//   - Tenured objects accumulate without compaction (Phase C4 risk)

function makeRecord(i: number): { id: number; name: string; tags: string[] } {
  return {
    id: i,
    name: "record_" + i,
    tags: ["tag_a_" + i, "tag_b_" + i, "tag_c_" + i, "tag_d_" + i],
  };
}

let lastId = 0;
let lastTagLen = 0;
for (let i = 0; i < 200_000; i++) {
  const r = makeRecord(i);
  // Touch fields so the optimizer can't elide the allocation.
  lastId = r.id;
  lastTagLen = r.tags.length;
}
console.log("done, lastId=" + lastId + " lastTagLen=" + lastTagLen);
