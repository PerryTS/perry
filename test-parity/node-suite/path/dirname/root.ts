import * as path from "node:path";

console.log("dirname root:", path.dirname("/"));
console.log("dirname single absolute:", path.dirname("/foo"));
console.log("dirname empty:", path.dirname(""));
console.log("dirname dot:", path.dirname("."));
