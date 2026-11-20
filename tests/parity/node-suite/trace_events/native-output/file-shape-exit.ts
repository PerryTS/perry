import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const marker = "__trace_events_file_child__";

if (process.argv.includes(marker)) {
  process.exit(0);
} else {
  const temporary = mkdtempSync(join(tmpdir(), "perry-trace-file-"));
  try {
    const script = process.argv[1];
    const args = ["--trace-events-enabled"];
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
      const document = JSON.parse(
        readFileSync(join(temporary, files[0]), "utf8"),
      );
      const events = document.traceEvents;
      console.log("shape:", Array.isArray(events), events.length > 0);
      console.log(
        "metadata:",
        events.some((event: any) => event.cat === "__metadata"),
      );
    }
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}
