import { Worker } from "node:worker_threads";

process.chdir("tests/parity/node-suite/worker_threads/worker-lifecycle");

async function run(mode: string) {
  const worker = new Worker("./process-exit-worker.cjs", { workerData: mode });
  const events: string[] = [];
  worker.on("message", (message) => {
    events.push(String(message));
    if ((mode === "terminate" || mode === "interrupt") && message === "ready") {
      void worker.terminate();
    }
  });
  worker.on("error", (error: any) => events.push(`error:${error.message}`));
  await new Promise<void>((resolve) => {
    worker.on("exit", (code) => {
      console.log(mode + ":", events.join(",") || "none", "exit:", code);
      resolve();
    });
  });
}

await run("explicit");
await run("natural");
await run("stored");
await run("override");
await run("reentrant");
await run("terminate");
await run("interrupt");
console.log("parent: alive");
