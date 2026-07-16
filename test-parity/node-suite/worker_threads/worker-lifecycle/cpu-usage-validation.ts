import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function outcome(fn: () => any): string {
  try {
    const value = fn();
    value?.catch?.(() => {});
    return `ok:${value?.constructor?.name ?? typeof value}`;
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

const worker = new Worker("./diagnostics-hold-worker.cjs");
worker.once("message", async () => {
  console.log(
    "accepted:",
    [undefined, null, Number.NaN].map((value) =>
      outcome(() => worker.cpuUsage(value as any))
    ).join(","),
  );
  console.log(
    "rejected:",
    [-1, 1.1, {}, [], true, Infinity].map((value) =>
      outcome(() => worker.cpuUsage(value as any))
    ).join(","),
  );
  console.log("terminate:", await worker.terminate());
});
worker.on("error", (error: any) => {
  console.log("error:", error?.name, error?.code ?? "");
});
