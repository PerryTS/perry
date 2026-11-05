import { AsyncLocalStorage, createHook, executionAsyncId, executionAsyncResource } from "node:async_hooks";
import { mkdtempSync, rmSync, watch, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "perry-watch-owner-"));
const path = join(root, "watched.txt");
const storage = new AsyncLocalStorage<string>();
let watcher: ReturnType<typeof watch> | undefined;
let resource: object | undefined;
let providerId = 0;
let before = 0;
let after = 0;
const hook = createHook({
  init(id, type, trigger, value) {
    if (type === "FSEVENTWRAP") {
      providerId = id;
      resource = value;
    }
  },
  before(id) { if (id === providerId) before++; },
  after(id) { if (id === providerId) after++; },
}).enable();
try {
  writeFileSync(path, "initial");
  const rows: string[] = [];
  let removedCalls = 0;
  const removed = () => { removedCalls++; };
  await new Promise<void>((resolve, reject) => {
    const deadline = setTimeout(() => reject(new Error("watch event missing")), 1500);
    storage.run("owner", () => {
      watcher = watch(path, function () {
        rows.push(`first:${storage.getStore()}:${this === watcher}:${executionAsyncId() === providerId}:${executionAsyncResource() === resource}`);
        watcher!.close();
      });
    });
    storage.run("later-listener", () => {
      watcher!.on("change", removed);
      watcher!.removeListener("change", removed);
      watcher!.on("change", function () {
        rows.push(`second:${storage.getStore()}:${this === watcher}:${executionAsyncId() === providerId}`);
        clearTimeout(deadline);
        resolve();
      });
    });
    setTimeout(() => writeFileSync(path, "changed"), 30);
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  console.log("watch owner callbacks:", rows.join("|"));
  console.log("watch removed callback:", removedCalls);
  console.log("watch provider hooks:", before > 0, before === after);
  console.log("watch owner outside:", String(storage.getStore()));
} finally {
  watcher?.close();
  hook.disable();
  rmSync(root, { recursive: true, force: true });
}
