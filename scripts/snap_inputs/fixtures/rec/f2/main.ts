// SNAPREC2 F2: the import() runs from a promise job; a (a dependency of sub) queues a job that is pending at b's init.
// Node prints: entry, b, job a.
await Promise.resolve();
console.log("entry");
await import("./sub.ts");
gc();
