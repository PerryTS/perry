import * as path from "node:path";

console.log("normalize repeated separators:", path.normalize("/foo/bar//baz"));
console.log("normalize dotdot:", path.normalize("/foo/bar/../baz"));
console.log("normalize relative:", path.normalize("foo/bar//baz"));
