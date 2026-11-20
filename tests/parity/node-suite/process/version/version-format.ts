// process.version is a "v"-prefixed string; process.versions is an object.
console.log("version is string:", typeof process.version === "string");
console.log("version starts v:", process.version.startsWith("v"));
console.log("versions is object:", typeof process.versions === "object");
