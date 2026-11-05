import { existsSync, readFileSync, rmSync } from "node:fs";

const target = "/tmp/perry-process-report-writeReport.json";

try {
  rmSync(target);
} catch {
}

const written = process.report.writeReport(target);
const parsed = JSON.parse(readFileSync(target, "utf8"));

console.log("returned target:", written === target);
console.log("file exists:", existsSync(target));
console.log(
  "json core:",
  [
    parsed.header.event,
    parsed.header.trigger,
    typeof parsed.header.filename,
    typeof parsed.javascriptStack,
    Array.isArray(parsed.nativeStack),
    typeof parsed.resourceUsage.fsActivity,
  ].join(","),
);

rmSync(target);
