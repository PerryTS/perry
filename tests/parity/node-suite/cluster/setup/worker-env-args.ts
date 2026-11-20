// setupPrimary args plus per-fork env overlays are observable in the worker.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.send!({
    args: process.argv.slice(2),
    shared: process.env.SHARED,
    local: process.env.LOCAL,
  });
} else {
  process.env.SHARED = "parent";
  cluster.setupPrimary({ args: ["alpha", "beta"] });
  const worker = cluster.fork({ LOCAL: "fork" });
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
