import * as fs from "node:fs";
fs.writeFileSync("wasip1-preopen.txt", "preopen works");
console.log(fs.readFileSync("wasip1-preopen.txt", "utf8"));
console.log(fs.existsSync("wasip1-preopen.txt"));
fs.unlinkSync("wasip1-preopen.txt");
console.log(fs.existsSync("wasip1-preopen.txt"));
