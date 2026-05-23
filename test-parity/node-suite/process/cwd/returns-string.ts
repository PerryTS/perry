// process.cwd() returns the current working directory as an absolute string.
const cwd = process.cwd();
console.log("is string:", typeof cwd === "string");
console.log("non-empty:", cwd.length > 0);
console.log("absolute:", cwd.startsWith("/") || /^[A-Za-z]:\\/.test(cwd));
