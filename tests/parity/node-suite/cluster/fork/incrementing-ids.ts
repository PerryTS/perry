// Worker ids are monotonic and cluster.workers tracks both live workers.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.on("message", () => process.disconnect!());
} else {
  const first = cluster.fork();
  const second = cluster.fork();
  const workers = [first, second];
  const watchdog = setTimeout(() => workers.forEach((w) => w.kill()), 5_000);
  watchdog.unref();
  console.log("ids:", first.id, second.id);
  console.log("registry:", Object.keys(cluster.workers ?? {}).sort().join(","));
  let online = 0;
  let exited = 0;
  for (const worker of workers) {
    worker.once("online", () => {
      if (++online === workers.length) workers.forEach((w) => w.send("stop"));
    });
    worker.once("exit", () => {
      if (++exited === workers.length) {
        clearTimeout(watchdog);
        console.log("empty:", Object.keys(cluster.workers ?? {}).length);
      }
    });
  }
}
