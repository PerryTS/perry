import * as path from "node:path";

console.log("resolve later abs wins:", path.resolve("/foo", "/bar", "baz"));
console.log("resolve final abs wins:", path.resolve("foo", "/bar"));
console.log("resolve multiple abs:", path.resolve("/a", "/b", "/c"));
