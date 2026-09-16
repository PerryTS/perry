// The worker half of `tokio_worker_agent_census.ts`. See that file for what
// this measures and why it is two files (a Worker whose entry is its own
// module does not link — see the report's defect section).
import { readdirSync, readFileSync } from "node:fs";
import { parentPort } from "node:worker_threads";

function threadNames(): string {
  try {
    const counts = new Map<string, number>();
    for (const t of readdirSync("/proc/self/task")) {
      try {
        const n = readFileSync(`/proc/self/task/${t}/comm`, "utf8").trim();
        counts.set(n, (counts.get(n) ?? 0) + 1);
      } catch {}
    }
    return [...counts.entries()]
      .sort((a, b) => (a[0] < b[0] ? -1 : 1))
      .map(([n, c]) => `${n} x${c}`)
      .join(", ");
  } catch {
    return "unavailable";
  }
}

const url = process.env.P8_URL ?? "http://127.0.0.1:8099/";
let status = "n/a";
try {
  const r = await fetch(url);
  status = String(r.status);
  await r.text();
} catch (e) {
  status = "error:" + (e as Error).message;
}
console.log(`worker-agent: status=${status} threads=[${threadNames()}]`);
parentPort?.postMessage("done");
