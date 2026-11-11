import * as nodeConsole from "node:console";

console.log("namespace object:", nodeConsole !== null && typeof nodeConsole === "object");
console.log("namespace log fn:", typeof nodeConsole.log === "function");
console.log("namespace error fn:", typeof nodeConsole.error === "function");
console.log("namespace Console:", typeof nodeConsole.Console);
