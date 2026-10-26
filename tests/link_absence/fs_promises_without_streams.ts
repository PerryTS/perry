// Merely reading through fs/promises must not install Web Streams or codecs.
import { readFile } from "node:fs/promises";
async function main() {
  console.log((await readFile("value.json", "utf8")).trim());
}
main();
