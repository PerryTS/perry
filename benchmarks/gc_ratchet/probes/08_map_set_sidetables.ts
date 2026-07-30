// GC ratchet probe: Map/Set side tables with object keys and values.
//
// Map and Set keep separately allocated storage the arena does not own, reached
// through registered side-table scanners rather than the object walk. Object
// keys make the entries participate in the reachability graph. The probe churns
// a bounded working set through many generations of entries so most keys and
// values die while the containers themselves stay live, which is the shape that
// breaks if a scanner stops being registered or starts over-retaining.

declare function gc(): void;

const GENERATIONS = 1400;
const WORKING_SET = 1024;

const map = new Map<object, number>();
const set = new Set<object>();

let checksum = 0;
for (let g = 0; g < GENERATIONS; g++) {
  map.clear();
  set.clear();
  for (let i = 0; i < WORKING_SET; i++) {
    const key = { g: g, i: i };
    map.set(key, (g * WORKING_SET + i) | 0);
    set.add(key);
  }
  map.forEach((value: number) => {
    checksum = (checksum + value) | 0;
  });
  checksum = (checksum + set.size + map.size) | 0;
}

map.clear();
set.clear();

gc();
const mu = process.memoryUsage();

console.log("probe:08_map_set_sidetables");
console.log("checksum:" + checksum);
console.log("final_sizes:" + map.size + "," + set.size);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
