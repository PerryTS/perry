import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const worker = new Worker("./termination-worker.cjs");

function outcome(label: string, fn: () => void) {
  try {
    fn();
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error?.name, error?.code ?? "");
  }
}

outcome("on null:", () => (worker as any).on("message", null));
outcome("once number:", () => (worker as any).once("message", 1));
outcome(
  "addListener object:",
  () => (worker as any).addListener("message", {}),
);
outcome("off undefined:", () => (worker as any).off("message", undefined));

worker.terminate().then((code) => console.log("terminate:", code));
worker.on("exit", (code) => console.log("exit:", code));
