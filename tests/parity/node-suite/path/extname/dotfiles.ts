import * as path from "node:path";

console.log("dotfile no ext:", path.extname(".gitignore"));
console.log("dotfile with ext:", path.extname(".profile.js"));
console.log("nested dotfile:", path.extname("/foo/.env"));
