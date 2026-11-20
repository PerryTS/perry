// process.exitCode round-trips integer values via set/get.
process.exitCode = 5;
const v = process.exitCode;
process.exitCode = 0;
console.log("round-trip:", v === 5);
