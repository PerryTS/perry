import * as path from "node:path";

console.log("win32 normalize UNC:", path.win32.normalize("\\\\server\\share\\foo\\..\\bar"));
console.log("win32 dirname drive relative:", path.win32.dirname("C:foo\\bar"));
console.log("win32 parse drive root:", path.win32.parse("C:\\").root);
