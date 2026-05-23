// process.memoryUsage() returns { rss, heapTotal, heapUsed, external,
// arrayBuffers } — all numbers (values are non-deterministic).
const m = process.memoryUsage();
console.log("rss:", typeof m.rss === "number");
console.log("heapTotal:", typeof m.heapTotal === "number");
console.log("heapUsed:", typeof m.heapUsed === "number");
console.log("external:", typeof m.external === "number");
