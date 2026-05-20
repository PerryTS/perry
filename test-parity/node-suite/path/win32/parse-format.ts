import * as path from "node:path";

const parsed = path.win32.parse("C:\\home\\user\\file.txt");
console.log("win32 parse:", parsed.root, parsed.dir, parsed.base, parsed.name, parsed.ext);
console.log("win32 format:", path.win32.format({ dir: "C:\\home\\user", name: "file", ext: ".txt" }));
