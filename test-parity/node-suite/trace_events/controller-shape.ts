import { createTracing } from "node:trace_events";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-controller-"));

try {
  chdir(temporary);
  const tracing = createTracing({ categories: ["node", "v8"] });

  console.log("constructor:", tracing.constructor.name);
  console.log("keys:", Object.keys(tracing).join(","));
  console.log(
    "own props:",
    String(Object.prototype.hasOwnProperty.call(tracing, "categories")),
    String(Object.prototype.hasOwnProperty.call(tracing, "enabled")),
    String(Object.prototype.hasOwnProperty.call(tracing, "enable")),
    String(Object.prototype.hasOwnProperty.call(tracing, "disable")),
  );
  console.log("categories:", tracing.categories);
  console.log("enabled:", String(tracing.enabled));
  console.log("methods:", typeof tracing.enable, typeof tracing.disable);
  console.log("method lengths:", tracing.enable.length, tracing.disable.length);
  console.log("enable return:", String(tracing.enable()));
  console.log("enabled after enable:", String(tracing.enabled));
  console.log("disable return:", String(tracing.disable()));
  console.log("enabled after disable:", String(tracing.enabled));
} finally {
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
