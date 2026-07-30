// GC ratchet probe: survivor aging and promotion into old-gen.
//
// Keeps one allocation in eight reachable from a module-level array while the
// rest cycle through a ring and die, so nursery survivors repeatedly clear the
// two-bit aging gate (HAS_SURVIVED -> TENURED) and get evacuated into
// OLD_ARENA. Post-collect `heapUsed` measures how compactly the surviving set
// ends up stored: pinning a tenured object in place — the expected consequence
// of dropping precise roots in favour of a conservative scan — shows up as
// retained bytes that no longer shrink.

declare function gc(): void;

const ITERATIONS = 320000;
const KEEP_EVERY = 8;
const RING = 128;

const live: { a: number; b: number }[] = [];
const ring: ({ a: number; b: number } | null)[] = [];
for (let i = 0; i < RING; i++) {
  ring.push(null);
}

let checksum = 0;
for (let i = 0; i < ITERATIONS; i++) {
  const o = { a: i, b: i * 3 };
  checksum = (checksum + o.a) | 0;
  if (i % KEEP_EVERY === 0) {
    live.push(o);
  } else {
    ring[i % RING] = o;
  }
}

for (let i = 0; i < RING; i++) {
  ring[i] = null;
}
for (let i = 0; i < live.length; i++) {
  checksum = (checksum + live[i].b) | 0;
}

gc();
const mu = process.memoryUsage();

console.log("probe:02_survivor_promotion");
console.log("checksum:" + checksum);
console.log("live:" + live.length);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
