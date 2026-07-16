import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./natural-exit-worker.cjs");
const stdin = worker.stdin;
const stdout = worker.stdout;
const stderr = worker.stderr;

function outcome(fn: () => any): string {
  try {
    fn();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

worker.on("exit", (code) => {
  console.log("exit:", code, worker.threadId, worker.threadName);
  console.log("postMessage:", outcome(() => worker.postMessage("after-exit")));
  console.log("ref:", outcome(() => worker.ref()));
  console.log("unref:", outcome(() => worker.unref()));
  console.log(
    "stream identity:",
    worker.stdin === stdin,
    worker.stdout === stdout,
    worker.stderr === stderr,
  );
});
