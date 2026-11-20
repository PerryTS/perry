import { REPLServer } from "node:repl";

console.log(Object.getPrototypeOf(REPLServer).name);
console.log(Object.getPrototypeOf(REPLServer.prototype).constructor.name);
console.log(Object.getPrototypeOf(REPLServer.prototype) === Object.prototype);
