// #11616: getBuiltinModule reached through an alias or optional call.
const proc = globalThis.process;
const path = proc.getBuiltinModule("node:path");
console.log("alias path.join:", path.join("a", "b"));
const os = globalThis.process?.getBuiltinModule?.("node:os");
console.log("optional os.EOL:", JSON.stringify(os?.EOL));
const fs = proc.getBuiltinModule("node:fs");
console.log("alias fs.existsSync:", fs.existsSync("/"));
const util = proc.getBuiltinModule("node:util");
console.log("alias util.format:", util.format("%s-%d", "x", 1));
