import * as traceEvents from "node:trace_events";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-state-"));
try {
  chdir(temporary);
  function enabled() {
    return String(traceEvents.getEnabledCategories());
  }

  const a = traceEvents.createTracing({ categories: ["b", "a"] });
  const b = traceEvents.createTracing({ categories: ["c", "a"] });
  const c = traceEvents.createTracing({ categories: ["d", "b"] });

  console.log("initial:", a.enabled, b.enabled, enabled());
  console.log("enable return:", a.enable());
  console.log("after a:", a.enabled, b.enabled, enabled());
  a.enable();
  console.log("after a again:", a.enabled, enabled());
  b.enable();
  console.log("after b:", a.enabled, b.enabled, enabled());
  c.enable();
  console.log("after c:", enabled());
  a.disable();
  console.log("after a disable:", a.enabled, b.enabled, enabled());
  b.disable();
  console.log("after b disable:", b.enabled, enabled());
  c.disable();
  console.log("after c disable:", c.enabled, enabled());

  const dup = traceEvents.createTracing({ categories: ["x", "x", "y"] });
  dup.enable();
  console.log("duplicates:", dup.categories, enabled());
  dup.disable();
  console.log("duplicates disabled:", enabled());
} finally {
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
