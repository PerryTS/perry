// Worker-side role flags and NODE_UNIQUE_ID consumption from Node's basic test.
import cluster from "node:cluster";
import { EventEmitter } from "node:events";

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
  cluster.worker!.send({
    isPrimary: cluster.isPrimary,
    isMaster: cluster.isMaster,
    isWorker: cluster.isWorker,
    id: cluster.worker?.id,
    state: cluster.worker?.state,
    processSame: cluster.worker?.process === process,
    connected: cluster.worker?.isConnected(),
    workerInstance: cluster.worker instanceof cluster.Worker,
    emitterInstance: cluster.worker instanceof EventEmitter,
    env: process.env.CLUSTER_PROBE,
    uniqueIdRemoved: process.env.NODE_UNIQUE_ID === undefined,
  });
}
