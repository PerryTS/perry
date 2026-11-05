import * as path from "node:path";

console.log("dot cwd:", path.resolve(".") === process.cwd());
console.log("dot child:", path.resolve(".", "foo").endsWith(path.join("foo")));
console.log("dotdot suffix:", path.resolve("foo", "..").endsWith(path.basename(process.cwd())));
