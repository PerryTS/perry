// memoryUsage() reports the arrayBuffers field (off-heap ArrayBuffer bytes).
console.log("arrayBuffers:", typeof process.memoryUsage().arrayBuffers === "number");
