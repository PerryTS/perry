import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { chdir, cwd } from "node:process";
import { createTracing, getEnabledCategories } from "node:trace_events";

const parent = cwd();
const temporary = mkdtempSync(join(tmpdir(), "perry-trace-categories-"));
const cases = [
  [""],
  ["z", "a", "m"],
  ["z", "z", "a"],
  ["a,b"],
  [" spaced "],
  ["*"],
];

try {
  chdir(temporary);
  for (const categories of cases) {
    const tracing = createTracing({ categories });
    tracing.enable();
    console.log(
      JSON.stringify(categories),
      "property:",
      JSON.stringify(tracing.categories),
      "global:",
      JSON.stringify(getEnabledCategories()),
    );
    tracing.disable();
  }
} finally {
  chdir(parent);
  rmSync(temporary, { recursive: true, force: true });
}
