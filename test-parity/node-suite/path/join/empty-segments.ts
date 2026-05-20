import * as path from "node:path";

console.log("join empty call:", path.join());
console.log("join empty strings:", path.join("", ""));
console.log("join skips empty:", path.join("foo", "", "bar"));
