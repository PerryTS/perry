// GC ratchet probe: nursery churn with a zero live set.
//
// Allocates a large number of objects that escape into a small ring buffer and
// are then dropped, so at `gc()` time every one of them is unreachable.
// Post-collect `heapUsed` is a direct measure of *false retention*: precise
// shadow-stack roots leave only the fixed runtime floor behind, while a
// conservative stack scan can pin arbitrary dead objects that stale words in
// the machine stack still point at.
//
// The ring is load-bearing. Allocating into a local that never escapes lets
// LLVM scalar-replace the object away entirely: an earlier draft of this probe
// ran in 10 ms, triggered zero collections, and reported 600 retained bytes —
// a benchmark that measured nothing. Every probe in this suite therefore parks
// its allocations in a heap container before dropping them.
//
// Metric contract: stdout carries only `probe:`/`checksum:` lines and is diffed
// byte-for-byte against the pinned Node oracle. Retention metrics go to stderr
// as `#gcmetric` lines and are Perry-vs-Perry only.

declare function gc(): void;

const ITERATIONS = 400000;
const RING = 256;

const ring: ({ a: number; b: number; c: number } | null)[] = [];
for (let i = 0; i < RING; i++) {
  ring.push(null);
}

let checksum = 0;
for (let i = 0; i < ITERATIONS; i++) {
  const o = { a: i, b: i * 2, c: (i & 1023) + 1 };
  ring[i % RING] = o;
  checksum = (checksum + o.a + o.c) | 0;
}

for (let i = 0; i < RING; i++) {
  ring[i] = null;
}

gc();
const mu = process.memoryUsage();

console.log("probe:01_nursery_churn");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
