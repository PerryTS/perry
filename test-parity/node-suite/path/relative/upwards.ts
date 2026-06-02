import * as path from "node:path";

console.log("parent:", path.relative("/foo/bar", "/foo"));
console.log("cousin:", path.relative("/foo/bar/baz", "/foo/qux"));
console.log("root to nested:", path.relative("/", "/foo/bar"));
