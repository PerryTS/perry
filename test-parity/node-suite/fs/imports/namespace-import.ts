import * as fs from "node:fs";

console.log("namespace object:", fs !== null && typeof fs === "object");
console.log("readFileSync function:", typeof fs.readFileSync);
console.log("writeFileSync function:", typeof fs.writeFileSync);
console.log("constants object:", fs.constants !== null && typeof fs.constants === "object");
