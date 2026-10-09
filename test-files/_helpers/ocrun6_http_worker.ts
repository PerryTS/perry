import http from "node:http";
import { parentPort, workerData, isMainThread } from "node:worker_threads";

// Keep a message consumer installed, as a worker serving RPC requests does.
parentPort!.on("message", () => {});
const url = "http://127.0.0.1:" + workerData;
let results: string[] = [];
// The second request takes an Agent socket slot when it is constructed.
const agent = new http.Agent({ keepAlive: true });

function response(res: any) {
  results.push([
    isMainThread, typeof res.headers, res.statusCode,
    res.headers["x-probe"], Object.keys(res.headers).includes("x-probe"),
  ].join(" "));
  res.on("data", () => {});
  res.on("end", () => {
    if (results.length === 1) {
      const request = http.request(url, { agent });
      request.on("response", response);
      request.end();
    } else {
      agent.destroy();
      parentPort!.postMessage(results.join("\n"));
    }
  });
}
http.get(url, response);
