// Worker.disconnect() returns itself, marks intentional disconnect, and closes
// the IPC channel before the process exits.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.on("message", () => {});
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () => {
    const returned = worker.disconnect();
    console.log("return/self:", returned === worker);
    console.log("intentional:", worker.exitedAfterDisconnect);
  });
  worker.once(
    "disconnect",
    () => console.log("connected after:", worker.isConnected()),
  );
  worker.once("exit", (code, signal) => {
    clearTimeout(watchdog);
    console.log("exit/dead:", code, signal, worker.isDead());
  });
}
