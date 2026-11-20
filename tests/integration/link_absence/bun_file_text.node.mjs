import {readFile} from "node:fs/promises";
console.log((await readFile("value.json", "utf8")).trim());
