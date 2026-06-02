import * as nodeConsole from "node:console";

console.log("global console object:", globalThis.console !== null && typeof globalThis.console === "object");
console.log("module/global log shape:", typeof nodeConsole.log === typeof globalThis.console.log);
console.log("global log fn:", typeof globalThis.console.log === "function");
