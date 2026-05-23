// process.exitCode defaults to undefined and is settable.
console.log("default:", process.exitCode === undefined);
process.exitCode = 0;
console.log("after set:", process.exitCode);
