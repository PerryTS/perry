import * as path from "node:path";

console.log("posix normalize double slash:", path.posix.normalize("//foo//bar"));
console.log("posix dirname double slash:", path.posix.dirname("//foo"));
console.log("posix basename trailing:", path.posix.basename("/foo/bar//"));
