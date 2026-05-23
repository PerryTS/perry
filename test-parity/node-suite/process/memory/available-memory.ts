// process.availableMemory() and process.constrainedMemory() return numbers.
console.log("available is number:", typeof process.availableMemory() === "number");
console.log("constrained is number:", typeof process.constrainedMemory() === "number");
