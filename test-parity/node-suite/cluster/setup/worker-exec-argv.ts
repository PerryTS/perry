// setupPrimary execArgv is forwarded independently from ordinary worker args.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.send!({ execArgv: process.execArgv, args: process.argv.slice(2) });
} else {
  cluster.setupPrimary({ execArgv: ["--no-warnings"], args: ["ordinary"] });
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
