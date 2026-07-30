// GC ratchet probe: dead objects under a stack high-water mark.
//
// This is the specific case a conservative stack scan is expected to get wrong.
// A deep recursion allocates in every frame and then unwinds completely; the
// machine stack below the current stack pointer still holds the old frames' bit
// patterns. Precise shadow-stack roots ignore that region entirely. A
// conservative scan that reads it treats those stale words as roots and keeps
// the whole recursion's garbage alive.
//
// Retained bytes here are the most sensitive signal in the suite for the
// shadow-stack removal campaign. `sink` forces the per-frame batch onto the
// heap so it cannot be scalar-replaced; only the last batch stays reachable
// after the unwind, so everything else is unambiguously garbage.

declare function gc(): void;

const DEPTH = 700;
const PER_FRAME = 48;
const REPEATS = 40;

let sink: { d: number; pad: number }[] | null = null;

function descend(depth: number): number {
  const scratch: { d: number; pad: number }[] = [];
  for (let i = 0; i < PER_FRAME; i++) {
    scratch.push({ d: depth, pad: i });
  }
  sink = scratch;
  let local = scratch[PER_FRAME - 1].d;
  if (depth > 0) {
    local = (local + descend(depth - 1)) | 0;
  }
  return local;
}

let checksum = 0;
for (let r = 0; r < REPEATS; r++) {
  checksum = (checksum + descend(DEPTH)) | 0;
}

sink = null;

// Everything allocated above is unreachable, but the frames that held it are
// still resident on the machine stack as stale words.
gc();
const mu = process.memoryUsage();

console.log("probe:04_dead_after_deep_stack");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
