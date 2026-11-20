// A replacement worker receives the next id and the dead worker is absent from
// cluster.workers before respawn completes.
import cluster from "node:cluster";

if (cluster.isWorker) {
  process.disconnect!();
} else {
  const first = cluster.fork();
  const watchdog = setTimeout(() => {
    first.kill();
    Object.values(cluster.workers ?? {}).forEach((worker) => worker?.kill());
  }, 5_000);
  watchdog.unref();
  first.once("exit", () => {
    console.log("first removed:", cluster.workers?.[first.id] === undefined);
    const second = cluster.fork();
    console.log("ids:", first.id, second.id);
    second.once("exit", () => {
      clearTimeout(watchdog);
      console.log(
        "second removed:",
        cluster.workers?.[second.id] === undefined,
      );
    });
  });
}
