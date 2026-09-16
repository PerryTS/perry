// The Worker half of `p9_socket_concurrency.ts`: one socket round-trip on this
// agent, and the result posted back as a string. No teardown of its own -- the
// parent leaves by `process.exit`, so nothing here can be blamed on a
// terminate() that did not return.
import net from "node:net";
import { parentPort, workerData } from "node:worker_threads";

const data = (workerData ?? {}) as { index?: number; echoPort?: number; budgetMs?: number };
const index = data.index ?? 0;
const echoPort = data.echoPort ?? 8098;
const budgetMs = data.budgetMs ?? 30000;

const answer = await new Promise<string>((resolve) => {
  const sock = net.connect(echoPort, "127.0.0.1");
  let seen = "";
  const timer = setTimeout(() => resolve("error:timeout"), budgetMs);
  sock.on("connect", () => sock.write(`p9-${index}\n`));
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

parentPort?.postMessage(answer);
