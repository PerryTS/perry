// process.argv is a non-empty array of strings (argv[0] is the executable;
// its value differs between `node` and the compiled binary, so only the
// shape is asserted).
console.log("is array:", Array.isArray(process.argv));
console.log("non-empty:", process.argv.length >= 1);
console.log("argv0 is string:", typeof process.argv[0] === "string");
console.log("all strings:", process.argv.every((a) => typeof a === "string"));
