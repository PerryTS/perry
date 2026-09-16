// turnloop P9: what does an agent's own loop cost in RSS?
//
// A `turnloop::Loop` preallocates its tables at `Loop::new` and the config
// cannot grow in place (PerryTS/turnloop#43), so the size is chosen once. The
// net profile is 4096 handles, 8192 operations and 64 x 16 KiB pooled buffers.
// Before P9 exactly one of those existed per process; now every JS agent that
// does network I/O has one, and a program with 64 Workers would have 64.
//
// This measures the DELTA rather than an absolute, because an absolute mixes in
// the JS heap, the thread stacks and the class image every Worker adopts. Run
// it at P9_AGENTS=1, 8 and 64 with P9_RSS_MODE=net (a loop at the net profile)
// and P9_RSS_MODE=idle (an agent that parks but never submits, so its loop
// stays at the WAIT profile: 16 handles, no pooled buffers). The difference
// between the two modes is the part this lane controls; the rest is what a
// Worker costs whatever the transport.
//
// ## Why this does not use postMessage or terminate()
//
// It did, and neither is usable here. Two PRE-EXISTING Perry defects, both
// reproduced on this branch's base commit and both therefore nothing to do with
// P9 -- see `p9_worker_message_fanin.ts` for the smaller of them:
//
//  1. With more than one Worker, a Worker's `postMessage` to the parent is not
//     reliably delivered: `await Promise.all(ready)` never resolves and the
//     program hangs. At 1 agent it works; at 2 it hangs; at 3 it sometimes
//     works. A measurement built on it would not be flaky, it would be absent.
//  2. `worker.terminate()` on a Worker parked in `parentPort.on("message")`
//     never completes, so even the 1-agent row hung after printing its answer.
//
// So readiness travels through the FILESYSTEM -- each worker writes one file
// naming its own fetch result -- and the process leaves by `process.exit`,
// which needs neither. That also keeps the subject assertion: `net_ok` counts
// workers whose fetch actually succeeded, and only those upgraded a loop to the
// NET profile. A row whose `net_ok` is below `agents` is measuring fewer loops
// than it counted, which is a finding and not a cheaper number.
//
// Linux only (it reads /proc/self/status). Elsewhere it says so rather than
// printing a zero that would read as "free".
import { mkdirSync, readdirSync, readFileSync, rmSync } from "node:fs";
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
const budgetMs = Number(process.env.P9_BUDGET_MS ?? "60000");
const readyDir = process.env.P9_READY_DIR ?? "/tmp/p9-rss-ready";

rmSync(readyDir, { recursive: true, force: true });
mkdirSync(readyDir, { recursive: true });

const before = rssKb();
if (before < 0) {
  console.log("VmRSS unavailable on this host: this run measures nothing");
}

const workerUrl = new URL("./_helpers/p9_rss_worker.ts", import.meta.url);
const workers: Worker[] = [];
for (let i = 0; i < agents; i++) {
  const w = new Worker(workerUrl, { workerData: { index: i, readyDir } });
  w.on("error", () => {});
  workers.push(w);
}

function readyFiles(): string[] {
  try {
    return readdirSync(readyDir);
  } catch {
    return [];
  }
}

const deadline = Date.now() + budgetMs;
let files = readyFiles();
while (files.length < agents && Date.now() < deadline) {
  await new Promise<void>((r) => setTimeout(r, 20));
  files = readyFiles();
}

// Read RSS while every agent is alive and idle -- which is the number the brief
// asks for -- and BEFORE anything is torn down.
const after = rssKb();
const delta = after - before;
const per = agents > 0 ? Math.round((delta / agents) * 10) / 10 : 0;

let netOk = 0;
let firstError = "";
for (const name of files) {
  let text = "";
  try {
    text = readFileSync(`${readyDir}/${name}`, "utf8").trim();
  } catch {
    text = "unreadable";
  }
  if (text.startsWith("ok:")) netOk += 1;
  else if (text !== "skipped" && !firstError) firstError = text;
}

console.log(
  `agents=${agents} mode=${mode} ready=${files.length}/${agents} ` +
    `rss_before_kb=${before} rss_after_kb=${after} delta_kb=${delta} per_agent_kb=${per} ` +
    `net_ok=${netOk}/${agents}` + (firstError ? ` first_error=${JSON.stringify(firstError)}` : ""),
);

// `process.exit` rather than terminate(): see the header. It is also what keeps
// the row above as the last thing measured -- a teardown that freed a loop
// before the reading would make the cost look smaller than it is.
process.exit(files.length === agents ? 0 : 1);
