import * as path from "node:path";

console.log("win32 relative:", path.win32.relative("C:\\foo\\a", "C:\\foo\\b"));
console.log("win32 resolve:", path.win32.resolve("C:\\foo", "bar"));
console.log("win32 isAbsolute drive:", path.win32.isAbsolute("C:\\foo"));
console.log("win32 isAbsolute slash:", path.win32.isAbsolute("\\foo"));
