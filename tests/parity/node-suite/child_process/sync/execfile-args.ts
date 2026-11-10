import { execFileSync } from "node:child_process";

const args = ["space value", 'quote"value', "unicode-中文", "", "--flag=value"];
const stdout = execFileSync(
  "node",
  [
    "-e",
    "process.stdout.write(JSON.stringify(process.argv.slice(1)))",
    ...args,
  ],
  { encoding: "utf8" },
);
console.log("args:", stdout);
