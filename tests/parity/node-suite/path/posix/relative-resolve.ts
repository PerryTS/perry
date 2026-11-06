import * as path from "node:path";

console.log("posix relative:", path.posix.relative("/foo/a", "/foo/b"));
console.log("posix resolve:", path.posix.resolve("/foo", "bar"));
console.log("posix isAbsolute:", path.posix.isAbsolute("/foo"));
