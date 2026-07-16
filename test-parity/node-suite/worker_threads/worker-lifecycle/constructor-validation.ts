import { MessageChannel, Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

async function outcome(create: () => Worker): Promise<string> {
  try {
    const worker = create();
    await worker.terminate();
    return "ok";
  } catch (error: any) {
    return `${error?.name}:${error?.code ?? ""}`;
  }
}

async function main() {
  console.log(
    "uncloneable workerData:",
    await outcome(() => new Worker("./constructor-validation-worker.cjs", {
      workerData: () => {},
    })),
  );

  const duplicate = new ArrayBuffer(8);
  console.log(
    "duplicate transfer:",
    await outcome(() => new Worker("./constructor-validation-worker.cjs", {
      workerData: duplicate,
      transferList: [duplicate, duplicate],
    })),
    duplicate.byteLength,
  );

  console.log(
    "invalid transfer:",
    await outcome(() => new Worker("./constructor-validation-worker.cjs", {
      transferList: [null as any],
    })),
  );

  const channel = new MessageChannel();
  console.log(
    "missing port transfer:",
    await outcome(() => new Worker("./constructor-validation-worker.cjs", {
      workerData: channel.port1,
    })),
  );
  channel.port1.close();
  channel.port2.close();
}

main().catch((error) => {
  console.log("unexpected:", error?.name, error?.message);
});
