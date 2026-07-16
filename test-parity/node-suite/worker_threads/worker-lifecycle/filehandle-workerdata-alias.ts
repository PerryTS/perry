import { open } from "node:fs/promises";
import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function main() {
  const handle = await open("./process-env-option-worker.cjs", "r");
  let worker: Worker;
  try {
    worker = new Worker("./filehandle-workerdata-alias-worker.cjs", {
      workerData: {
        direct: handle,
        alias: handle,
        map: new Map([["handle", handle]]),
        set: new Set([handle]),
      },
      transferList: [handle as any],
    });
  } catch (error: any) {
    console.log("construction:", error?.name, error?.code ?? "");
    await handle.close().catch(() => {});
    return;
  }

  console.log("parent detached:", handle.fd === -1);
  worker.on("message", async (message) => {
    console.log("worker:", JSON.stringify(message));
    try {
      await handle.readFile();
      console.log("parent read: ok");
    } catch (error: any) {
      console.log("parent read:", error?.name, error?.code ?? "");
    }
  });
  worker.on("error", (error: any) => {
    console.log("error:", error?.name, error?.code ?? "");
  });
  worker.on("exit", async (code) => {
    console.log("exit:", code);
    await handle.close().catch(() => {});
  });
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
