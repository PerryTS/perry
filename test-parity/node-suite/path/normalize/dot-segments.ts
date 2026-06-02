import * as path from "node:path";

console.log("normalize dot:", path.normalize("./foo/./bar"));
console.log("normalize leading dotdot:", path.normalize("../foo/bar"));
console.log("normalize collapses above relative:", path.normalize("foo/../../bar"));
