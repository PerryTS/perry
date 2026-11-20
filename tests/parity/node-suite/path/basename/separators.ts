import * as path from "node:path";

console.log("only slash:", path.basename("/"));
console.log("double slash:", path.basename("//"));
console.log("nested trailing slash:", path.basename("/foo/bar/baz//"));
console.log("dot segment:", path.basename("/foo/./bar"));
console.log("dotdot segment:", path.basename("/foo/../bar"));
