// Additional deterministic advanced-serialization built-ins selected from
// Node's and Bun's cluster suites.
import cluster from "node:cluster";

if (cluster.isPrimary) cluster.setupPrimary({ serialization: "advanced" });
if (cluster.isWorker) {
  process.once("message", (message: any) =>
    process.send!({
      regexp: message.regexp instanceof RegExp &&
        `${message.regexp.source}/${message.regexp.flags}`,
      arrayBuffer: message.arrayBuffer instanceof ArrayBuffer &&
        Array.from(new Uint8Array(message.arrayBuffer)),
      set: message.set instanceof Set && Array.from(message.set),
      error: message.error instanceof Error &&
        [message.error.name, message.error.message],
    }));
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () =>
    worker.send({
      regexp: /cluster/gi,
      arrayBuffer: new Uint8Array([4, 8, 15]).buffer,
      set: new Set(["a", "b"]),
      error: new TypeError("probe"),
    }));
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
