// `process` inherits the Object.prototype predicates (#13963).
import proc from "node:process";

console.log("hasOwnProperty:", typeof process.hasOwnProperty);
console.log("own env:", process.hasOwnProperty("env"));
console.log("own missing:", process.hasOwnProperty("noSuchProperty"));
console.log("default import own argv:", proc.hasOwnProperty("argv"));
console.log("isPrototypeOf:", process.isPrototypeOf({}));
