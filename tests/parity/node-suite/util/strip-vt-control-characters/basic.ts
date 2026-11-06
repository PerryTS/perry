import { stripVTControlCharacters } from "node:util";

console.log(stripVTControlCharacters("\u001b[31mred\u001b[39m plain"));
console.log(stripVTControlCharacters("plain"));
