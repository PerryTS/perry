import { open } from "node:fs/promises";
import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function main() {
  const handle = await open("./process-env-option-worker.cjs", "r");
  try {
    const worker = new Worker("./natural-exit-worker.cjs", {
      name: {} as any,
      workerData: { handle },
      transferList: [handle as any],
    });
    console.log("construction: ok", handle.fd === -1);
    worker.on("error", (error: any) => {
      console.log("error:", error?.name, error?.code ?? "");
    });
    worker.on("exit", async (code) => {
      console.log("exit:", code, handle.fd === -1);
      const buffer = Buffer.alloc(1);
      try {
        const result = await handle.read(buffer, 0, 1, 0);
        console.log("parent readable:", result.bytesRead, buffer[0]);
      } catch (readError: any) {
        console.log("parent read:", readError?.code ?? readError?.name);
      }
      await handle.close().catch(() => {});
    });
  } catch (error: any) {
    console.log(
      "construction:",
      error?.name,
      error?.code ?? "",
      handle.fd === -1,
    );
    const buffer = Buffer.alloc(1);
    try {
      const result = await handle.read(buffer, 0, 1, 0);
      console.log("parent readable:", result.bytesRead, buffer[0]);
    } catch (readError: any) {
      console.log("parent read:", readError?.code ?? readError?.name);
    }
    await handle.close().catch(() => {});
  }
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
