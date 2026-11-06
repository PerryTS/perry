// The child process and IPC channel expose deterministic connection and
// ref/unref controls while the worker is online.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.on("message", () => process.disconnect!());
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  worker.once("online", () => {
    const channel = worker.process.channel as any;
    console.log(
      "process:",
      worker.process.connected,
      typeof worker.process.send,
      typeof worker.process.disconnect,
      typeof worker.process.kill,
    );
    console.log(
      "channel:",
      channel !== null,
      typeof channel?.ref,
      typeof channel?.unref,
    );
    channel?.unref();
    channel?.ref();
    worker.send("stop");
  });
  worker.once("exit", () => clearTimeout(watchdog));
}
