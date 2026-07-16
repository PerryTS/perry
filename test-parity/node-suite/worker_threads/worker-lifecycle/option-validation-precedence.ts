import { Worker } from "node:worker_threads";

function construct(label: string, options: Record<string, any>): Promise<void> {
  return new Promise((resolve) => {
    try {
      const worker = new Worker("./internal-thread-worker.cjs", options);
      console.log(label, "ok");
      worker.on("error", (error: any) => {
        console.log(label, "error", error?.name, error?.code ?? "");
      });
      worker.on("exit", (code) => {
        console.log(label, "exit", code);
        resolve();
      });
      worker.terminate();
    } catch (error: any) {
      console.log(label, error?.name, error?.code ?? "", error?.message);
      resolve();
    }
  });
}

async function main() {
  await construct("argv before env:", {
    argv: [{
      toString() {
        throw new Error("argv-coercion");
      },
    }],
    env: 42,
  });

  const envBuffer = new ArrayBuffer(8);
  await construct("env before transfer:", {
    env: 42,
    workerData: envBuffer,
    transferList: [envBuffer],
  });
  console.log("env ownership:", envBuffer.byteLength);

  const nameBuffer = new ArrayBuffer(8);
  await construct("name before transfer:", {
    name: {},
    workerData: nameBuffer,
    transferList: [nameBuffer],
  });
  console.log("name ownership:", nameBuffer.byteLength);
}

main().catch((error) =>
  console.log("unexpected:", error?.name, error?.message)
);
