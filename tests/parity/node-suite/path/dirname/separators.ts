import * as path from "node:path";

console.log("double slash:", path.dirname("//"));
console.log("many slash:", path.dirname("////"));
console.log("trailing file slash:", path.dirname("/foo/bar/"));
console.log("relative trailing:", path.dirname("foo/bar/"));
console.log("dotdot:", path.dirname("../foo/bar"));
