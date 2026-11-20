import * as path from "node:path";

console.log("dot relative:", path.isAbsolute("./foo"));
console.log("dotdot relative:", path.isAbsolute("../foo"));
console.log("absolute dot:", path.isAbsolute("/foo/./bar"));
console.log("absolute dotdot:", path.isAbsolute("/foo/../bar"));
