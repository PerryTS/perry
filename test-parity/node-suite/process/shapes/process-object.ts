// The `process` global is an object with the documented top-level shapes.
console.log("typeof process:", typeof process);
console.log("env is object:", typeof process.env === "object");
console.log("argv is array:", Array.isArray(process.argv));
console.log("versions is object:", typeof process.versions === "object");
console.log("pid is number:", typeof process.pid === "number");
console.log("pid positive:", process.pid > 0);
