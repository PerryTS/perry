import * as path from "node:path";

console.log("dirname file:", path.dirname("/foo/bar/baz.txt"));
console.log("dirname dir:", path.dirname("/foo/bar/baz"));
console.log("dirname relative:", path.dirname("foo/bar/baz"));
