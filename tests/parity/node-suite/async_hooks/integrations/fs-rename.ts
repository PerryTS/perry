import { AsyncLocalStorage } from "node:async_hooks";
import { rename, readFileSync, writeFileSync, rmSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const storage = new AsyncLocalStorage<string>();
const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-rename-"));
const source = join(root, "perry-async-hooks-fs-rename-source.txt");
const target = join(root, "perry-async-hooks-fs-rename-target.txt");

try {
  rmSync(source, { force: true });
  rmSync(target, { force: true });
  writeFileSync(source, "rename");
  await storage.run(
    "fs-rename",
    () =>
      new Promise<void>((resolve, reject) => {
        rename(source, target, (error) => {
          console.log("fs.rename store:", storage.getStore());
          if (error) return reject(error);
          resolve();
        });
      }),
  );
  console.log("fs.rename content:", readFileSync(target, "utf8"));
  rmSync(target, { force: true });
  console.log("fs.rename outside:", String(storage.getStore()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
