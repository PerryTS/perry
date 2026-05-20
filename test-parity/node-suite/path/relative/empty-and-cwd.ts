import * as path from "node:path";

console.log("empty to cwd:", path.relative("", process.cwd()));
console.log("cwd to empty:", path.relative(process.cwd(), ""));
console.log("empty to child:", path.relative("", "foo/bar").endsWith(path.join("foo", "bar")));
