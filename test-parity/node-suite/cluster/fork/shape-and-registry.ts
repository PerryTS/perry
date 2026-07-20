// Node v26.5.0 primary.fork(): synchronous return shape and registry insertion.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.disconnect!();
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  console.log("worker:", worker instanceof cluster.Worker, worker.id);
  console.log(
    "process:",
    worker.process === (worker as any).process,
    typeof worker.process.pid,
  );
  console.log("registered:", cluster.workers?.[worker.id] === worker);
  console.log(
    "initial:",
    worker.state,
    worker.exitedAfterDisconnect,
    worker.isConnected(),
    worker.isDead(),
  );
  worker.once("exit", () => {
    clearTimeout(watchdog);
    console.log("removed:", cluster.workers?.[worker.id] === undefined);
  });
}
