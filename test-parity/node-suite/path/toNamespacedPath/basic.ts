import * as path from "node:path";

console.log("absolute:", path.toNamespacedPath("/foo/bar"));
console.log("relative:", path.toNamespacedPath("foo/bar"));
console.log("empty:", path.toNamespacedPath(""));
