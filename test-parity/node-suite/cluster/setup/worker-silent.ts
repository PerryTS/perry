// silent:true pipes worker stdio; the write callback plus data event form an
// explicit completion barrier before disconnect.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.stdout.write("worker-output\n", () => process.send!("written"));
} else {
  cluster.setupPrimary({ silent: true });
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  let data = "";
  let written = false;
  (worker.process.stdout as any).setEncoding("utf8");
  worker.process.stdout!.on("data", (chunk) => {
    data += chunk;
    finish();
  });
  worker.once("message", () => {
    written = true;
    finish();
  });
  function finish() {
    if (!written || !data.includes("worker-output")) return;
    console.log("piped:", worker.process.stdout !== null, data.trim());
    worker.disconnect();
  }
  worker.once("exit", () => clearTimeout(watchdog));
}
