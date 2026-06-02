import * as path from "node:path";

const parsed = path.parse("/home/user/file.txt");
console.log("root:", parsed.root);
console.log("dir:", parsed.dir);
console.log("base:", parsed.base);
console.log("name:", parsed.name);
console.log("ext:", parsed.ext);
