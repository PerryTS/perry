// cluster.disconnect waits for every live worker and calls its callback once.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.on("message", () => {});
} else {
  const workers = [cluster.fork(), cluster.fork()];
  const watchdog = setTimeout(
    () => workers.forEach((worker) => worker.kill()),
    5_000,
  );
  watchdog.unref();
  let online = 0;
  let exits = 0;
  for (const worker of workers) {
    worker.once("online", () => {
      if (++online === workers.length) {
        console.log(
          "disconnect return:",
          cluster.disconnect(() => console.log("callback")) === undefined,
        );
      }
    });
    worker.once("exit", () => {
      if (++exits === workers.length) {
        clearTimeout(watchdog);
        console.log("all exited:", Object.keys(cluster.workers ?? {}).length);
      }
    });
  }
}
