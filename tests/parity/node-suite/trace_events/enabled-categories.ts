import * as traceEvents from "node:trace_events";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-enabled-"));
try {
  chdir(temporary);
  const a = traceEvents.createTracing({ categories: ["z", "a", "z", "m"] });
  const b = traceEvents.createTracing({ categories: ["b", "a"] });

  function enabled(label: string) {
    const value = traceEvents.getEnabledCategories();
    console.log(label + ":", String(value), typeof value);
  }

  console.log(
    "exports:",
    typeof traceEvents.createTracing,
    typeof traceEvents.getEnabledCategories,
  );
  console.log(
    "export lengths:",
    traceEvents.createTracing.length,
    traceEvents.getEnabledCategories.length,
  );
  console.log("initial enabled:", String(a.enabled), String(b.enabled));
  enabled("global initial");

  console.log("enable a:", String(a.enable()));
  console.log("after enable a:", String(a.enabled), String(b.enabled));
  enabled("global after a");

  console.log("enable a again:", String(a.enable()));
  console.log("enable b:", String(b.enable()));
  enabled("global after b");

  console.log("disable a:", String(a.disable()));
  console.log("after disable a:", String(a.enabled), String(b.enabled));
  enabled("global after disable a");

  console.log("disable a again:", String(a.disable()));
  console.log("disable b:", String(b.disable()));
  enabled("global after disable b");
} finally {
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
