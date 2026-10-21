import { createReadStream, createWriteStream, unlinkSync, mkdtempSync, rmSync } from "node:fs";
import { AsyncLocalStorage } from "node:async_hooks";

import { tmpdir } from "node:os";
import { join } from "node:path";

const storage = new AsyncLocalStorage<string>();
const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-streams-"));
const path = join(root, "perry-async-hooks-fs-streams.txt");

try {

  try {
    unlinkSync(path);
  } catch {}

  await storage.run(
    "fs-streams",
    () =>
      new Promise<void>((resolve, reject) => {
        const writer = createWriteStream(path);
        writer.on("error", reject);
        writer.on("finish", () => {
          console.log("write stream finish store:", storage.getStore());
          const chunks: string[] = [];
          const reader = createReadStream(path, { encoding: "utf8" });
          reader.on("error", reject);
          reader.on("data", (chunk) => {
            console.log("read stream data store:", storage.getStore());
            chunks.push(String(chunk));
          });
          reader.on("end", () => {
            console.log(
              "read stream end store:",
              storage.getStore(),
              chunks.join(""),
            );
            unlinkSync(path);
            resolve();
          });
        });
        writer.end("stream-payload");
      }),
  );

  console.log("fs streams outside:", String(storage.getStore()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
