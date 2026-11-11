import * as path from "node:path";

console.log("base over name/ext:", path.format({ dir: "/tmp", base: "base.txt", name: "name", ext: ".js" }));
console.log("dir over root:", path.format({ root: "/", dir: "/tmp", base: "file.txt" }));
console.log("ext without dot:", path.format({ dir: "/tmp", name: "file", ext: "txt" }));
