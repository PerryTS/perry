// process.release is an object whose `name` identifies the runtime.
console.log("is object:", typeof process.release === "object" && process.release !== null);
console.log("name is string:", typeof process.release.name === "string");
