// Does a `worker_threads` agent still reach tokio? (turnloop P8)
//
// Every lane from P1 on left its tokio transport in place for one shared
// reason, and it is the same predicate in every crate:
// `perry_runtime::turnloop_net::available()` is false unless the calling
// thread owns an agent loop, and only the PRIMARY agent does
// (`event_pump::agent_loop::net_available` -> `current_agent() ==
// PRIMARY_AGENT`). A Worker therefore declines to the legacy path for net,
// TLS, HTTP server, fetch, SMTP and all four database drivers at once.
//
// That is a claim about a condition in Rust. This is the probe that makes it a
// measurement: the same `fetch` call, once on the primary agent and once
// inside a Worker, with the OS thread names read out of /proc both times.
// tokio names its pool `tokio-rt-worker`; turnloop names its blocking threads
// `turnloop-blocki`. A run where the Worker's census shows no `tokio-rt-worker`
// would mean the decline is NOT reached, and would falsify the inventory —
// which is the point of running it rather than reasoning about it.
//
// Linux only: it reads /proc/self/task. On a host without /proc the census
// prints `unavailable` and the run proves nothing, so it says so rather than
// printing a zero that would read as "no tokio".
import { readdirSync, readFileSync } from "node:fs";
import { Worker, isMainThread, parentPort } from "node:worker_threads";

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

const URL_UNDER_TEST = process.env.P8_URL ?? "http://127.0.0.1:8099/";

async function probe(label: string): Promise<void> {
  let status = "n/a";
  try {
    const r = await fetch(URL_UNDER_TEST);
    status = String(r.status);
    await r.text();
  } catch (e) {
    status = "error:" + (e as Error).message;
  }
  console.log(`${label}: status=${status} threads=[${threadNames()}]`);
}

async function main(): Promise<void> {
  if (!isMainThread) {
    await probe("worker-agent");
    parentPort?.postMessage("done");
    return;
  }
  console.log(`idle: threads=[${threadNames()}]`);
  await probe("primary-agent");
  const w = new Worker(new URL(import.meta.url));
  await new Promise<void>((resolve) => {
    w.on("message", () => resolve());
    w.on("error", (e) => {
      console.log("worker error:", (e as Error).message);
      resolve();
    });
  });
  await w.terminate();
  console.log(`after: threads=[${threadNames()}]`);
}

void main();
