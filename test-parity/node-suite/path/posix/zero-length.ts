import * as path from "node:path";

console.log("posix basename empty:", path.posix.basename(""));
console.log("posix dirname empty:", path.posix.dirname(""));
console.log("posix normalize empty:", path.posix.normalize(""));
console.log("posix join empty:", path.posix.join(""));
console.log("posix resolve empty equals cwd:", path.posix.resolve("") === process.cwd());
