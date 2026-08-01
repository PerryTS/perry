// Advanced IPC framing must coexist with cluster's SCHED_RR socket handoff.
import cluster from "node:cluster";
import { connect, createServer } from "node:net";

if (cluster.isPrimary) cluster.setupPrimary({ serialization: "advanced" });

if (cluster.isWorker) {
  createServer((socket) => socket.end("advanced-ok")).listen(0, "127.0.0.1");
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("listening", (address) => {
    const socket = connect(address.port, "127.0.0.1");
    let data = "";
    socket.setEncoding("utf8");
    socket.on("data", (chunk) => data += chunk);
    socket.once("end", () => {
      console.log("response:", data);
      worker.disconnect();
    });
  });
  worker.once("exit", (code, signal) => {
    clearTimeout(watchdog);
    console.log("exit:", code, signal);
  });
}
