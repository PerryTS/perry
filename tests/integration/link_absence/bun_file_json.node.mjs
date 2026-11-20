import {readFile} from "node:fs/promises";
const text = await readFile("value.json", "utf8");
const operations = {json: () => JSON.parse(text)};
console.log(operations[process.argv[2]]().answer);
