import * as path from "node:path";

console.log("dir+base:", path.format({ dir: "/home/user", base: "file.txt" }));
console.log("root+base:", path.format({ root: "/", base: "file.txt" }));
console.log("name+ext:", path.format({ dir: "/home/user", name: "file", ext: ".txt" }));
