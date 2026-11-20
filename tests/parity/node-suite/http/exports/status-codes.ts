// #2519: node:http STATUS_CODES status-code → reason-phrase map.
import * as http from "node:http";
import { STATUS_CODES } from "node:http";

console.log("typeof:", typeof STATUS_CODES);
console.log("namespace identity:", http.STATUS_CODES === STATUS_CODES);
console.log("count:", Object.keys(STATUS_CODES).length);
console.log("200:", STATUS_CODES[200]);
console.log("string-key 200:", STATUS_CODES["200"]);
console.log("301:", STATUS_CODES[301]);
console.log("404:", STATUS_CODES[404]);
console.log("418:", STATUS_CODES[418]);
console.log("500:", STATUS_CODES[500]);
console.log("511:", STATUS_CODES[511]);
console.log("missing 299:", STATUS_CODES[299]);
