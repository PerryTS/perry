// Does a `worker_threads` agent still reach tokio? (turnloop P8)
//
// Every lane from P1 on left its tokio transport in place for one shared
// reason, and it is the same predicate in every crate:
// `perry_runtime::turnloop_net::available()` is false unless the calling
// thread owns an agent loop, and only the PRIMARY agent does
// (`event_pump::agent_loop::net_available` -> `current_agent() ==
// PRIMARY_AGENT`). A Worker therefore declines to the legacy path for net,
// TLS, the HTTP server, fetch, SMTP and all four database drivers at once.
//
// That is a claim about a condition in Rust. This is the probe that makes it a
// measurement: the same `fetch` call, once on the primary agent and once
// inside a Worker, with the OS thread names read out of /proc both times, and
// `PERRY_LOOP_STATS=1`'s `p6 http_submitted=/declined=` counters around it. A
// Worker census showing `http_submitted` rise would mean the decline is NOT
// reached and would falsify the inventory — which is why it is run rather than
// reasoned about.
//
// Linux only: it reads /proc/self/task. On a host without /proc the census
// prints `unavailable` and the run proves nothing, so it says so rather than
// printing a zero that would read as "no tokio".
//
// Needs an HTTP origin at $P8_URL (default http://127.0.0.1:8099/), e.g.
// `python3 -m http.server 8099 --bind 127.0.0.1`.
import { readdirSync, readFileSync } from "node:fs";
import { Worker } from "node:worker_threads";

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
const workerUrl = new URL("./_helpers/tokio_worker_agent_census_worker.ts", import.meta.url);

console.log(`idle: threads=[${threadNames()}]`);

let status = "n/a";
try {
  const r = await fetch(url);
  status = String(r.status);
  await r.text();
} catch (e) {
  status = "error:" + (e as Error).message;
}
console.log(`primary-agent: status=${status} threads=[${threadNames()}]`);

const w = new Worker(workerUrl);
await new Promise<void>((resolve) => {
  w.on("message", () => resolve());
  w.on("error", (e: Error) => {
    console.log("worker error:", e.message);
    resolve();
  });
});
await w.terminate();
console.log(`after: threads=[${threadNames()}]`);
