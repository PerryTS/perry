import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

const previous = process.env.PERRY_PROCESS_ENV_OPTION;
process.env.PERRY_PROCESS_ENV_OPTION = "parent-value";

function restore() {
  if (previous === undefined) {
    delete process.env.PERRY_PROCESS_ENV_OPTION;
  } else {
    process.env.PERRY_PROCESS_ENV_OPTION = previous;
  }
}

try {
  const worker = new Worker("./process-env-option-worker.cjs", {
    env: process.env,
  });

  worker.on("message", (value) => console.log("worker:", value));
  worker.on("error", (error: any) => {
    console.log("error:", error?.name, error?.code ?? "");
  });
  worker.on("exit", (code) => {
    console.log("exit:", code, process.env.PERRY_PROCESS_ENV_OPTION);
    restore();
  });
} catch (error: any) {
  console.log("construction:", error?.name, error?.code ?? "");
  restore();
}
