// process.getBuiltinModule(id) loads a builtin without an import (Node 22.3+).
console.log("is function:", typeof process.getBuiltinModule === "function");
