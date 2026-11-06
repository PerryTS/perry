import { AsyncLocalStorage } from "node:async_hooks";
import { access, constants, writeFileSync, unlinkSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const storage = new AsyncLocalStorage<string>();
const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-access-"));
const path = join(root, "perry-async-hooks-fs-access.txt");

try {
  writeFileSync(path, "access");
  try {
    await storage.run(
      "fs-access",
      () =>
        new Promise<void>((resolve, reject) => {
          access(path, constants.R_OK, (error) => {
            console.log("fs.access store:", storage.getStore());
            if (error) return reject(error);
            resolve();
          });
        }),
    );
  } finally {
    unlinkSync(path);
  }
  console.log("fs.access outside:", String(storage.getStore()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
