import os from "node:os";

console.log("default object:", os !== null && typeof os === "object");
console.log("default platform fn:", typeof os.platform === "function");
console.log("default eol string:", typeof os.EOL === "string");
console.log("default devNull string:", typeof os.devNull === "string");
