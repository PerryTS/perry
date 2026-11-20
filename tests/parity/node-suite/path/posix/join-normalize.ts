import * as path from "node:path";

console.log("posix join:", path.posix.join("/foo", "bar", "baz"));
console.log("posix join dotdot:", path.posix.join("/foo", "..", "bar"));
console.log("posix normalize:", path.posix.normalize("/foo/bar//baz/.."));
