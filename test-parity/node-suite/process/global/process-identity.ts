// The `process` global is the same object as globalThis.process.
console.log("same object:", (globalThis as any).process === process);
console.log("typeof:", typeof (globalThis as any).process);
