import * as nodeConsole from "console";

console.log("prefixless object:", nodeConsole !== null && typeof nodeConsole === "object");
console.log("prefixless log fn:", typeof nodeConsole.log === "function");
console.log("prefixless Console:", typeof nodeConsole.Console);
