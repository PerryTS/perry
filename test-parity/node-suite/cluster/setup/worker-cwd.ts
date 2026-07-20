// Node test-cluster-cwd.js with a repository-controlled working directory.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.send!({
    suffix: process.cwd().endsWith(
      "test-parity/node-suite/cluster/fixtures/cwd",
    ),
    absolute: process.cwd().startsWith("/"),
  });
} else {
  cluster.setupPrimary({ cwd: "test-parity/node-suite/cluster/fixtures/cwd" });
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
