import process from "node:process";
console.log("default-import typeof:", typeof process);
console.log("default-import argv:", process.argv.length > 0 ? "have argv" : "no argv");

console.log("global typeof:", typeof (globalThis as any).process);
