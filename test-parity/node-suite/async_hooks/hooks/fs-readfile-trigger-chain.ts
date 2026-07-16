import { createHook, executionAsyncId } from "node:async_hooks";
import { readFile, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const path = join(tmpdir(), "perry-async-hooks-readfile-chain.txt");
rmSync(path, { force: true });
writeFileSync(path, "read-file-chain");
const parentId = executionAsyncId();
type Entry = {
  asyncId: number;
  triggerAsyncId: number;
  before: number;
  after: number;
  destroy: number;
};
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const hook = createHook({
  init(asyncId, type, triggerAsyncId) {
    if (type !== "FSREQCALLBACK") return;
    const entry = { asyncId, triggerAsyncId, before: 0, after: 0, destroy: 0 };
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

let result = "";
try {
  result = await new Promise<string>((resolve, reject) => {
    readFile(path, "utf8", (error, data) =>
      error ? reject(error) : resolve(data),
    );
  });
  await new Promise<void>((resolve) => setImmediate(resolve));
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  hook.disable();
  rmSync(path, { force: true });
}

console.log("readFile hook result/count:", result, entries.length);
console.log(
  "readFile hook trigger chain:",
  entries.length > 0 &&
    entries.every((entry, index) =>
      index === 0
        ? entry.triggerAsyncId === parentId
        : entry.triggerAsyncId === entries[index - 1].asyncId,
    ),
);
console.log(
  "readFile hook lifecycles:",
  entries
    .map((entry) => `${entry.before}/${entry.after}/${entry.destroy}`)
    .join(","),
);
