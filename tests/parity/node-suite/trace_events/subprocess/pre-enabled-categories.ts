import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createTracing, getEnabledCategories } from "node:trace_events";

const marker = "__trace_events_flag_child__";

if (process.argv.includes(marker)) {
  console.log("initial:", String(getEnabledCategories()));
  const tracing = createTracing({ categories: ["dynamic", "flag.alpha"] });
  tracing.enable();
  console.log("with dynamic:", String(getEnabledCategories()));
  tracing.disable();
  console.log("after disable:", String(getEnabledCategories()));
} else {
  const temporary = mkdtempSync(join(tmpdir(), "perry-trace-flags-"));
  try {
    const script = process.argv[1];
    const args = ["--trace-event-categories", "flag.beta,flag.alpha"];
    if (typeof script === "string" && script.endsWith(".ts")) args.push(script);
    args.push(marker);

    const result = spawnSync(process.execPath, args, {
      cwd: temporary,
      encoding: "utf8",
    });
    console.log("status:", result.status);
    console.log((result.stdout || "").trim());
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}
