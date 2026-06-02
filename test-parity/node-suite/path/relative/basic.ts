import * as path from "node:path";

console.log("same:", path.relative("/foo", "/foo"));
console.log("sibling:", path.relative("/foo/a", "/foo/b"));
console.log("deeper:", path.relative("/foo", "/foo/bar/baz"));
