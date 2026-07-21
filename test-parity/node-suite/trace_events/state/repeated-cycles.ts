import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";
import { createTracing, getEnabledCategories } from "node:trace_events";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-cycles-"));

try {
  chdir(temporary);
  const left = createTracing({ categories: ["shared", "left"] });
  const right = createTracing({ categories: ["right", "shared"] });

  for (let cycle = 1; cycle <= 3; cycle++) {
    right.enable();
    left.enable();
    left.enable();
    console.log("cycle", cycle, "enabled:", String(getEnabledCategories()));
    right.disable();
    console.log("cycle", cycle, "left only:", String(getEnabledCategories()));
    left.disable();
    left.disable();
    console.log("cycle", cycle, "disabled:", String(getEnabledCategories()));
  }
} finally {
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
