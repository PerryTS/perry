import * as fs from "node:fs";

const ents = fs.readdirSync("/tmp", { withFileTypes: true });
console.log("entry[0] typeof:", typeof ents[0]);
console.log("entry[0] is string:", typeof ents[0] === "string");
console.log("entry[0].isFile type:", typeof (ents[0] as any).isFile);

// Without options should still return strings
const names = fs.readdirSync("/tmp");
console.log("default entry typeof:", typeof names[0]);
