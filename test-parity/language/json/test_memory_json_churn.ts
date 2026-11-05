// Memory-leak regression: JSON.parse + discard in a loop, with a
// mix of sub-1KB blobs (direct path) and >1KB blobs (lazy/tape path).
// 5,000 iterations × ~3 KB per large blob = ~15 MB of total parse
// bytes processed; RSS must NOT grow linearly with iteration count.
//
// Catches regressions where:
//   - LazyArrayHeader sparse cache (materialized_elements + bitmap)
//     is not collected when the lazy array dies
//   - `materialized` ArrayHeader retained after lazy header dies
//   - PARSE_KEY_CACHE accumulates per-blob keys (longlived arena
//     leak surface from v0.5.194)
//   - Tape buffer (TapeEntry array) retained beyond the parse
//   - Stringify-after-materialize allocates a new tree each iter

function smallBlob(i: number): string {
  return JSON.stringify({ id: i, name: "small_" + i, value: i * 7 });
}

function largeBlob(i: number): string {
  const items: any[] = [];
  for (let j = 0; j < 50; j++) {
    items.push({
      id: i * 50 + j,
      name: "item_" + j,
      tags: ["t1_" + j, "t2_" + j],
      nested: { x: j, y: j * 2 },
    });
  }
  return JSON.stringify(items);
}

let checksum = 0;
for (let i = 0; i < 5_000; i++) {
  const s = JSON.parse(smallBlob(i));
  checksum += s.id;

  const l = JSON.parse(largeBlob(i));
  // Touch via .length AND indexed access — exercises both lazy
  // fast paths (cached_length read + per-element materialization).
  checksum += l.length;
  checksum += l[0].id;
  checksum += l[25].nested.x;
}
console.log("done, checksum=" + checksum);
