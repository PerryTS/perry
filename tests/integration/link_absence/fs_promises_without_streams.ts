// Merely reading through fs/promises must not install Web Streams or codecs.
import { readFile } from "node:fs/promises";
async function main() {
  const names = ["value.json"];
  const index = process.argv.length - 3;
  console.log((await readFile(names[index], "utf8")).trim());
}
main();
