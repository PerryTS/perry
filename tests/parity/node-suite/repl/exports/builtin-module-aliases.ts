import repl from "node:repl";

console.log(Array.isArray(repl.builtinModules));
console.log(Array.isArray((repl as any)._builtinLibs));
console.log(repl.builtinModules === (repl as any)._builtinLibs);
console.log(repl.builtinModules.includes("fs"));
console.log(repl.builtinModules.includes("node:fs"));
