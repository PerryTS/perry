import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function outcome(fn: () => Promise<any>): Promise<string> {
  try {
    const result = await fn();
    return `ok:${result?.constructor?.name ?? typeof result}`;
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

async function main() {
  const worker = new Worker("./natural-exit-worker.cjs");
  const code = await new Promise<number>((resolve) =>
    worker.once("exit", resolve)
  );
  try {
    const utilization = worker.performance.eventLoopUtilization();
    console.log(
      "utilization:",
      Object.keys(utilization).sort().join(","),
      [
        typeof utilization.idle,
        typeof utilization.active,
        typeof utilization.utilization,
      ].join(","),
      utilization.idle === 0 && utilization.active === 0 &&
        utilization.utilization === 0,
    );
  } catch (error: any) {
    console.log("utilization:", error?.name, error?.code ?? "");
  }
  console.log("heap:", await outcome(() => worker.getHeapStatistics()));
  console.log("cpu:", await outcome(() => worker.cpuUsage()));
  console.log("exit:", code);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
