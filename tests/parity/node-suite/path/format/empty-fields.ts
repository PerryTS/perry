import * as path from "node:path";

console.log("empty object:", path.format({}));
console.log("only name:", path.format({ name: "file" }));
console.log("only ext:", path.format({ ext: ".txt" }));
