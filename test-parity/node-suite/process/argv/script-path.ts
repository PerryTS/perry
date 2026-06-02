// process.argv has at least [exec, script]; argv[1] is a string (its value
// differs between `node script.ts` and the compiled binary).
console.log("length >= 2:", process.argv.length >= 2);
console.log("argv[1] is string:", typeof process.argv[1] === "string");
