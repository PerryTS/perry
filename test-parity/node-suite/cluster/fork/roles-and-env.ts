// Worker-side role flags and NODE_UNIQUE_ID consumption from Node's basic test.
import cluster from "node:cluster";

if (cluster.isPrimary) {
  const worker = cluster.fork({ CLUSTER_PROBE: "present" });
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
} else {
  process.send!({
    isPrimary: cluster.isPrimary,
    isMaster: cluster.isMaster,
    isWorker: cluster.isWorker,
    id: cluster.worker?.id,
    env: process.env.CLUSTER_PROBE,
    uniqueIdRemoved: process.env.NODE_UNIQUE_ID === undefined,
  });
}
