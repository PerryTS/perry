import * as path from "node:path";

console.log("many args:", path.join("/foo", "bar", "baz", "qux"));
console.log("many with empty:", path.join("", "/foo", "", "bar", ""));
console.log("many dotdot:", path.join("/foo", "bar", "..", "baz", "."));
