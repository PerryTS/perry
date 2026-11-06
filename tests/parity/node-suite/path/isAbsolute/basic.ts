import * as path from "node:path";

console.log("slash:", path.isAbsolute("/"));
console.log("absolute path:", path.isAbsolute("/foo/bar"));
console.log("relative path:", path.isAbsolute("foo/bar"));
console.log("empty:", path.isAbsolute(""));
