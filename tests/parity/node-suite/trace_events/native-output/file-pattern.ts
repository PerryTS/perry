import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const marker = "__trace_events_pattern_child__";

if (!process.argv.includes(marker)) {
  const temporary = mkdtempSync(join(tmpdir(), "perry-trace-pattern-"));
  try {
    const script = process.argv[1];
    const pattern = "trace-${pid}-${rotation}-${pid}-${rotation}.json";
    const args = [
      "--trace-events-enabled",
      "--trace-event-file-pattern",
      pattern,
    ];
    if (typeof script === "string" && script.endsWith(".ts")) args.push(script);
    args.push(marker);
    const result = spawnSync(process.execPath, args, {
      cwd: temporary,
      encoding: "utf8",
    });
    const files = readdirSync(temporary);
    const expected = `trace-${result.pid}-1-${result.pid}-1.json`;
    console.log("status/files:", result.status, files.length);
    console.log(
      "placeholder match:",
      files.length === 1 && files[0] === expected,
    );
    if (files.length === 1) {
      console.log(
        "valid trace:",
        Array.isArray(
          JSON.parse(readFileSync(join(temporary, files[0]), "utf8"))
            .traceEvents,
        ),
      );
    }
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
}
