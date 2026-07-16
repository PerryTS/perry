import fsDefault, { readFile } from "node:fs";
import { createRequire, syncBuiltinESMExports } from "node:module";

const req = createRequire(import.meta.url);
const cjsFs = req("node:fs");
const original = cjsFs.readFile;
const replacement = function parityReadFile() {};
try {
  console.log("initial identity:", fsDefault === cjsFs, readFile === original);
  cjsFs.readFile = replacement;
  console.log("before sync:", readFile === original, readFile === replacement);
  console.log("return:", String(syncBuiltinESMExports()));
  console.log("after sync:", readFile === original, readFile === replacement);
} finally {
  cjsFs.readFile = original;
  syncBuiltinESMExports();
}
console.log("restored:", readFile === original, cjsFs.readFile === original);
