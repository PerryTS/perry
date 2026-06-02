import * as path from "node:path";

const cwd = process.cwd();
console.log("cwd basename:", path.basename(cwd));
console.log("resolve relative suffix:", path.resolve("foo", "bar").endsWith(path.join("foo", "bar")));
console.log("resolve empty is cwd:", path.resolve() === cwd);
