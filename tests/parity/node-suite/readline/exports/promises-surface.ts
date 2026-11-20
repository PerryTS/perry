import * as promises from "node:readline/promises";

console.log(Object.keys(promises).sort().join(","));
console.log(typeof promises.Interface, typeof promises.Readline);
console.log(promises.Interface.length, promises.Readline.length);
