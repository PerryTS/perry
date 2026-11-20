import { isMainThread, parentPort, receiveMessageOnPort, workerData } from "node:worker_threads";

if (!isMainThread) {
  const queued = receiveMessageOnPort(workerData.extraQueued);
  const emptyQueued = receiveMessageOnPort(workerData.extraQueued);
  if (queued?.message !== "queued" || emptyQueued !== undefined) {
    throw new Error("Queued WorkerData port did not retain its pending message.");
  }
  workerData.extraQueued.close();
  const endpoint = workerData.endpoint;
  endpoint.on("message", (value: string) => {
    if (value === "ping") {
      endpoint.postMessage({ kind: "pong", value, alias: endpoint === workerData.alias,
        controlDistinct: endpoint !== parentPort });
    } else if (value === "stop") {
      endpoint.close();
      parentPort!.once("message", () => parentPort!.close());
      parentPort!.postMessage("done");
    }
  });
  endpoint.postMessage({ kind: "ready" });
}
