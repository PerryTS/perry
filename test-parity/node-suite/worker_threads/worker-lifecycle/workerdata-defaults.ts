import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function run(label: string, options?: { workerData?: any }): Promise<void> {
  return new Promise((resolve) => {
    const worker = new Worker("./default-data-worker.cjs", options);
    worker.on("message", (message: any) => {
      console.log(
        `${label}:`,
        message?.isMainThread,
        message?.type,
        message?.isNull,
        message?.value,
      );
    });
    worker.on("exit", (code) => {
      console.log(`${label} exit:`, code);
      resolve();
    });
  });
}

async function main() {
  await run("omitted");
  await run("undefined", { workerData: undefined });
  await run("null", { workerData: null });
  await run("object", { workerData: { value: 5 } });
}

main().catch((error) => console.log("unexpected:", error?.name, error?.message));
