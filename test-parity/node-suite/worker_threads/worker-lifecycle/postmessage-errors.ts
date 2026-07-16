import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./postmessage-errors-worker.cjs");

function outcome(label: string, value: any, transfer?: readonly any[]) {
  try {
    worker.postMessage(value, transfer as any);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

worker.on("online", () => {
  outcome("function:", () => 1);
  outcome("symbol:", Symbol("value"));

  const buffer = new ArrayBuffer(8);
  outcome("rollback:", { buffer, invalid() {} }, [buffer]);
  console.log("buffer retained:", buffer.byteLength);

  worker.postMessage("finish");
});
worker.on("message", (message) => {
  console.log("message:", message);
  worker.terminate().then((code) => console.log("terminate:", code));
});
worker.on("exit", (code) => console.log("exit:", code));
