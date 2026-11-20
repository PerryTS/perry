import { spawnSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const marker = "__trace_events_filter_child__";

if (!process.argv.includes(marker)) {
  for (const category of ["node.bootstrap", '""']) {
    const temporary = mkdtempSync(join(tmpdir(), "perry-trace-filter-"));
    try {
      const script = process.argv[1];
      const args = ["--trace-event-categories", category];
      if (typeof script === "string" && script.endsWith(".ts")) {
        args.push(script);
      }
      args.push(marker);
      const result = spawnSync(process.execPath, args, {
        cwd: temporary,
        encoding: "utf8",
      });
      const files = readdirSync(temporary).filter((name) =>
        name.endsWith(".log")
      );
      console.log(
        "case:",
        category,
        "status/files:",
        result.status,
        files.length,
      );
      if (files.length === 1) {
        const events =
          JSON.parse(readFileSync(join(temporary, files[0]), "utf8"))
            .traceEvents;
        const application = events.filter((event: any) =>
          event.cat !== "__metadata"
        );
        console.log(
          "metadata present:",
          events.some((event: any) => event.cat === "__metadata"),
        );
        console.log("application count positive:", application.length > 0);
        console.log(
          "filter respected:",
          application.every((event: any) =>
            event.cat === "node,node.bootstrap"
          ),
        );
      }
    } finally {
      rmSync(temporary, { recursive: true, force: true });
    }
  }
}
