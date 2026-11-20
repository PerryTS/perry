import path from "node:path";

console.log("ext dotdot:", JSON.stringify(path.win32.extname("..")));
console.log("ext nested dotdot:", JSON.stringify(path.win32.extname("C:\\tmp\\..")));
console.log("parse dotdot:", JSON.stringify(path.win32.parse("..")));
console.log("parse nested dotdot:", JSON.stringify(path.win32.parse("C:\\tmp\\..")));
