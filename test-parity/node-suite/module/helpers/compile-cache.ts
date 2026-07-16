import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import * as Module from "node:module";

const status = Module.constants.compileCacheStatus;
const root = mkdtempSync(join(tmpdir(), "perry-node-module-cache-probe-"));
const firstDirectory = join(root, "first");
const secondDirectory = join(root, "second");

try {
  console.log("status keys:", Object.keys(status).sort().join(","));
  console.log(
    "status values:",
    Object.keys(status)
      .sort()
      .map((key) => `${key}=${status[key]}`)
      .join(","),
  );
  console.log("cache before:", String(Module.getCompileCacheDir()));

  const first = Module.enableCompileCache(firstDirectory);
  console.log("first keys:", Object.keys(first).sort().join(","));
  console.log("first enabled:", first.status === status.ENABLED);
  console.log(
    "first directory contains:",
    String(first.directory).includes("perry-node-module-cache-probe"),
  );
  console.log(
    "cache after contains:",
    String(Module.getCompileCacheDir()).includes(
      "perry-node-module-cache-probe",
    ),
  );

  const capturedGetCompileCacheDir = Module.getCompileCacheDir;
  console.log(
    "captured cache contains:",
    String(capturedGetCompileCacheDir()).includes(
      "perry-node-module-cache-probe",
    ),
  );

  const second = Module.enableCompileCache(secondDirectory);
  console.log("second already:", second.status === status.ALREADY_ENABLED);
  console.log("flush:", String(Module.flushCompileCache()));
} finally {
  rmSync(root, { recursive: true, force: true });
}
