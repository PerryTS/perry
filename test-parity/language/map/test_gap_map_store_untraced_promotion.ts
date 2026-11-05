// Gap test (#12141): a Map that survives an in-place promotion of the young
// generation must keep its storage. Each round replaces the module-level Map
// with a new one of 160k object keys and drops the previous cohort. A copying
// minor that promoted the young blocks without tracing them used to free the
// live Map's storage, and the next `set` crashed with SIGSEGV.
// Run: node --experimental-strip-types test_gap_map_store_untraced_promotion.ts

let activeMap = new Map<object, number>();
let checksum = 0;
for (let round = 0; round < 4; round++) {
  activeMap = new Map<object, number>();
  const probes: object[] = [];
  for (let i = 0; i < 160000; i++) {
    const key = { id: i };
    activeMap.set(key, i + round);
    if (i % 4096 === 0) probes.push(key);
  }
  for (const key of probes) checksum = (checksum + activeMap.get(key)!) % 1000000007;
  checksum = (checksum + activeMap.size) % 1000000007;
  console.log("round", round, "size", activeMap.size);
}
let iterated = 0;
for (const [key, value] of activeMap) {
  iterated = (iterated + value + (key as { id: number }).id) % 1000000007;
}
console.log("checksum", checksum, "iterated", iterated);
