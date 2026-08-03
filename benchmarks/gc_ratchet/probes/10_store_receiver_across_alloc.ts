// GC ratchet probe: the store receiver held across an allocating RHS.
//
// `a[i] = v` evaluates the receiver first and the value last — spec order — so
// the receiver sits in an SSA register while the RHS runs. When the RHS
// allocates, an evacuating minor can relocate the array underneath it: the slot
// the register was loaded from is a registered root and evacuation rewrites it,
// but the register is not, so the store lands in retired from-space.
//
// THREE THINGS MAKE THIS PROBE BITE, and dropping any one of them makes it
// silently measure nothing:
//
//  1. The array is a MODULE-LEVEL binding. A local array lives in a shadow slot
//     the collector rewrites, and the bug does not reproduce.
//  2. The store happens INSIDE A FUNCTION. The identical loop written at top
//     level is clean.
//  3. The RHS ALLOCATES. `sink[i] = i` cannot collect, so codegen correctly
//     emits no rooting at all and there is no window.
//
// This is NOT observable from output alone: evacuation copies rather than
// zeroes, so the stale address still holds the old bytes and the program prints
// the right answer. It takes `PERRY_GC_PROTECT_FROMSPACE=1` — which unmaps
// retired from-space — to turn the latent stale access into a fault. Run it as
// the other probes are run, plus:
//
//     PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=64
//
// Regression probe for the array-store receiver fix; found via #7341.
//
// Metric contract: stdout carries only `probe:`/`checksum:` lines and is diffed
// byte-for-byte against the pinned Node oracle. Retention metrics go to stderr
// as `#gcmetric` lines and are Perry-vs-Perry only.

declare function gc(): void;

const SLOTS = 1024;
const ITERATIONS = 200000;

// (1) module-level, so the receiver is loaded from a global handle rather than
// a shadow slot.
const sink: ({ a: number; b: string } | null)[] = new Array(SLOTS);
for (let i = 0; i < SLOTS; i++) {
  sink[i] = null;
}

let checksum = 0;

// (2) inside a function, and (3) an allocating RHS.
function churn(n: number): void {
  for (let i = 0; i < n; i++) {
    sink[i & (SLOTS - 1)] = { a: i, b: "s" + (i & 7) };
  }
}

churn(ITERATIONS);

// Read every slot back so a store that landed in abandoned memory shows up as a
// missing or wrong entry rather than being quietly dropped.
for (let i = 0; i < SLOTS; i++) {
  const entry = sink[i];
  if (entry !== null) {
    checksum = (checksum + entry.a + entry.b.length) | 0;
  }
}

for (let i = 0; i < SLOTS; i++) {
  sink[i] = null;
}

gc();
const mu = process.memoryUsage();

console.log("probe:10_store_receiver_across_alloc");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
