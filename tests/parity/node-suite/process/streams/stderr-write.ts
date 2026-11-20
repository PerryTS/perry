// process.stderr.write(str) returns a boolean (and writes to stderr).
const ret = process.stderr.write("to-stderr\n");
console.log("returns boolean:", typeof ret === "boolean");
