import * as fs from "fs";

console.log("prefixless object:", fs !== null && typeof fs === "object");
console.log("prefixless readFileSync:", typeof fs.readFileSync);
console.log("prefixless openAsBlob:", typeof fs.openAsBlob);
console.log("prefixless constants:", typeof fs.constants.F_OK);
