import { watch } from "node:fs/promises";
import { mkdtempSync, rmSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
const directory = mkdtempSync(join(tmpdir(), "perry-nmi-watch-"));
const controller = new AbortController();
const makeWatcher = watch;
try {
    const iterator = makeWatcher(directory, { signal: controller.signal });
    setTimeout(() => controller.abort(), 5);
    try { await iterator.next(); console.log("unexpected event"); }
    catch (error) { console.log(error.name); }
} finally { rmSync(directory, { recursive: true, force: true }); }
