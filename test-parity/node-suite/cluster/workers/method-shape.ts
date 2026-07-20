// Real primary Worker method ownership, arity, and aliases.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.disconnect!();
} else {
  const worker = cluster.fork();
  const watchdog = setTimeout(() => worker.kill(), 5_000);
  watchdog.unref();
  for (
    const name of [
      "send",
      "kill",
      "destroy",
      "disconnect",
      "isConnected",
      "isDead",
    ] as const
  ) {
    console.log(
      name,
      typeof (worker as any)[name],
      (worker as any)[name]?.length,
      Object.hasOwn(worker, name),
    );
  }
  console.log("kill/destroy same:", worker.kill === worker.destroy);
  worker.once("exit", () => clearTimeout(watchdog));
}
