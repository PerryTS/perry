// In a program that uses worker_threads, every `new URL("<script>",
// import.meta.url)` is a worker entry candidate. This one names a template
// file that is not valid JavaScript: compiling it fails, which must only be a
// compile-time warning, and the program reads the file as data.
import { isMainThread } from "node:worker_threads";
import { readFileSync } from "node:fs";

const template = new URL("./_helpers/worker_url_template.js", import.meta.url);
const text = readFileSync(template, "utf8");
console.log("main thread:", isMainThread);
console.log("template has placeholder:", text.includes("{{ greeting }}"));
