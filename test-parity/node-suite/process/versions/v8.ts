// process.versions.v8 is a string (the engine version; value differs).
console.log("v8 is string:", typeof process.versions.v8 === "string");
console.log("non-empty:", process.versions.v8.length > 0);
