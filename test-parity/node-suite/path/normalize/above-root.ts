import * as path from "node:path";

console.log("absolute above root:", path.normalize("/foo/../../bar"));
console.log("relative above root:", path.normalize("foo/../../bar"));
console.log("many dotdot relative:", path.normalize("../../foo"));
