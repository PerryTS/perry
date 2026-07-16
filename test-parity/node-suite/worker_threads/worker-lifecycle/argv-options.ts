import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function start(label: string, argv: any) {
  try {
    const worker = new Worker("./argv-worker.cjs", { argv });
    worker.on("message", (message) => console.log(label, message));
    worker.on(
      "error",
      (error: any) =>
        console.log(label, "error", error?.name, error?.code ?? ""),
    );
    worker.on("exit", (code) => console.log(label, "exit", code));
  } catch (error: any) {
    console.log(label, "throw", error?.name, error?.code ?? "");
  }
}

start("coerced:", [null, "text", 123, true]);
start("invalid option:", "text");
start("invalid entry:", [Symbol("value")]);
