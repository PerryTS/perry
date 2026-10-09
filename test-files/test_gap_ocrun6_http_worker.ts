// HTTP client callbacks and the values they read belong to the issuing worker.
import http from "node:http";
import { Worker } from "node:worker_threads";

const server = http.createServer((_req, res) => {
  res.setHeader("x-probe", "yes");
  res.end("ok");
});
await new Promise<void>(resolve => server.listen(0, "127.0.0.1", resolve));
const worker = new Worker(new URL("./_helpers/ocrun6_http_worker.ts", import.meta.url), {
  workerData: (server.address() as any).port,
});
const watchdog = setTimeout(() => {
  console.log("worker timed out");
  process.exit(3);
}, 10000);
const result = await new Promise<string>(resolve => {
  worker.on("message", resolve);
  worker.on("error", error => resolve("worker error: " + error.message));
});
clearTimeout(watchdog);
console.log(result);
await worker.terminate();
server.close();
