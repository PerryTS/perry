// Node test-cluster-message.js: process.send reaches both Worker and cluster
// message listeners with the same worker and payload.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.send!({ kind: "ready", values: [1, 2, 3] });
} else {
  const worker = cluster.fork();
  const seen: string[] = [];
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("message", (message) => {
    seen.push(`worker:${JSON.stringify(message)}`);
    if (seen.length === 2) worker.disconnect();
  });
  cluster.once("message", (value, message) => {
    seen.push(`cluster:${value === worker}:${JSON.stringify(message)}`);
    if (seen.length === 2) worker.disconnect();
  });
  worker.once("exit", () => {
    clearTimeout(watchdog);
    console.log(seen.sort().join("|"));
  });
}
