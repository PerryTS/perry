import * as path from "node:path";

console.log("join double slash:", path.join("/foo//", "bar"));
console.log("join dotdot:", path.join("/foo", "..", "bar"));
console.log("join dot:", path.join("/foo", ".", "bar"));
