import { AsyncLocalStorage } from "node:async_hooks";
import { statSync, truncate, writeFileSync, unlinkSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const storage = new AsyncLocalStorage<string>();
const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-truncate-"));
const path = join(root, "perry-async-hooks-fs-truncate.txt");

try {
  writeFileSync(path, "truncate");
  await storage.run(
    "fs-truncate",
    () =>
      new Promise<void>((resolve, reject) => {
        truncate(path, 3, (error) => {
          console.log("fs.truncate store:", storage.getStore());
          if (error) return reject(error);
          resolve();
        });
      }),
  );
  console.log("fs.truncate size:", statSync(path).size);
  unlinkSync(path);
  console.log("fs.truncate outside:", String(storage.getStore()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
