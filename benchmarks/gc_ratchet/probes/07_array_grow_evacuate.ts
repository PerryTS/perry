// GC ratchet probe: array element storage growth and reallocation.
//
// A growing array replaces its inline header-plus-elements allocation and
// leaves a forwarding stub at the old address. Evacuation must repair holders
// of the current allocation. The probe grows many arrays past
// several reallocation boundaries, keeps a small live sample, cycles the rest
// through a ring so they are genuinely heap-allocated, then drops them.
// #9790: copied_bytes/freed_bytes remain informational on this probe because
// block placement changes when nursery pressure is checked. See the ratchet
// README and evidence/9790-array-growth-pacing.json; all other gates remain.

declare function gc(): void;

const ARRAYS = 6000;
const LENGTH = 640;
const KEEP_EVERY = 64;
const RING = 32;

const survivors: number[][] = [];
const ring: (number[] | null)[] = [];
for (let i = 0; i < RING; i++) {
  ring.push(null);
}

function grow(seed: number): number {
  const a: number[] = [];
  for (let i = 0; i < LENGTH; i++) {
    a.push((seed + i) | 0);
  }
  if (seed % KEEP_EVERY === 0) {
    survivors.push(a);
  } else {
    ring[seed % RING] = a;
  }
  return a[LENGTH - 1];
}

let checksum = 0;
for (let i = 0; i < ARRAYS; i++) {
  checksum = (checksum + grow(i)) | 0;
}
for (let i = 0; i < RING; i++) {
  ring[i] = null;
}
for (let i = 0; i < survivors.length; i++) {
  checksum = (checksum + survivors[i].length) | 0;
}

gc();
const mu = process.memoryUsage();

console.log("probe:07_array_grow_evacuate");
console.log("checksum:" + checksum);
console.log("survivors:" + survivors.length);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
