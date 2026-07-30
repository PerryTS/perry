// GC ratchet probe: old-to-young stores and the remembered set.
//
// Builds a long-lived table, ages it into old-gen with explicit collects, then
// overwrites its slots with freshly allocated nursery objects. Every store is
// an old->young edge the write barrier must record, and every overwritten slot
// drops its previous target. Retained bytes after the final collect should
// track the live table only; a barrier or root-scanning change that
// over-retains the replaced generations shows up here first.

declare function gc(): void;

const TABLE_SIZE = 4096;
const ROUNDS = 120;

const table: { v: number; tag: number }[] = [];
for (let i = 0; i < TABLE_SIZE; i++) {
  table.push({ v: i, tag: 0 });
}

// Age the table into old-gen before the write storm.
gc();
gc();

let checksum = 0;
for (let round = 0; round < ROUNDS; round++) {
  for (let i = 0; i < TABLE_SIZE; i++) {
    const fresh = { v: round * TABLE_SIZE + i, tag: round };
    table[i] = fresh;
    checksum = (checksum + fresh.tag) | 0;
  }
}

for (let i = 0; i < TABLE_SIZE; i++) {
  checksum = (checksum + table[i].v) | 0;
}

gc();
const mu = process.memoryUsage();

console.log("probe:03_cross_gen_writes");
console.log("checksum:" + checksum);
console.error("#gcmetric heap_used_bytes=" + mu.heapUsed);
console.error("#gcmetric heap_total_bytes=" + mu.heapTotal);
console.error("#gcmetric rss_bytes=" + mu.rss);
