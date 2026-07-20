// Default JSON serialization preserves JSON values and Buffer's toJSON form.
import cluster from "node:cluster";
import { Buffer } from "node:buffer";

if (cluster.isWorker) {
  process.once("message", (message: any) => {
    process.send!({
      nested: message.nested,
      bufferType: message.buffer?.type,
      bufferData: message.buffer?.data,
    });
  });
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once(
    "online",
    () =>
      worker.send({ nested: { ok: true }, buffer: Buffer.from([1, 2, 255]) }),
  );
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
