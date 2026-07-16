import { createHook } from "node:async_hooks";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { opendir, type Dir } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

type Entry = { id: number; events: string[]; resource: object };
const entries: Entry[] = [];
const byId = new Map<number, Entry>();
const path = join(tmpdir(), `perry-async-hooks-dirhandle-${process.pid}`);
rmSync(path, { recursive: true, force: true });
mkdirSync(path);
writeFileSync(join(path, "entry.txt"), "entry");
const hook = createHook({
  init(asyncId, type, _triggerAsyncId, resource) {
    if (type !== "DIRHANDLE") return;
    const entry = { id: asyncId, events: ["init"], resource };
    entries.push(entry);
    byId.set(asyncId, entry);
  },
  before(asyncId) {
    byId.get(asyncId)?.events.push("before");
  },
  after(asyncId) {
    byId.get(asyncId)?.events.push("after");
  },
  destroy(asyncId) {
    byId.get(asyncId)?.events.push("destroy");
  },
}).enable();
let directory: Dir | undefined;
let opened = false;
try {
  directory = await opendir(path);
  opened = true;
  await directory.close();
  directory = undefined;
  await new Promise<void>((resolve) => setImmediate(resolve));
} finally {
  if (directory) await directory.close();
  hook.disable();
  rmSync(path, { recursive: true, force: true });
}
console.log("dirhandle operation:", opened);
console.log(
  "dirhandle resources:",
  entries.length,
  entries.length === 1 && entries[0].id > 0,
  entries.length === 1 && typeof entries[0].resource === "object",
);
console.log(
  "dirhandle events:",
  entries.map((entry) => entry.events.join(">")).join("|"),
);
