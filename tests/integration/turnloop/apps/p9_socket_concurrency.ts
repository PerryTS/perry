// Discriminates the two explanations for a failure the RSS probe found: a
// `net.connect` round-trip inside a Worker is reliable at one or two agents and
// becomes unreliable above that, with `write ENOENT` and hangs.
//
// ENOENT on a WRITE is `turnloop_net`'s "this id is not in my table", so the
// two candidate causes are very different:
//
//   P9_MODE=agents   N Workers, one socket each  -> N agents, N loops, N tables
//   P9_MODE=primary  one agent, N sockets        -> 1 agent, 1 loop, 1 table
//
// If `primary` is clean at the same N, the failure is about crossing AGENTS and
// belongs to this lane. If `primary` fails too, it is Perry's socket layer under
// concurrency and this lane only made it reachable from a Worker.
//
// Needs the TCP echo at $P9_ECHO_PORT (default 8098).
import net from "node:net";
import { Worker } from "node:worker_threads";

const mode = process.env.P9_MODE ?? "agents";
const agents = Number(process.env.P9_AGENTS ?? "8");
const budgetMs = Number(process.env.P9_BUDGET_MS ?? "30000");
const echoPort = Number(process.env.P9_ECHO_PORT ?? "8098");

function echoOnce(tag: string): Promise<string> {
  return new Promise<string>((resolve) => {
    const sock = net.connect(echoPort, "127.0.0.1");
    let seen = "";
    const timer = setTimeout(() => resolve("error:timeout"), budgetMs);
    sock.on("connect", () => sock.write(`${tag}\n`));
    sock.on("data", (chunk: unknown) => {
      seen += typeof chunk === "string" ? chunk : String(chunk);
      sock.end();
    });
    sock.on("close", () => {
      clearTimeout(timer);
      resolve(String(seen).trim() ? "ok" : "error:no-data");
    });
    sock.on("error", (e: Error) => {
      clearTimeout(timer);
      resolve("error:" + e.message);
    });
  });
}

const results: string[] = [];

if (mode === "primary") {
  // N sockets at once on ONE agent. Started together, awaited together, so the
  // concurrency is the same shape the worker arm has.
  const all = await Promise.all(
    Array.from({ length: agents }, (_, i) => echoOnce(`p9-${i}`)),
  );
  results.push(...all);
} else {
  const workerUrl = new URL("./_helpers/p9_socket_concurrency_worker.ts", import.meta.url);
  const workers: Worker[] = [];
  const waits: Promise<string>[] = [];
  for (let i = 0; i < agents; i++) {
    const w = new Worker(workerUrl, { workerData: { index: i, echoPort, budgetMs } });
    waits.push(
      new Promise<string>((resolve) => {
        w.on("message", (m: unknown) => resolve(String(m)));
        w.on("error", (e: Error) => resolve("worker-error:" + e.message));
        w.on("exit", () => resolve("worker-exit-without-answer"));
      }),
    );
    workers.push(w);
  }
  // A watchdog, because the failure includes a HANG and a hang cannot report
  // itself. `unref` keeps it off the happy path.
  const watchdog = setTimeout(() => {
    console.log(`mode=${mode} agents=${agents} WATCHDOG: ${results.length} answered`);
    process.exit(3);
  }, budgetMs + 5000);
  if (typeof (watchdog as { unref?: () => void }).unref === "function") {
    (watchdog as { unref: () => void }).unref();
  }
  results.push(...(await Promise.all(waits)));
  clearTimeout(watchdog);
}

const ok = results.filter((r) => r === "ok").length;
const failures = new Map<string, number>();
for (const r of results) {
  if (r !== "ok") failures.set(r, (failures.get(r) ?? 0) + 1);
}
const detail = [...failures.entries()].map(([k, v]) => `${k} x${v}`).join(", ");
console.log(
  `mode=${mode} agents=${agents} ok=${ok}/${agents}${detail ? ` failures: ${detail}` : ""}`,
);
process.exit(ok === agents ? 0 : 1);
