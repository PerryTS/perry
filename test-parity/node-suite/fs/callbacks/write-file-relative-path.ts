// Callback writes to a bare file name land in the cwd, as in Node.
import { appendFile, mkdtempSync, readFileSync, rmSync, writeFile } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
const dir = mkdtempSync(join(tmpdir(), "relative-write-"));
const previous = process.cwd();
process.chdir(dir);
writeFile("relative.out", "first\n", (err) => {
  console.log("writeFile", err ? err.code : "ok", readFileSync("relative.out", "utf8").length);
  appendFile("relative.out", "second\n", (err2) => {
    console.log("appendFile", err2 ? err2.code : "ok", readFileSync("relative.out", "utf8").length);
    process.chdir(previous);
    rmSync(dir, { recursive: true });
  });
});
