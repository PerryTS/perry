// The send callback is an explicit completion barrier for IPC queueing.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.once("message", () => process.send!("ack"));
} else {
  const worker = cluster.fork();
  const order: string[] = [];
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () => {
    order.push("online");
    worker.send("ping", (error) => order.push(`callback:${error === null}`));
  });
  worker.once("message", () => {
    order.push("message");
    worker.disconnect();
  });
  worker.once("exit", () => {
    clearTimeout(watchdog);
    console.log(order.join("|"));
  });
}
