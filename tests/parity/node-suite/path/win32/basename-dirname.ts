import * as path from "node:path";

console.log("win32 basename:", path.win32.basename("C:\\foo\\bar\\baz.txt"));
console.log("win32 basename ext:", path.win32.basename("C:\\foo\\bar\\baz.txt", ".txt"));
console.log("win32 dirname:", path.win32.dirname("C:\\foo\\bar\\baz.txt"));
