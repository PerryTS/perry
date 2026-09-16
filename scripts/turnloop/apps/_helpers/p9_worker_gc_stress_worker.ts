// The Worker half of `p9_worker_gc_stress.ts`.
//
// A worker agent's loop holds JS values across completions — the promise a
// `fetch` will settle, the listeners a socket will call — so it is exactly the
// shape #7154's instruments exist for. The stress arm needs two things the
// acceptance fixture does not have:
//
//  1. an ALLOCATING LOOP between submission and await, or no back-edge poll is
//     emitted, the seeded schedule selects nothing, and `PERRY_GC_SCHEDULE_*`
//     exits 70 saying the run proved nothing (P6 and P7 both hit this); and
//  2. a retained object graph, so an evacuation has something to move and
//     something to rewrite.
import net from "node:net";
import { parentPort } from "node:worker_threads";

const url = process.env.P9_URL ?? "http://127.0.0.1:8099/";
const echoPort = Number(process.env.P9_ECHO_PORT ?? "8098");

function churn(retained: string[][]): number {
  let n = 0;
  for (let i = 0; i < 4000; i++) {
    const row = [`p9-${i}`, `row-${i}`, `${i * 3}`];
    if (i % 400 === 0) retained.push(row);
    n += row[0].length + row[1].length + row[2].length;
  }
  return n;
}

const retained: string[][] = [];

const inflight = fetch(url);
let churned = churn(retained);
const res = await inflight;
const bytes = (await res.text()).length;
churned += churn(retained);

const echoed = await new Promise<string>((resolve) => {
  const sock = net.connect(echoPort, "127.0.0.1");
  let seen = "";
  sock.on("connect", () => {
    sock.write("p9-gc\n");
    churned += churn(retained);
  });
  sock.on("data", (chunk: unknown) => {
    seen += typeof chunk === "string" ? chunk : String(chunk);
    sock.end();
  });
  sock.on("close", () => resolve(String(seen).trim()));
  sock.on("error", (e: Error) => resolve("error:" + e.message));
});
churned += churn(retained);

// Read the retained graph AFTER every collection point, so a moved-but-not-
// rewritten slot shows up as wrong output rather than as nothing at all.
let fingerprint = 0;
for (const row of retained) fingerprint += row[0].length + row[2].length;

console.log(
  `worker-agent gc-stress: status=${res.status} bytes=${bytes} ` +
    `echo=${JSON.stringify(echoed)} retained=${retained.length} ` +
    `fingerprint=${fingerprint} churned=${churned}`,
);
parentPort?.postMessage("done");
