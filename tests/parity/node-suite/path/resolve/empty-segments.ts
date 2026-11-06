import * as path from "node:path";

console.log("empty arg cwd:", path.resolve("") === process.cwd());
console.log("empty then child:", path.resolve("", "foo").endsWith(path.join("foo")));
console.log("child then empty:", path.resolve("foo", "").endsWith(path.join("foo")));
