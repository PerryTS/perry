import * as path from "node:path";

console.log("join abs:", path.join("/foo", "bar", "baz"));
console.log("join rel:", path.join("foo", "bar", "baz"));
console.log("join single:", path.join("foo"));
