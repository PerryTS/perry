// argv0 / execPath / title are string properties (values differ between
// runtimes, so only the type is asserted).
console.log("argv0 is string:", typeof process.argv0 === "string");
console.log("execPath is string:", typeof process.execPath === "string");
console.log("title is string:", typeof process.title === "string");
