import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createTracing } from "node:trace_events";

const marker = "__trace_events_disable_child__";

if (process.argv.includes(marker)) {
  const tracing = createTracing({ categories: ["node.console"] });
  tracing.enable();
  console.time("included-marker");
  console.timeEnd("included-marker");
  tracing.disable();
  console.time("excluded-marker");
  console.timeEnd("excluded-marker");
} else {
  const temporary = mkdtempSync(join(tmpdir(), "perry-trace-disable-"));
  try {
    const script = process.argv[1];
    const args: string[] = [];
    if (typeof script === "string" && script.endsWith(".ts")) args.push(script);
    args.push(marker);
    const result = spawnSync(process.execPath, args, {
      cwd: temporary,
      encoding: "utf8",
    });
    const files = readdirSync(temporary).filter((name) =>
      name.endsWith(".log")
    );
    console.log("status/files:", result.status, files.length);
    if (files.length === 1) {
      const events = JSON.parse(
        readFileSync(join(temporary, files[0]), "utf8"),
      ).traceEvents;
      console.log(
        "included/excluded:",
        events.some((event: any) => event.name?.endsWith("included-marker")),
        events.some((event: any) => event.name?.endsWith("excluded-marker")),
      );
    }
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}
