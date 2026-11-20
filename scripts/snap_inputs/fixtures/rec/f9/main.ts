// The entry sets two globals, then a dependency allocates heavily and keeps one entry object in an array.
// Held run: exactly one hold row is held by the last module. The same program run without the collection hold reports no hold rows.
(globalThis as any).x = { x: Math.random() };
(globalThis as any).e = { k: 1 };
await Promise.resolve();
await import("./m.ts");
(globalThis as any).e.k = 2;
gc();
console.log("REC_F9");
