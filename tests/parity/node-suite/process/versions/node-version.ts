// process.versions.node is a string (the value differs between runtimes).
console.log("node is string:", typeof process.versions.node === "string");
console.log("non-empty:", process.versions.node.length > 0);
