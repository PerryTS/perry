// process.availableMemory() / constrainedMemory() return numbers.
console.log("availableMemory:", typeof process.availableMemory() === "number");
console.log("constrainedMemory:", typeof process.constrainedMemory() === "number");
