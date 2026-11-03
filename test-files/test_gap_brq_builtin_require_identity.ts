import fs from "fs";
import * as ns from "fs";
import required from "./fixtures/BRQ-builtin-requires.cjs";

console.log("bare-prefixed", required.fs === required.prefixed);
console.log("process-global", required.process === process);
console.log("fs-promises", required.fs.promises === required.promises);
console.log("static-default", required.fs === fs);
console.log("static-namespace-default", ns.default === required.fs);
console.log("commonjs-no-default", required.fs.default === undefined);
async function check() {
  const dynamic = await import("fs");
  console.log("dynamic-default", required.fs === dynamic.default);
  console.log("namespace-identity", ns === dynamic);
  console.log("named-export", dynamic.readFileSync === fs.readFileSync);
}
check();
import { fs as namedFs } from "./fixtures/BRQ-builtin-requires.cjs";
console.log("named-cjs-export", namedFs === fs);
