import { AsyncLocalStorage } from "node:async_hooks";
import { mkdtempSync, rmSync, unwatchFile, watchFile, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "perry-watch-file-teardown-"));
const path = join(root, "watched.txt");
const storage = new AsyncLocalStorage<string>();
try {
  writeFileSync(path, "initial");
  const immediate = () => {};
  watchFile(path, { interval: 10 }, immediate);
  unwatchFile(path, immediate);
  console.log("watchFile immediate removal:", true);
  let removedCalls = 0;
  let calls = 0;
  await new Promise<void>((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("watchFile event missing")), 1500);
    const removed = () => { removedCalls++; };
    storage.run("poll-owner", () => {
      const first = watchFile(path, { interval: 10 }, removed);
      first.unref();
      first.ref();
      function listener(curr, prev) {
        calls++;
        console.log("watchFile callback:", storage.getStore(), this === first, curr.size > prev.size);
        unwatchFile(path, removed);
        unwatchFile(path, listener);
        clearTimeout(deadline);
        resolve();
      }
      const second = watchFile(path, { interval: 10 }, listener);
      unwatchFile(path, removed);
      console.log("watchFile shared watcher:", first === second);
    });
    setTimeout(() => writeFileSync(path, "changed-and-longer"), 30);
  });
  console.log("watchFile callback counts:", calls, removedCalls);
  console.log("watchFile outside:", String(storage.getStore()));
  // Leave only unref'ed pollers pending: process exit is part of this test.
  const passive = join(root, "passive.txt");
  const unrefed = join(root, "unrefed.txt");
  writeFileSync(passive, "passive");
  writeFileSync(unrefed, "unrefed");
  watchFile(passive, { interval: 20, persistent: false }, () => {});
  watchFile(unrefed, { interval: 20 }, () => {}).unref();
  console.log("watchFile nonpersistent pending:", true);
} finally {
  unwatchFile(path);
  rmSync(root, { recursive: true, force: true });
}
