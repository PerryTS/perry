// Portable default Worker.kill()/destroy() termination metadata.
import cluster from "node:cluster";

if (cluster.isWorker) {
  setInterval(() => {}, 1_000);
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.process.kill("SIGKILL"), 5_000);
  watchdog.unref();
  worker.once("online", () => {
    console.log("kill returns:", worker.kill() === undefined);
  });
  worker.once("exit", (code, signal) => {
    clearTimeout(watchdog);
    console.log("exit:", code, signal);
    console.log(
      "flags:",
      worker.exitedAfterDisconnect,
      worker.isConnected(),
      worker.isDead(),
    );
  });
}
