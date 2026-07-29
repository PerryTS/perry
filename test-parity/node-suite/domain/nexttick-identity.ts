const original = process.nextTick;
await import("node:domain");
console.log(process.nextTick === original);
