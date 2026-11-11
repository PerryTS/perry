import path from "node:path";

console.log("default basename:", path.basename("/foo/bar.txt"));
console.log("default join:", path.join("/foo", "bar"));
console.log("default sep:", path.sep);
