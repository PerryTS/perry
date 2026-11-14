// The worker's process.execArgv crosses the fork as a JSON array of strings
// (PERRY_PROCESS_EXEC_ARGV in a native self-fork): quotes, backslashes and
// non-ASCII text in a flag value come back unchanged, as does the order.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.send!({ execArgv: process.execArgv });
} else {
  console.log("primary execArgv", JSON.stringify(process.execArgv));
  cluster.setupPrimary({
    execArgv: ["--no-warnings", '--title=café "q" \\ / \u{1f600}', "--no-deprecation"],
  });
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("message", (message: { execArgv: string[] }) => {
    console.log(JSON.stringify(message.execArgv));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
