// SNAPCUT2 F7 stale-target: the entry body leaves dead objects before its first await; b allocates into their cells.
// A pointer to such a cell is a pointer to b's object, not to an entry object: no P-entry (c) row.
for (let i = 0; i < 200000; i++) { const x = { k: i }; }
await Promise.resolve();
await import("./b.ts");
gc();
console.log("REC_F7");
