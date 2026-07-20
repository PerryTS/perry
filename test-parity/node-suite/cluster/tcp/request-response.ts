// End-to-end ephemeral TCP sharing foundation with socket/server completion
// barriers before graceful worker disconnect.
import cluster from "node:cluster";
import { connect, createServer } from "node:net";

if (cluster.isWorker) {
  createServer((socket) => socket.end("cluster-ok")).listen(0, "127.0.0.1");
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
