import * as path from "node:path";

console.log("trailing dir:", path.basename("/foo/bar/"));
console.log("many trailing:", path.basename("/foo/bar///"));
console.log("root:", path.basename("/"));
console.log("empty:", path.basename(""));
