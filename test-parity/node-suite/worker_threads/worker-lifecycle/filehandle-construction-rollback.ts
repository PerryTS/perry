import { open } from "node:fs/promises";
import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function readable(handle: any): Promise<string> {
  try {
    const buffer = Buffer.alloc(5);
    const result = await handle.read(buffer, 0, buffer.length, 0);
    return `${result.bytesRead}:${buffer.toString("utf8")}`;
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

async function attempt(
  label: string,
  workerData: (handle: any) => unknown,
  transferList: (handle: any) => any[],
) {
  const handle = await open("./process-env-option-worker.cjs", "r");
  let worker: Worker | undefined;
  let result = "created";

  try {
    worker = new Worker("./constructor-validation-worker.cjs", {
      workerData: workerData(handle),
      transferList: transferList(handle),
    });
  } catch (error: any) {
    result = `${error?.name}:${error?.code ?? ""}`;
  } finally {
    await worker?.terminate().catch(() => {});
  }

  console.log(label, result, handle.fd >= 0, await readable(handle));
  await handle.close().catch(() => {});
}

async function main() {
  await attempt(
    "clone rollback:",
    (handle) => ({ handle, bad: () => {} }),
    (handle) => [handle],
  );
  await attempt(
    "duplicate rollback:",
    (handle) => ({ handle }),
    (handle) => [handle, handle],
  );
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
