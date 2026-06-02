import * as path from "node:path";

console.log("abs to rel suffix:", path.relative("/foo", "bar").endsWith("bar"));
console.log("rel to abs suffix:", path.relative("foo", "/bar").endsWith("bar"));
console.log("dot to child:", path.relative(".", "foo/bar"));
