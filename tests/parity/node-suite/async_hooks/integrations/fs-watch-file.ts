import { AsyncLocalStorage } from "node:async_hooks";
import { unwatchFile, watchFile, writeFileSync, unlinkSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const storage = new AsyncLocalStorage<string>();
const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-watch-file-"));
const path = join(root, "perry-async-hooks-fs-watch-file.txt");

try {
  writeFileSync(path, "before");
  await storage.run(
    "fs-watch-file",
    () =>
      new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => {
          unwatchFile(path);
          reject(new Error("fs.watchFile timeout"));
        }, 2000);
        watchFile(path, { interval: 20 }, () => {
          console.log("fs.watchFile store:", storage.getStore());
          clearTimeout(timeout);
          unwatchFile(path);
          resolve();
        });
        setTimeout(() => writeFileSync(path, "after"), 50);
      }),
  );
  unlinkSync(path);
  console.log("fs.watchFile outside:", String(storage.getStore()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
