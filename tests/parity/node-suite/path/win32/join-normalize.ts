import * as path from "node:path";

console.log("win32 join:", path.win32.join("C:\\foo", "bar", "baz"));
console.log("win32 join dotdot:", path.win32.join("C:\\foo", "..", "bar"));
console.log("win32 normalize:", path.win32.normalize("C:\\foo\\bar\\..\\baz"));
