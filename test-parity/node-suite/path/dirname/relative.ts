import * as path from "node:path";

console.log("dirname one segment:", path.dirname("foo"));
console.log("dirname dot child:", path.dirname("./foo"));
console.log("dirname dotdot child:", path.dirname("../foo"));
console.log("dirname nested dotdot:", path.dirname("foo/../bar"));
