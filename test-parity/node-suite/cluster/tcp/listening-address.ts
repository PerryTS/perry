// A single worker listening on port 0 emits a normalized address descriptor.
import cluster from "node:cluster";
import { createServer } from "node:net";

if (cluster.isWorker) {
  createServer((socket) => socket.end()).listen(0, "127.0.0.1");
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("listening", (address) => {
    console.log(
      "worker address:",
      address.address,
      address.addressType,
      Number.isInteger(address.port),
      address.port > 0,
      address.fd,
    );
    worker.disconnect();
  });
  cluster.once("listening", (value, address) => {
    console.log(
      "cluster address:",
      value === worker,
      address.address,
      address.addressType,
      address.port > 0,
    );
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
