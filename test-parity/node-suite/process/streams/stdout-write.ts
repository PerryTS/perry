// process.stdout.write(str) writes to stdout and returns a boolean.
const ret = process.stdout.write("written-to-stdout\n");
console.log("returns boolean:", typeof ret === "boolean");
