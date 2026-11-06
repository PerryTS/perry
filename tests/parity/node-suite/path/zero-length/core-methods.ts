import * as path from "node:path";

console.log("basename empty:", path.basename(""));
console.log("dirname empty:", path.dirname(""));
console.log("extname empty:", path.extname(""));
console.log("isAbsolute empty:", path.isAbsolute(""));
console.log("normalize empty:", path.normalize(""));
console.log("join empty:", path.join(""));
console.log("resolve empty equals cwd:", path.resolve("") === process.cwd());
console.log("relative empty empty:", path.relative("", ""));
