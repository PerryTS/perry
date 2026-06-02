import { fileURLToPath, pathToFileURL } from "node:url";

console.log("file path:", fileURLToPath("file:///foo/bar%20baz.txt"));
console.log("path href:", pathToFileURL("/foo/bar baz.txt").href);
