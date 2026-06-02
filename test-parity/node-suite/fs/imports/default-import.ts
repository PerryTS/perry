import fs from "node:fs";

console.log("default object:", fs !== null && typeof fs === "object");
console.log("existsSync function:", typeof fs.existsSync);
console.log("mkdirSync function:", typeof fs.mkdirSync);
console.log("readdirSync function:", typeof fs.readdirSync);
console.log("openAsBlob function:", typeof fs.openAsBlob, fs.openAsBlob.length);
