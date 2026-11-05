import { createHook } from "node:async_hooks";
import { rmSync, writeFileSync, mkdtempSync } from "node:fs";
import { open, type FileHandle } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";

const root = mkdtempSync(join(tmpdir(), "perry-async-hooks-fs-promises-lifecycle-"));
const path = join(root, "perry-async-hooks-fs-promises-lifecycle.txt");

try {
  writeFileSync(path, "abc");
  const tracked = new Set(["FSREQPROMISE", "FILEHANDLE", "FILEHANDLECLOSEREQ"]);
  type Entry = {
    asyncId: number;
    type: string;
    before: number;
    after: number;
    destroy: number;
  };
  const entries: Entry[] = [];
  const byId = new Map<number, Entry>();
  const hook = createHook({
    init(asyncId, type) {
      if (!tracked.has(type)) return;
      const entry = { asyncId, type, before: 0, after: 0, destroy: 0 };
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

  let handle: FileHandle | undefined;
  let content = "";
  let size = -1;
  try {
    handle = await open(path, "r");
    const buffer = Buffer.alloc(3);
    await handle.read(buffer, 0, 3, 0);
    content = buffer.toString();
    size = (await handle.stat()).size;
    await handle.close();
    handle = undefined;
    await new Promise<void>((resolve) => setImmediate(resolve));
    await new Promise<void>((resolve) => setImmediate(resolve));
  } finally {
    try {
      if (handle) await handle.close();
    } finally {
      hook.disable();
    }
  }

  console.log("fs promises hook result:", content, size);
  for (const type of ["FSREQPROMISE", "FILEHANDLE", "FILEHANDLECLOSEREQ"]) {
    const selected = entries.filter((entry) => entry.type === type);
    console.log(
      `${type} fs promises lifecycle:`,
      selected.length,
      selected
        .map((entry) => `${entry.before}/${entry.after}/${entry.destroy}`)
        .join(","),
    );
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
