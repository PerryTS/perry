// The Worker half of `p9_socket_concurrency.ts`: one socket round-trip on this
// agent, and the result posted back as a string. No teardown of its own -- the
// parent leaves by `process.exit`, so nothing here can be blamed on a
// terminate() that did not return.
//
// The payload carries this agent's index and the answer is checked AGAINST IT,
// not merely for being non-empty. That is the difference between "my socket
// produced data" and "my socket produced MY data": if the per-socket event queue
// this path drains is shared between agents, a worker can be handed another
// worker's bytes, and a probe that only checked for non-emptiness would score
// that as a pass.
import net from "node:net";
import { parentPort, workerData } from "node:worker_threads";

const data = (workerData ?? {}) as { index?: number; echoPort?: number; budgetMs?: number };
const index = data.index ?? 0;
const echoPort = data.echoPort ?? 8098;
const budgetMs = data.budgetMs ?? 30000;
const payload = `p9-${index}`;

const answer = await new Promise<string>((resolve) => {
  const sock = net.connect(echoPort, "127.0.0.1");
  let seen = "";
  const timer = setTimeout(() => resolve("error:timeout"), budgetMs);
  sock.on("connect", () => sock.write(`${payload}\n`));
  sock.on("data", (chunk: unknown) => {
    seen += typeof chunk === "string" ? chunk : String(chunk);
    sock.end();
  });
  sock.on("close", () => {
    clearTimeout(timer);
    const got = String(seen).trim();
    if (got === payload) resolve("ok");
    else if (got === "") resolve("error:no-data");
    else resolve(`error:CROSSED-AGENT got=${JSON.stringify(got)} want=${JSON.stringify(payload)}`);
  });
  sock.on("error", (e: Error) => {
    clearTimeout(timer);
    resolve("error:" + e.message);
  });
});

parentPort?.postMessage(answer);
