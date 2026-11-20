// After the disconnect event, send returns false and reports the closed-channel
// error through its callback instead of throwing or leaking the worker.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.on("message", () => {});
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () => worker.disconnect());
  worker.once("disconnect", () => {
    const returned = worker.send("late", (error: any) => {
      console.log("callback:", error?.name, error?.code);
    });
    console.log("returned:", returned);
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
