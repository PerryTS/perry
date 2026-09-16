// turnloop P9: what does an agent's own loop cost in RSS?
//
// A `turnloop::Loop` preallocates its tables at `Loop::new` and the config
// cannot grow in place (PerryTS/turnloop#43), so the size is chosen once. The
// net profile is 4096 handles, 8192 operations and 64 x 16 KiB pooled buffers.
// Before P9 exactly one of those existed per process; now every JS agent that
// does network I/O has one, and a program with 64 Workers would have 64.
//
// This measures the delta rather than an absolute, because an absolute mixes
// in the JS heap, the thread stacks and the class image every Worker adopts.
// Run it at P9_AGENTS=1, 8 and 64 with P9_RSS_MODE=net (a loop at the net
// profile) and P9_RSS_MODE=idle (an agent that parks but never submits, so its
// loop stays at the WAIT profile: 16 handles, no pooled buffers). The
// difference between the two modes is the part this lane controls; the rest is
// what a Worker costs whatever the transport.
//
// Linux only (it reads /proc/self/status). Elsewhere it says so rather than
// printing a zero that would read as "free".
import { readFileSync } from "node:fs";
import { Worker } from "node:worker_threads";

function rssKb(): number {
  try {
    const status = readFileSync("/proc/self/status", "utf8");
    const line = status.split("\n").find((l) => l.startsWith("VmRSS:"));
    return line ? Number(line.replace(/[^0-9]/g, "")) : -1;
  } catch {
    return -1;
  }
}

const agents = Number(process.env.P9_AGENTS ?? "1");
const mode = process.env.P9_RSS_MODE ?? "net";
const before = rssKb();
if (before < 0) {
  console.log("VmRSS unavailable on this host: this run measures nothing");
}

const workerUrl = new URL("./_helpers/p9_rss_worker.ts", import.meta.url);
const workers: Worker[] = [];
const ready: Promise<void>[] = [];
for (let i = 0; i < agents; i++) {
  const w = new Worker(workerUrl);
  workers.push(w);
  ready.push(
    new Promise<void>((resolve) => {
      w.on("message", (m: unknown) => {
        if (m === "ready") resolve();
      });
      w.on("error", () => resolve());
    }),
  );
}
await Promise.all(ready);

const after = rssKb();
const delta = after - before;
const per = agents > 0 ? Math.round((delta / agents) * 10) / 10 : 0;
console.log(
  `agents=${agents} mode=${mode} rss_before_kb=${before} rss_after_kb=${after} ` +
    `delta_kb=${delta} per_agent_kb=${per}`,
);

for (const w of workers) w.postMessage("stop");
for (const w of workers) await w.terminate();
console.log("done");
