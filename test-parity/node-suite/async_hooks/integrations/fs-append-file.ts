import { AsyncLocalStorage } from "node:async_hooks";
import { appendFile, readFileSync, writeFileSync, unlinkSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const storage = new AsyncLocalStorage<string>();
const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-append-file-"));
const path = join(root, "perry-async-hooks-fs-append.txt");

try {
  writeFileSync(path, "a");
  await storage.run(
    "fs-append",
    () =>
      new Promise<void>((resolve, reject) => {
        appendFile(path, "b", (error) => {
          console.log("fs.appendFile store:", storage.getStore());
          if (error) return reject(error);
          resolve();
        });
      }),
  );
  console.log("fs.appendFile content:", readFileSync(path, "utf8"));
  unlinkSync(path);
  console.log("fs.appendFile outside:", String(storage.getStore()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
