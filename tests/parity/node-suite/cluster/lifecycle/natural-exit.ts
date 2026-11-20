// A worker's normal exit reports disconnect before exit with stable payloads.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.disconnect!();
} else {
  const events: string[] = [];
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.on("disconnect", () => events.push("worker:disconnect"));
  cluster.on(
    "disconnect",
    (value) => events.push(`cluster:disconnect:${value === worker}`),
  );
  worker.on(
    "exit",
    (code, signal) => events.push(`worker:exit:${code}:${signal}`),
  );
  cluster.on("exit", (value, code, signal) => {
    events.push(`cluster:exit:${value === worker}:${code}:${signal}`);
    clearTimeout(watchdog);
    console.log(events.join("|"));
  });
}
