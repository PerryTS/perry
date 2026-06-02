import * as path from "node:path";

const parsed = path.posix.parse("/home/user/file.txt");
console.log("posix parse:", parsed.root, parsed.dir, parsed.base, parsed.name, parsed.ext);
console.log("posix format:", path.posix.format({ dir: "/home/user", name: "file", ext: ".txt" }));
