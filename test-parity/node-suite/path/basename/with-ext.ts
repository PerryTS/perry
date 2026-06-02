import * as path from "node:path";

console.log("strip .txt:", path.basename("/foo/bar/baz.txt", ".txt"));
console.log("strip non-match:", path.basename("/foo/bar/baz.txt", ".js"));
console.log("strip whole base:", path.basename("/foo/bar/baz", "baz"));
