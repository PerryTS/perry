import { createHook, executionAsyncId } from "node:async_hooks";
import { rmSync, unwatchFile, watch, watchFile, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const path = join(tmpdir(), "perry-async-hooks-watcher-lifecycles.txt");
rmSync(path, { force: true });
writeFileSync(path, "watchers");
const parentId = executionAsyncId();
type Entry = {
  asyncId: number;
  type: string;
  triggerAsyncId: number;
  before: number;
  after: number;
  destroy: number;
};
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type !== "FSEVENTWRAP" && type !== "STATWATCHER") return;
    const entry = {
      asyncId,
      type,
      triggerAsyncId,
      before: 0,
      after: 0,
      destroy: 0,
    };
    entries.push(entry);
    byId.set(asyncId, entry);
  },
  before(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.before++;
  },
  after(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.after++;
  },
  destroy(asyncId) {
    const entry = byId.get(asyncId);
    if (entry) entry.destroy++;
  },
}).enable();

try {
  const eventWatcher = watch(path, () => {});
  eventWatcher.close();
  watchFile(path, { interval: 20 }, () => {});
  unwatchFile(path);
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
  unwatchFile(path);
  rmSync(path, { force: true });
}

for (const type of ["FSEVENTWRAP", "STATWATCHER"]) {
  const selected = entries.filter((entry) => entry.type === type);
  console.log(
    `${type} watcher lifecycle:`,
    selected.length,
    selected.length === 1 &&
      selected.every((entry) => entry.triggerAsyncId === parentId),
    selected
      .map((entry) => `${entry.before}/${entry.after}/${entry.destroy}`)
      .join(","),
  );
}
