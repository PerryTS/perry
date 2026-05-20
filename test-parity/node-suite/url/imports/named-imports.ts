import { URL, URLSearchParams, fileURLToPath } from "node:url";

console.log("URL href:", new URL("https://example.com/a").href);
console.log("params:", new URLSearchParams("a=1").get("a"));
console.log("file path:", fileURLToPath("file:///tmp/a.txt"));
