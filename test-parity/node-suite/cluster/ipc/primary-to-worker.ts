// Worker.send round trip with a self-contained JSON payload.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.once("message", (message) => process.send!({ echo: message }));
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () => {
    console.log("send boolean:", worker.send({ text: "hello", n: 7 }));
  });
  worker.once("message", (message) => {
    console.log("reply:", JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
