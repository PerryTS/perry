import { Worker } from "node:worker_threads";

process.chdir("test-parity/node-suite/worker_threads/worker-lifecycle");

function values(buffer: ArrayBuffer): string {
  try {
    return Array.from(new Uint8Array(buffer)).join(",");
  } catch {
    return "detached";
  }
}

function probe(
  label: string,
  create: (buffer: ArrayBuffer) => Worker,
) {
  const buffer = new Uint8Array([9, 8, 7]).buffer;
  let status: string;
  let worker: Worker | undefined;

  try {
    worker = create(buffer);
    status = "created";
  } catch (error: any) {
    status = `${error?.name}:${error?.code ?? ""}`;
  }

  console.log(label, status, buffer.byteLength, values(buffer));
  worker?.terminate().catch(() => {});
}

probe(
  "name:",
  (buffer) =>
    new Worker("./constructor-validation-worker.cjs", {
      name: {} as any,
      workerData: buffer,
      transferList: [buffer],
    }),
);
probe(
  "invalid list:",
  (buffer) =>
    new Worker("./constructor-validation-worker.cjs", {
      workerData: buffer,
      transferList: [buffer, null as any],
    }),
);
probe(
  "clone rollback:",
  (buffer) =>
    new Worker("./constructor-validation-worker.cjs", {
      workerData: { buffer, uncloneable: () => {} },
      transferList: [buffer],
    }),
);
