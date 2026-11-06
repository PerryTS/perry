import * as fs from "node:fs";
fs.writeFileSync("wasi-preopen.txt", "preopen works");
console.log(fs.readFileSync("wasi-preopen.txt", "utf8"));
fs.unlinkSync("wasi-preopen.txt");
