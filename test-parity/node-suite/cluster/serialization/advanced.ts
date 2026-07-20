// Node and Bun cluster advanced-serialization selections cover structured
// clone values that JSON cannot preserve.
import cluster from "node:cluster";

cluster.setupPrimary({ serialization: "advanced" });
if (cluster.isWorker) {
  process.once("message", (message: any) => {
    process.send!({
      date: message.date instanceof Date && message.date.toISOString(),
      map: message.map instanceof Map && Array.from(message.map.entries()),
      bigint: typeof message.bigint === "bigint" && message.bigint.toString(),
      typed: message.typed instanceof Uint16Array && Array.from(message.typed),
    });
  });
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () =>
    worker.send({
      date: new Date("2020-01-02T03:04:05.000Z"),
      map: new Map([["key", 9]]),
      bigint: 12345678901234567890n,
      typed: new Uint16Array([2, 500]),
    }));
  worker.once("message", (message) => {
    console.log(JSON.stringify(message));
    worker.disconnect();
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
