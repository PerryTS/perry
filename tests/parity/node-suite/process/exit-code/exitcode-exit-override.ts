// #6666: an explicit process.exit(0) overrides a previously set nonzero
// process.exitCode (node rc=0).
process.exitCode = 3;
console.log("x");
process.exit(0);
