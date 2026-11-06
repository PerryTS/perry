import * as path from "node:path";

console.log("no ext file:", path.extname("README"));
console.log("no ext path:", path.extname("/foo/bar/baz"));
console.log("empty:", path.extname(""));
