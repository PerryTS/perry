import * as path from "node:path";

console.log("absolute trailing:", path.normalize("/foo/bar/"));
console.log("relative trailing dot:", path.normalize("foo/bar/."));
console.log("relative trailing dotdot slash:", path.normalize("foo/bar/../"));
