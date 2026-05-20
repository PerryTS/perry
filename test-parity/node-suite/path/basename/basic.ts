import * as path from "node:path";

console.log("basename /foo/bar/baz.txt:", path.basename("/foo/bar/baz.txt"));
console.log("basename /foo/bar:", path.basename("/foo/bar"));
console.log("basename relative:", path.basename("foo/bar/baz"));
