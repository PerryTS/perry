import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

try {
  const worker = new Worker("./natural-exit-worker.cjs", {
    env: { NODE_OPTIONS: "--title=worker" },
  });
  console.log("construct: ok");
  worker.on(
    "error",
    (error) => console.log("error:", error.name, (error as any).code ?? ""),
  );
  worker.on("exit", (code) => console.log("exit:", code));
} catch (error) {
  console.log("construct:", (error as any)?.name, (error as any)?.code ?? "");
}
