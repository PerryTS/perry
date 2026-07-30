// GC ratchet probe: closure environments and captured cells.
//
// Closure conversion boxes captured locals into heap cells, and the
// async-to-generator transform boxes every body local into a shared mutable
// cell. Those cells are a distinct allocation family from plain objects. This
// probe creates batches of closures over per-iteration bindings, invokes them,
// parks the batch in a module-level slot so it cannot be optimised away, then
// drops it so the environments become garbage.

declare function gc(): void;

const BATCHES = 700;
const PER_BATCH = 512;

let sink: (() => number)[] | null = null;

function makeBatch(seed: number): number {
  const fns: (() => number)[] = [];
  for (let i = 0; i < PER_BATCH; i++) {
    const captured = { base: seed + i, extra: i * 2 };
    fns.push(() => (captured.base + captured.extra) | 0);
  }
  sink = fns;
  let sum = 0;
  for (let i = 0; i < fns.length; i++) {
    sum = (sum + fns[i]()) | 0;
  }
  return sum;
}

let checksum = 0;
for (let b = 0; b < BATCHES; b++) {
  checksum = (checksum + makeBatch(b)) | 0;
}

sink = null;

gc();
const mu = process.memoryUsage();

console.log("probe:05_closure_capture");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
