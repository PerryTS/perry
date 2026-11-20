// Node emits fork on the cluster, then online on the Worker, then on cluster.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.disconnect!();
} else {
  const order: string[] = [];
  cluster.once("fork", () => order.push("cluster:fork"));
  cluster.once("online", () => order.push("cluster:online"));
  const worker = cluster.fork();
  order.push("returned");
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", function () {
    order.push(`worker:online:${this === worker}`);
  });
  worker.once("exit", () => {
    clearTimeout(watchdog);
    console.log(order.join("|"));
  });
}
