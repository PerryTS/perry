// HWS argv: ["value.json", "value.json"]
// Reading argv files in a loop must not install Web Streams or compression.
import * as fs from "fs";
let words = 0;
for (const filename of process.argv.slice(2)) {
  const content = fs.readFileSync(filename, "utf8");
  words += content.trim().split(/\s+/).length;
}
console.log(words);
