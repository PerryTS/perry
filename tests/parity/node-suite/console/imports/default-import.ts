import nodeConsole from "node:console";

console.log("default object:", nodeConsole !== null && typeof nodeConsole === "object");
console.log("default log fn:", typeof nodeConsole.log === "function");
console.log("default count fn:", typeof nodeConsole.count === "function");
