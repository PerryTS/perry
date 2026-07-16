import { unlinkSync } from "node:fs";
import { readFile, unlink, writeFile } from "node:fs/promises";
import { AsyncLocalStorage } from "node:async_hooks";

const storage = new AsyncLocalStorage<string>();
const path = "/tmp/perry-async-hooks-fs-promises.txt";

try {
  unlinkSync(path);
} catch {}

const result = await storage.run("fs-promises", async () => {
  await writeFile(path, "promise-payload");
  console.log("writeFile promise store:", storage.getStore());
  const data = await readFile(path, "utf8");
  console.log("readFile promise store:", storage.getStore(), data);
  await unlink(path);
  console.log("unlink promise store:", storage.getStore());
  return "fs-promises-result";
});

console.log("fs promises result:", result);
console.log("fs promises outside:", String(storage.getStore()));
