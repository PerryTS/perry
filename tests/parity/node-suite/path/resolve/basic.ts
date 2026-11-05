import * as path from "node:path";

console.log("resolve abs plus rel:", path.resolve("/foo", "bar"));
console.log("resolve dot:", path.resolve("/foo", ".", "bar"));
console.log("resolve dotdot:", path.resolve("/foo/bar", "..", "baz"));
