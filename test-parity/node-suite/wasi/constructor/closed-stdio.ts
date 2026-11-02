import { WASI } from "node:wasi";
import { closeSync } from "node:fs";

new WASI({ version: "preview1", stdin: 0 });
console.log("open stdin: accepted");
closeSync(0);
try {
  new WASI({ version: "preview1", stdin: 0 });
  console.log("closed stdin: accepted");
} catch (error: any) {
  console.log("closed stdin:", error.name, error.code);
}
