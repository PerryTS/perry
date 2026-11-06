import * as path from "node:path";

console.log("normalize leading double:", path.normalize("//foo/bar"));
console.log("normalize trailing slash:", path.normalize("foo/bar/"));
console.log("normalize empty:", path.normalize(""));
