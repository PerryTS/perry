import * as path from "node:path";

console.log("same relative:", path.relative("foo/bar", "foo/bar"));
console.log("relative sibling:", path.relative("foo/a", "foo/b"));
console.log("relative deeper:", path.relative("foo", "foo/bar/baz"));
