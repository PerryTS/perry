// Node Worker.send delegates to ChildProcess.send argument validation.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.on("message", () => {});
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () => {
    for (const value of [undefined, Symbol("probe"), () => {}]) {
      try {
        worker.send(value as any);
        console.log(typeof value, "accepted");
      } catch (error: any) {
        console.log(typeof value, error.name, error.code);
      }
    }
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
